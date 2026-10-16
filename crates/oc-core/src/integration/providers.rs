//! Built-in integration registrations, 1:1 with v2's plugin space:
//! catalog-wide key/env methods (`plugin/models-dev.ts:33-52`), the `OpenCode`
//! Console device flow (`opencode.ts:88-165`), GitHub Copilot
//! (`github-copilot.ts:13-176`), xAI (`xai.ts`), `DigitalOcean`
//! (`digitalocean.ts`), `Poe` (`poe.ts`).
//!
//! Epoch arithmetic converts provider `expires_in` seconds through `f64`
//! (JSON numbers); the millisecond casts are deliberate.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]

use super::flows::{self, CallbackPage, LoopbackOptions};
use super::types::{
    CredentialValue, FormAnswer, FormCondition, FormField, FormSelectOption, Method, OAuthValue,
};
use super::{AuthMode, Authorization, IntegrationHost, OAuthImplementation};
use futures::future::BoxFuture;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Installs every built-in integration onto the host.
pub fn install(host: &IntegrationHost, catalog: &crate::provider_catalog::catalog::Catalog) {
    install_catalog_methods(host, catalog);
    install_opencode(host);
    install_github_copilot(host);
    install_xai(host);
    install_digitalocean(host);
    install_poe(host);
}

// ---------------------------------------------------------------------------
// Catalog key/env methods (plugin/models-dev.ts:33-52)
// ---------------------------------------------------------------------------

fn install_catalog_methods(
    host: &IntegrationHost,
    catalog: &crate::provider_catalog::catalog::Catalog,
) {
    for snapshot in catalog.providers() {
        if snapshot.environment.is_empty() {
            continue;
        }
        let id = snapshot.info.id.0.clone();
        host.update_integration(&id, &snapshot.info.name);
        host.update_method(&id, Method::Key { label: None, form: None }, None);
        // v2's environmentNames filter (models-dev.ts:77-84): azure
        // advertises project/location/ADC vars that are not keys.
        let names = if id == "azure" {
            let mut filtered: Vec<String> = snapshot
                .environment
                .iter()
                .filter(|name| name.ends_with("_API_KEY"))
                .cloned()
                .collect();
            filtered.push("AZURE_COGNITIVE_SERVICES_API_KEY".to_owned());
            filtered
        } else {
            snapshot.environment.clone()
        };
        host.update_method(&id, Method::Env { names }, None);
    }
}

// ---------------------------------------------------------------------------
// OpenCode Console / zen (plugin/provider/opencode.ts)
// ---------------------------------------------------------------------------

const OPENCODE_CLIENT_ID: &str = "opencode-cli";
const OPENCODE_DEFAULT_SERVER: &str = "https://opencode.ai/console";

fn install_opencode(host: &IntegrationHost) {
    host.update_integration("opencode", "OpenCode Console");
    let form = vec![FormField {
        type_: "string".into(),
        key: "server".into(),
        title: None,
        description: None,
        placeholder: None,
        default: Some(Value::String(OPENCODE_DEFAULT_SERVER.into())),
        pattern: None,
        required: false,
        hidden: true,
        options: None,
        when: None,
    }];
    host.update_method(
        "opencode",
        Method::OAuth {
            id: "device".into(),
            label: "OpenCode Console account".into(),
            form: Some(form),
        },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(opencode_authorize(answer))),
            refresh: Some(Arc::new(|value| Box::pin(opencode_refresh(value)))),
            label: Some(Arc::new(opencode_label)),
        }),
    );
    host.update_method(
        "opencode",
        Method::Key { label: Some("API key (service account)".into()), form: None },
        None,
    );
}

/// `answer.server` normalized (no trailing slash).
fn opencode_server(answer: &FormAnswer) -> String {
    let server = answer
        .get("server")
        .and_then(Value::as_str)
        .filter(|server| !server.is_empty())
        .unwrap_or(OPENCODE_DEFAULT_SERVER);
    server.trim_end_matches('/').to_owned()
}

async fn opencode_authorize(answer: FormAnswer) -> Result<Authorization, String> {
    let server = opencode_server(&answer);
    let http = reqwest::Client::new();
    let device = flows::request_device_code(
        &http,
        &format!("{server}/auth/device/code"),
        vec![
            ("client_id", OPENCODE_CLIENT_ID.to_owned()),
            ("supports_org_scope", "true".to_owned()),
        ],
    )
    .await?;
    // verification_uri_complete resolved against the server root
    // (opencode.ts:118-127: `new URL(complete, server + "/")`): full URLs
    // pass through, absolute paths rebase, bare paths append.
    let verification = match device.verification_uri_complete.as_deref() {
        Some(complete) if complete.starts_with("http://") || complete.starts_with("https://") => {
            complete.to_owned()
        }
        Some(complete) if complete.starts_with('/') => format!("{server}{complete}"),
        Some(complete) => format!("{server}/{complete}"),
        None => device.verification_uri.clone(),
    };
    let interval = device.interval.unwrap_or(5.0);
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = {
        let http = http.clone();
        let endpoint = format!("{server}/auth/device/token");
        Box::pin(async move {
            flows::run_device_poll(
                http,
                endpoint,
                OPENCODE_CLIENT_ID.to_owned(),
                device.device_code,
                interval,
                |body| async move {
                    let access =
                        body["access_token"].as_str().ok_or("access_token missing")?.to_owned();
                    let refresh =
                        body["refresh_token"].as_str().ok_or("refresh_token missing")?.to_owned();
                    let expires_in = body["expires_in"].as_f64().unwrap_or(3600.0);
                    let mut metadata = BTreeMap::new();
                    if let Some(org_id) = body["org_id"].as_str() {
                        metadata.insert("orgID".to_owned(), Value::String(org_id.to_owned()));
                        metadata.insert("orgName".to_owned(), Value::String(org_id.to_owned()));
                    }
                    Ok(CredentialValue::OAuth(OAuthValue {
                        method_id: "device".into(),
                        refresh,
                        access,
                        expires: crate::database::migrations::now_ms()
                            + (expires_in * 1000.0) as i64,
                        metadata: if metadata.is_empty() { None } else { Some(metadata) },
                    }))
                },
            )
            .await
        })
    };
    Ok(Authorization {
        url: verification,
        instructions: format!("Enter code: {}", device.user_code),
        expires_at: device
            .expires_in
            .map(|seconds| crate::database::migrations::now_ms() + (seconds * 1000.0) as i64),
        mode: AuthMode::Auto(callback),
    })
}

async fn opencode_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    let server = value
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("server"))
        .and_then(Value::as_str)
        .map_or_else(|| OPENCODE_DEFAULT_SERVER.to_owned(), String::from);
    let http = reqwest::Client::new();
    let response = http
        .post(format!("{server}/auth/device/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", value.refresh.as_str()),
            ("client_id", OPENCODE_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    if response.status().as_u16() == 400 {
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if body["error"] == "invalid_grant" {
            return Err("Reconnect OpenCode Console to continue".into());
        }
        return Err(format!("refresh failed: {}", body["error"].as_str().unwrap_or("unknown")));
    }
    let token: Value = response.json().await.map_err(|error| format!("bad token body: {error}"))?;
    let access = token["access_token"].as_str().ok_or("access_token missing")?.to_owned();
    let refresh = token["refresh_token"].as_str().ok_or("refresh_token missing")?.to_owned();
    let expires_in = token["expires_in"].as_f64().unwrap_or(3600.0);
    let mut metadata = value.metadata.clone().unwrap_or_default();
    if let Some(org_id) = token["org_id"].as_str() {
        let existing_name = metadata.get("orgName").and_then(Value::as_str);
        let name = match existing_name {
            Some(name) if metadata.get("orgID").and_then(Value::as_str) == Some(org_id) => {
                name.to_owned()
            }
            _ => org_id.to_owned(),
        };
        metadata.insert("orgID".into(), Value::String(org_id.to_owned()));
        metadata.insert("orgName".into(), Value::String(name));
    }
    Ok(CredentialValue::OAuth(OAuthValue {
        method_id: value.method_id,
        refresh,
        access,
        expires: crate::database::migrations::now_ms() + (expires_in * 1000.0) as i64,
        metadata: if metadata.is_empty() { None } else { Some(metadata) },
    }))
}

fn opencode_label(value: &CredentialValue) -> Option<String> {
    let CredentialValue::OAuth(oauth) = value else { return None };
    oauth
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("orgName"))
        .and_then(Value::as_str)
        .map(String::from)
}

// ---------------------------------------------------------------------------
// GitHub Copilot (plugin/provider/github-copilot.ts)
// ---------------------------------------------------------------------------

const COPILOT_CLIENT_ID: &str = "Ov23li8tweQw6odWQebz";
const COPILOT_SCOPE: &str = "read:user";
const COPILOT_USER_API_VERSION: &str = "2025-04-01";
const COPILOT_API_VERSION: &str = "2026-08-01";

fn install_github_copilot(host: &IntegrationHost) {
    host.update_integration("github-copilot", "GitHub Copilot");
    let form = vec![
        FormField {
            type_: "string".into(),
            key: "deploymentType".into(),
            title: Some("Select GitHub deployment type".into()),
            description: None,
            placeholder: None,
            default: None,
            pattern: None,
            required: true,
            hidden: false,
            options: Some(vec![
                FormSelectOption {
                    label: "GitHub.com".into(),
                    value: "github.com".into(),
                    description: Some("Public".into()),
                },
                FormSelectOption {
                    label: "GitHub Enterprise".into(),
                    value: "enterprise".into(),
                    description: Some("Data residency or self-hosted".into()),
                },
            ]),
            when: None,
        },
        FormField {
            type_: "string".into(),
            key: "enterpriseUrl".into(),
            title: Some("Enter your GitHub Enterprise URL or domain".into()),
            description: None,
            placeholder: Some("company.ghe.com or https://company.ghe.com".into()),
            default: None,
            pattern: None,
            required: true,
            hidden: false,
            options: None,
            when: Some(vec![FormCondition {
                key: "deploymentType".into(),
                op: "eq".into(),
                value: "enterprise".into(),
            }]),
        },
    ];
    host.update_method(
        "github-copilot",
        Method::OAuth {
            id: "device".into(),
            label: "Login with GitHub Copilot".into(),
            form: Some(form),
        },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(copilot_authorize(answer))),
            refresh: None, // copilot tokens carry no refresh (github-copilot.ts:126-171)
            label: None,
        }),
    );
    // v2 removes the catalog key method (github-copilot.ts:213).
    host.remove_method("github-copilot", &Method::Key { label: None, form: None });
}

/// `normalizeDomain` (github-copilot.ts): accept a URL or bare domain. An
/// explicit `http(s)://` origin (self-hosted GHE, loopback tests) is kept
/// verbatim — scheme, host, and port.
fn copilot_domain(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_owned();
    }
    trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned()
}

fn copilot_urls(domain: &str) -> (String, String, String) {
    if domain.starts_with("http://") || domain.starts_with("https://") {
        let origin = domain.trim_end_matches('/');
        return (
            format!("{origin}/login/device/code"),
            format!("{origin}/login/oauth/access_token"),
            origin.to_owned(),
        );
    }
    let base = if domain == "github.com" {
        "https://github.com".to_owned()
    } else {
        format!("https://{domain}")
    };
    let api = if domain == "github.com" {
        "https://api.github.com".to_owned()
    } else {
        format!("https://api.{domain}")
    };
    (format!("{base}/login/device/code"), format!("{base}/login/oauth/access_token"), api)
}

fn copilot_entitlement_error(user: &Value) -> Option<String> {
    // github-copilot.ts copilotEntitlementError: an explicit false or a
    // limited-signup block denies login; a missing field never does.
    if user["chat_enabled"].as_bool() == Some(false) {
        return Some("GitHub Copilot not enabled on your account".into());
    }
    if user["error"].as_str().is_some_and(|error| error.contains("copilot")) {
        return Some(format!(
            "GitHub Copilot entitlement error: {}",
            user["error"].as_str().unwrap_or_default()
        ));
    }
    None
}

async fn copilot_authorize(answer: FormAnswer) -> Result<Authorization, String> {
    let enterprise = answer.get("deploymentType").and_then(Value::as_str) == Some("enterprise");
    let enterprise_url = answer.get("enterpriseUrl").and_then(Value::as_str);
    if enterprise && enterprise_url.is_none() {
        return Err("Enterprise URL is required".into());
    }
    let domain = if enterprise {
        copilot_domain(enterprise_url.unwrap_or_default())
    } else {
        "github.com".to_owned()
    };
    let (device_endpoint, token_endpoint, api) = copilot_urls(&domain);
    let http = reqwest::Client::new();
    let device = flows::request_device_code(
        &http,
        &device_endpoint,
        vec![("client_id", COPILOT_CLIENT_ID.to_owned()), ("scope", COPILOT_SCOPE.to_owned())],
    )
    .await?;
    let interval = device.interval.unwrap_or(5.0);
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = {
        let http = http.clone();
        let token_endpoint = token_endpoint.clone();
        let api = api.clone();
        let domain = domain.clone();
        Box::pin(async move {
            flows::run_device_poll(
                http.clone(),
                token_endpoint,
                COPILOT_CLIENT_ID.to_owned(),
                device.device_code,
                interval,
                {
                    let http = http.clone();
                    let api = api.clone();
                    let domain = domain.clone();
                    move |body| {
                        let http = http.clone();
                        let api = api.clone();
                        let domain = domain.clone();
                        async move {
                            let access = body["access_token"]
                                .as_str()
                                .ok_or("access_token missing")?
                                .to_owned();
                            // Entitlement check (github-copilot.ts:84-119): only
                            // an explicit denial blocks login; a failed or
                            // malformed lookup must not.
                            let user = match http
                                .get(format!("{api}/copilot_internal/user"))
                                .header("Authorization", format!("Bearer {access}"))
                                .header("Accept", "application/json")
                                .header("X-GitHub-Api-Version", COPILOT_USER_API_VERSION)
                                .send()
                                .await
                            {
                                Ok(response) if response.status().is_success() => {
                                    response.json::<Value>().await.unwrap_or(Value::Null)
                                }
                                _ => Value::Null,
                            };
                            let _ = COPILOT_API_VERSION; // transport header (copilot models sync)
                            if let Some(denied) = copilot_entitlement_error(&user) {
                                return Err(denied);
                            }
                            let api_endpoint = user["endpoints"]["api"]
                                .as_str()
                                .map(|endpoint| endpoint.trim_end_matches('/').to_owned());
                            let mut metadata = BTreeMap::new();
                            if enterprise {
                                metadata.insert("enterpriseUrl".into(), Value::String(domain));
                            }
                            if let Some(endpoint) = api_endpoint {
                                metadata.insert("apiEndpoint".into(), Value::String(endpoint));
                            }
                            Ok(CredentialValue::OAuth(OAuthValue {
                                method_id: "device".into(),
                                refresh: access.clone(),
                                access,
                                expires: 0, // never: the token is exchanged per-use (github-copilot.ts:131-146)
                                metadata: if metadata.is_empty() { None } else { Some(metadata) },
                            }))
                        }
                    }
                },
            )
            .await
        })
    };
    Ok(Authorization {
        url: device.verification_uri.clone(),
        instructions: format!("Enter code: {}", device.user_code),
        expires_at: device
            .expires_in
            .map(|seconds| crate::database::migrations::now_ms() + (seconds * 1000.0) as i64),
        mode: AuthMode::Auto(callback),
    })
}

// ---------------------------------------------------------------------------
// xAI (plugin/provider/xai.ts)
// ---------------------------------------------------------------------------

const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_ISSUER: &str = "https://auth.x.ai/oauth2";
const XAI_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";

fn install_xai(host: &IntegrationHost) {
    host.update_integration("xai", "xAI");
    host.update_method(
        "xai",
        Method::OAuth { id: "device".into(), label: "SuperGrok Subscription".into(), form: None },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(xai_authorize(answer))),
            refresh: Some(Arc::new(|value| Box::pin(xai_refresh(value)))),
            label: None,
        }),
    );
    host.update_method(
        "xai",
        Method::Key { label: Some("Manually enter API Key".into()), form: None },
        None,
    );
}

async fn xai_authorize(_answer: FormAnswer) -> Result<Authorization, String> {
    let http = reqwest::Client::new();
    let device = flows::request_device_code(
        &http,
        &format!("{XAI_ISSUER}/device/code"),
        vec![
            ("client_id", XAI_CLIENT_ID.to_owned()),
            ("scope", XAI_SCOPE.to_owned()),
            ("referrer", "opencode".to_owned()),
        ],
    )
    .await?;
    let interval = device.interval.unwrap_or(5.0);
    let lifetime = device.expires_in.filter(|seconds| *seconds > 0.0);
    let url =
        device.verification_uri_complete.clone().unwrap_or_else(|| device.verification_uri.clone());
    let instructions = format!(
        "Open {} on any device and enter code: {}",
        device.verification_uri, device.user_code
    );
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = {
        let http = http.clone();
        Box::pin(async move {
            flows::run_device_poll(
                http,
                format!("{XAI_ISSUER}/token"),
                XAI_CLIENT_ID.to_owned(),
                device.device_code,
                interval,
                |body| async move {
                    let access =
                        body["access_token"].as_str().ok_or("access_token missing")?.to_owned();
                    let refresh = body["refresh_token"].as_str().unwrap_or_default().to_owned();
                    let expires_in = body["expires_in"].as_f64().unwrap_or(3600.0);
                    Ok(CredentialValue::OAuth(OAuthValue {
                        method_id: "device".into(),
                        refresh,
                        access,
                        expires: crate::database::migrations::now_ms()
                            + (expires_in * 1000.0) as i64,
                        metadata: None,
                    }))
                },
            )
            .await
        })
    };
    Ok(Authorization {
        url,
        instructions,
        expires_at: lifetime
            .map(|seconds| crate::database::migrations::now_ms() + (seconds * 1000.0) as i64),
        mode: AuthMode::Auto(callback),
    })
}

async fn xai_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    let http = reqwest::Client::new();
    let response = http
        .post(format!("{XAI_ISSUER}/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", value.refresh.as_str()),
            ("client_id", XAI_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    if !response.status().is_success() {
        let body: Value = response.json().await.unwrap_or(Value::Null);
        return Err(format!(
            "Failed to refresh xAI token: {}",
            body["error_description"].as_str().or(body["error"].as_str()).unwrap_or("unknown")
        ));
    }
    let token: Value = response.json().await.map_err(|error| format!("bad token body: {error}"))?;
    let access = token["access_token"].as_str().ok_or("access_token missing")?.to_owned();
    let refresh = token["refresh_token"].as_str().unwrap_or(&value.refresh).to_owned();
    let expires_in = token["expires_in"].as_f64().unwrap_or(3600.0);
    Ok(CredentialValue::OAuth(OAuthValue {
        method_id: value.method_id,
        refresh,
        access,
        expires: crate::database::migrations::now_ms() + (expires_in * 1000.0) as i64,
        metadata: None,
    }))
}

/// url-encoded query string from pairs (`a=1&b=2`).
fn query_string(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", urlencoding::encode(key), urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

// ---------------------------------------------------------------------------
// DigitalOcean (plugin/provider/digitalocean.ts) — implicit loopback flow
// ---------------------------------------------------------------------------

const DO_CLIENT_ID: &str = "b1a6c5158156caac821fd1b30253ca8acb52454a48fa744420e41889cb589f82";
const DO_AUTHORIZE: &str = "https://cloud.digitalocean.com/v1/oauth/authorize";
const DO_CALLBACK_PORT: u16 = 1456;
const DO_SCOPE: &str = "genai:read inference:query";

fn install_digitalocean(host: &IntegrationHost) {
    host.update_integration("digitalocean", "DigitalOcean");
    host.update_method(
        "digitalocean",
        Method::OAuth { id: "browser".into(), label: "Login with DigitalOcean".into(), form: None },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(digitalocean_authorize(answer))),
            refresh: None, // implicit tokens are not refreshable (digitalocean.ts:40-118)
            label: None,
        }),
    );
}

async fn digitalocean_authorize(_answer: FormAnswer) -> Result<Authorization, String> {
    let state = flows::random_state(32);
    let listener = flows::listen_loopback(LoopbackOptions {
        port: DO_CALLBACK_PORT,
        host: "localhost".into(),
        callback_path: "/auth/callback",
        page: CallbackPage::Bootstrap {
            token_path: "/auth/token".into(),
            provider: "DigitalOcean".into(),
        },
    })
    .await
    .map_err(|error| format!("failed to bind callback port {DO_CALLBACK_PORT}: {error}"))?;
    let redirect = format!("http://localhost:{}/auth/callback", listener.port);
    let url = format!(
        "{DO_AUTHORIZE}?{}",
        query_string(&[
            ("response_type", "token"),
            ("client_id", DO_CLIENT_ID),
            ("redirect_uri", redirect.as_str()),
            ("scope", DO_SCOPE),
            ("state", state.as_str()),
        ])
    );
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let params = listener.wait().await?;
        let error = params.get("error_description").or_else(|| params.get("error")).cloned();
        if let Some(error) = error {
            return Err(error);
        }
        let access = params.get("access_token").ok_or("Missing access token")?;
        if params.get("state").map(String::as_str) != Some(state.as_str()) {
            return Err("Invalid OAuth state".into());
        }
        let expires_in: f64 = params
            .get("expires_in")
            .and_then(|value| value.parse().ok())
            .unwrap_or(60.0 * 60.0 * 24.0 * 30.0);
        Ok(CredentialValue::OAuth(OAuthValue {
            method_id: "browser".into(),
            refresh: String::new(),
            access: access.clone(),
            expires: crate::database::migrations::now_ms() + (expires_in * 1000.0) as i64,
            metadata: None,
        }))
    });
    Ok(Authorization {
        url,
        instructions:
            "Complete authorization in your browser. This window will close automatically.".into(),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

// ---------------------------------------------------------------------------
// Poe (plugin/provider/poe.ts) — code + PKCE loopback flow
// ---------------------------------------------------------------------------

const POE_CLIENT_ID: &str = "9rKyfhAcBR0rEmvPpKcVxpDzqEBzZPPK";
const POE_ISSUER: &str = "https://poe.com";
const POE_TOKEN: &str = "https://api.poe.com/token";
const POE_SCOPE: &str = "apikey:create";

fn install_poe(host: &IntegrationHost) {
    host.update_integration("poe", "Poe");
    host.update_method(
        "poe",
        Method::OAuth {
            id: "browser".into(),
            label: "Login with Poe (browser)".into(),
            form: None,
        },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(poe_authorize(answer))),
            // Poe-issued keys remain usable until expiry, then require
            // another login (poe.ts:29-38).
            refresh: Some(Arc::new(|value| Box::pin(poe_refresh(value)))),
            label: None,
        }),
    );
}

async fn poe_authorize(_answer: FormAnswer) -> Result<Authorization, String> {
    let PkcePair { verifier, challenge } = pkce_pair();
    let state = flows::random_state(32);
    let listener = flows::listen_loopback(LoopbackOptions {
        port: 0,
        host: "127.0.0.1".into(),
        callback_path: "/callback",
        page: CallbackPage::Html(flows::success_page("Poe")),
    })
    .await
    .map_err(|error| format!("failed to bind loopback listener: {error}"))?;
    let redirect = format!("http://127.0.0.1:{}/callback", listener.port);
    let url = format!(
        "{}/oauth/authorize?{}",
        POE_ISSUER,
        query_string(&[
            ("response_type", "code"),
            ("client_id", POE_CLIENT_ID),
            ("redirect_uri", redirect.as_str()),
            ("scope", POE_SCOPE),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
        ])
    );
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let params = listener.wait().await?;
        if let Some(error) = params.get("error").or_else(|| params.get("error_description")) {
            return Err(error.clone());
        }
        if params.get("state").map(String::as_str) != Some(state.as_str()) {
            return Err("Invalid OAuth state".into());
        }
        let code = params.get("code").ok_or("missing code")?;
        let http = reqwest::Client::new();
        let response = http
            .post(POE_TOKEN)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", POE_CLIENT_ID),
                ("code", code.as_str()),
                ("redirect_uri", redirect.as_str()),
                ("code_verifier", verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|error| format!("request failed: {error}"))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(format!(
                "Failed to exchange Poe code: {}",
                body["error_description"].as_str().or(body["error"].as_str()).unwrap_or("unknown")
            ));
        }
        let access = body["access_token"].as_str().ok_or("access_token missing")?.to_owned();
        let expires_in = body["expires_in"].as_f64().unwrap_or(30.0 * 24.0 * 3600.0);
        Ok(CredentialValue::OAuth(OAuthValue {
            method_id: "browser".into(),
            refresh: String::new(),
            access,
            expires: crate::database::migrations::now_ms() + (expires_in * 1000.0) as i64,
            metadata: None,
        }))
    });
    Ok(Authorization {
        url,
        instructions:
            "Complete authorization in your browser. This window will close automatically.".into(),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

struct PkcePair {
    verifier: String,
    challenge: String,
}

fn pkce_pair() -> PkcePair {
    let pkce = flows::pkce_s256();
    PkcePair { verifier: pkce.verifier, challenge: pkce.challenge }
}

#[allow(
    clippy::unused_async,
    clippy::unused_async_trait_impl,
    reason = "async surface parity with integration.ts"
)]
async fn poe_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    if value.expires > crate::database::migrations::now_ms() {
        Ok(CredentialValue::OAuth(value))
    } else {
        Err("Poe API key expired. Log in with Poe again.".into())
    }
}
