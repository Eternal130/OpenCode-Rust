//! Parity tests for the server-side attachment materialization pipeline
//! (core/session/prompt.ts `materializeAttachment`).

use base64::Engine as _;
use oc_schema::prompt::InputFileAttachment;

use super::prompt::{
    detect_mime, materialize_attachment, materialize_files, AttachmentError,
    MAX_ATTACHMENT_BYTES,
};

fn input(uri: String) -> InputFileAttachment {
    InputFileAttachment { uri, name: None, description: None, mention: None }
}

fn decode_b64(file: &oc_schema::prompt::FileAttachment) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(&file.data).unwrap()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("oc-prompt-attach-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// --- data URLs -------------------------------------------------------------

#[test]
fn decodes_base64_data_urls() {
    let bytes = b"hello attachment".to_vec();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let file = materialize_attachment(&input(format!("data:application/octet-stream;base64,{encoded}")), None)
        .unwrap();
    assert_eq!(decode_b64(&file), bytes);
    // The data-URL header mime is IGNORED (data URLs resolve with
    // `mime: undefined`, prompt.ts:97-105) — `Mime.detect` sniffs the
    // bytes: textual ⇒ text/plain.
    assert_eq!(file.mime, "text/plain");
    assert_eq!(file.source, oc_schema::prompt::FileSource::Inline);
    assert!(file.name.is_none());
}

#[test]
fn decodes_percent_encoded_non_base64_data_urls() {
    // prompt.ts:217-218 — non-base64 payloads go through decodeURIComponent.
    let file =
        materialize_attachment(&input("data:text/plain,hello%20w%C3%B6rld".into()), None)
            .unwrap();
    assert_eq!(decode_b64(&file), b"hello w\xc3\xb6rld".to_vec());
}

#[test]
fn rejects_non_canonical_and_broken_data_urls() {
    // prompt.ts:219-221 — non-canonical base64 (`AAECAw=`) is rejected.
    let error = materialize_attachment(
        &input("data:image/png;base64,AAECAw=".into()),
        None,
    )
    .unwrap_err();
    assert_eq!(
        error,
        AttachmentError::InvalidDataUrl { label: "inline attachment".into() },
        "{error}"
    );
    // No comma ⇒ invalid.
    assert!(matches!(
        materialize_attachment(&input("data:image/png".into()), None),
        Err(AttachmentError::InvalidDataUrl { .. })
    ));
    // A named attachment labels errors with the name, not the bytes
    // (prompt.ts:206-208).
    let error = materialize_attachment(
        &input("data:;base64,%%".into()),
        None,
    )
    .unwrap_err();
    assert_eq!(error, AttachmentError::InvalidDataUrl { label: "inline attachment".into() });
}

// --- size cap --------------------------------------------------------------

#[test]
fn rejects_oversize_attachments() {
    // prompt.ts:107-111 — the decoded-bytes cap names the label.
    let big = vec![b'a'; MAX_ATTACHMENT_BYTES + 1];
    let encoded = base64::engine::general_purpose::STANDARD.encode(&big);
    let error = materialize_attachment(
        &input(format!("data:application/octet-stream;base64,{encoded}")),
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("Attachment exceeds the {MAX_ATTACHMENT_BYTES} byte limit: inline attachment")
    );

    // prompt.ts:192-196 — the stat-time cap fires BEFORE the read and
    // names the uri.
    let dir = scratch("oversize");
    let path = dir.join("big.bin");
    std::fs::write(&path, vec![b'a'; MAX_ATTACHMENT_BYTES + 1]).unwrap();
    let error = materialize_attachment(
        &input(format!("file://{}", path.display())),
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("Attachment exceeds the {MAX_ATTACHMENT_BYTES} byte limit: file://{}", path.display())
    );
    std::fs::remove_file(&path).ok();
    std::fs::remove_dir(&dir).ok();
}

// --- file:// reads + line slicing -------------------------------------------

#[test]
fn slices_start_end_lines_for_text_plain() {
    // prompt.ts:114-123 — `?start=&end=` slice the decoded UTF-8 lines
    // `[start-1, end)` when the sniffed mime is text/plain.
    let dir = scratch("slicing");
    let path = dir.join("notes.txt");
    std::fs::write(&path, "one\ntwo\nthree\nfour\nfive\n").unwrap();
    // JS `slice(start - 1, end)` is end-EXCLUSIVE: `?start=2&end=4`
    // yields lines 2-4.
    let uri = format!("file://{}?start=2&end=4", path.display());
    let file = materialize_attachment(&input(uri), None).unwrap();
    assert_eq!(file.mime, "text/plain");
    assert_eq!(decode_b64(&file), b"two\nthree\nfour".to_vec());
    assert_eq!(file.name.as_deref(), Some("notes.txt"));

    // start without end runs through the trailing empty line.
    let uri = format!("file://{}?start=4", path.display());
    let file = materialize_attachment(&input(uri), None).unwrap();
    assert_eq!(decode_b64(&file), b"four\nfive\n".to_vec());

    // Non-positive / non-integer params are ignored (positiveInt,
    // prompt.ts:227-231) — the whole file stands.
    let uri = format!("file://{}?start=0&end=abc", path.display());
    let file = materialize_attachment(&input(uri), None).unwrap();
    assert_eq!(decode_b64(&file), b"one\ntwo\nthree\nfour\nfive\n".to_vec());
    std::fs::remove_file(&path).ok();
    std::fs::remove_dir(&dir).ok();
}

#[test]
fn materializes_directory_listings() {
    // prompt.ts:172-190 — directories first (with a trailing separator),
    // then files by name, joined with newlines, application/x-directory.
    let dir = scratch("listing/src");
    std::fs::create_dir_all(dir.join("beta")).unwrap();
    std::fs::create_dir_all(dir.join("alpha")).unwrap();
    std::fs::write(dir.join("z.txt"), b"z").unwrap();
    std::fs::write(dir.join("a.txt"), b"a").unwrap();
    let parent = dir.parent().unwrap();
    let uri = format!("file://{}", dir.display());
    let file = materialize_attachment(&input(uri.clone()), None).unwrap();
    assert_eq!(file.mime, "application/x-directory");
    assert_eq!(file.name.as_deref(), Some("src"));
    assert_eq!(file.source, oc_schema::prompt::FileSource::Uri { uri });
    let separator = std::path::MAIN_SEPARATOR;
    assert_eq!(
        String::from_utf8(decode_b64(&file)).unwrap(),
        format!("alpha{separator}\nbeta{separator}\na.txt\nz.txt")
    );
    let _ = parent;
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

// --- image normalization ----------------------------------------------------

/// A real 1x1 PNG (encoded at test time so the normalizer can decode
/// it; a bare signature header cannot).
fn tiny_png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode 1x1 png");
    bytes.into_inner()
}

#[test]
fn image_attachments_normalize_under_limits_and_pass_through_without() {
    let png = tiny_png();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    // `None` limits = the ResizerUnavailableError passthrough arm
    // (prompt.ts:145).
    let passthrough = materialize_attachment(
        &input(format!("data:image/png;base64,{encoded}")),
        None,
    )
    .unwrap();
    assert_eq!(passthrough.data, encoded);
    assert_eq!(passthrough.mime, "image/png");

    // Under limits the normalizer also passes the payload through
    // untouched (image.rs passthrough returns the original base64).
    let limits = crate::image::Limits::default();
    let normalized = materialize_attachment(
        &input(format!("data:image/png;base64,{encoded}")),
        Some(&limits),
    )
    .unwrap();
    assert_eq!(normalized.data, encoded);
    assert_eq!(normalized.mime, "image/png");

    // A payload that sniffs as an image but cannot decode is an
    // AttachmentError carrying the normalizer's message (prompt.ts:
    // 146-147) — the data-URL header mime is ignored, `Mime.detect`
    // reads the signature.
    let truncated = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    let junk = base64::engine::general_purpose::STANDARD.encode(truncated);
    let error = materialize_attachment(
        &input(format!("data:image/png;base64,{junk}")),
        Some(&limits),
    )
    .unwrap_err();
    assert!(matches!(error, AttachmentError::Image(_)), "{error}");
}

// --- mime + files-array plumbing ---------------------------------------------

#[test]
fn mime_detection_matches_ref() {
    // mime.ts — including the pdf + text/plain arms the old server-side
    // sniffer lacked.
    assert_eq!(detect_mime(b"%PDF-"), "application/pdf");
    assert_eq!(detect_mime(b"just some words\n"), "text/plain");
    assert_eq!(detect_mime(&[]), "text/plain");
    assert_eq!(detect_mime(&[0, 1, 2, 3]), "application/octet-stream");
    assert_eq!(detect_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]), "image/png");
}

#[test]
fn materialize_files_maps_none_and_fails_whole_batch() {
    assert!(materialize_files(None, None).unwrap().is_none());
    assert!(materialize_files(Some(&[]), None).unwrap().is_none());
    let good = input("data:text/plain,hi".into());
    let bad = input("https://example.test/a.png".into());
    assert!(matches!(
        materialize_files(Some(&[good, bad]), None),
        Err(AttachmentError::UnsupportedUri { .. })
    ));
}
