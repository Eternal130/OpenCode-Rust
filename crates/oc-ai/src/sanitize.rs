//! Surrogate sanitization (port of `src/utils/sanitize.ts`) — a **Rust-level
//! no-op by construction**.
//!
//! The TS helper exists because JavaScript strings can contain unpaired
//! (lone) UTF-16 surrogates, which break providers when serialized
//! (`String.prototype.toWellFormed` replaces them with U+FFFD). Rust
//! `String`s are guaranteed well-formed UTF-8 at the type level, and
//! `serde_json` cannot produce lone surrogates from valid strings, so there
//! is nothing to repair. This module keeps the pre-body transform pipeline
//! shape aligned with the TS package (`client.ts` applies
//! `sanitizeSurrogates` before body build) and documents the parity
//! divergence.

/// Identity over well-formed Rust strings (see module docs).
#[allow(clippy::must_use_candidate)]
pub fn sanitize_surrogates(value: &str) -> &str {
    value
}

#[cfg(test)]
mod tests {
    #[test]
    fn rust_strings_cannot_carry_lone_surrogates() {
        // The TS suite feeds "\uD800" (a lone surrogate) through the
        // sanitizer; in Rust that input cannot exist as a &str. Round-tripping
        // arbitrary provider text is therefore already well-formed.
        assert_eq!(super::sanitize_surrogates("plain text"), "plain text");
        assert_eq!(super::sanitize_surrogates("snowman ☃ / emoji 😀"), "snowman ☃ / emoji 😀");
    }
}
