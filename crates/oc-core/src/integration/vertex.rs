//! The Google Vertex credential binding (BE-4 item 2): resolves GCP OAuth
//! tokens through the integration credential store — the authorized-user
//! ADC file (`GOOGLE_APPLICATION_CREDENTIALS` / the gCloud well-known path)
//! refreshed against `oauth2.googleapis.com`, the same source v2's
//! google-auth-library ADC chain reads — into the shared
//! [`CredentialBindings`] snapshot as a bearer token for the `google-vertex`
//! provider. Service-account ADC key files mint RS256 self-signed JWT
//! grants (CRED-V3, google-auth-library's JWT client);
//! `GOOGLE_OAUTH_ACCESS_TOKEN` env ADC still wins at
//! request time (see `oc-ai::protocols::vertex::adc_access_token`).

#![allow(clippy::doc_markdown)] // ADC field names in prose stay unquoted

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::copilot::ProviderBinding;
use super::IntegrationHost;

/// The one scope Vertex requests (google-vertex-shared.ts `SCOPE`).
const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// google-auth-library signs one-hour self-signed JWT grants.
const JWT_GRANT_LIFETIME_SECS: i64 = 3600;
const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// The ADC file candidates (google-auth-library's `getClient` order):
/// explicit `GOOGLE_APPLICATION_CREDENTIALS`, then the gCloud well-known
/// path.
fn adc_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(explicit) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        if !explicit.trim().is_empty() {
            paths.push(PathBuf::from(explicit));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        paths.push(
            PathBuf::from(home)
                .join(".config")
                .join("gcloud")
                .join("application_default_credentials.json"),
        );
    }
    paths
}

/// The refreshable ADC shape (authorized_user): client_id + client_secret +
/// refresh_token, exchanged at the OAuth token endpoint.
#[derive(Debug, Clone)]
struct AuthorizedUser {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

/// The service_account ADC shape (CRED-V3): RS256 self-signed JWT
/// grants — `iss` = client_email, `aud` = the token endpoint — signed
/// with the file's PEM private key (google-auth-library's JWT client).
#[derive(Debug, Clone)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    /// The key file's own endpoint (preferred when present).
    token_uri: Option<String>,
}

/// One parsed ADC file.
enum AdcCredential {
    AuthorizedUser(AuthorizedUser),
    ServiceAccount(ServiceAccount),
}

fn parse_adc(content: &str) -> Result<Option<AdcCredential>, String> {
    let value: Value =
        serde_json::from_str(content).map_err(|error| format!("ADC file is not JSON: {error}"))?;
    match value.get("type").and_then(Value::as_str) {
        Some("authorized_user") => Ok(Some(AdcCredential::AuthorizedUser(AuthorizedUser {
            client_id: value
                .get("client_id")
                .and_then(Value::as_str)
                .ok_or("authorized_user ADC file missing client_id")?
                .to_string(),
            client_secret: value
                .get("client_secret")
                .and_then(Value::as_str)
                .ok_or("authorized_user ADC file missing client_secret")?
                .to_string(),
            refresh_token: value
                .get("refresh_token")
                .and_then(Value::as_str)
                .ok_or("authorized_user ADC file missing refresh_token")?
                .to_string(),
        }))),
        Some("service_account") => Ok(Some(AdcCredential::ServiceAccount(ServiceAccount {
            client_email: value
                .get("client_email")
                .and_then(Value::as_str)
                .ok_or("service_account ADC file missing client_email")?
                .to_string(),
            private_key: value
                .get("private_key")
                .and_then(Value::as_str)
                .ok_or("service_account ADC file missing private_key")?
                .to_string(),
            token_uri: value
                .get("token_uri")
                .and_then(Value::as_str)
                .filter(|uri| !uri.is_empty())
                .map(str::to_string),
        }))),
        other => Err(format!("unsupported ADC file type {other:?}")),
    }
}

/// One OAuth token-endpoint grant POST → the access token.
async fn token_grant(
    http: &reqwest::Client,
    token_url: &str,
    form: &[(&str, &str)],
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: Option<String>,
        error_description: Option<String>,
    }
    let response = http
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await
        .map_err(|error| format!("ADC token request failed: {error}"))?;
    let status = response.status();
    let body: TokenResponse = response
        .json()
        .await
        .map_err(|error| format!("ADC token response failed to decode ({status}): {error}"))?;
    body.access_token.filter(|token| !token.is_empty()).ok_or_else(|| {
        format!(
            "ADC token refresh returned no access token{}",
            body.error_description
                .map(|description| format!(": {description}"))
                .unwrap_or_default()
        )
    })
}

async fn refresh(
    http: &reqwest::Client,
    adc: &AuthorizedUser,
    token_url: &str,
) -> Result<String, String> {
    token_grant(
        http,
        token_url,
        &[
            ("client_id", adc.client_id.as_str()),
            ("client_secret", adc.client_secret.as_str()),
            ("refresh_token", adc.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ],
    )
    .await
}

/// The service-account leg (CRED-V3): self-sign an RS256 JWT (`iss` =
/// client_email, `scope`, `aud` = the token endpoint, one-hour window)
/// and exchange it as a jwt-bearer grant.
async fn service_account_token(
    http: &reqwest::Client,
    account: &ServiceAccount,
    fallback_token_url: &str,
) -> Result<String, String> {
    let token_url =
        account.token_uri.as_deref().filter(|uri| !uri.is_empty()).unwrap_or(fallback_token_url);
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system clock is before the epoch: {error}"))?
            .as_secs(),
    )
    .unwrap_or(i64::MAX);
    let header = serde_json::json!({ "alg": "RS256", "typ": "JWT" });
    let claims = serde_json::json!({
        "iss": account.client_email,
        "scope": CLOUD_PLATFORM_SCOPE,
        "aud": token_url,
        "iat": now,
        "exp": now + JWT_GRANT_LIFETIME_SECS,
    });
    let signing_input = format!("{}.{}", jwt_segment(&header), jwt_segment(&claims));
    let signature = sign_rs256(&account.private_key, signing_input.as_bytes())?;
    let assertion = format!("{signing_input}.{signature}");
    token_grant(
        http,
        token_url,
        &[("grant_type", JWT_BEARER_GRANT), ("assertion", assertion.as_str())],
    )
    .await
}

/// One base64url-encoded (unpadded) JSON JWT segment.
fn jwt_segment(value: &Value) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_string().as_bytes())
}

/// RS256 (RSASSA-PKCS1-v1_5 with SHA-256) over `message`, signed with
/// the PKCS#8 PEM private key.
fn sign_rs256(private_key_pem: &str, message: &[u8]) -> Result<String, String> {
    use base64::Engine as _;
    let der = pem_body(private_key_pem)?;
    let key_pair = ring::signature::RsaKeyPair::from_pkcs8(&der)
        .map_err(|error| format!("invalid service account private key: {error:?}"))?;
    let rng = ring::rand::SystemRandom::new();
    let mut signature = vec![0u8; key_pair.public().modulus_len()];
    key_pair
        .sign(&ring::signature::RSA_PKCS1_SHA256, &rng, message, &mut signature)
        .map_err(|error| format!("service account JWT signing failed: {error:?}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature))
}

/// The DER bytes inside a single-block PEM envelope.
fn pem_body(pem: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    let begin = pem.find("-----BEGIN").ok_or("service account private key is not PEM-encoded")?;
    let rest = &pem[begin..];
    let header_end = rest.find('\n').ok_or("malformed PEM header")?;
    let end = rest.find("-----END").ok_or("unterminated PEM block")?;
    if end <= header_end {
        return Err("malformed PEM block".into());
    }
    let base64_text: String =
        rest[header_end + 1..end].chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(base64_text.as_bytes())
        .map_err(|error| format!("service account private key is not valid base64: {error}"))
}

impl IntegrationHost {
    /// The `google-vertex` credential binding (BE-4): the ADC file's
    /// token — authorized-user refresh or service-account JWT grant
    /// (CRED-V3) — as the bearer credential. `None` when no ADC file
    /// exists or its grant fails (env ADC still applies at request
    /// time).
    pub async fn vertex_binding(&self) -> Option<ProviderBinding> {
        self.vertex_binding_with_token_url(TOKEN_URL).await
    }

    /// The binding against an injectable token endpoint (tests replay
    /// the grant through a mock; service-account files prefer their own
    /// `token_uri`).
    pub async fn vertex_binding_with_token_url(&self, token_url: &str) -> Option<ProviderBinding> {
        let _ = CLOUD_PLATFORM_SCOPE; // scope rides both grant types
        let path = adc_paths().into_iter().find(|path| path.is_file())?;
        let content = tokio::fs::read_to_string(&path).await.ok()?;
        let adc = parse_adc(&content).ok()??;
        let token = match &adc {
            AdcCredential::AuthorizedUser(user) => refresh(self.http(), user, token_url).await,
            AdcCredential::ServiceAccount(account) => {
                service_account_token(self.http(), account, token_url).await
            }
        }
        .ok()?;
        Some(ProviderBinding {
            base_url: None,
            // The resolver merges this into the model connection as the
            // explicit access token; a config/env key still wins per v2
            // precedence (apiKey cannot combine with accessToken upstream).
            api_key: token,
            headers: BTreeMap::new(),
        })
    }
}

use std::collections::BTreeMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn adc_json(kind: &str) -> String {
        match kind {
            "authorized_user" => serde_json::json!({
                "type": "authorized_user",
                "client_id": "id.apps.googleusercontent.com",
                "client_secret": "secret",
                "refresh_token": "refresh",
            })
            .to_string(),
            _ => serde_json::json!({
                "type": "service_account",
                "client_email": "sa@test.iam.gserviceaccount.com",
                "private_key": "-----BEGIN PRIVATE KEY-----…",
                "token_uri": "https://oauth2.googleapis.com/token",
            })
            .to_string(),
        }
    }

    #[test]
    fn parses_authorized_user_and_service_account_files() {
        let adc = parse_adc(&adc_json("authorized_user")).unwrap().expect("authorized_user");
        let AdcCredential::AuthorizedUser(user) = &adc else {
            panic!("authorized_user shape");
        };
        assert_eq!(user.client_id, "id.apps.googleusercontent.com");
        assert_eq!(user.refresh_token, "refresh");
        let adc = parse_adc(&adc_json("service_account")).unwrap().expect("service_account");
        let AdcCredential::ServiceAccount(account) = &adc else {
            panic!("service_account shape");
        };
        assert_eq!(account.client_email, "sa@test.iam.gserviceaccount.com");
        assert_eq!(account.private_key, "-----BEGIN PRIVATE KEY-----…");
        assert_eq!(account.token_uri.as_deref(), Some("https://oauth2.googleapis.com/token"));
        assert!(parse_adc("not json").is_err());
        assert!(parse_adc(&serde_json::json!({ "type": "external_account" }).to_string()).is_err());
        // A key file without an email cannot sign its iss claim.
        assert!(parse_adc(
            &serde_json::json!({ "type": "service_account", "private_key": "k" }).to_string()
        )
        .is_err());
    }

    #[test]
    fn pem_body_extracts_der_and_rejects_garbage() {
        assert_eq!(
            pem_body("-----BEGIN PRIVATE KEY-----\nZm9v\n-----END PRIVATE KEY-----\n"),
            Ok(vec![b'f', b'o', b'o'])
        );
        assert!(pem_body("not pem").is_err());
        assert!(pem_body("-----BEGIN PRIVATE KEY-----\n!!!\n-----END PRIVATE KEY-----").is_err());
    }
}
