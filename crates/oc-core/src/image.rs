//! Port of `image.ts` + `image/photon.ts` — the model-input-side image
//! limits pipeline (`Image.normalize`): decode, enforce
//! `max_width`/`max_height`/`max_base64_bytes`, and `autoResize`
//! downscaling (REF uses photon WASM with Lanczos3; here the `image`
//! crate's Lanczos3 resize + the same PNG-then-JPEG-quality encoder
//! ladder).
//!
//! Error messages are byte-identical to the REF `DecodeError`/`SizeError`
//! `message` overrides so downstream notes match v2 output. The
//! `ResizerUnavailableError` degradation (photon WASM missing on some
//! runtimes) has no Rust equivalent — the resizer is always available —
//! so that passthrough branch simply never fires.

// Lint posture: ports of TS sources keep the mechanical-idiom allows the
// runner established (numeric casts, exhaustive tables, ownership queries).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::module_name_repetitions,
    clippy::result_large_err
)]

use base64::Engine as _;
use oc_schema::tool::Content;

/// REF JPEG encoder ladder (`JPEG_QUALITIES`): tried after PNG, in order.
const JPEG_QUALITIES: [u8; 5] = [80, 85, 70, 55, 40];

/// REF image ladder step (photon.ts: successive `* 0.75` floors).
const LADDER_FACTOR: f64 = 0.75;

/// `Image.Limits` — resolved limits (REF defaults when config omits keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub auto_resize: bool,
    pub max_width: u32,
    pub max_height: u32,
    pub max_base64_bytes: usize,
}

impl Default for Limits {
    /// REF `initial` state: `autoResize: true, maxWidth/maxHeight: 2000,
    /// maxBase64Bytes: 5 * 1024 * 1024`.
    fn default() -> Self {
        Self {
            auto_resize: true,
            max_width: 2000,
            max_height: 2000,
            max_base64_bytes: 5 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// Resolve from the `config.media.image` document overlay: unset keys
    /// keep the REF defaults.
    #[must_use]
    pub fn from_config(config: &oc_schema::config::media::Image) -> Self {
        let defaults = Self::default();
        let positive = |value: Option<oc_schema::common::PositiveInt>| {
            value.map_or(defaults.max_width, |v| v as u32)
        };
        Self {
            auto_resize: config.auto_resize.unwrap_or(defaults.auto_resize),
            max_width: positive(config.max_width),
            max_height: positive(config.max_height),
            max_base64_bytes: config
                .max_base64_bytes
                .map_or(defaults.max_base64_bytes, |v| v as usize),
        }
    }
}

/// `Image.DecodeError` / `Image.SizeError` — the `message` strings are
/// byte-identical to the REF tagged-error overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizeError {
    /// `Image could not be decoded: {resource}`
    Decode { resource: String },
    /// `Image {resource} is {width}x{height} with base64 size {bytes},
    /// exceeding configured limits {maxWidth}x{maxHeight}/{maxBytes} bytes`
    Size {
        resource: String,
        width: u32,
        height: u32,
        bytes: usize,
        max_width: u32,
        max_height: u32,
        max_bytes: usize,
    },
}

impl std::fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode { resource } => {
                write!(f, "Image could not be decoded: {resource}")
            }
            Self::Size { resource, width, height, bytes, max_width, max_height, max_bytes } => {
                write!(
                    f,
                    "Image {resource} is {width}x{height} with base64 size {bytes}, \
                     exceeding configured limits {max_width}x{max_height}/{max_bytes} bytes"
                )
            }
        }
    }
}

impl std::error::Error for NormalizeError {}

/// The `FileSystem.Content & { encoding: "base64" }` result of
/// `Image.normalize`: base64 payload plus the (possibly re-encoded) MIME.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub base64: String,
    pub mime: String,
}

/// `Image.Photon.normalize` (photon.ts) — decode, limit-check, downscale
/// ladder with PNG→JPEG re-encode until the base64 size fits.
///
/// `resource` is the display name used in error messages. The input
/// `base64` payload passes through untouched when it already satisfies
/// every limit (REF returns `content` as-is).
pub fn normalize(
    resource: &str,
    base64_content: &str,
    limits: &Limits,
) -> Result<Normalized, NormalizeError> {
    let bytes = base64_content.len();
    let decode_error = || NormalizeError::Decode { resource: resource.to_string() };
    let size_error = |width: u32, height: u32| NormalizeError::Size {
        resource: resource.to_string(),
        width,
        height,
        bytes,
        max_width: limits.max_width,
        max_height: limits.max_height,
        max_bytes: limits.max_base64_bytes,
    };

    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_content)
        .map_err(|_| decode_error())?;
    // photon's `new_from_byteslice` decodes PNG/JPEG (its `get_bytes` /
    // `get_bytes_jpeg` re-encode set); anything else is a DecodeError.
    let format = image::guess_format(&raw).map_err(|_| decode_error())?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return Err(decode_error());
    }
    let decoded = image::load_from_memory_with_format(&raw, format).map_err(|_| decode_error())?;
    let (width, height) = (decoded.width(), decoded.height());
    if width <= limits.max_width && height <= limits.max_height && bytes <= limits.max_base64_bytes
    {
        // Passthrough: the caller keeps its original MIME.
        return Ok(Normalized { base64: base64_content.to_string(), mime: String::new() });
    }
    if !limits.auto_resize {
        return Err(size_error(width, height));
    }

    let scale = limits.max_width as f64 / width as f64;
    let scale = scale.min(limits.max_height as f64 / height as f64).min(1.0);
    // REF builds up to 32 candidate sizes: the scaled target first, then
    // successive `floor(previous * 0.75)` shrink steps (min 1, deduped).
    let first = (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    );
    let mut sizes: Vec<(u32, u32)> = Vec::with_capacity(32);
    let (mut pw, mut ph) = first;
    loop {
        if sizes.len() >= 32 || sizes.last().is_some_and(|&(w, h)| w == 1 && h == 1) {
            break;
        }
        let next = if sizes.is_empty() {
            first
        } else {
            (
                if pw == 1 { 1 } else { ((pw as f64 * LADDER_FACTOR).floor() as u32).max(1) },
                if ph == 1 { 1 } else { ((ph as f64 * LADDER_FACTOR).floor() as u32).max(1) },
            )
        };
        if sizes.iter().any(|&(w, h)| w == next.0 && h == next.1) {
            break;
        }
        sizes.push(next);
        pw = next.0;
        ph = next.1;
    }

    for &(target_w, target_h) in &sizes {
        let resized = image::imageops::resize(
            &decoded,
            target_w,
            target_h,
            image::imageops::FilterType::Lanczos3,
        );
        let resized = image::DynamicImage::ImageRgba8(resized);
        // Encoder ladder: PNG, then JPEG at each quality (photon.ts order).
        for (mime, encoded) in encode_candidates(&resized) {
            // Base64 uses four bytes per three input bytes, incl. padding.
            if encoded.len().div_ceil(3) * 4 <= limits.max_base64_bytes {
                let payload = base64::engine::general_purpose::STANDARD.encode(&encoded);
                return Ok(Normalized { base64: payload, mime: mime.to_string() });
            }
        }
    }
    Err(size_error(width, height))
}

/// The `(mime, bytes)` encoder candidates for one resized frame, in REF
/// order: PNG first, then JPEG at [80, 85, 70, 55, 40].
fn encode_candidates(image: &image::DynamicImage) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::with_capacity(1 + JPEG_QUALITIES.len());
    let mut png = Vec::new();
    if image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
        out.push(("image/png", png));
    }
    for quality in JPEG_QUALITIES {
        let mut jpeg = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality);
        if image.write_with_encoder(encoder).is_ok() {
            out.push(("image/jpeg", jpeg));
        }
    }
    out
}

/// `normalizeImages` (tool.ts:74-95) — normalize every inline image
/// (`file` content with an `image/*` MIME and a `data:` base64 URI) in a
/// tool result's content list.
///
/// Degradations match REF exactly: an undecodable image is dropped with a
/// trailing `[N image(s) omitted: could not be decoded.]` note; an
/// unresizable one (over limits even after the ladder) is dropped with
/// `[N image(s) omitted: could not be resized below the image size limit.]`.
pub fn normalize_tool_content(content: Vec<Content>, limits: &Limits) -> Vec<Content> {
    let mut out = Vec::with_capacity(content.len());
    let mut decode_count = 0usize;
    let mut size_count = 0usize;
    for item in content {
        let Content::File { uri, mime, name } = &item else {
            out.push(item);
            continue;
        };
        if !mime.starts_with("image/") {
            out.push(item);
            continue;
        }
        let Some(base64) = data_uri_base64(uri) else {
            out.push(item);
            continue;
        };
        let resource = name.clone().unwrap_or_else(|| format!("{mime} tool output"));
        match normalize(&resource, base64, limits) {
            // Passthrough keeps the original MIME (`mime` is empty on the
            // untouched path); the re-encode path carries the new one.
            Ok(normalized) => {
                let new_mime =
                    if normalized.mime.is_empty() { mime.clone() } else { normalized.mime };
                out.push(Content::File {
                    uri: format!("data:{new_mime};base64,{}", normalized.base64),
                    mime: new_mime,
                    name: name.clone(),
                });
            }
            Err(NormalizeError::Decode { .. }) => decode_count += 1,
            Err(NormalizeError::Size { .. }) => size_count += 1,
        }
    }
    if decode_count > 0 {
        out.push(Content::Text {
            text: format!(
                "[{} image{} omitted: could not be decoded.]",
                decode_count,
                if decode_count == 1 { "" } else { "s" }
            ),
        });
    }
    if size_count > 0 {
        out.push(Content::Text {
            text: format!(
                "[{} image{} omitted: could not be resized below the image size limit.]",
                size_count,
                if size_count == 1 { "" } else { "s" }
            ),
        });
    }
    out
}

/// `data:[mime];base64,<payload>` extraction (`/^data:[^,]*;base64,(.*)$/s`).
fn data_uri_base64(uri: &str) -> Option<&str> {
    let rest = uri.strip_prefix("data:")?;
    let comma = rest.find(',')?;
    if !rest[..comma].ends_with(";base64") {
        return None;
    }
    Some(&rest[comma + 1..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbaImage};

    use base64::engine::general_purpose::STANDARD as B64;

    /// A solid PNG of the given size (no binary fixtures — generated).
    fn png(width: u32, height: u32) -> String {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([200, 30, 30, 255]),
        ));
        let mut bytes = Vec::new();
        image.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        B64.encode(bytes)
    }

    fn png_dimensions(base64_payload: &str) -> (u32, u32) {
        let raw = B64.decode(base64_payload).unwrap();
        let decoded = image::load_from_memory(&raw).unwrap();
        (decoded.width(), decoded.height())
    }

    #[test]
    fn defaults_match_ref() {
        let limits = Limits::default();
        assert!(limits.auto_resize);
        assert_eq!((limits.max_width, limits.max_height), (2000, 2000));
        assert_eq!(limits.max_base64_bytes, 5 * 1024 * 1024);
    }

    #[test]
    fn config_resolution_keeps_unset_defaults() {
        let limits = Limits::from_config(&oc_schema::config::media::Image::default());
        assert_eq!(limits, Limits::default());
        let configured = oc_schema::config::media::Image {
            auto_resize: Some(false),
            max_width: Some(64),
            max_base64_bytes: Some(2048),
            ..Default::default()
        };
        let limits = Limits::from_config(&configured);
        assert!(!limits.auto_resize);
        assert_eq!(limits.max_width, 64);
        assert_eq!(limits.max_height, 2000);
        assert_eq!(limits.max_base64_bytes, 2048);
    }

    #[test]
    fn passthrough_when_within_limits() {
        let payload = png(8, 8);
        let limits = Limits {
            auto_resize: true,
            max_width: 100,
            max_height: 100,
            max_base64_bytes: 1 << 20,
        };
        let normalized = normalize("a.png", &payload, &limits).unwrap();
        assert_eq!(normalized.base64, payload);
        assert!(normalized.mime.is_empty());
    }

    #[test]
    fn decode_error_for_garbage_and_unsupported_formats() {
        let limits = Limits::default();
        let err = normalize("x.bin", "not-base64!!!", &limits).unwrap_err();
        assert_eq!(err, NormalizeError::Decode { resource: "x.bin".into() });
        assert_eq!(err.to_string(), "Image could not be decoded: x.bin");
        // A well-formed base64 blob that is not a PNG/JPEG also fails decode
        // (photon decodes PNG/JPEG only).
        let err = normalize("y.gif", &B64.encode(b"GIF89a........"), &limits).unwrap_err();
        assert!(matches!(err, NormalizeError::Decode { .. }));
    }

    #[test]
    fn resize_shrinks_to_max_width_preserving_aspect() {
        let payload = png(200, 100);
        let limits =
            Limits { auto_resize: true, max_width: 50, max_height: 50, max_base64_bytes: 1 << 20 };
        let normalized = normalize("wide.png", &payload, &limits).unwrap();
        let (w, h) = png_dimensions(&normalized.base64);
        assert_eq!((w, h), (50, 25), "aspect ratio 2:1 preserved");
        assert_eq!(normalized.mime, "image/png");
    }

    #[test]
    fn resize_shrinks_to_max_height_preserving_aspect() {
        let payload = png(100, 200);
        let limits =
            Limits { auto_resize: true, max_width: 50, max_height: 50, max_base64_bytes: 1 << 20 };
        let normalized = normalize("tall.png", &payload, &limits).unwrap();
        let (w, h) = png_dimensions(&normalized.base64);
        assert_eq!((w, h), (25, 50));
    }

    #[test]
    fn byte_limit_rejected_when_auto_resize_disabled() {
        let payload = png(64, 64);
        let limits =
            Limits { auto_resize: false, max_width: 32, max_height: 32, max_base64_bytes: 1 << 20 };
        let err = normalize("blocked.png", &payload, &limits).unwrap_err();
        let NormalizeError::Size {
            resource,
            width,
            height,
            bytes,
            max_width,
            max_height,
            max_bytes,
        } = &err
        else {
            panic!("expected size error, got {err:?}");
        };
        assert_eq!(
            (resource.as_str(), *width, *height, *max_width, *max_height),
            ("blocked.png", 64, 64, 32, 32)
        );
        assert_eq!(*bytes, payload.len());
        assert_eq!(*max_bytes, 1 << 20);
        assert_eq!(
            err.to_string(),
            format!(
                "Image blocked.png is 64x64 with base64 size {}, exceeding configured limits 32x32/{} bytes",
                payload.len(),
                1 << 20
            )
        );
    }

    #[test]
    fn byte_limit_rejected_when_over_even_after_resize() {
        // Max byte budget so small that even the smallest ladder step's
        // PNG/JPEG encodes exceed it.
        let payload = png(400, 400);
        let limits =
            Limits { auto_resize: true, max_width: 32, max_height: 32, max_base64_bytes: 64 };
        let err = normalize("huge.png", &payload, &limits).unwrap_err();
        assert!(matches!(err, NormalizeError::Size { .. }));
        // Before resize (autoResize off) the same input also rejects.
        let limits_off =
            Limits { auto_resize: false, max_width: 32, max_height: 32, max_base64_bytes: 64 };
        assert!(matches!(
            normalize("huge.png", &payload, &limits_off),
            Err(NormalizeError::Size { .. })
        ));
    }

    #[test]
    fn jpeg_reencode_used_when_png_exceeds_byte_budget() {
        // A noisy texture makes PNG large; the JPEG ladder must fit.
        let mut inner = RgbaImage::new(300, 300);
        for y in 0..300 {
            for x in 0..300 {
                inner.put_pixel(
                    x,
                    y,
                    image::Rgba([
                        (x * 7 % 256) as u8,
                        (y * 13 % 256) as u8,
                        ((x + y) % 256) as u8,
                        255,
                    ]),
                );
            }
        }
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(inner)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let payload = B64.encode(&bytes);
        let limits = Limits {
            auto_resize: true,
            max_width: 2000,
            max_height: 2000,
            max_base64_bytes: 32 * 1024,
        };
        let normalized = normalize("noise.png", &payload, &limits).unwrap();
        assert_eq!(normalized.mime, "image/jpeg");
        assert!(normalized.base64.len() <= limits.max_base64_bytes);
        let raw = B64.decode(&normalized.base64).unwrap();
        assert_eq!(image::guess_format(&raw).unwrap(), image::ImageFormat::Jpeg);
    }

    #[test]
    fn format_passthrough_for_non_image_content() {
        let limits =
            Limits { auto_resize: true, max_width: 4, max_height: 4, max_base64_bytes: 16 };
        let content = vec![
            Content::Text { text: "hello".into() },
            Content::File { uri: "file:///x.txt".into(), mime: "text/plain".into(), name: None },
            // image mime but not a data URI — passes through untouched.
            Content::File { uri: "file:///x.png".into(), mime: "image/png".into(), name: None },
        ];
        let out = normalize_tool_content(content, &limits);
        assert_eq!(out.len(), 3);
        assert_eq!(
            out[2],
            Content::File { uri: "file:///x.png".into(), mime: "image/png".into(), name: None }
        );
    }

    #[test]
    fn tool_content_notes_for_omitted_images() {
        let limits =
            Limits { auto_resize: false, max_width: 4, max_height: 4, max_base64_bytes: 16 };
        let big = png(64, 64);
        let content = vec![
            Content::Text { text: "t".into() },
            Content::File {
                uri: format!("data:image/png;base64,{big}"),
                mime: "image/png".into(),
                name: Some("one.png".into()),
            },
            Content::File {
                uri: "data:image/png;base64,!!!not-base64!!!".into(),
                mime: "image/png".into(),
                name: None,
            },
        ];
        let out = normalize_tool_content(content, &limits);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], Content::Text { text: "t".into() });
        assert_eq!(
            out[1],
            Content::Text { text: "[1 image omitted: could not be decoded.]".into() }
        );
        assert_eq!(
            out[2],
            Content::Text {
                text: "[1 image omitted: could not be resized below the image size limit.]".into()
            }
        );
    }

    #[test]
    fn tool_content_resizes_and_updates_uri_mime() {
        let limits =
            Limits { auto_resize: true, max_width: 16, max_height: 16, max_base64_bytes: 1 << 20 };
        let payload = png(64, 64);
        let content = vec![Content::File {
            uri: format!("data:image/png;base64,{payload}"),
            mime: "image/png".into(),
            name: Some("shot.png".into()),
        }];
        let out = normalize_tool_content(content, &limits);
        let Content::File { uri, mime, .. } = &out[0] else {
            panic!("expected file content, got {:?}", out[0]);
        };
        assert_eq!(mime, "image/png");
        let base64_part = data_uri_base64(uri).unwrap();
        assert_eq!(png_dimensions(base64_part), (16, 16));
    }

    #[test]
    fn data_uri_extraction_rejects_non_base64() {
        assert_eq!(data_uri_base64("data:image/png;base64,AAA"), Some("AAA"));
        assert_eq!(data_uri_base64("data:image/png,AAA"), None);
        assert_eq!(data_uri_base64("https://x/y"), None);
    }
}
