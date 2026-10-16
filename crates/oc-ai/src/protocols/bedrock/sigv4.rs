//! AWS `SigV4` request signing (port of `utils/bedrock-auth.ts` over
//! `aws4fetch`, plus the static/env credential slice of the AWS default
//! chain the v2 `defaultChain` resolves through `@aws-sdk/credential-providers`).
//!
//! The canonical-request algorithm is the AWS Signature Version 4
//! specification implemented over plain `sha2`: canonical request → string
//! to sign → derived signing key → `Authorization` header. Everything is
//! pure and injectable-time so the official AWS documentation vectors can
//! pin the output (see tests).

#![allow(
    clippy::cast_possible_truncation, // event-stream header widths mirror the AWS wire format
    clippy::cast_possible_wrap, // epoch-days arithmetic is u64 by construction
    clippy::cast_sign_loss,
    clippy::doc_markdown, // AWS/SigV4 identifiers in prose stay unquoted
    clippy::format_collect,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::redundant_closure_for_method_calls,
    clippy::unnested_or_patterns,
)]

use crate::route::auth::{AuthError, AuthInput, Definition};
use crate::schema::AiError;
use sha2::{Digest, Sha256};

use super::sigv4;

/// Static AWS credentials for `SigV4` (port of `BedrockAuth.Credentials`).
#[derive(Debug, Clone, PartialEq)]
pub struct Credentials {
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

/// Where a `SigV4` credential comes from — static config or the env slice of
/// the AWS default chain (`AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`/
/// `AWS_SESSION_TOKEN`), re-read at apply time like v2's per-request chain.
#[derive(Debug, Clone, Default)]
pub enum CredentialSource {
    /// No source: signing fails with the typed invalid-request error (v2
    /// `sigV4(undefined)` message).
    #[default]
    None,
    /// Explicit static credentials.
    Static(Credentials),
    /// `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ optional
    /// `AWS_SESSION_TOKEN`), resolved per request.
    Env,
}

/// Bedrock auth configuration (port of `BedrockAuth.resolveAuth` inputs):
/// bearer wins over `SigV4`; explicit static credentials win over the env
/// chain, matching the AWS SDK precedence for `AWS_BEARER_TOKEN_BEDROCK`.
#[derive(Debug, Clone, Default)]
pub struct BedrockAuthOptions {
    /// Bedrock API key (`AWS_BEARER_TOKEN_BEDROCK` fallback); bearer auth
    /// takes precedence over `SigV4`.
    pub api_key: Option<String>,
    /// Restrict to one mode: `sigv4` ignores the bearer token, `bearer`
    /// requires one (validated by the caller).
    pub mode: Option<BedrockAuthMode>,
    /// Explicit region (falls back to `AWS_REGION`, `AWS_DEFAULT_REGION`,
    /// then `us-east-1`).
    pub region: Option<String>,
    /// Static `SigV4` credentials (region on the credentials wins for the
    /// `SigV4` scope when no explicit region is set).
    pub credentials: Option<Credentials>,
}

/// v2 `ResolveAuthOptions.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BedrockAuthMode {
    Bearer,
    SigV4,
}

/// Region resolution (port of `BedrockAuth.resolveRegion`): explicit region,
/// then the static credentials' region, then `AWS_REGION` /
/// `AWS_DEFAULT_REGION`, then `us-east-1`.
#[must_use]
pub fn resolve_region(region: Option<&str>, credentials: Option<&Credentials>) -> String {
    if let Some(region) = region.filter(|region| !region.is_empty()) {
        return region.to_string();
    }
    if let Some(credentials) = credentials {
        return credentials.region.clone();
    }
    std::env::var("AWS_REGION")
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .ok()
        .filter(|region| !region.is_empty())
        .unwrap_or_else(|| "us-east-1".to_string())
}

/// Resolve credentials from the env slice of the default chain.
fn env_credentials(region: &str) -> Result<Credentials, AuthError> {
    let access_key_id = env_non_empty("AWS_ACCESS_KEY_ID").ok_or(missing("AWS_ACCESS_KEY_ID"))?;
    let secret_access_key =
        env_non_empty("AWS_SECRET_ACCESS_KEY").ok_or(missing("AWS_SECRET_ACCESS_KEY"))?;
    Ok(Credentials {
        region: region.to_string(),
        access_key_id,
        secret_access_key,
        session_token: env_non_empty("AWS_SESSION_TOKEN"),
    })
}

fn missing(name: &str) -> AuthError {
    AuthError { source: name.to_string(), message: format!("{name} is not set") }
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

/// The signed request output: headers to merge into the transport request.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedRequest {
    pub headers: Vec<(String, String)>,
}

/// Sign one request with `SigV4` (port of `signRequest`; deterministic for the
/// same instant). Signs `host`, `content-type`, `x-amz-date`, and
/// `x-amz-security-token` (when present) plus every caller header except
/// `user-agent` and `authorization` (aws4fetch signs the headers it is
/// given and injects host/date).
pub fn sign(
    input: &AuthInput<'_>,
    credentials: &Credentials,
    service: &str,
    instant: u64,
) -> Result<SignedRequest, AiError> {
    let url = split_url(input.url);
    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, value) in input.headers.iter() {
        let lower = name.to_ascii_lowercase();
        if lower == "user-agent" || lower == "authorization" {
            continue;
        }
        headers.push((lower, value.to_string()));
    }
    headers.push(("host".to_string(), url.host.clone()));
    let amz_date = amz_date(instant);
    headers.push(("x-amz-date".to_string(), amz_date.clone()));
    if let Some(token) = credentials.session_token.as_ref().filter(|t| !t.is_empty()) {
        headers.push(("x-amz-security-token".to_string(), token.clone()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect::<String>();
    let signed_headers =
        headers.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(";");
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        input.method,
        canonical_path(&url.path),
        canonical_query(&url.query),
        canonical_headers,
        signed_headers,
        hex(&Sha256::digest(input.body.as_bytes())),
    );
    let date = &amz_date[..8];
    let scope = format!("{date}/{}/{service}/aws4_request", credentials.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );
    let signing_key =
        signing_key(&credentials.secret_access_key, date, &credentials.region, service);
    let signature = hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        credentials.access_key_id, scope, signed_headers, signature
    );
    let mut out = headers;
    out.push(("authorization".to_string(), authorization));
    Ok(SignedRequest { headers: out })
}

/// The auth definition for a Bedrock route (port of `BedrockAuth.resolveAuth`
/// and `sigV4`): bearer when an API key resolves, else `SigV4` over static
/// or env credentials, failing with the typed v2 message when neither is
/// present.
pub fn resolve_auth(options: &BedrockAuthOptions, service: &str, name: &str) -> Definition {
    let options = options.clone();
    let service = service.to_string();
    let name = name.to_string();
    Definition::new(move |input: &AuthInput<'_>| {
        let bearer_token = match options.mode {
            Some(BedrockAuthMode::SigV4) => None,
            _ => options
                .api_key
                .clone()
                .filter(|key| !key.is_empty())
                .or_else(|| env_non_empty("AWS_BEARER_TOKEN_BEDROCK")),
        };
        if let Some(token) = bearer_token {
            let mut headers = input.headers.clone();
            headers.set("authorization", format!("Bearer {token}"));
            return Ok(headers);
        }
        let region = resolve_region(options.region.as_deref(), options.credentials.as_ref());
        let credentials = match &options.credentials {
            Some(credentials) => {
                let mut credentials = credentials.clone();
                credentials.region = region;
                credentials
            }
            None => env_credentials(&region).map_err(|error| AuthError {
                source: name.clone(),
                message: format!(
                    "{name} requires either route bearer auth or AWS credentials configured on the route ({})"
                    , error.message
                ),
            })?,
        };
        match sigv4::sign(input, &credentials, &service, unix_now()) {
            Ok(signed) => {
                let mut headers = input.headers.clone();
                headers.remove("authorization");
                for (header, value) in signed.headers {
                    headers.set(header, value);
                }
                Ok(headers)
            }
            Err(error) => Err(AuthError {
                source: name.clone(),
                message: format!("{name} SigV4 signing failed: {}", error.message()),
            }),
        }
    })
}

/// Seconds since the Unix epoch (injectable in tests via [`sign`]).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

// =============================================================================
// Canonical-request primitives (AWS SigV4 specification)
// =============================================================================

struct SplitUrl {
    host: String,
    path: String,
    query: Vec<(String, String)>,
}

/// Split `scheme://host/path?query` without a URL crate (the transport seam
/// owns real URL parsing; signing only needs the raw parts).
fn split_url(url: &str) -> SplitUrl {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let authority_start = base.find("://").map_or(0, |position| position + 3);
    let host_end =
        base[authority_start..].find('/').map_or(base.len(), |position| authority_start + position);
    let host = base[authority_start..host_end].to_string();
    let path = if host_end == base.len() { "/".to_string() } else { base[host_end..].to_string() };
    let query = query
        .map(|query| {
            query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| match pair.split_once('=') {
                    Some((key, value)) => (key.to_string(), value.to_string()),
                    None => (pair.to_string(), String::new()),
                })
                .collect()
        })
        .unwrap_or_default();
    SplitUrl { host, path, query }
}

/// RFC 3986 unreserved + the AWS-allowed sub-delims; everything else
/// percent-encodes (uppercase hex).
fn uri_encode(value: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b'/' if !encode_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Canonical URI: each path segment RFC-3986 encoded, `/` preserved
/// (the `aws4fetch` canonicalization).
fn canonical_path(path: &str) -> String {
    let path = if path.is_empty() { "/" } else { path };
    uri_encode(path, false)
}

/// Canonical query string: RFC-3986 encoded key/value pairs sorted by key
/// then value (AWS spec §2); empty query stays empty.
fn canonical_query(query: &[(String, String)]) -> String {
    let mut pairs: Vec<(String, String)> =
        query.iter().map(|(key, value)| (uri_encode(key, true), uri_encode(value, true))).collect();
    pairs.sort();
    pairs.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join("&")
}

/// `YYYYMMDDTHHMMSSZ` from a Unix timestamp (UTC; proleptic Gregorian).
fn amz_date(unix: u64) -> String {
    let days = unix / 86_400;
    let seconds = unix % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// Howard Hinnant's `civil_from_days` (days since 1970-01-01 → y/m/d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// HMAC-SHA256 over plain `sha2` (block-size padding per FIPS 198-1).
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner_block = [0u8; 64];
    let mut outer_block = [0u8; 64];
    for position in 0..64 {
        inner_block[position] = key_block[position] ^ 0x36;
        outer_block[position] = key_block[position] ^ 0x5c;
    }
    let inner = Sha256::new().chain_update(inner_block).chain_update(message);
    let outer = Sha256::new().chain_update(outer_block).chain_update(inner.finalize());
    let digest = outer.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// `SigV4` derived signing key: `HMAC("AWS4"+secret, date) → region → service
/// → "aws4_request"`.
pub fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, service.as_bytes());
    hmac_sha256(&service_key, b"aws4_request")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// =============================================================================
// Tests — the AWS documentation SigV4 vector pins the whole chain
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::auth::Headers;

    fn auth_input<'a>(
        url: &'a str,
        body: &'a str,
        headers: &[(&'a str, &'a str)],
    ) -> AuthInput<'a> {
        auth_input_with_method("POST", url, body, headers)
    }

    fn auth_input_with_method<'a>(
        method: &'a str,
        url: &'a str,
        body: &'a str,
        headers: &[(&'a str, &'a str)],
    ) -> AuthInput<'a> {
        AuthInput {
            request_http: None,
            method,
            url,
            body,
            headers: Headers::from_pairs(
                headers.iter().map(|(k, v)| (k.to_string(), v.to_string())),
            ),
        }
    }

    /// AWS "Signature Version 4" documentation example (GET iam `ListUsers`,
    /// 2015-08-30T12:36:00Z, us-east-1, AKIDEXAMPLE). The fixed instant
    /// reproduces the documented signature byte-for-byte.
    #[test]
    fn signs_the_aws_documentation_vector() {
        let credentials = Credentials {
            region: "us-east-1".into(),
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
        };
        // 2015-08-30T12:36:00Z
        let instant = 1_440_938_160;
        let url = "https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08";
        let content_type = ("content-type", "application/x-www-form-urlencoded; charset=utf-8");
        let authorization = |method: &str| {
            let input = auth_input_with_method(method, url, "", &[content_type]);
            let signed = sign(&input, &credentials, "iam", instant).unwrap();
            signed
                .headers
                .iter()
                .find(|(name, _)| name == "authorization")
                .map(|(_, value)| value.clone())
                .expect("authorization header")
        };
        // The documented GET signature.
        assert_eq!(
            authorization("GET"),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
        // The same request POSTed (cross-checked against an independent
        // python hmac reference).
        assert_eq!(
            authorization("POST"),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=f547bbfb9841d3d1477f36d746d8d5ebe4bc113fecd11ec483363bd2bfcb4164"
        );
    }

    #[test]
    fn session_tokens_join_the_signed_headers() {
        let credentials = Credentials {
            region: "us-east-1".into(),
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: Some("SESSION".into()),
        };
        let input = auth_input(
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/m/converse-stream",
            "{}",
            &[],
        );
        let signed = sign(&input, &credentials, "bedrock", 1_440_942_960).unwrap();
        let authorization = signed
            .headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .map(|(_, value)| value.clone())
            .unwrap();
        assert!(
            authorization.contains("SignedHeaders=host;x-amz-date;x-amz-security-token"),
            "{authorization}"
        );
        let token = signed
            .headers
            .iter()
            .find(|(name, _)| name == "x-amz-security-token")
            .map(|(_, value)| value.clone())
            .unwrap();
        assert_eq!(token, "SESSION");
    }

    #[test]
    fn query_pairs_sort_and_encode_canonically() {
        assert_eq!(canonical_query(&[]), "");
        let query = vec![
            ("b".to_string(), "2".to_string()),
            ("a".to_string(), "z/x y".to_string()),
            ("a".to_string(), "1".to_string()),
        ];
        assert_eq!(canonical_query(&query), "a=1&a=z%2Fx%20y&b=2");
    }

    #[test]
    fn paths_encode_per_segment_keeping_slashes() {
        assert_eq!(canonical_path("/model/a.b:c/d"), "/model/a.b%3Ac/d");
        assert_eq!(canonical_path(""), "/");
    }

    #[test]
    fn amz_date_formats_utc_from_epoch_days() {
        assert_eq!(amz_date(1_440_938_160), "20150830T123600Z");
        assert_eq!(amz_date(0), "19700101T000000Z");
        // 2024-02-29 leap day
        assert_eq!(amz_date(1_709_164_800), "20240229T000000Z");
    }

    #[test]
    fn hmac_sha256_matches_rfc_4231_vector() {
        // RFC 4231 test case 2
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(hex(&mac), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn resolve_region_prefers_explicit_then_credentials_then_env_default() {
        let credentials = Credentials {
            region: "eu-central-1".into(),
            access_key_id: "k".into(),
            secret_access_key: "s".into(),
            session_token: None,
        };
        assert_eq!(resolve_region(Some("us-west-2"), Some(&credentials)), "us-west-2");
        assert_eq!(resolve_region(None, Some(&credentials)), "eu-central-1");
        assert_eq!(resolve_region(None, None), "us-east-1");
    }
}
