//! Prompt attachment materialization: thin delegation to the canonical
//! oc-core port of `core/session/prompt.ts`'s `materializeAttachment`
//! (`oc_core::session::prompt`). The client POSTs `files` in the
//! **`PromptInput.FileAttachment`** shape; the inbox stores the resolved
//! `Prompt.FileAttachment`.
//!
//! The HTTP handler runs without the location's image limits, so image
//! normalization takes the `Image.ResizerUnavailableError` passthrough
//! arm here (`None` limits, prompt.ts:145).

pub use oc_core::session::prompt::{
    detect_mime, materialize_attachment as materialize_attachment_with_limits,
    AttachmentError, MAX_ATTACHMENT_BYTES,
};
use oc_schema::prompt::{FileAttachment, InputFileAttachment};

/// Materialize one input attachment (no image resizer at the HTTP
/// boundary — the passthrough arm).
pub fn materialize_attachment(
    input: &InputFileAttachment,
) -> Result<FileAttachment, AttachmentError> {
    oc_core::session::prompt::materialize_attachment(input, None)
}

/// Materialize the `files` array of a `session.prompt` body.
/// `None` (absent/null) passes through as `None`; a bad entry fails
/// the whole request (v2's `AttachmentError` rejects the prompt).
pub fn materialize_files(
    inputs: Option<&[InputFileAttachment]>,
) -> Result<Option<Vec<FileAttachment>>, AttachmentError> {
    oc_core::session::prompt::materialize_files(inputs, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY_PNG: &[u8] = include_bytes!("../../tests/fixtures/r07-tiny-4x2.png");

    #[test]
    fn delegates_inline_data_urls() {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(TINY_PNG);
        let input = InputFileAttachment {
            uri: format!("data:image/png;base64,{encoded}"),
            name: Some("clipboard.png".into()),
            description: None,
            mention: None,
        };
        let file = materialize_attachment(&input).unwrap();
        assert_eq!(file.mime, "image/png");
        assert_eq!(file.data, encoded);
        assert_eq!(file.source, oc_schema::prompt::FileSource::Inline);
        assert_eq!(file.name.as_deref(), Some("clipboard.png"), "input name wins");
    }

    #[test]
    fn delegates_file_urls_and_rejects_bad_ones() {
        let dir = std::env::temp_dir().join(format!("oc-r07-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tiny.png");
        std::fs::write(&path, TINY_PNG).unwrap();
        let uri = format!("file://{}", path.display());
        let file = materialize_attachment(&input(uri)).unwrap();
        assert_eq!(file.mime, "image/png");
        assert_eq!(file.name.as_deref(), Some("tiny.png"));
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();

        assert!(matches!(
            materialize_attachment(&input("https://example.test/a.png".into())),
            Err(AttachmentError::UnsupportedUri { .. })
        ));
        assert!(matches!(
            materialize_attachment(&input("data:image/png;base64,!!!".into())),
            Err(AttachmentError::InvalidDataUrl { .. })
        ));
        assert!(matches!(
            materialize_attachment(&input("file:///nonexistent-r07/x.png".into())),
            Err(AttachmentError::ReadFailed { .. })
        ));
    }

    #[test]
    fn maps_none_and_empty() {
        assert_eq!(materialize_files(None).unwrap(), None);
        assert_eq!(materialize_files(Some(&[])).unwrap(), None);
    }

    fn input(uri: String) -> InputFileAttachment {
        InputFileAttachment { uri, name: None, description: None, mention: None }
    }
}
