//! RS256/JWKS ID-token signature verification — the jose half of v2's
//! `verifyIDToken` (chatgpt.ts:439-459): `createRemoteJWKSet` +
//! `jwtVerify(..., { algorithms: ["RS256"] })`, on ring.
//!
//! Ruling 2026-10-23 (controller, binding): v2 constructs the remote
//! JWKS set FRESH on every verification and jose's cache is per-object,
//! so the net v2 behavior is ONE JWKS fetch per ID-token verification —
//! no cross-call cache, no cooldown. [`RemoteJwks`] mirrors that: every
//! [`RemoteJwks::verify`] fetches the document, selects the JWK by
//! `kid`, and verifies RS256. Error strings are specific CAUSES for the
//! server-side log; the caller (`providers_enterprise::verify_id_token`)
//! collapses every one into v2's single user-facing message.

use base64::Engine;
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde_json::Value;

/// The only accepted JWS algorithm (jose `algorithms: ["RS256"]`).
const ALG_RS256: &str = "RS256";

/// A remote JWKS endpoint, fetched fresh on every verification.
pub(crate) struct RemoteJwks {
    url: String,
    http: reqwest::Client,
}

impl RemoteJwks {
    pub(crate) fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), http: reqwest::Client::new() }
    }

    /// Verifies the token's RS256 signature: header `alg` allowlist,
    /// `kid` JWK selection, one document fetch, ring verification of the
    /// `header.payload` ASCII bytes.
    pub(crate) async fn verify(&self, token: &str) -> Result<(), String> {
        let (header, _, _) = split_jws(token)?;
        let Some(kid) = header.get("kid").and_then(Value::as_str) else {
            return Err("token header carries no kid".into());
        };
        let jwk = self.key_for_kid(kid).await?;
        verify_with_jwk(token, &jwk)
    }

    /// Fetches the JWKS document (per call — see the module ruling) and
    /// selects the JWK whose `kid` matches the token header.
    async fn key_for_kid(&self, kid: &str) -> Result<Value, String> {
        let response = self
            .http
            .get(&self.url)
            .send()
            .await
            .map_err(|error| format!("JWKS fetch failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("JWKS fetch failed: HTTP {}", response.status().as_u16()));
        }
        let document: Value = response
            .json()
            .await
            .map_err(|error| format!("JWKS document is not JSON: {error}"))?;
        document
            .get("keys")
            .and_then(Value::as_array)
            .and_then(|keys| {
                keys.iter().find(|jwk| jwk.get("kid").and_then(Value::as_str) == Some(kid))
            })
            .cloned()
            .ok_or_else(|| format!("no JWK matches kid {kid:?}"))
    }
}

/// Splits a compact JWS into (header JSON, `header.payload` ASCII bytes,
/// decoded signature).
fn split_jws(token: &str) -> Result<(Value, &[u8], Vec<u8>), String> {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("token is not a three-part JWS".into());
    }
    let header_bytes = engine
        .decode(parts[0])
        .map_err(|error| format!("token header is not base64url: {error}"))?;
    let header: Value = serde_json::from_slice(&header_bytes)
        .map_err(|error| format!("token header is not JSON: {error}"))?;
    let signature = engine
        .decode(parts[2])
        .map_err(|error| format!("token signature is not base64url: {error}"))?;
    let (signing_input, _) = token.rsplit_once('.').expect("three-part JWS re-splits");
    Ok((header, signing_input.as_bytes(), signature))
}

/// Header checks + ring verification of the token against one selected
/// JWK (jose's jwtVerify against a resolved key).
fn verify_with_jwk(token: &str, jwk: &Value) -> Result<(), String> {
    let (header, signing_input, signature) = split_jws(token)?;
    if header.get("alg").and_then(Value::as_str) != Some(ALG_RS256) {
        return Err(format!("alg {:?} is not allowed (RS256 only)", header.get("alg")));
    }
    let key = rsa_public_key(jwk)?;
    key.verify(&RSA_PKCS1_2048_8192_SHA256, signing_input, &signature)
        .map_err(|error| format!("RS256 signature mismatch: {error:?}"))
}

/// The RSA public key as ring consumes it: JWK `n`/`e` (base64url,
/// big-endian, no leading zero bytes) decoded into raw components.
fn rsa_public_key(jwk: &Value) -> Result<RsaPublicKeyComponents<Vec<u8>>, String> {
    if jwk.get("kty").and_then(Value::as_str) != Some("RSA") {
        return Err("matched JWK is not an RSA key".into());
    }
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let modulus = jwk.get("n").and_then(Value::as_str).ok_or("JWK carries no modulus")?;
    let exponent = jwk.get("e").and_then(Value::as_str).ok_or("JWK carries no exponent")?;
    let modulus = engine
        .decode(modulus)
        .map_err(|error| format!("JWK modulus is not base64url: {error}"))?;
    let exponent = engine
        .decode(exponent)
        .map_err(|error| format!("JWK exponent is not base64url: {error}"))?;
    if modulus.is_empty() || exponent.is_empty() {
        return Err("JWK modulus/exponent is empty".into());
    }
    Ok(RsaPublicKeyComponents { n: modulus, e: exponent })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed signing fixture as a ring key pair (see
    /// `integration/tests.rs` for the full verification matrix; these
    /// unit tests isolate the crypto path from the JWKS fetch).
    fn fixture_key() -> ring::signature::RsaKeyPair {
        let pem = include_str!("testdata/id_token_signing_key.pem");
        let body: String = pem
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .flat_map(str::chars)
            .collect();
        let der = base64::engine::general_purpose::STANDARD.decode(body).expect("PEM decodes");
        ring::signature::RsaKeyPair::from_pkcs8(&der).expect("fixture key parses")
    }

    #[test]
    fn ring_sign_verify_round_trip() {
        let key_pair = fixture_key();
        let message = b"header.payload";
        let mut signature = vec![0u8; key_pair.public().modulus_len()];
        key_pair
            .sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &ring::rand::SystemRandom::new(),
                message,
                &mut signature,
            )
            .expect("signs");
        let components = RsaPublicKeyComponents::<Vec<u8>>::from(key_pair.public());
        components
            .verify(&RSA_PKCS1_2048_8192_SHA256, message, &signature)
            .expect("round-trip verifies");
    }

    #[test]
    fn jwk_value_path_round_trip() {
        let key_pair = fixture_key();
        let components = RsaPublicKeyComponents::<Vec<u8>>::from(key_pair.public());
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let n = engine.encode(components.n);
        let e = engine.encode(components.e);
        let header = r#"{"alg":"RS256","kid":"sign-in"}"#;
        let payload = r#"{"sub":"user_123"}"#;
        let signing_input = format!(
            "{}.{}",
            engine.encode(header.as_bytes()),
            engine.encode(payload.as_bytes())
        );
        let mut signature = vec![0u8; key_pair.public().modulus_len()];
        key_pair
            .sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &ring::rand::SystemRandom::new(),
                signing_input.as_bytes(),
                &mut signature,
            )
            .expect("signs");
        let token = format!("{signing_input}.{}", engine.encode(&signature));
        let jwk: Value = serde_json::from_str(&format!(
            r#"{{"kty":"RSA","kid":"sign-in","n":"{n}","e":"{e}"}}"#
        ))
        .expect("jwk json");
        verify_with_jwk(&token, &jwk).expect("jwk value path verifies");
    }
}
