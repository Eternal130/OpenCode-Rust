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
    super::providers_enterprise::install(host);
}

// ---------------------------------------------------------------------------
// Catalog key/env methods (plugin/models-dev.ts:33-52)
// ---------------------------------------------------------------------------

pub(crate) fn install_catalog_methods(
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
        // v2's environmentNames filter (models-dev.ts:77-87): providers
        // advertise env vars that configure auth rather than carry a key
        // — azure (project/location/ADC), google-vertex (project,
        // location, ADC file), cloudflare-workers-ai (account/token
        // scoping), and bedrock (SigV4 chain) — only the listed names
        // may become env credentials.
        let names = match id.as_str() {
            "azure" => {
                let mut filtered: Vec<String> = snapshot
                    .environment
                    .iter()
                    .filter(|name| name.ends_with("_API_KEY"))
                    .cloned()
                    .collect();
                filtered.push("AZURE_COGNITIVE_SERVICES_API_KEY".to_owned());
                filtered
            }
            // GoogleVertexPlugin handles activation/credentials
            // (google-vertex.ts) — only the Express Mode key is a
            // credential.
            "google-vertex" => vec!["GOOGLE_VERTEX_API_KEY".to_owned()],
            "cloudflare-workers-ai" => vec![
                "CLOUDFLARE_API_KEY".to_owned(),
                "CLOUDFLARE_WORKERS_AI_TOKEN".to_owned(),
                "CLOUDFLARE_API_TOKEN".to_owned(),
            ],
            // AmazonBedrockPlugin (amazon-bedrock.ts:22-27): models.dev
            // advertises AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY/
            // AWS_REGION alongside the bearer token; only the bearer
            // token is a key — the rest feed the SigV4 chain.
            "amazon-bedrock" => vec!["AWS_BEARER_TOKEN_BEDROCK".to_owned()],
            _ => snapshot.environment.clone(),
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
        format: None,
        min_length: None,
        max_length: None,
        custom: None,
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
    let http = oc_http::client();
    // G-244 — JSON body (`send()` schemaBodyJson, opencode.ts:107-113,
    // 644-650): `supports_org_scope` is a JSON boolean.
    let device = flows::request_device_code_json(
        &http,
        &format!("{server}/auth/device/code"),
        serde_json::json!({
            "client_id": OPENCODE_CLIENT_ID,
            "supports_org_scope": true,
        }),
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
        let endpoint = format!("{server}/auth/device/token");
        let server = server.clone();
        Box::pin(async move {
            // G-244 — sleep-first polling (opencode.ts:567-570).
            let decode_http = http.clone();
            flows::run_device_poll_zen(
                http,
                endpoint,
                OPENCODE_CLIENT_ID.to_owned(),
                device.device_code,
                interval,
                move |body| {
                    let http = decode_http.clone();
                    let server = server.clone();
                    async move { opencode_credential(&http, &server, body).await }
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

/// G-244 — the zen credential assembly (opencode.ts:592-623): after the
/// device exchange, two concurrent GETs (`/api/user`, `/api/orgs`) with
/// the fresh access token; the org resolves by `org_id` (a listed id
/// that is absent from `/api/orgs` is a hard error) or the name/id-
/// sorted first org; metadata records ALL FIVE keys
/// (server/accountID/email/orgID/orgName) so refresh and the console
/// config fetch keep hitting the SAME (self-hosted) server.
async fn opencode_credential(
    http: &reqwest::Client,
    server: &str,
    body: Value,
) -> Result<CredentialValue, String> {
    let access = body["access_token"].as_str().ok_or("access_token missing")?.to_owned();
    let refresh = body["refresh_token"].as_str().ok_or("refresh_token missing")?.to_owned();
    let expires_in = body["expires_in"].as_f64().unwrap_or(3600.0);
    // Effect.all(..., { concurrency: 2 }) — the two discovery GETs ride
    // the same request headers (Accept + User-Agent, bearer token).
    let headers = |request: reqwest::RequestBuilder| {
        request
            .header("Accept", "application/json")
            .header("User-Agent", flows::app_user_agent())
            .header("Authorization", format!("Bearer {access}"))
    };
    let (user, orgs) = tokio::join!(
        headers(http.get(format!("{server}/api/user"))).send(),
        headers(http.get(format!("{server}/api/orgs"))).send(),
    );
    let user: Value = user
        .map_err(|error| format!("request failed: {error}"))?
        .json()
        .await
        .map_err(|error| format!("bad user body: {error}"))?;
    let orgs: Value = orgs
        .map_err(|error| format!("request failed: {error}"))?
        .json()
        .await
        .map_err(|error| format!("bad orgs body: {error}"))?;
    let account_id = user["id"].as_str().ok_or("user id missing")?.to_owned();
    let email = user["email"].as_str().ok_or("email missing")?.to_owned();
    let orgs_array = orgs.as_array().ok_or("bad orgs body")?;
    let mut org_list = Vec::with_capacity(orgs_array.len());
    for org in orgs_array {
        let id = org["id"].as_str().ok_or("org id missing")?.to_owned();
        let name = org["name"].as_str().ok_or("org name missing")?.to_owned();
        org_list.push((id, name));
    }
    // Org resolution (opencode.ts:601-607): `org_id` must match a listed
    // org; without one the name-then-id sorted first org wins.
    let org = if let Some(org_id) = body["org_id"].as_str() {
        let Some(found) = org_list.iter().find(|(id, _)| id == org_id) else {
            return Err(format!("OpenCode organization not found: {org_id}"));
        };
        Some(found.clone())
    } else {
        org_list.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        org_list.first().cloned()
    };
    let mut metadata = BTreeMap::new();
    metadata.insert("server".to_owned(), Value::String(server.to_owned()));
    metadata.insert("accountID".to_owned(), Value::String(account_id));
    metadata.insert("email".to_owned(), Value::String(email));
    if let Some((org_id, org_name)) = org {
        metadata.insert("orgID".to_owned(), Value::String(org_id));
        metadata.insert("orgName".to_owned(), Value::String(org_name));
    }
    Ok(CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh,
        access,
        expires: crate::database::migrations::now_ms() + (expires_in * 1000.0) as i64,
        metadata: Some(metadata),
    }))
}

async fn opencode_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    let server = value
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("server"))
        .and_then(Value::as_str)
        .map_or_else(|| OPENCODE_DEFAULT_SERVER.to_owned(), String::from);
    let http = oc_http::client();
    let response = http
        .post(format!("{server}/auth/device/token"))
        // G-234① — the ref headers() set (Accept + User-Agent) on the
        // refresh post too; G-244 — the body is JSON (opencode.ts:644-650
        // schemaBodyJson), not form-encoded.
        .header("Accept", "application/json")
        .header("User-Agent", flows::app_user_agent())
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": value.refresh,
            "client_id": OPENCODE_CLIENT_ID,
        }))
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
            format: None,
            min_length: None,
            max_length: None,
            custom: None,
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
            format: None,
            min_length: None,
            max_length: None,
            custom: None,
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

/// `pub(super)` — unit-tested directly (G-284③ dual messages).
pub(super) fn copilot_entitlement_error(user: &Value) -> Option<String> {
    // github-copilot.ts:365-371 copilotEntitlementError: ONLY an explicit
    // `chat_enabled === false` denies (first), and the message picks by
    // `can_signup_for_limited`. G-284③ — the self-made
    // `error.contains("copilot")` branch could reverse-reject (any
    // account whose lookup error mentions copilot) and is gone.
    if user["chat_enabled"].as_bool() != Some(false) {
        return None;
    }
    if user["can_signup_for_limited"].as_bool() == Some(true) {
        return Some(
            "This GitHub account is not signed up for GitHub Copilot. Sign up for Copilot Free at https://github.com/features/copilot/plans and connect again."
                .into(),
        );
    }
    Some(
        "This GitHub account does not have GitHub Copilot access. It needs an active Copilot subscription or a seat assigned by an organization."
            .into(),
    )
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
    let http = oc_http::client();
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
    let http = oc_http::client();
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
                    // G-284② — a missing refresh_token is a hard error at
                    // login (xai.ts:190-199: both the response token and
                    // any fallback absent → fail, never store empty).
                    let refresh = body["refresh_token"]
                        .as_str()
                        .filter(|token| !token.is_empty())
                        .ok_or("xAI token response is missing refresh_token")?
                        .to_owned();
                    let expires = xai_token_expiration(body["expires_in"].as_f64(), &access);
                    Ok(CredentialValue::OAuth(OAuthValue {
                        method_id: "device".into(),
                        refresh,
                        access,
                        expires,
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

/// G-284② — `tokenExpiration` (xai.ts:217-228): `expires_in` (positive)
/// wins; otherwise the access token's JWT `exp` claim; otherwise
/// now + 3600s.
/// `pub(super)` — unit-tested directly (G-284② JWT exp fallback).
pub(super) fn xai_token_expiration(expires_in: Option<f64>, access_token: &str) -> i64 {
    if let Some(expires_in) = expires_in.filter(|seconds| *seconds > 0.0) {
        return crate::database::migrations::now_ms() + (expires_in * 1000.0) as i64;
    }
    if let Some(exp) = flows::jwt_claims(access_token)
        .as_ref()
        .and_then(|claims| claims.get("exp"))
        .and_then(Value::as_f64)
        .filter(|exp| *exp > 0.0)
    {
        return (exp * 1000.0) as i64;
    }
    crate::database::migrations::now_ms() + 3_600_000
}

async fn xai_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    let http = oc_http::client();
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
    // G-284② — `credential(methodID, tokens, value.refresh, …)`
    // (xai.ts:121,190-199): the rotated token falls back to the CURRENT
    // refresh; both absent is a hard error, never a silent empty store.
    let refresh = token["refresh_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .map_or_else(|| value.refresh.clone(), str::to_owned);
    if refresh.is_empty() {
        return Err("xAI token response is missing refresh_token".into());
    }
    let expires = xai_token_expiration(token["expires_in"].as_f64(), &access);
    Ok(CredentialValue::OAuth(OAuthValue {
        method_id: value.method_id,
        refresh,
        access,
        expires,
        // xai.ts:121 — the credential metadata carries across refreshes.
        metadata: value.metadata,
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

const POE_CLIENT_ID: &str = "client_728290227fc048cc9262091a1ea197ea";
const POE_ISSUER: &str = "https://poe.com";
const POE_TOKEN: &str = "https://api.poe.com/token";
const POE_SCOPE: &str = "apikey:create";
/// Poe API keys never outlive this (poe.ts:13 `maxExpiry`, ms).
const POE_MAX_EXPIRY: i64 = 8_640_000_000_000_000;

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

async fn poe_authorize(answer: FormAnswer) -> Result<Authorization, String> {
    poe_authorize_at(answer, POE_TOKEN).await
}

/// The Poe browser flow with an injectable token endpoint (test seam:
/// tests replay the code exchange through a loopback stub).
pub async fn poe_authorize_at(
    _answer: FormAnswer,
    token_url: &str,
) -> Result<Authorization, String> {
    let PkcePair { verifier, challenge } = pkce_pair();
    let state = flows::random_state(32);
    let listener = flows::listen_loopback(LoopbackOptions {
        port: 0,
        host: "127.0.0.1".into(),
        callback_path: "/callback",
        page: CallbackPage::Branded { provider: "Poe".to_owned() },
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
    let token_url = token_url.to_owned();
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let params = listener.wait().await?;
        // v2 checks state first, then the optional iss pin, then the
        // error params, then the code (poe.ts:117-126).
        if params.get("state").map(String::as_str) != Some(state.as_str()) {
            return Err("Invalid OAuth state".into());
        }
        // Poe's client pins this issuer but does not require iss; its
        // documented callbacks may omit it (poe.ts:119-120).
        if let Some(iss) = params.get("iss").filter(|iss| !iss.is_empty()) {
            if iss != POE_ISSUER {
                return Err("Invalid OAuth issuer".into());
            }
        }
        if params.contains_key("error") {
            let detail = params
                .get("error_description")
                .or_else(|| params.get("error"))
                .map(String::as_str)
                .filter(|detail| !detail.is_empty())
                .unwrap_or("Authorization denied");
            // Never echo the state nonce back (poe.ts:122-123).
            return Err(if detail.contains(state.as_str()) {
                "Poe authorization failed".into()
            } else {
                detail.to_owned()
            });
        }
        let code = params.get("code").map(String::as_str).unwrap_or_default().trim();
        if code.is_empty() {
            return Err("Missing authorization code".into());
        }
        poe_exchange(&token_url, code, &redirect, &verifier).await
    });
    Ok(Authorization {
        url,
        instructions:
            "Complete authorization in your browser. This window will close automatically.".into(),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

/// The code → API-key exchange (poe.ts:128-163): Poe's token endpoint
/// returns an `api_key` (+ optional `api_key_expires_in` seconds), not an
/// OAuth access token.
async fn poe_exchange(
    token_url: &str,
    code: &str,
    redirect: &str,
    verifier: &str,
) -> Result<CredentialValue, String> {
    let http = oc_http::client();
    let response = http
        .post(token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", POE_CLIENT_ID),
            ("code", code),
            ("redirect_uri", redirect),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|_| "Poe token exchange request failed".to_owned())?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let error: Option<Value> = serde_json::from_str(&body).ok();
        let detail = error
            .as_ref()
            .and_then(|error| error.get("error_description"))
            .or_else(|| error.as_ref().and_then(|error| error.get("error")))
            .and_then(Value::as_str)
            .filter(|detail| !detail.is_empty());
        return Err(match detail {
            // Error details must never echo the code/verifier secrets
            // (poe.ts:145-152).
            Some(detail) if !detail.contains(code) && !detail.contains(verifier) => {
                format!("Poe token exchange failed: {detail}")
            }
            _ => {
                let status_code = status.as_u16();
                format!("Poe token exchange failed ({status_code})")
            }
        });
    }
    let body: Value = response.json().await.map_err(|_| "Invalid Poe token response".to_owned())?;
    // Token schema (poe.ts:14-17): non-empty trimmed api_key without
    // whitespace; optional (nullable) non-negative integer expiry.
    let api_key = body
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty() && !key.contains(char::is_whitespace))
        .ok_or_else(|| "Invalid Poe token response".to_owned())?;
    let expires = match body.get("api_key_expires_in") {
        None | Some(Value::Null) => Some(POE_MAX_EXPIRY),
        Some(value) => match value.as_i64() {
            Some(seconds) if seconds >= 0 => {
                crate::database::migrations::now_ms().checked_add(seconds.saturating_mul(1000))
            }
            _ => return Err("Invalid Poe token response".into()),
        },
    }
    .ok_or("Invalid Poe API key expiry")?;
    if expires > POE_MAX_EXPIRY {
        return Err("Invalid Poe API key expiry".into());
    }
    Ok(CredentialValue::OAuth(OAuthValue {
        method_id: "browser".into(),
        refresh: String::new(),
        access: api_key.to_owned(),
        expires,
        metadata: None,
    }))
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
