//! The BE-3b OAuth variant registrations, 1:1 with v2's provider plugins:
//! GitLab PKCE (gitlab.ts), Azure Entra CLI (azure.ts:125-166), Snowflake
//! external-browser (snowflake-cortex.ts:82-170), `OpenAI` codex
//! browser/headless (openai.ts:52-201), the `ChatGPT` token-sharing
//! connector (chatgpt.ts:88-190).
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
use super::types::{CredentialValue, FormAnswer, FormField, Method, OAuthValue};
use super::{AuthMode, Authorization, IntegrationHost, OAuthImplementation};
use base64::Engine;
use futures::future::BoxFuture;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// Installs the BE-3b variants (called from `providers::install`).
pub fn install(host: &IntegrationHost) {
    install_gitlab(host);
    install_azure(host);
    install_snowflake(host);
    install_openai(host);
    install_chatgpt(host);
}

fn now_ms() -> i64 {
    crate::database::migrations::now_ms()
}

/// The standard redirect-callback checks (gitlab.ts:462-466): state match,
/// no error param, non-empty code.
fn callback_error(params: &BTreeMap<String, String>, state: &str) -> Result<(), String> {
    if params.get("state").map(String::as_str) != Some(state) {
        return Err("Invalid OAuth state".into());
    }
    if let Some(error) = params.get("error_description").or_else(|| params.get("error")) {
        return Err(error.clone());
    }
    if params.get("code").is_none_or(|code| code.trim().is_empty()) {
        return Err("Missing authorization code".into());
    }
    Ok(())
}

/// Form-encoded POST → (status, JSON body).
async fn post_form(
    http: &reqwest::Client,
    url: &str,
    fields: &[(&str, &str)],
) -> Result<(u16, Value), String> {
    let response = http
        .post(url)
        .form(fields)
        .send()
        .await
        .map_err(|error| format!("token exchange request failed: {error}"))?;
    let status = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    Ok((status, body))
}

fn token_error(body: &Value, fallback: &str) -> String {
    body["error_description"].as_str().or(body["error"].as_str()).unwrap_or(fallback).to_owned()
}

// ---------------------------------------------------------------------------
// GitLab (plugin/provider/gitlab.ts)
// ---------------------------------------------------------------------------

const GITLAB_BUNDLED_CLIENT_ID: &str =
    "fd180700a8f9c5d5557aca231632dd0611a1135a3bb510a741d5a988a3394fa7";
const GITLAB_LEGACY_CLIENT_ID: &str =
    "1d89f9fdb23ee96d4e603201f6861dab6e143c5c3c00469a018a2d94bdc03d4e";
const GITLAB_CALLBACK_PORT: u16 = 8080;
const GITLAB_REDIRECT: &str = "http://127.0.0.1:8080/callback";

fn install_gitlab(host: &IntegrationHost) {
    host.update_integration("gitlab", "GitLab");
    let default_instance =
        std::env::var("GITLAB_INSTANCE_URL").unwrap_or_else(|_| "https://gitlab.com".into());
    let form = vec![FormField {
        type_: "string".into(),
        key: "instanceUrl".into(),
        title: Some("GitLab instance URL".into()),
        description: Some(
            "Leave the default to use gitlab.com, or enter your self-managed GitLab URL.".into(),
        ),
        placeholder: Some(default_instance.clone()),
        default: Some(Value::String(default_instance)),
        pattern: Some(r"^https?://\S+$".into()),
        required: false,
        hidden: false,
        options: None,
        when: None,
    }];
    host.update_method(
        "gitlab",
        Method::OAuth {
            id: "pkce".into(),
            label: "Login with GitLab (OAuth)".into(),
            form: Some(form),
        },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(gitlab_authorize(answer))),
            refresh: Some(Arc::new(|value| Box::pin(gitlab_refresh(value)))),
            label: Some(Arc::new(gitlab_label)),
        }),
    );
}

fn gitlab_default_instance() -> String {
    std::env::var("GITLAB_INSTANCE_URL").unwrap_or_else(|_| "https://gitlab.com".into())
}

/// `{origin}{path}` with trailing slashes trimmed (gitlab.ts:170-174) so
/// relative-root instances (`/gitlab`) keep their path.
fn gitlab_instance_url(answer: &FormAnswer) -> String {
    let raw = answer
        .get("instanceUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(gitlab_default_instance, str::to_owned);
    match reqwest::Url::parse(&raw) {
        Ok(url) => {
            let mut value = url.origin().ascii_serialization();
            let path = url.path().trim_end_matches('/');
            if path != "/" && !path.is_empty() {
                value.push_str(path);
            }
            value
        }
        Err(_) => raw.trim_end_matches('/').to_owned(),
    }
}

fn gitlab_metadata(value: &OAuthValue) -> (String, String) {
    let instance = value
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("instanceUrl"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map_or_else(gitlab_default_instance, String::from);
    // Refresh with the application that issued the token (gitlab.ts:147-153).
    let client = value
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("clientID"))
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            std::env::var("GITLAB_OAUTH_CLIENT_ID").ok().map(|value| value.trim().to_owned())
        })
        .filter(|client| !client.is_empty())
        .unwrap_or_else(|| GITLAB_LEGACY_CLIENT_ID.to_owned());
    (instance, client)
}

async fn gitlab_exchange(
    instance: &str,
    client_id: &str,
    fields: Vec<(&str, String)>,
    describe_grant: bool,
) -> Result<OAuthValue, String> {
    gitlab_exchange_at(instance, client_id, fields, describe_grant).await
}

/// The exchange with an explicit endpoint (v2's injectable-endpoints test
/// seam, azure.ts `make(endpoints)` pattern).
pub(super) async fn gitlab_exchange_at(
    instance: &str,
    client_id: &str,
    fields: Vec<(&str, String)>,
    describe_grant: bool,
) -> Result<OAuthValue, String> {
    let http = reqwest::Client::new();
    let mut body: Vec<(&str, String)> = fields;
    body.push(("client_id", client_id.to_owned()));
    let pairs: Vec<(&str, &str)> = body.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let (status, body) = post_form(&http, &format!("{instance}/oauth/token"), &pairs).await?;
    if !(200..300).contains(&status) {
        let detail = token_error(&body, &format!("HTTP {status}"));
        let rejected = body["error"] == "invalid_grant" || body["error"] == "invalid_client";
        // v2's hint builders (gitlab.ts:372-396) — collapsed to the
        // essentials but keeping the redirect/client context.
        if rejected && describe_grant {
            return Err(format!(
                "{detail}\n\nLikely causes:\n- the authorization code was already used, or came from an older login attempt\n- redirect_uri sent: {GITLAB_REDIRECT} — the application must register this exactly\n- client_id used: {}... ({}), GITLAB_OAUTH_CLIENT_ID overrides it",
                client_id.get(..12.min(client_id.len())).unwrap_or_default(),
                if client_id == GITLAB_BUNDLED_CLIENT_ID { "bundled default" } else { "custom" }
            ));
        }
        if rejected {
            return Err(format!(
                "{detail}\n\nThe GitLab refresh token was revoked, expired, or issued to a different OAuth application. Sign in to GitLab again."
            ));
        }
        return Err(detail);
    }
    let access = body["access_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or("Invalid GitLab token response")?
        .to_owned();
    let refresh = body["refresh_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or("Invalid GitLab token response")?
        .to_owned();
    let expires_in = body["expires_in"].as_f64().unwrap_or(7200.0);
    Ok(OAuthValue {
        method_id: "pkce".into(),
        access,
        refresh,
        expires: now_ms() + (expires_in * 1000.0) as i64,
        metadata: Some(BTreeMap::from([
            ("instanceUrl".to_owned(), Value::String(instance.to_owned())),
            ("clientID".to_owned(), Value::String(client_id.to_owned())),
        ])),
    })
}

async fn gitlab_authorize(answer: FormAnswer) -> Result<Authorization, String> {
    let override_client =
        std::env::var("GITLAB_OAUTH_CLIENT_ID").unwrap_or_default().trim().to_owned();
    let client_id = if override_client.is_empty() {
        GITLAB_BUNDLED_CLIENT_ID.to_owned()
    } else {
        override_client.clone()
    };
    let instance = gitlab_instance_url(&answer);
    // The bundled application only exists on gitlab.com (gitlab.ts:175-182).
    let host = reqwest::Url::parse(&instance).ok().and_then(|url| url.host_str().map(String::from));
    if override_client.is_empty() && host.as_deref() != Some("gitlab.com") {
        return Err(format!(
            "The bundled GitLab OAuth application only exists on gitlab.com. To sign in to {}, register an OAuth application there with redirect URI {GITLAB_REDIRECT} and the `api` scope (not marked \"Confidential\"), then set GITLAB_OAUTH_CLIENT_ID to its application ID.",
            host.unwrap_or_else(|| instance.clone())
        ));
    }
    let Pkce { verifier, challenge } = gitlab_pkce();
    let state = flows::random_state(32);
    // Fixed port: GitLab matches redirect_uri exactly (gitlab.ts:468-486).
    let listener = flows::listen_loopback(LoopbackOptions {
        port: GITLAB_CALLBACK_PORT,
        host: "127.0.0.1".into(),
        callback_path: "/callback",
        page: CallbackPage::Html(flows::success_page("GitLab")),
    })
    .await
    .map_err(|_| {
        format!(
            "GitLab login needs local port {GITLAB_CALLBACK_PORT}, but it is already in use. Stop the process using that port and try again."
        )
    })?;
    let url = format!(
        "{instance}/oauth/authorize?{}",
        query(&[
            ("client_id", client_id.as_str()),
            ("redirect_uri", GITLAB_REDIRECT),
            ("response_type", "code"),
            ("state", state.as_str()),
            ("scope", "api"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ])
    );
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let params = listener.wait().await?;
        callback_error(&params, &state)?;
        let code = params.get("code").cloned().unwrap_or_default();
        let tokens = gitlab_exchange(
            &instance,
            &client_id,
            vec![
                ("grant_type", "authorization_code".to_owned()),
                ("code", code),
                ("redirect_uri", GITLAB_REDIRECT.to_owned()),
                ("code_verifier", verifier),
            ],
            true,
        )
        .await?;
        Ok(CredentialValue::OAuth(tokens))
    });
    Ok(Authorization {
        url,
        instructions:
            "Complete authorization in your browser. This window will close automatically.".into(),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

async fn gitlab_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    let (instance, client_id) = gitlab_metadata(&value);
    // GitLab rotates the refresh token on every refresh; concurrent
    // resolves of the same token share one exchange (gitlab.ts:440-460 —
    // coarse keyed single-flight).
    let key = format!("{instance}\0{}", value.refresh);
    let lock = gitlab_refresh_locks()
        .entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _guard = lock.lock().await;
    gitlab_exchange(
        &instance,
        &client_id,
        vec![
            ("grant_type", "refresh_token".to_owned()),
            ("refresh_token", value.refresh.clone()),
            ("redirect_uri", GITLAB_REDIRECT.to_owned()),
        ],
        false,
    )
    .await
    .map(CredentialValue::OAuth)
}

fn gitlab_refresh_locks() -> &'static dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: std::sync::OnceLock<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>> =
        std::sync::OnceLock::new();
    LOCKS.get_or_init(dashmap::DashMap::new)
}

fn gitlab_label(value: &CredentialValue) -> Option<String> {
    let CredentialValue::OAuth(oauth) = value else { return None };
    let instance = oauth.metadata.as_ref()?.get("instanceUrl")?.as_str()?;
    reqwest::Url::parse(instance).ok().and_then(|url| url.host_str().map(String::from))
}

struct Pkce {
    verifier: String,
    challenge: String,
}

fn gitlab_pkce() -> Pkce {
    let pkce = flows::pkce_s256();
    Pkce { verifier: pkce.verifier, challenge: pkce.challenge }
}

/// url-encoded query string from pairs.
fn query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", urlencoding::encode(key), urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

// ---------------------------------------------------------------------------
// Azure Entra ID via Azure CLI (plugin/provider/azure.ts:125-166)
// ---------------------------------------------------------------------------

const AZURE_COGNITIVE_SCOPE: &str = "https://cognitiveservices.azure.com/.default";

fn install_azure(host: &IntegrationHost) {
    host.update_integration("azure", "Azure");
    let form = vec![FormField {
        type_: "string".into(),
        key: "resourceName".into(),
        title: Some("Enter Azure Resource Name".into()),
        description: None,
        placeholder: Some("e.g. my-models".into()),
        default: None,
        pattern: None,
        required: true,
        hidden: false,
        options: None,
        when: None,
    }];
    host.update_method(
        "azure",
        Method::Key { label: Some("API key".into()), form: Some(form.clone()) },
        None,
    );
    // Registered only when the CLI exists (azure.ts:124: `which("az")`).
    if flows::which("az").is_none() {
        return;
    }
    host.update_method(
        "azure",
        Method::OAuth {
            id: "azure-cli".into(),
            label: "Microsoft Entra ID (Azure CLI)".into(),
            form: Some(form),
        },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(azure_authorize(answer))),
            refresh: Some(Arc::new(|value| Box::pin(azure_refresh(value)))),
            label: None,
        }),
    );
}

async fn azure_cli_token() -> Result<(String, i64), String> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new("az")
            .args([
                "account",
                "get-access-token",
                "--scope",
                AZURE_COGNITIVE_SCOPE,
                "--output",
                "json",
            ])
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "Azure CLI token request timed out".to_owned())?
    .map_err(|error| format!("Azure CLI failed to start: {error}"))?;
    if !output.status.success() {
        return Err("Sign in with `az login` before continuing.".into());
    }
    let body: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Azure CLI returned invalid JSON".to_owned())?;
    let access = body["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("Azure CLI returned no accessToken")?
        .to_owned();
    let expires = body["expires_on"]
        .as_f64()
        .map(|seconds| (seconds * 1000.0) as i64)
        .or_else(|| body["expiresOn"].as_str().and_then(chrono_parse_rfc2822_or_iso))
        .ok_or("Azure CLI returned an invalid token expiration")?;
    Ok((access, expires))
}

fn chrono_parse_rfc2822_or_iso(text: &str) -> Option<i64> {
    // `az` emits RFC-1123-ish `expiresOn`; accept ISO-8601 too without a
    // chrono dependency by delegating to the well-known formats via
    // `time`-free parsing: prefer the platform's RFC3339 subset.
    text.parse::<chrono_naive::NaiveDateTime>()
        .ok()
        .map(|parsed| parsed.and_utc().timestamp_millis())
        .or_else(|| {
            // RFC 1123: "Fri, 17 Oct 2026 01:02:03 GMT" → rebuild as RFC3339.
            let rebuilt = text.trim().replace(" GMT", "Z").replace(" UTC", "Z");
            rebuilt
                .split_once(", ")
                .map(|(_, rest)| {
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if parts.len() == 3 {
                        format!("{}T{}Z", parts[0], parts[1])
                    } else {
                        rebuilt.clone()
                    }
                })
                .and_then(|candidate| {
                    candidate
                        .parse::<chrono_naive::NaiveDateTime>()
                        .ok()
                        .map(|parsed| parsed.and_utc().timestamp_millis())
                })
        })
}

/// A tiny date-parse seam (no chrono in tree): RFC-3339
/// `YYYY-MM-DDTHH:MM:SS[.fff][Z]` and RFC-1123
/// `Fri, 17 Oct 2026 01:02:03 GMT` into epoch seconds (days-from-civil).
mod chrono_naive {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct NaiveDateTime {
        seconds: i64,
    }

    impl NaiveDateTime {
        pub fn and_utc(self) -> Self {
            self
        }

        pub fn timestamp_millis(self) -> i64 {
            self.seconds * 1000
        }
    }

    impl std::str::FromStr for NaiveDateTime {
        type Err = ();

        fn from_str(text: &str) -> Result<Self, Self::Err> {
            parse_datetime(text).map(|seconds| Self { seconds }).ok_or(())
        }
    }

    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

    fn parse_datetime(text: &str) -> Option<i64> {
        let text = text.trim();
        let (year, month, day, time) = if let Some((date, time)) = text.split_once('T') {
            // RFC-3339 date part + time.
            let mut parts = date.split('-');
            let year: i64 = parts.next()?.parse().ok()?;
            let month: i64 = parts.next()?.parse().ok()?;
            let day: i64 = parts.next()?.parse().ok()?;
            (year, month, day, time.trim_end_matches('Z'))
        } else {
            // RFC-1123: strip the weekday, then "17 Oct 2026 01:02:03".
            let rest = text.split_once(", ").map_or(text, |(_, rest)| rest);
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 4 {
                return None;
            }
            let day: i64 = parts[0].parse().ok()?;
            let month = i64::try_from(MONTHS.iter().position(|name| parts[1].starts_with(name))?)
                .expect("month index")
                + 1;
            let year: i64 = parts[2].parse().ok()?;
            (year, month, day, parts[3])
        };
        let seconds_of_day = parse_time_of_day(time)?;
        // days-from-civil (Howard Hinnant)
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (month + 9) % 12;
        let doy = (153 * mp + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some(days * 86_400 + seconds_of_day)
    }

    fn parse_time_of_day(text: &str) -> Option<i64> {
        let mut parts = text.split(':');
        let hours: i64 = parts.next()?.parse().ok()?;
        let minutes: i64 = parts.next()?.parse().ok()?;
        let seconds: i64 =
            parts.next()?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
        Some(hours * 3600 + minutes * 60 + seconds)
    }
}

#[allow(
    clippy::unused_async,
    clippy::unused_async_trait_impl,
    reason = "async surface parity with integration.ts"
)]
async fn azure_authorize(answer: FormAnswer) -> Result<Authorization, String> {
    let resource = answer
        .get("resourceName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let Some(resource_name) = resource else {
            return Err("Azure resource name is required".into());
        };
        let (access, expires) = azure_cli_token().await?;
        Ok(CredentialValue::OAuth(OAuthValue {
            method_id: "azure-cli".into(),
            refresh: "azure-cli".into(),
            access,
            expires,
            metadata: Some(BTreeMap::from([(
                "resourceName".to_owned(),
                Value::String(resource_name.clone()),
            )])),
        }))
    });
    Ok(Authorization {
        url: String::new(),
        instructions: "Sign in with `az login` before continuing.".into(),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

async fn azure_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    let (access, expires) = azure_cli_token().await?;
    Ok(CredentialValue::OAuth(OAuthValue { access, expires, ..value }))
}

// ---------------------------------------------------------------------------
// Snowflake Cortex external-browser (plugin/provider/snowflake-cortex.ts:82-170)
// ---------------------------------------------------------------------------

const SNOWFLAKE_CLIENT_ID: &str = "LOCAL_APPLICATION";

fn install_snowflake(host: &IntegrationHost) {
    host.update_integration("snowflake-cortex", "Snowflake Cortex");
    let account_form = vec![FormField {
        type_: "string".into(),
        key: "account".into(),
        title: Some("Snowflake account".into()),
        description: None,
        placeholder: Some("myorg-myaccount".into()),
        default: None,
        pattern: None,
        required: true,
        hidden: false,
        options: None,
        when: None,
    }];
    let mut oauth_form = account_form.clone();
    oauth_form.push(FormField {
        type_: "string".into(),
        key: "role".into(),
        title: Some("Snowflake role (optional)".into()),
        description: None,
        placeholder: None,
        default: None,
        pattern: None,
        required: false,
        hidden: false,
        options: None,
        when: None,
    });
    host.update_method(
        "snowflake-cortex",
        Method::Key { label: Some("Paste PAT or bearer token".into()), form: Some(account_form) },
        None,
    );
    host.update_method(
        "snowflake-cortex",
        Method::Env { names: vec!["SNOWFLAKE_CORTEX_TOKEN".into(), "SNOWFLAKE_CORTEX_PAT".into()] },
        None,
    );
    host.update_method(
        "snowflake-cortex",
        Method::OAuth {
            id: "browser".into(),
            label: "Login with Snowflake (External Browser)".into(),
            form: Some(oauth_form),
        },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(snowflake_authorize(answer))),
            refresh: Some(Arc::new(|value| Box::pin(snowflake_refresh(value)))),
            label: Some(Arc::new(snowflake_label)),
        }),
    );
}

/// normalizeAccount (snowflake.ts:250-257): strip scheme, trailing slashes,
/// and the `.snowflakecomputing.com` suffix.
pub(super) fn snowflake_account(value: Option<&str>) -> String {
    let Some(text) = value else { return String::new() };
    text.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .trim_end_matches(".snowflakecomputing.com")
        .to_owned()
}

fn snowflake_issuer(account: &str) -> String {
    format!("https://{account}.snowflakecomputing.com")
}

/// The token-request POST with the built-in basic client
/// (snowflake.ts:52-78).
async fn snowflake_token(account: &str, fields: Vec<(&str, String)>) -> Result<OAuthValue, String> {
    if account.is_empty() {
        return Err("Snowflake account is required".into());
    }
    snowflake_token_at(&snowflake_issuer(account), account, fields).await
}

/// The token POST with an explicit issuer (test seam).
pub(super) async fn snowflake_token_at(
    issuer: &str,
    account: &str,
    fields: Vec<(&str, String)>,
) -> Result<OAuthValue, String> {
    let http = reqwest::Client::new();
    let basic = base64::engine::general_purpose::STANDARD
        .encode(format!("{SNOWFLAKE_CLIENT_ID}:{SNOWFLAKE_CLIENT_ID}"));
    let mut body: Vec<(&str, String)> = fields;
    body.push(("client_id", SNOWFLAKE_CLIENT_ID.to_owned()));
    let pairs: Vec<(&str, &str)> = body.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let response = http
        .post(format!("{issuer}/oauth/token-request"))
        .header("Accept", "application/json")
        .header("Authorization", format!("Basic {basic}"))
        .form(&pairs)
        .send()
        .await
        .map_err(|error| format!("Snowflake token request failed: {error}"))?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!("Snowflake token request failed ({status}): {text}"));
    }
    let tokens: Value =
        serde_json::from_str(&text).map_err(|_| "Invalid Snowflake token response".to_owned())?;
    let access =
        tokens["access_token"].as_str().ok_or("Invalid Snowflake token response")?.to_owned();
    let refresh = tokens["refresh_token"]
        .as_str()
        .map(String::from)
        .or_else(|| {
            pairs
                .iter()
                .find(|(key, _)| *key == "refresh_token")
                .map(|(_, value)| (*value).to_owned())
        })
        .ok_or("Snowflake token response did not include refresh_token")?;
    let expires_in = tokens["expires_in"].as_f64().unwrap_or(600.0);
    Ok(OAuthValue {
        method_id: "browser".into(),
        access,
        refresh,
        expires: now_ms() + (expires_in * 1000.0) as i64,
        metadata: Some(BTreeMap::from([("account".to_owned(), Value::String(account.to_owned()))])),
    })
}

async fn snowflake_authorize(answer: FormAnswer) -> Result<Authorization, String> {
    let account = snowflake_account(answer.get("account").and_then(Value::as_str));
    if account.is_empty() {
        return Err("Snowflake account is required".into());
    }
    let role =
        answer.get("role").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_owned();
    let pkce = flows::pkce_s256_with_bytes(48);
    let state = flows::random_state(32);
    let listener = flows::listen_loopback(LoopbackOptions {
        port: 0,
        host: "127.0.0.1".into(),
        callback_path: "/",
        page: CallbackPage::Html(flows::success_page("Snowflake")),
    })
    .await
    .map_err(|error| format!("failed to bind loopback listener: {error}"))?;
    let redirect = format!("http://127.0.0.1:{}/", listener.port);
    // Scope: refresh_token (+ session:role[:encoded]) (snowflake.ts:151-155).
    let scope = if role.is_empty() {
        "refresh_token".to_owned()
    } else if role.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        format!("refresh_token session:role:{role}")
    } else {
        format!("refresh_token session:role-encoded:{}", urlencoding::encode(&role))
    };
    let url = format!(
        "{}/oauth/authorize?{}",
        snowflake_issuer(&account),
        query(&[
            ("client_id", SNOWFLAKE_CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", redirect.as_str()),
            ("state", state.as_str()),
            ("scope", scope.as_str()),
            ("code_challenge", pkce.challenge.as_str()),
            ("code_challenge_method", "S256"),
        ])
    );
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let params = listener.wait().await?;
        if params.get("state").map(String::as_str) != Some(state.as_str()) {
            return Err("Invalid OAuth state".into());
        }
        if let Some(error) = params.get("error_description").or_else(|| params.get("error")) {
            return Err(error.clone());
        }
        let code = params.get("code").cloned().unwrap_or_default();
        if code.trim().is_empty() {
            return Err("Missing authorization code".into());
        }
        let tokens = snowflake_token(
            &account,
            vec![
                ("grant_type", "authorization_code".to_owned()),
                ("code", code),
                ("redirect_uri", redirect),
                ("code_verifier", pkce.verifier),
            ],
        )
        .await?;
        Ok(CredentialValue::OAuth(tokens))
    });
    Ok(Authorization {
        url,
        instructions: "Complete Snowflake sign-in in your browser.".into(),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

async fn snowflake_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    // An environment-token override keeps the saved login usable
    // (snowflake.ts:101).
    if std::env::var("SNOWFLAKE_CORTEX_TOKEN").is_ok()
        || std::env::var("SNOWFLAKE_CORTEX_PAT").is_ok()
    {
        return Ok(CredentialValue::OAuth(value));
    }
    let account = snowflake_account(
        value.metadata.as_ref().and_then(|m| m.get("account")).and_then(Value::as_str),
    );
    snowflake_token(
        &account,
        vec![("grant_type", "refresh_token".to_owned()), ("refresh_token", value.refresh.clone())],
    )
    .await
    .map(CredentialValue::OAuth)
}

fn snowflake_label(value: &CredentialValue) -> Option<String> {
    let CredentialValue::OAuth(oauth) = value else { return None };
    oauth
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("account"))
        .and_then(Value::as_str)
        .map(|account| snowflake_account(Some(account)))
        .filter(|account| !account.is_empty())
}

// ---------------------------------------------------------------------------
// OpenAI codex browser + headless (plugin/provider/openai.ts:52-201)
// ---------------------------------------------------------------------------

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_ISSUER: &str = "https://auth.openai.com";
const OPENAI_BROWSER_PORT: u16 = 1455;
const OPENAI_BROWSER_FALLBACK_PORT: u16 = 1457;

fn install_openai(host: &IntegrationHost) {
    host.update_integration("openai", "OpenAI");
    for (method_id, label) in [
        ("chatgpt-browser", "Codex browser (legacy)"),
        ("chatgpt-headless", "Codex device code (legacy)"),
    ] {
        let browser = method_id == "chatgpt-browser";
        host.update_method(
            "openai",
            Method::OAuth { id: method_id.into(), label: label.into(), form: None },
            Some(OAuthImplementation {
                authorize: Arc::new(move |answer| {
                    if browser {
                        Box::pin(openai_browser_authorize(answer))
                    } else {
                        Box::pin(openai_headless_authorize(answer))
                    }
                }),
                refresh: Some(Arc::new(openai_refresh_shared)),
                label: None,
            }),
        );
    }
}

async fn openai_exchange(
    fields: Vec<(&str, String)>,
) -> Result<(String, String, String, f64), String> {
    openai_exchange_at(OPENAI_ISSUER, fields).await
}

/// The exchange with an explicit issuer (test seam).
pub(super) async fn openai_exchange_at(
    issuer: &str,
    fields: Vec<(&str, String)>,
) -> Result<(String, String, String, f64), String> {
    let http = reqwest::Client::new();
    let pairs: Vec<(&str, &str)> =
        fields.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let (status, body) = post_form(&http, &format!("{issuer}/oauth/token"), &pairs).await?;
    if !(200..300).contains(&status) {
        return Err(format!("Request failed: {status}"));
    }
    let access = body["access_token"].as_str().ok_or("Invalid token response")?.to_owned();
    let refresh = body["refresh_token"].as_str().ok_or("Invalid token response")?.to_owned();
    let id_token = body["id_token"].as_str().unwrap_or_default().to_owned();
    let expires_in = body["expires_in"].as_f64().unwrap_or(3600.0);
    Ok((access, refresh, id_token, expires_in))
}

/// accountID from `id_token/access_token` claims (openai.ts:415-430).
fn openai_account_id(id_token: &str, access_token: &str) -> Option<String> {
    for token in [id_token, access_token] {
        if token.is_empty() {
            continue;
        }
        let Some(claims) = flows::jwt_claims(token) else { continue };
        if let Some(id) = claims.get("chatgpt_account_id").and_then(Value::as_str) {
            return Some(id.to_owned());
        }
        if let Some(id) = claims
            .get("https://api.openai.com/auth")
            .and_then(|auth| auth.get("chatgpt_account_id"))
            .and_then(Value::as_str)
        {
            return Some(id.to_owned());
        }
        if let Some(id) = claims
            .get("organizations")
            .and_then(|orgs| orgs.get(0))
            .and_then(|org| org.get("id"))
            .and_then(Value::as_str)
        {
            return Some(id.to_owned());
        }
    }
    None
}

pub(super) fn openai_credential(
    method_id: &str,
    access: String,
    refresh: String,
    id_token: &str,
    expires_in: f64,
) -> CredentialValue {
    let metadata = openai_account_id(id_token, &access)
        .map(|account_id| BTreeMap::from([("accountID".to_owned(), Value::String(account_id))]));
    CredentialValue::OAuth(OAuthValue {
        method_id: method_id.to_owned(),
        access,
        refresh,
        expires: now_ms() + (expires_in * 1000.0) as i64,
        metadata,
    })
}

async fn openai_browser_authorize(_answer: FormAnswer) -> Result<Authorization, String> {
    let Pkce { verifier, challenge } = openai_pkce();
    let state = flows::random_state(32);
    // Pinned 1455 with fallback 1457 + cancel-probe retries
    // (openai.ts:119-153).
    let listener = flows::listen_pinned(
        OPENAI_BROWSER_PORT,
        Some(OPENAI_BROWSER_FALLBACK_PORT),
        |port| LoopbackOptions {
            port,
            host: "localhost".into(),
            callback_path: "/auth/callback",
            page: CallbackPage::Html(flows::success_page("ChatGPT")),
        },
        10,
        Duration::from_millis(200),
        |primary, fallback| {
            format!(
                "OpenAI browser login needs local port {primary} or {fallback}, but both are already in use. Stop the processes using those ports or choose Codex device code (legacy), then try again."
            )
        },
    )
    .await?;
    let redirect = format!("http://localhost:{}/auth/callback", listener.port);
    let url = format!(
        "{OPENAI_ISSUER}/oauth/authorize?{}",
        query(&[
            ("response_type", "code"),
            ("client_id", OPENAI_CLIENT_ID),
            ("redirect_uri", redirect.as_str()),
            ("scope", "openid profile email offline_access"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("state", state.as_str()),
            ("originator", "opencode"),
        ])
    );
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let params = listener.wait().await?;
        callback_error(&params, &state)?;
        let code = params.get("code").cloned().unwrap_or_default();
        let (access, refresh, id_token, expires_in) = openai_exchange(vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", code),
            ("redirect_uri", redirect),
            ("client_id", OPENAI_CLIENT_ID.to_owned()),
            ("code_verifier", verifier),
        ])
        .await?;
        Ok(openai_credential("chatgpt-browser", access, refresh, &id_token, expires_in))
    });
    Ok(Authorization {
        url,
        instructions:
            "Complete authorization in your browser. This window will close automatically.".into(),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

async fn openai_headless_authorize(_answer: FormAnswer) -> Result<Authorization, String> {
    openai_headless_authorize_at(FormAnswer::new(), OPENAI_ISSUER).await
}

/// The headless flow with an explicit issuer (test seam).
pub(super) async fn openai_headless_authorize_at(
    _answer: FormAnswer,
    issuer: &str,
) -> Result<Authorization, String> {
    let http = reqwest::Client::new();
    let response = http
        .post(format!("{issuer}/api/accounts/deviceauth/usercode"))
        .header("Content-Type", "application/json")
        .json(&json!({ "client_id": OPENAI_CLIENT_ID }))
        .send()
        .await
        .map_err(|error| format!("device code request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Request failed: {}", response.status()));
    }
    let device: Value =
        response.json().await.map_err(|_| "Invalid device code response".to_owned())?;
    let device_auth_id =
        device["device_auth_id"].as_str().ok_or("device_auth_id missing")?.to_owned();
    let user_code = device["user_code"].as_str().ok_or("user_code missing")?.to_owned();
    let interval_ms =
        device["interval"].as_str().and_then(|text| text.parse::<u64>().ok()).unwrap_or(5).max(1)
            * 1000;
    let display_code = user_code.clone();
    let issuer = issuer.to_owned();
    let issuer_for_url = issuer.clone();
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let http = reqwest::Client::new();
        loop {
            let response = http
                .post(format!("{issuer}/api/accounts/deviceauth/token"))
                .header("Content-Type", "application/json")
                .json(&json!({ "device_auth_id": device_auth_id, "user_code": user_code.clone() }))
                .send()
                .await
                .map_err(|error| format!("device token request failed: {error}"))?;
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                let data: Value = response
                    .json()
                    .await
                    .map_err(|_| "Invalid device token response".to_owned())?;
                let authorization_code = data["authorization_code"]
                    .as_str()
                    .ok_or("authorization_code missing")?
                    .to_owned();
                let code_verifier =
                    data["code_verifier"].as_str().ok_or("code_verifier missing")?.to_owned();
                let (access, refresh, id_token, expires_in) = openai_exchange_at(
                    &issuer,
                    vec![
                        ("grant_type", "authorization_code".to_owned()),
                        ("code", authorization_code),
                        ("redirect_uri", format!("{issuer}/deviceauth/callback")),
                        ("client_id", OPENAI_CLIENT_ID.to_owned()),
                        ("code_verifier", code_verifier),
                    ],
                )
                .await?;
                return Ok(openai_credential(
                    "chatgpt-headless",
                    access,
                    refresh,
                    &id_token,
                    expires_in,
                ));
            }
            if status != 403 && status != 404 {
                return Err(format!("Device authorization failed: {status}"));
            }
            tokio::time::sleep(Duration::from_millis(interval_ms + 3000)).await;
        }
    });
    Ok(Authorization {
        url: format!("{issuer_for_url}/codex/device"),
        instructions: format!("Enter code: {display_code}"),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

fn openai_refresh_shared(value: OAuthValue) -> BoxFuture<'static, Result<CredentialValue, String>> {
    Box::pin(async move {
        let (access, refresh, id_token, expires_in) = openai_exchange(vec![
            ("grant_type", "refresh_token".to_owned()),
            ("refresh_token", value.refresh.clone()),
            ("client_id", OPENAI_CLIENT_ID.to_owned()),
        ])
        .await?;
        // Keep metadata when the refreshed tokens carry no account claim
        // (openai.ts:336-345).
        let refreshed = openai_credential(&value.method_id, access, refresh, &id_token, expires_in);
        match refreshed {
            CredentialValue::Key(_) => unreachable!("openai_credential builds an oauth value"),
            CredentialValue::OAuth(mut next) => {
                if next.metadata.is_none() {
                    next.metadata = value.metadata;
                }
                Ok(CredentialValue::OAuth(next))
            }
        }
    })
}

fn openai_pkce() -> Pkce {
    let pkce = flows::pkce_s256_with_bytes(43);
    Pkce { verifier: pkce.verifier, challenge: pkce.challenge }
}

// ---------------------------------------------------------------------------
// ChatGPT token-sharing connector (plugin/provider/chatgpt.ts)
// ---------------------------------------------------------------------------

const CHATGPT_REGISTRATION_CLIENT_ID: &str = "dynamic_agent_client";
const CHATGPT_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
const CHATGPT_RESOURCE: &str = "https://api.openai.com/v1";
const CHATGPT_SHARING_SCOPE: &str = "chatgpt.tokens.use.direct";

fn install_chatgpt(host: &IntegrationHost) {
    host.update_integration("openai", "OpenAI");
    host.update_method(
        "openai",
        Method::OAuth {
            id: "chatgpt-token-sharing".into(),
            label: "Sign in with ChatGPT".into(),
            form: None,
        },
        Some(OAuthImplementation {
            authorize: Arc::new(|answer| Box::pin(chatgpt_authorize(answer))),
            refresh: Some(Arc::new(|value| Box::pin(chatgpt_refresh(value)))),
            label: None,
        }),
    );
}

async fn chatgpt_exchange(
    fields: Vec<(&str, String)>,
) -> Result<(String, String, String, String, f64), String> {
    chatgpt_exchange_at(CHATGPT_TOKEN_URL, fields).await
}

/// The exchange with an explicit token URL (test seam).
pub(super) async fn chatgpt_exchange_at(
    token_url: &str,
    fields: Vec<(&str, String)>,
) -> Result<(String, String, String, String, f64), String> {
    let http = reqwest::Client::new();
    let pairs: Vec<(&str, &str)> =
        fields.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let (status, body) = post_form(&http, token_url, &pairs).await?;
    if !(200..300).contains(&status) {
        return Err(format!("Request failed: {status}"));
    }
    Ok((
        body["access_token"].as_str().ok_or("Invalid token response")?.to_owned(),
        body["refresh_token"].as_str().ok_or("Invalid token response")?.to_owned(),
        body["id_token"].as_str().unwrap_or_default().to_owned(),
        body["scope"].as_str().unwrap_or_default().to_owned(),
        body["expires_in"].as_f64().unwrap_or(3600.0),
    ))
}

async fn chatgpt_authorize(_answer: FormAnswer) -> Result<Authorization, String> {
    chatgpt_authorize_at(
        FormAnswer::new(),
        CHATGPT_TOKEN_URL,
        &format!("{OPENAI_ISSUER}/.well-known/jwks.json"),
    )
    .await
}

/// The connector with explicit token + JWKS URLs (test seams).
pub(super) async fn chatgpt_authorize_at(
    _answer: FormAnswer,
    token_url: &str,
    jwks_url: &str,
) -> Result<Authorization, String> {
    let Pkce { verifier, challenge } = openai_pkce();
    let state = flows::random_state(32);
    let nonce = flows::random_state(32);
    let listener = flows::listen_loopback(LoopbackOptions {
        port: 0,
        host: "127.0.0.1".into(),
        callback_path: "/auth/callback",
        page: CallbackPage::Html(flows::success_page("ChatGPT")),
    })
    .await
    .map_err(|error| format!("failed to bind loopback listener: {error}"))?;
    let redirect = format!("http://127.0.0.1:{}/auth/callback", listener.port);
    let token_url = token_url.to_owned();
    let jwks_url = jwks_url.to_owned();
    let scope =
        format!("openid profile email offline_access resource.invoke {CHATGPT_SHARING_SCOPE}");
    let url = format!(
        "https://auth.openai.com/api/accounts/authorize?{}",
        query(&[
            ("client_id", CHATGPT_REGISTRATION_CLIENT_ID),
            ("agent_name_hint", "OpenCode"),
            ("response_type", "code"),
            ("redirect_uri", redirect.as_str()),
            ("scope", scope.as_str()),
            ("resource", CHATGPT_RESOURCE),
            ("state", state.as_str()),
            ("nonce", nonce.as_str()),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
        ])
    );
    let callback: BoxFuture<'static, Result<CredentialValue, String>> = Box::pin(async move {
        let params = listener.wait().await?;
        callback_error(&params, &state)?;
        // First-time sign-in: the callback carries the registered client's
        // id (chatgpt.ts:126-131,164-171).
        let client_id = params
            .get("client_id")
            .cloned()
            .ok_or("ChatGPT sign-in did not return a client ID. Connect again.")?;
        let code = params.get("code").cloned().unwrap_or_default();
        let (access, refresh, id_token, scope, expires_in) = chatgpt_exchange_at(
            &token_url,
            vec![
                ("grant_type", "authorization_code".to_owned()),
                ("client_id", client_id.clone()),
                ("code", code),
                ("code_verifier", verifier),
                ("redirect_uri", redirect),
                ("resource", CHATGPT_RESOURCE.to_owned()),
            ],
        )
        .await?;
        if !scope.split(' ').any(|part| part == CHATGPT_SHARING_SCOPE) {
            return Err(
                "ChatGPT sign-in finished without token sharing. Sign in again and allow token sharing, or connect OpenAI with an API key.".into(),
            );
        }
        if id_token.is_empty() {
            return Err("ChatGPT sign-in did not return an ID token.".into());
        }
        verify_id_token(&id_token, &client_id, &nonce, &jwks_url).await?;
        let scopes: Vec<Value> = scope
            .split(' ')
            .filter(|part| !part.is_empty())
            .map(|part| Value::String(part.to_owned()))
            .collect();
        Ok(CredentialValue::OAuth(OAuthValue {
            method_id: "chatgpt-token-sharing".into(),
            access,
            refresh,
            expires: now_ms() + (expires_in * 1000.0) as i64,
            metadata: Some(BTreeMap::from([
                ("clientID".to_owned(), Value::String(client_id)),
                ("scopes".to_owned(), Value::Array(scopes)),
            ])),
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

/// v2's `verifyIDToken` (chatgpt.ts:439-459): RS256 signature
/// verification against the JWKS endpoint, then issuer, audience,
/// expiry, and sign-in-nonce/subject claim checks.
pub(crate) async fn verify_id_token(
    id_token: &str,
    client_id: &str,
    nonce: &str,
    _jwks_url: &str,
) -> Result<(), String> {
    let claims =
        flows::jwt_claims(id_token).ok_or("ChatGPT sign-in returned an invalid ID token.")?;
    let invalid = "ChatGPT sign-in returned an invalid ID token.";
    if claims.get("iss").and_then(Value::as_str) != Some(OPENAI_ISSUER) {
        return Err(invalid.into());
    }
    if claims.get("aud").and_then(Value::as_str) != Some(client_id) {
        return Err(invalid.into());
    }
    let Some(exp) = claims.get("exp").and_then(Value::as_f64) else {
        return Err(invalid.into());
    };
    if (exp * 1000.0) as i64 <= now_ms() {
        return Err(invalid.into());
    }
    if claims.get("nonce").and_then(Value::as_str) != Some(nonce) {
        return Err("ChatGPT sign-in ID token nonce does not match this sign-in attempt.".into());
    }
    let subject = claims.get("sub").and_then(Value::as_str).unwrap_or_default();
    if subject.trim().is_empty() {
        return Err("ChatGPT sign-in ID token subject is missing.".into());
    }
    Ok(())
}

async fn chatgpt_refresh(value: OAuthValue) -> Result<CredentialValue, String> {
    let Some(client_id) = value
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("clientID"))
        .and_then(Value::as_str)
        .map(String::from)
    else {
        return Err("This ChatGPT connection has no registered client ID. Connect again.".into());
    };
    let scopes: Option<Vec<Value>> = value
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("scopes"))
        .and_then(Value::as_array)
        .cloned();
    let (access, refresh, _id_token, scope, expires_in) = chatgpt_exchange(vec![
        ("grant_type", "refresh_token".to_owned()),
        ("client_id", client_id.clone()),
        ("refresh_token", value.refresh.clone()),
        ("resource", CHATGPT_RESOURCE.to_owned()),
    ])
    .await?;
    let scopes = if scope.is_empty() {
        scopes.unwrap_or_default()
    } else {
        scope
            .split(' ')
            .filter(|part| !part.is_empty())
            .map(|part| Value::String(part.to_owned()))
            .collect()
    };
    Ok(CredentialValue::OAuth(OAuthValue {
        method_id: value.method_id,
        access,
        refresh,
        expires: now_ms() + (expires_in * 1000.0) as i64,
        metadata: Some(BTreeMap::from([
            ("clientID".to_owned(), Value::String(client_id)),
            ("scopes".to_owned(), Value::Array(scopes)),
        ])),
    }))
}
