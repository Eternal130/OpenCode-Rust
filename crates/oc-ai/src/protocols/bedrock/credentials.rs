#![allow(
    clippy::doc_markdown, // AWS/SSO/IMDS identifiers in prose stay unquoted
    clippy::items_after_statements, // leg helpers read top-down like the TS chain
)]

//! AWS default credential chain for Bedrock SigV4 (ledger AI-M10, port of
//! `packages/ai/src/protocols/utils/bedrock-auth.ts:36-55` over
//! `@aws-sdk/credential-providers` `fromNodeProviderChain`).
//!
//! Resolution order (v2 re-runs the chain on **every request** so
//! credentials rotated on disk are always re-read):
//!
//! 1. **static** — credentials configured on the route (handled by the
//!    caller, [`super::sigv4::resolve_auth`], before the chain);
//! 2. **env** — `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ optional
//!    `AWS_SESSION_TOKEN`);
//! 3. **profile** — shared credentials/config INI files
//!    (`AWS_SHARED_CREDENTIALS_FILE` / `AWS_CONFIG_FILE`, default
//!    `~/.aws/...`), profile = explicit > `AWS_PROFILE` > `default`;
//!    static keys, or the SSO leg (`sso-session`/legacy start URL + cached
//!    access token + `federation/credentials` portal call);
//! 4. **IMDS** — EC2 instance metadata (IMDSv2 token first, v1 fallback),
//!    endpoint overridable via `AWS_EC2_METADATA_SERVICE_ENDPOINT`.
//!
//! Legs the file declares but this port does not implement
//! (`credential_process`, assume-role/`source_profile`, web identity, ECS
//! container credentials) fail loudly instead of silently falling through
//! to IMDS. Network legs (SSO portal, IMDS) run over the
//! `reqwest-transport` feature against injectable endpoints — tests point
//! them at local mock servers (ADR-0006: no real endpoints).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::schema::{AiError, AiErrorReason, ReasonFields};

use super::sigv4::Credentials;

/// Default IMDS endpoint (v3 credential-provider-imds).
pub const DEFAULT_IMDS_ENDPOINT: &str = "http://169.254.169.254";

/// Per-request timeout for the network legs (v3 IMDS default, 1s).
pub const LEG_TIMEOUT: Duration = Duration::from_secs(1);

/// IMDSv2 token TTL (botocore/v3 use 21600s = 6h).
const IMDS_TOKEN_TTL_SECONDS: &str = "21600";

// ---------------------------------------------------------------------------
// Injectable sources
// ---------------------------------------------------------------------------

/// Environment view: the process env by default, an explicit map in tests.
#[derive(Debug, Clone, Default)]
pub enum EnvSource {
    #[default]
    Process,
    Map(BTreeMap<String, String>),
}

impl EnvSource {
    fn get(&self, name: &str) -> Option<String> {
        match self {
            Self::Process => std::env::var(name).ok().map(|value| value.trim().to_string()),
            Self::Map(map) => map.get(name).cloned(),
        }
    }

    fn non_empty(&self, name: &str) -> Option<String> {
        self.get(name).filter(|value| !value.is_empty())
    }
}

/// Shared-config locations (defaults follow the AWS CLI conventions).
#[derive(Debug, Clone, Default)]
pub struct ChainPaths {
    pub credentials_file: Option<PathBuf>,
    pub config_file: Option<PathBuf>,
    /// SSO token cache root (`~/.aws` holds `sso/cache/<sha1>.json`).
    pub aws_dir: Option<PathBuf>,
}

/// Network endpoints for the chain legs (defaults derived per call).
#[derive(Debug, Clone, Default)]
pub struct ChainEndpoints {
    /// SSO portal base; default `https://portal.sso.{region}.amazonaws.com`.
    pub sso_portal: Option<String>,
    /// IMDS base; default `AWS_EC2_METADATA_SERVICE_ENDPOINT` or
    /// `http://169.254.169.254`.
    pub imds: Option<String>,
}

/// Default-chain inputs (v2 `DefaultChainOptions` + injectable seams).
#[derive(Debug, Clone, Default)]
pub struct DefaultChainOptions {
    pub region: String,
    /// Shared config profile (default `AWS_PROFILE` or `default`).
    pub profile: Option<String>,
    pub env: EnvSource,
    pub paths: ChainPaths,
    pub endpoints: ChainEndpoints,
}

impl DefaultChainOptions {
    #[must_use]
    pub fn new(region: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            profile: None,
            env: EnvSource::Process,
            paths: ChainPaths::default(),
            endpoints: ChainEndpoints::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Chain resolution
// ---------------------------------------------------------------------------

/// One leg's failure (kept for the aggregate message).
struct LegError {
    leg: &'static str,
    message: String,
}

/// Resolve credentials through the default chain (v2 `defaultChain`):
/// env → profile (static keys, else SSO) → IMDS. The last leg's failure
/// becomes the aggregate `Authentication` error.
pub async fn resolve_default_chain(options: &DefaultChainOptions) -> Result<Credentials, AiError> {
    let mut failures: Vec<LegError> = Vec::new();

    if let Some(credentials) = env_leg(options) {
        return Ok(credentials);
    }
    failures.push(LegError {
        leg: "env",
        message: "AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY are not set".to_string(),
    });

    match profile_leg(options).await {
        Ok(Some(credentials)) => return Ok(credentials),
        Ok(None) => failures.push(LegError {
            leg: "profile",
            message: format!(
                "profile `{}` defines no credentials",
                effective_profile(options).unwrap_or_else(|| "default".to_string())
            ),
        }),
        Err(error) => failures.push(LegError { leg: "profile", message: error }),
    }

    match imds_leg(options).await {
        Ok(credentials) => return Ok(credentials),
        Err(error) => failures.push(LegError { leg: "imds", message: error }),
    }

    let detail = failures
        .iter()
        .map(|failure| format!("{}: {}", failure.leg, failure.message))
        .collect::<Vec<_>>()
        .join("; ");
    Err(AiError::new(AiErrorReason::Authentication {
        fields: ReasonFields::new(format!(
            "AWS default credential chain failed: could not load credentials from any providers ({detail})"
        )),
    }))
}

/// The env leg (v3 `fromEnv`).
fn env_leg(options: &DefaultChainOptions) -> Option<Credentials> {
    let access_key_id = options.env.non_empty("AWS_ACCESS_KEY_ID")?;
    let secret_access_key = options.env.non_empty("AWS_SECRET_ACCESS_KEY")?;
    Some(Credentials {
        region: options.region.clone(),
        access_key_id,
        secret_access_key,
        session_token: options.env.non_empty("AWS_SESSION_TOKEN"),
    })
}

fn effective_profile(options: &DefaultChainOptions) -> Option<String> {
    options
        .profile
        .clone()
        .or_else(|| options.env.non_empty("AWS_PROFILE"))
        .or_else(|| Some("default".to_string()))
}

/// The shared-config leg: static keys, else SSO. `Ok(None)` = the profile
/// exists but defines no credential source.
async fn profile_leg(options: &DefaultChainOptions) -> Result<Option<Credentials>, String> {
    let Some(profile) = effective_profile(options) else {
        return Ok(None);
    };
    let credentials_ini = read_ini(&credentials_path(options, "AWS_SHARED_CREDENTIALS_FILE"));
    let config_ini = read_ini(&config_path(options));
    let mut merged: BTreeMap<String, String> = BTreeMap::new();
    if let Some(sections) = config_ini.sections.get(&format!("profile {profile}")) {
        merged.extend(sections.clone());
    }
    // the credentials file may use bare `[name]` sections
    if let Some(sections) = credentials_ini.sections.get(&profile) {
        merged.extend(sections.clone());
    }
    if let Some(sections) = credentials_ini.sections.get(&format!("profile {profile}")) {
        merged.extend(sections.clone());
    }
    if merged.is_empty() {
        return Ok(None);
    }
    // static keys
    if let (Some(access_key_id), Some(secret_access_key)) =
        (merged.get("aws_access_key_id").cloned(), merged.get("aws_secret_access_key").cloned())
    {
        return Ok(Some(Credentials {
            region: options.region.clone(),
            access_key_id,
            secret_access_key,
            session_token: merged.get("aws_session_token").cloned(),
        }));
    }
    // legs we do not implement must not silently fall through to IMDS
    if merged.contains_key("credential_process") {
        return Err(format!("profile `{profile}` uses credential_process, which is not supported"));
    }
    if merged.contains_key("source_profile") || merged.contains_key("role_arn") {
        return Err(format!(
            "profile `{profile}` uses assume-role credentials, which are not supported"
        ));
    }
    if merged.contains_key("web_identity_token_file") {
        return Err(format!(
            "profile `{profile}` uses web identity credentials, which are not supported"
        ));
    }
    // SSO leg
    sso_leg(options, &merged, &profile).await.map(Some)
}

/// The SSO leg: `sso-session NAME` (config `[sso-session NAME]`) or legacy
/// `sso_start_url` on the profile, cached access token
/// (`~/.aws/sso/cache/<sha1(key)>.json`), then the federation portal
/// (`GET /federation/credentials?account_id=&role_name=` with the
/// `x-amz-sso_bearer_token` header).
async fn sso_leg(
    options: &DefaultChainOptions,
    profile: &BTreeMap<String, String>,
    profile_name: &str,
) -> Result<Credentials, String> {
    let (sso_region, cache_key) = if let Some(session_name) = profile.get("sso_session").cloned() {
        let config_ini = read_ini(&config_path(options));
        let session =
            config_ini.sections.get(&format!("sso-session {session_name}")).ok_or_else(|| {
                format!("profile `{profile_name}` references missing sso-session `{session_name}`")
            })?;
        let sso_region = session
            .get("sso_region")
            .cloned()
            .ok_or_else(|| format!("sso-session `{session_name}` has no sso_region"))?;
        if !session.contains_key("sso_start_url") {
            return Err(format!("sso-session `{session_name}` has no sso_start_url"));
        }
        // the sso-session token cache is keyed by the session name
        (sso_region, session_name)
    } else {
        let start_url = profile
            .get("sso_start_url")
            .cloned()
            .ok_or_else(|| format!("profile `{profile_name}` defines no credential source"))?;
        let sso_region = profile.get("sso_region").cloned().ok_or_else(|| {
            format!("profile `{profile_name}` has sso_start_url without sso_region")
        })?;
        // legacy SSO tokens are cached by start URL
        (sso_region, start_url)
    };
    let account_id = profile
        .get("sso_account_id")
        .ok_or_else(|| format!("profile `{profile_name}` has no sso_account_id"))?;
    let role_name = profile
        .get("sso_role_name")
        .ok_or_else(|| format!("profile `{profile_name}` has no sso_role_name"))?;

    // cached token
    let cache_dir = aws_dir(options).join("sso").join("cache");
    let digest = {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(cache_key.as_bytes());
        hasher.finalize()
    };
    let token_path = cache_dir.join(format!("{digest:x}.json"));
    let token_json: Value = std::fs::read_to_string(&token_path)
        .map_err(|error| format!("no SSO token cache at {} ({error})", token_path.display()))
        .and_then(|text| {
            serde_json::from_str(&text).map_err(|error| format!("invalid SSO token cache: {error}"))
        })?;
    let access_token = token_json
        .get("accessToken")
        .and_then(Value::as_str)
        .ok_or_else(|| "SSO token cache has no accessToken".to_string())?
        .to_string();

    // federation portal
    let base = options
        .endpoints
        .sso_portal
        .clone()
        .unwrap_or_else(|| format!("https://portal.sso.{sso_region}.amazonaws.com"));
    let url =
        format!("{base}/federation/credentials?account_id={account_id}&role_name={role_name}");
    let body = http_get_json(&url, &[("x-amz-sso_bearer_token", access_token.as_str())])
        .await
        .map_err(|error| format!("SSO role credentials failed: {error}"))?;
    let role_credentials = body
        .get("roleCredentials")
        .ok_or_else(|| "SSO portal response has no roleCredentials".to_string())?;
    let access_key_id = role_credentials
        .get("accessKeyId")
        .and_then(Value::as_str)
        .ok_or_else(|| "SSO roleCredentials has no accessKeyId".to_string())?
        .to_string();
    let secret_access_key = role_credentials
        .get("secretAccessKey")
        .and_then(Value::as_str)
        .ok_or_else(|| "SSO roleCredentials has no secretAccessKey".to_string())?
        .to_string();
    let session_token =
        role_credentials.get("sessionToken").and_then(Value::as_str).map(str::to_string);
    Ok(Credentials {
        region: options.region.clone(),
        access_key_id,
        secret_access_key,
        session_token,
    })
}

/// The IMDS leg: IMDSv2 token (`PUT /latest/api/token`) with v1 fallback,
/// role name, role credentials.
async fn imds_leg(options: &DefaultChainOptions) -> Result<Credentials, String> {
    let base = options
        .endpoints
        .imds
        .clone()
        .or_else(|| options.env.non_empty("AWS_EC2_METADATA_SERVICE_ENDPOINT"))
        .unwrap_or_else(|| DEFAULT_IMDS_ENDPOINT.to_string());
    let token = imds_token(&base).await;
    let auth_header = token.as_deref().map(|token| ("x-aws-ec2-metadata-token", token));
    let headers: &[(&str, &str)] = auth_header.as_slice();

    let role_url = format!("{base}/latest/meta-data/iam/security-credentials/");
    let role_list = http_get_text(&role_url, headers)
        .await
        .map_err(|error| format!("IMDS role name failed: {error}"))?;
    let role =
        role_list.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().to_string();
    if role.is_empty() {
        return Err("IMDS returned no IAM role".to_string());
    }
    let credentials_url = format!("{base}/latest/meta-data/iam/security-credentials/{role}");
    let body = http_get_json(&credentials_url, headers)
        .await
        .map_err(|error| format!("IMDS role credentials failed: {error}"))?;
    let access_key_id = body
        .get("AccessKeyId")
        .and_then(Value::as_str)
        .ok_or_else(|| "IMDS credentials have no AccessKeyId".to_string())?
        .to_string();
    let secret_access_key = body
        .get("SecretAccessKey")
        .and_then(Value::as_str)
        .ok_or_else(|| "IMDS credentials have no SecretAccessKey".to_string())?
        .to_string();
    let session_token = body.get("Token").and_then(Value::as_str).map(str::to_string);
    Ok(Credentials {
        region: options.region.clone(),
        access_key_id,
        secret_access_key,
        session_token,
    })
}

/// IMDSv2 token; 404/403/405 (or a disabled IMDSv2) falls back to v1
/// (no token header).
#[cfg(feature = "reqwest-transport")]
async fn imds_token(base: &str) -> Option<String> {
    let url = format!("{base}/latest/api/token");
    let response = reqwest::Client::new()
        .put(&url)
        .timeout(LEG_TIMEOUT)
        .header("x-aws-ec2-metadata-token-ttl-seconds", IMDS_TOKEN_TTL_SECONDS)
        .send()
        .await
        .ok()?;
    let status = response.status().as_u16();
    if matches!(status, 403..=405) {
        return None;
    }
    if !(200..300).contains(&status) {
        return None;
    }
    response.text().await.ok().filter(|token| !token.is_empty())
}

#[cfg(not(feature = "reqwest-transport"))]
async fn imds_token(_base: &str) -> Option<String> {
    None
}

#[cfg(feature = "reqwest-transport")]
async fn http_get_text(url: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    let mut request = reqwest::Client::new().get(url).timeout(LEG_TIMEOUT);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.map_err(|error| error.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    Ok(text)
}

#[cfg(not(feature = "reqwest-transport"))]
async fn http_get_text(_url: &str, _headers: &[(&str, &str)]) -> Result<String, String> {
    Err("SSO/IMDS legs require the reqwest-transport feature".to_string())
}

#[cfg(feature = "reqwest-transport")]
async fn http_get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value, String> {
    let text = http_get_text(url, headers).await?;
    serde_json::from_str(&text).map_err(|error| format!("invalid JSON response: {error}"))
}

#[cfg(not(feature = "reqwest-transport"))]
async fn http_get_json(_url: &str, _headers: &[(&str, &str)]) -> Result<Value, String> {
    Err("SSO/IMDS legs require the reqwest-transport feature".to_string())
}

// ---------------------------------------------------------------------------
// INI files
// ---------------------------------------------------------------------------

/// A parsed INI document: section header → key/value pairs (lowercased
/// keys, the AWS config convention).
#[derive(Debug, Default)]
pub struct Ini {
    pub sections: BTreeMap<String, BTreeMap<String, String>>,
}

/// Parse the AWS INI shape (`[default]`, `[profile x]`, `[sso-session x]`).
pub fn parse_ini(text: &str) -> Ini {
    let mut ini = Ini::default();
    let mut section: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = Some(line[1..line.len() - 1].trim().to_string());
            ini.sections.entry(section.clone().unwrap_or_default()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if let Some(section) = section.as_deref() {
            ini.sections
                .entry(section.to_string())
                .or_default()
                .insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    ini
}

fn read_ini(path: &PathBuf) -> Ini {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_ini(&text),
        Err(_) => Ini::default(),
    }
}

fn home_dir(env: &EnvSource) -> Option<PathBuf> {
    env.non_empty("HOME").map(PathBuf::from)
}

fn credentials_path(options: &DefaultChainOptions, env_override: &str) -> PathBuf {
    options
        .paths
        .credentials_file
        .clone()
        .or_else(|| options.env.non_empty(env_override).map(PathBuf::from))
        .or_else(|| home_dir(&options.env).map(|home| home.join(".aws").join("credentials")))
        .unwrap_or_else(|| PathBuf::from(""))
}

fn config_path(options: &DefaultChainOptions) -> PathBuf {
    options
        .paths
        .config_file
        .clone()
        .or_else(|| options.env.non_empty("AWS_CONFIG_FILE").map(PathBuf::from))
        .or_else(|| home_dir(&options.env).map(|home| home.join(".aws").join("config")))
        .unwrap_or_else(|| PathBuf::from(""))
}

fn aws_dir(options: &DefaultChainOptions) -> PathBuf {
    options
        .paths
        .aws_dir
        .clone()
        .or_else(|| home_dir(&options.env).map(|home| home.join(".aws")))
        .unwrap_or_else(|| PathBuf::from(".aws"))
}

// ---------------------------------------------------------------------------
// Sync bridge (the SigV4 auth definition applies synchronously)
// ---------------------------------------------------------------------------

/// A dedicated single-thread runtime for the network legs of the chain —
/// the SigV4 [`Definition`](crate::route::auth::Definition) applies
/// synchronously, but SSO/IMDS need async HTTP. Callers on a host runtime
/// block one worker for at most the ~seconds the legs' 1s timeouts allow.
#[cfg(feature = "reqwest-transport")]
pub fn bridge<T>(future: impl std::future::Future<Output = T>) -> T {
    static DEDICATED: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let runtime = DEDICATED.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("dedicated credential-chain runtime")
    });
    runtime.block_on(future)
}

#[cfg(not(feature = "reqwest-transport"))]
pub fn bridge<T>(future: impl std::future::Future<Output = T>) -> T {
    // Without the HTTP feature the chain has no network legs; nothing to
    // drive — the callers' futures resolve without awaiting I/O.
    futures::executor::block_on(future)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_parses_sections_and_ignores_comments() {
        let ini = parse_ini(
            "[default]\naws_access_key_id = akid\n; comment\nAWS_SECRET_ACCESS_KEY = secret\n[profile prod]\nsso_session = corp\n[sso-session corp]\nsso_start_url = https://x.aws/start\n",
        );
        assert_eq!(ini.sections["default"]["aws_access_key_id"], "akid");
        assert_eq!(ini.sections["default"]["aws_secret_access_key"], "secret");
        assert_eq!(ini.sections["profile prod"]["sso_session"], "corp");
        assert_eq!(ini.sections["sso-session corp"]["sso_start_url"], "https://x.aws/start");
    }

    #[test]
    fn env_leg_reads_the_injected_map() {
        let options = DefaultChainOptions {
            region: "us-east-1".into(),
            profile: None,
            env: EnvSource::Map(BTreeMap::from([
                ("AWS_ACCESS_KEY_ID".to_string(), "AKID".to_string()),
                ("AWS_SECRET_ACCESS_KEY".to_string(), "SECRET".to_string()),
                ("AWS_SESSION_TOKEN".to_string(), "TOKEN".to_string()),
            ])),
            paths: ChainPaths::default(),
            endpoints: ChainEndpoints::default(),
        };
        let credentials = env_leg(&options).unwrap();
        assert_eq!(credentials.access_key_id, "AKID");
        assert_eq!(credentials.session_token.as_deref(), Some("TOKEN"));
        assert_eq!(credentials.region, "us-east-1");
    }
}
