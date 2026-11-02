//! Content hashing (`packages/util/src/hash.ts`).

use sha1::Sha1;
use sha2::{Digest, Sha256};

/// `Hash.fast` — SHA-1 of the UTF-8 bytes of `input`, lowercase hex
/// (G-162②: unified util; previously each call site rolled its own sha1).
#[must_use]
pub fn sha1_hex(input: impl AsRef<[u8]>) -> String {
    use sha1::Digest as _;
    let digest = Sha1::digest(input.as_ref());
    hex_lower(&digest)
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// SHA-256 of the UTF-8 bytes of `input`, lowercase hex.
#[must_use]
pub fn sha256_hex(input: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(input.as_ref());
    hex_lower(&digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string() {
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha1_known_vectors() {
        assert_eq!(sha1_hex(""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex("abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex("latest"), sha1_hex("latest".as_bytes()));
    }

    #[test]
    fn known_vectors() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex("hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn unicode_bytes() {
        // "✓ à la mode" hashed over UTF-8 bytes.
        assert_eq!(sha256_hex("✓ à la mode").len(), 64);
        assert_eq!(sha256_hex("✓ à la mode"), sha256_hex("✓ à la mode".as_bytes()));
    }

    #[test]
    fn hex_is_lowercase_and_sized() {
        let digest = sha256_hex(b"\x00\xff");
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn distinct_inputs() {
        assert_ne!(sha256_hex("a"), sha256_hex("b"));
    }
}
