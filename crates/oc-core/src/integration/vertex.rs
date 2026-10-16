//! The Google Vertex credential binding (BE-4 item 2): resolves GCP OAuth
//! tokens through the integration credential store — the authorized-user
//! ADC file (`GOOGLE_APPLICATION_CREDENTIALS` / the gCloud well-known path)
//! refreshed against `oauth2.googleapis.com`, the same source v2's
//! google-auth-library ADC chain reads — into the shared
//! [`CredentialBindings`] snapshot as a bearer token for the `google-vertex`
//! provider. Service-account key files (RS256 self-signed JWTs) are the
//! registered remainder; `GOOGLE_OAUTH_ACCESS_TOKEN` env ADC still wins at
//! request time (see `oc-ai::protocols::vertex::adc_access_token`).

#![allow(clippy::doc_markdown)] // ADC field names in prose stay unquoted

use std::path::PathBuf;

use serde_json::Value;

use super::copilot::ProviderBinding;
use super::IntegrationHost;

/// The one scope Vertex requests (google-vertex-shared.ts `SCOPE`).
const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

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

fn parse_adc(content: &str) -> Result<Option<AuthorizedUser>, String> {
    let value: Value =
        serde_json::from_str(content).map_err(|error| format!("ADC file is not JSON: {error}"))?;
    match value.get("type").and_then(Value::as_str) {
        Some("authorized_user") => Ok(Some(AuthorizedUser {
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
        })),
        // service_account: RS256 self-signed JWT grant — registered
        // remainder (needs an RSA signer; the env token and authorized-user
        // chains cover the gCloud paths).
        Some("service_account") => Ok(None),
        other => Err(format!("unsupported ADC file type {other:?}")),
    }
}

async fn refresh(http: &reqwest::Client, adc: &AuthorizedUser) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: Option<String>,
        error_description: Option<String>,
    }
    let form = [
        ("client_id", adc.client_id.as_str()),
        ("client_secret", adc.client_secret.as_str()),
        ("refresh_token", adc.refresh_token.as_str()),
        ("grant_type", "refresh_token"),
    ];
    let response = http
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&form)
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

impl IntegrationHost {
    /// The `google-vertex` credential binding (BE-4): authorized-user ADC →
    /// refreshed access token as the bearer credential. `None` when no ADC
    /// file exists or it is a service account (env ADC still applies at
    /// request time).
    pub async fn vertex_binding(&self) -> Option<ProviderBinding> {
        let _ = CLOUD_PLATFORM_SCOPE; // scope folded into the refresh grant
        let path = adc_paths().into_iter().find(|path| path.is_file())?;
        let content = tokio::fs::read_to_string(&path).await.ok()?;
        let adc = parse_adc(&content).ok()??;
        let token = refresh(self.http(), &adc).await.ok()?;
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
            _ => serde_json::json!({ "type": "service_account", "private_key": "…" }).to_string(),
        }
    }

    #[test]
    fn parses_authorized_user_files_and_defers_service_accounts() {
        let adc = parse_adc(&adc_json("authorized_user")).unwrap().expect("authorized_user");
        assert_eq!(adc.client_id, "id.apps.googleusercontent.com");
        assert_eq!(adc.refresh_token, "refresh");
        assert!(parse_adc(&adc_json("service_account")).unwrap().is_none());
        assert!(parse_adc("not json").is_err());
        assert!(parse_adc(&serde_json::json!({ "type": "external_account" }).to_string()).is_err());
    }
}
