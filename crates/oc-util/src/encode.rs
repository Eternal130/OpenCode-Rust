//! String encoding helpers (`packages/util/src/encode.ts`): unpadded URL-safe
//! base64 plus the FNV-1a base-36 checksums used for content addressing.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

/// Encoding/decoding failures.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// Input is not valid URL-safe base64.
    #[error("invalid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    /// Decoded bytes are not valid UTF-8.
    #[error("invalid utf-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}

/// Unpadded URL-safe base64 of the UTF-8 bytes of `value`.
#[must_use]
pub fn base64url_encode(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(value.as_bytes())
}

/// Unpadded URL-safe base64 of raw bytes.
#[must_use]
pub fn base64url_encode_bytes(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

/// Decodes unpadded URL-safe base64 (padding tolerated) into bytes.
///
/// # Errors
///
/// Returns [`EncodeError::Base64`] for malformed input and
/// [`EncodeError::Utf8`] never (bytes are returned verbatim).
pub fn base64url_decode(value: &str) -> Result<Vec<u8>, EncodeError> {
    let trimmed = value.trim_end_matches('=');
    Ok(URL_SAFE_NO_PAD.decode(trimmed)?)
}

/// Decodes unpadded URL-safe base64 (padding tolerated) into a string.
///
/// # Errors
///
/// Returns [`EncodeError::Base64`] for malformed input and
/// [`EncodeError::Utf8`] when the bytes are not valid UTF-8.
pub fn base64url_decode_string(value: &str) -> Result<String, EncodeError> {
    let bytes = base64url_decode(value)?;
    String::from_utf8(bytes).map_err(|err| EncodeError::Utf8(err.utf8_error()))
}

const BASE36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn to_base36(mut value: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push(BASE36[(value % 36) as usize]);
        value /= 36;
    }
    digits.reverse();
    String::from_utf8(digits).expect("digits are ASCII")
}

/// FNV-1a checksum over the UTF-16 code units of `content`, rendered as base
/// 36. Empty input yields `None`, matching the original.
#[must_use]
pub fn checksum(content: &str) -> Option<String> {
    if content.is_empty() {
        return None;
    }
    let mut hash: u32 = 0x811c_9dc5;
    for unit in content.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    Some(to_base36(hash))
}

/// Checksum that samples five 4096-code-unit windows when `content` exceeds
/// `limit` code units, formatted `len:c1:c2:c3:c4:c5`.
#[must_use]
pub fn sampled_checksum(content: &str, limit: usize) -> Option<String> {
    if content.is_empty() {
        return None;
    }
    let units: Vec<u16> = content.encode_utf16().collect();
    let len = units.len();
    if len <= limit {
        return checksum(content);
    }
    let len_signed = len.cast_signed();
    let size = 4096_isize;
    let quarter = len_signed / 4;
    let points = [0, quarter, quarter * 2, quarter * 3, len_signed - size];
    let parts: Vec<String> = points
        .iter()
        .map(|&point| {
            // start = max(0, min(len - size, point - size / 2))
            let ceiling = len_signed - size;
            let start = ceiling.min(point - size / 2).max(0).cast_unsigned();
            let end = (start + size.cast_unsigned()).min(len);
            let window = String::from_utf16_lossy(&units[start..end]);
            checksum(&window).unwrap_or_default()
        })
        .collect();
    Some(format!("{len}:{}", parts.join(":")))
}

/// Formats a byte slice as lowercase hex (helper shared with [`crate::hash`]).
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpadded_url_safe_utf8_base64() {
        // Vectors from packages/util/src/encode.test.ts.
        assert_eq!(base64url_encode("hello"), "aGVsbG8");
        assert_eq!(base64url_encode("✓ à la mode"), "4pyTIMOgIGxhIG1vZGU");
        assert_eq!(base64url_decode_string("4pyTIMOgIGxhIG1vZGU").unwrap(), "✓ à la mode");
        assert_eq!(base64url_decode_string("dXNlcjpwYXNz").unwrap(), "user:pass");
    }

    #[test]
    fn url_safe_alphabet_avoids_plus_slash() {
        // 0xfb 0xef 0xbe packs into six-bit groups 62,62,62,62, which render
        // "++++" in standard base64 and "----" in the URL-safe alphabet.
        let encoded = base64url_encode_bytes(&[0xfb, 0xef, 0xbe]);
        assert_eq!(encoded, "----");
        assert!(!encoded.contains('+') && !encoded.contains('/') && !encoded.contains('='));
        assert_eq!(base64url_encode_bytes(&[0xff]), "_w");
    }

    #[test]
    fn rejects_invalid_base64() {
        assert!(base64url_decode("%%%").is_err());
        assert!(base64url_decode("a").is_err()); // dangling single char
                                                 // `/` belongs to the standard alphabet, not the URL-safe one.
        assert!(base64url_decode("////").is_err());
        assert!(base64url_decode("--__").is_ok());
    }

    #[test]
    fn round_trips() {
        for value in ["", "a", "user:pass", "✓ à la mode", "multi\nline\ttext"] {
            assert_eq!(base64url_decode_string(&base64url_encode(value)).unwrap(), value);
        }
    }

    #[test]
    fn tolerates_padding() {
        assert_eq!(base64url_decode_string("aGVsbG8=").unwrap(), "hello");
    }

    #[test]
    fn rejects_invalid_utf8_payload() {
        let bytes = base64url_encode_bytes(&[0xff, 0xfe]);
        assert!(base64url_decode_string(&bytes).is_err());
        assert_eq!(base64url_decode(&bytes).unwrap(), vec![0xff, 0xfe]);
    }

    #[test]
    fn stable_fnv_checksums() {
        // Vectors from packages/util/src/encode.test.ts.
        assert_eq!(checksum(""), None);
        assert_eq!(checksum("hello").as_deref(), Some("m3bicr"));
        assert_eq!(checksum("✓ à la mode").as_deref(), Some("jmczk0"));
    }

    #[test]
    fn sampled_checksum_boundaries() {
        let value = "abcdef".repeat(100); // 600 code units
        assert_eq!(sampled_checksum(&value, value.len()), checksum(&value));
        assert_eq!(
            sampled_checksum(&value, value.len() - 1).as_deref(),
            Some("600:1isj1k5:1isj1k5:1isj1k5:1isj1k5:1isj1k5")
        );
    }

    #[test]
    fn sampled_checksum_windows_large_inputs() {
        let value = "x".repeat(600_000);
        let sampled = sampled_checksum(&value, 500_000).unwrap();
        // len:five parts, each window is all "x" so identical checksums.
        assert!(sampled.starts_with("600000:"));
        assert_eq!(sampled.split(':').count(), 6);
    }

    #[test]
    fn hex_helper() {
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
