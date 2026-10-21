//! MCP OAuth — port of `packages/core/src/mcp/oauth.ts` (PLUG-D30).
//!
//! The authorization-code + PKCE login flow for remote MCP servers:
//! RFC 9728 resource discovery (the server's 401 `WWW-Authenticate`
//! challenge names its resource metadata, or
//! `/.well-known/oauth-protected-resource` is probed), RFC 8414
//! authorization-server metadata (or a configured
//! `auth_server_metadata_url` stands in for discovery), dynamic client
//! registration (RFC 7591) unless the config pins a `client_id`, a
//! loopback `127.0.0.1` callback listener for the browser redirect, the
//! authorization-code exchange at the token endpoint, and the
//! refresh-token grant with rotation on reconnect.
//!
//! Credentials are stored as `Credential.OAuth` rows in the shared
//! `credential` table (keyed by the registry-derived integration id,
//! `mcp_<sha1(name\0url)[0..16]>`) — the same rows the integration
//! host serves, so a login survives process restarts.
//!
//! Deviations from v2 (documented, behavior-preserving):
//! - the authorize flow runs where it is invoked (the CLI `mcp auth`
//!   command drives it in-process); v2 registers a server-side
//!   `authorize` hook on the integration — the CLI-side port produces
//!   the identical user flow (printed URL → browser → loopback
//!   callback → stored credential) without the engine-side
//!   integration registration this port's registry cannot reach;
//! - CIMD (client_id_metadata_document_supported) is not pre-selected:
//!   the flow registers dynamically (DCR) or uses the configured
//!   client, which covers the interop-tested servers.

use std::collections::BTreeMap;

use base64::Engine as _;
use rand::RngCore;
use serde_json::Value;

use crate::integration::types::{CredentialValue, OAuthValue};
// Re-exported for the flow's callers (the CLI drives the auto callback).
pub use crate::integration::{AuthMode, Authorization};
use oc_schema::mcp::{OAuthConfig, OAuthValue as SchemaOAuthValue, RemoteConfig};

/// v2 refresh margin (integration.ts:710): refresh when the token
/// expires within 5 minutes.
pub const REFRESH_MARGIN_MS: i64 = 5 * 60 * 1000;

/// `CLIENT_METADATA_URL`-equivalent client identity (CIMD is not
/// pre-selected; DCR posts these fields).
const CLIENT_NAME: &str = "opencode";
const CLIENT_URI: &str = "https://opencode.ai";

/// Metadata keys on the stored credential (oauth.ts `toCredential`).
const META_SERVER_URL: &str = "serverUrl";
const META_TOKEN_TYPE: &str = "tokenType";
const META_SCOPE: &str = "scope";
const META_ISSUER: &str = "issuer";
const META_CLIENT: &str = "client";

/// The token endpoint's answer (oauth.ts `StoredOAuthTokens`).
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
pub struct StoredTokens {
    #[serde(alias = "accessToken")]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default = "default_token_type")]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
}

fn default_token_type() -> String {
    "Bearer".to_owned()
}

/// A registered OAuth client (oauth.ts `StoredOAuthClientInformation`).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredClient {
    pub client_id: String,
    pub client_secret: Option<String>,
}

/// The credential-table view of one stored sign-in: everything the
/// connect-time provider needs (oauth.ts `connectProvider`'s store).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredCredential {
    pub id: String,
    pub method_id: String,
    pub access: String,
    pub refresh: String,
    /// Absolute epoch-millis deadline; `0` = never expires.
    pub expires: i64,
    pub token_type: String,
    pub scope: Option<String>,
    pub client: Option<StoredClient>,
}

fn metadata_string(metadata: &Option<BTreeMap<String, Value>>, key: &str) -> Option<String> {
    metadata.as_ref()?.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn metadata_client(metadata: &Option<BTreeMap<String, Value>>) -> Option<StoredClient> {
    let client = metadata.as_ref()?.get(META_CLIENT)?;
    Some(StoredClient {
        client_id: client.get("client_id").and_then(Value::as_str)?.to_owned(),
        client_secret: client
            .get("client_secret")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// `toCredential` (oauth.ts:238-258): the token answer + client over
/// the v2 `Credential.OAuth` shape. `expires == 0` means non-expiring.
#[must_use]
pub fn to_credential_value(
    method_id: &str,
    server_url: &str,
    tokens: &StoredTokens,
    client: Option<&StoredClient>,
) -> CredentialValue {
    let now = crate::database::migrations::now_ms();
    let mut metadata = BTreeMap::new();
    metadata.insert(META_SERVER_URL.to_owned(), Value::String(server_url.to_owned()));
    metadata.insert(
        META_TOKEN_TYPE.to_owned(),
        Value::String(if tokens.token_type.is_empty() {
            "Bearer".to_owned()
        } else {
            tokens.token_type.clone()
        }),
    );
    if let Some(scope) = &tokens.scope {
        metadata.insert(META_SCOPE.to_owned(), Value::String(scope.clone()));
    }
    if let Some(issuer) = &tokens.issuer {
        metadata.insert(META_ISSUER.to_owned(), Value::String(issuer.clone()));
    }
    if let Some(client) = client {
        let mut value = serde_json::Map::new();
        value.insert("client_id".to_owned(), Value::String(client.client_id.clone()));
        if let Some(secret) = &client.client_secret {
            value.insert("client_secret".to_owned(), Value::String(secret.clone()));
        }
        metadata.insert(META_CLIENT.to_owned(), Value::Object(value));
    }
    CredentialValue::OAuth(OAuthValue {
        method_id: method_id.to_owned(),
        refresh: tokens.refresh_token.clone().unwrap_or_default(),
        access: tokens.access_token.clone(),
        expires: tokens.expires_in.map_or(0, |seconds| {
            now.saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX / 4) * 1000)
        }),
        metadata: Some(metadata),
    })
}

/// `toTokens` (oauth.ts:260-270): the stored credential back into the
/// token shape (the SDK/provider view).
#[must_use]
pub fn tokens_from_value(value: &OAuthValue) -> StoredTokens {
    let metadata = &value.metadata;
    StoredTokens {
        access_token: value.access.clone(),
        refresh_token: (!value.refresh.is_empty()).then(|| value.refresh.clone()),
        token_type: metadata_string(metadata, META_TOKEN_TYPE)
            .unwrap_or_else(|| "Bearer".to_owned()),
        expires_in: (value.expires != 0)
            .then(|| u64::try_from((value.expires - now_ms()).max(0) / 1000).unwrap_or(0)),
        scope: metadata_string(metadata, META_SCOPE),
        issuer: metadata_string(metadata, META_ISSUER),
    }
}

/// The credential-table row view (id + the OAuthValue fields the
/// connect-time provider consults).
#[must_use]
pub fn stored_credential(id: &str, value: &OAuthValue) -> StoredCredential {
    StoredCredential {
        id: id.to_owned(),
        method_id: value.method_id.clone(),
        access: value.access.clone(),
        refresh: value.refresh.clone(),
        expires: value.expires,
        token_type: metadata_string(&value.metadata, META_TOKEN_TYPE)
            .unwrap_or_else(|| "Bearer".to_owned()),
        scope: metadata_string(&value.metadata, META_SCOPE),
        client: metadata_client(&value.metadata),
    }
}

/// Applies a rotated token answer onto a stored credential value,
/// keeping the row's metadata client (the SDK's `saveTokens`).
#[must_use]
pub fn value_with_tokens(previous: &OAuthValue, tokens: &StoredTokens) -> OAuthValue {
    let mut metadata = previous.metadata.clone().unwrap_or_default();
    if let Some(scope) = &tokens.scope {
        metadata.insert(META_SCOPE.to_owned(), Value::String(scope.clone()));
    }
    if let Some(issuer) = &tokens.issuer {
        metadata.insert(META_ISSUER.to_owned(), Value::String(issuer.clone()));
    }
    metadata.insert(
        META_TOKEN_TYPE.to_owned(),
        Value::String(if tokens.token_type.is_empty() {
            "Bearer".to_owned()
        } else {
            tokens.token_type.clone()
        }),
    );
    OAuthValue {
        method_id: previous.method_id.clone(),
        refresh: tokens.refresh_token.clone().unwrap_or_else(|| previous.refresh.clone()),
        access: tokens.access_token.clone(),
        expires: tokens.expires_in.map_or(0, |seconds| {
            now_ms().saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX / 4) * 1000)
        }),
        metadata: Some(metadata),
    }
}

fn now_ms() -> i64 {
    crate::database::migrations::now_ms()
}

// ---------------------------------------------------------------------------
// discovery (oauth.ts:105-122 + the SDK's RFC 9728/8414 walk)
// ---------------------------------------------------------------------------

/// The authorization-server metadata the flow needs.
#[derive(Debug, Clone)]
pub struct Discovery {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scope: Option<String>,
}

/// `{origin+path}/.well-known/oauth-protected-resource` — the RFC 9728
/// well-known location for a resource server URL.
#[must_use]
pub fn protected_resource_url(server_url: &str) -> Option<String> {
    let url = reqwest::Url::parse(server_url).ok()?;
    let authority = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str()?),
        None => url.host_str()?.to_owned(),
    };
    let path = url.path().trim_end_matches('/');
    Some(format!(
        "{scheme}://{authority}/.well-known/oauth-protected-resource{path}",
        scheme = url.scheme(),
    ))
}

/// `{server}/.well-known/oauth-authorization-server` (RFC 8414 style
/// suffix; the interop-tested servers publish it at the root).
#[must_use]
pub fn authorization_server_metadata_url(authorization_server: &str) -> Option<String> {
    let url = reqwest::Url::parse(authorization_server).ok()?;
    let authority = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str()?),
        None => url.host_str()?.to_owned(),
    };
    Some(format!("{scheme}://{authority}/.well-known/oauth-authorization-server", scheme = url.scheme()))
}

async fn get_json(http: &reqwest::Client, url: &str) -> Result<Value, String> {
    let response = http
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| format!("HTTP request to {url} failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status} trying to load {url}"));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| format!("invalid JSON from {url}: {error}"))
}

fn parse_metadata(body: &Value) -> Result<Discovery, String> {
    let authorization = body
        .get("authorization_endpoint")
        .and_then(Value::as_str)
        .ok_or("metadata has no authorization_endpoint")?
        .to_owned();
    let token = body
        .get("token_endpoint")
        .and_then(Value::as_str)
        .ok_or("metadata has no token_endpoint")?
        .to_owned();
    let scope = body
        .get("scopes_supported")
        .and_then(Value::as_array)
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|joined| !joined.is_empty());
    Ok(Discovery {
        authorization_endpoint: authorization,
        token_endpoint: token,
        registration_endpoint: body
            .get("registration_endpoint")
            .and_then(Value::as_str)
            .map(str::to_owned),
        scope,
    })
}

/// `configuredDiscovery` (oauth.ts:105-122): a configured
/// `auth_server_metadata_url` stands in for RFC 9728 discovery.
async fn configured_discovery(http: &reqwest::Client, oauth: &OAuthConfig) -> Option<Discovery> {
    let url = oauth.auth_server_metadata_url.as_deref()?;
    let body = get_json(http, url).await.ok()?;
    parse_metadata(&body).ok()
}

/// The RFC 9728 walk: the resource server's protected-resource metadata
/// names its authorization servers; the first one's metadata document
/// answers (oauth.ts `discoverOAuthServerInfo`).
async fn discover(
    http: &reqwest::Client,
    _oauth: &OAuthConfig,
    resource_metadata_url: Option<&str>,
    server_url: &str,
) -> Result<Discovery, String> {
    let probe = resource_metadata_url
        .map(str::to_owned)
        .or_else(|| protected_resource_url(server_url))
        .ok_or_else(|| format!("cannot derive the resource metadata URL for {server_url}"))?;
    let resource = get_json(http, &probe).await?;
    let server = resource
        .get("authorization_servers")
        .and_then(Value::as_array)
        .and_then(|servers| servers.first())
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{probe} lists no authorization_servers"))?
        .to_owned();
    let metadata_url = authorization_server_metadata_url(&server)
        .ok_or_else(|| format!("invalid authorization server url {server}"))?;
    let body = get_json(http, &metadata_url).await?;
    parse_metadata(&body)
}

/// The connect-time discovery: configured first, RFC 9728 second.
async fn discovery_for(
    http: &reqwest::Client,
    oauth: &OAuthConfig,
    resource_metadata_url: Option<&str>,
    server_url: &str,
) -> Result<Discovery, String> {
    if let Some(found) = configured_discovery(http, oauth).await {
        return Ok(found);
    }
    discover(http, oauth, resource_metadata_url, server_url).await
}

// ---------------------------------------------------------------------------
// the 401 challenge (oauth.ts:399-421)
// ---------------------------------------------------------------------------

/// What the resource server's 401 advertised.
#[derive(Debug, Clone, Default)]
pub struct Challenge {
    pub resource_metadata_url: Option<String>,
    pub scope: Option<String>,
}

/// Parses a `WWW-Authenticate` header's params (`resource_metadata="…"`,
/// `scope="…"`) — quoted or bare. The leading auth scheme (`Bearer `,…)
/// is stripped first.
fn www_authenticate_params(header: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    // Drop the scheme token before the first space (`Bearer realm=…`).
    // ONE split — a quoted value may itself contain spaces.
    let params = header
        .split_once(' ')
        .map(|(_, rest)| rest)
        .filter(|rest| rest.contains('='))
        .map_or_else(|| header.to_owned(), str::to_owned);
    for part in params.split(',') {
        let Some((key, value)) = part.split_once('=') else { continue };
        let key = key.trim();
        let mut value = value.trim();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = &value[1..value.len() - 1];
        }
        out.insert(key.to_owned(), value.to_owned());
    }
    out
}

/// POSTs the MCP initialize request and extracts the 401's
/// `WWW-Authenticate` challenge (5s bound, failures tolerated —
/// discovery falls back to the well-known probe).
pub async fn challenge(http: &reqwest::Client, config: &RemoteConfig) -> Challenge {
    let attempt = async {
        let response = http
            .post(&config.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 0,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": CLIENT_NAME },
                },
            })
            .to_string())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let header = response
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        Ok::<_, String>(header)
    };
    let Ok(header) = tokio::time::timeout(std::time::Duration::from_secs(5), attempt).await else {
        return Challenge::default();
    };
    let Some(header) = header.ok().flatten() else { return Challenge::default() };
    let params = www_authenticate_params(&header);
    Challenge {
        resource_metadata_url: params.get("resource_metadata").cloned(),
        scope: params.get("scope").cloned(),
    }
}

// ---------------------------------------------------------------------------
// PKCE + state
// ---------------------------------------------------------------------------

fn base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_bytes(count: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; count];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

fn code_challenge(verifier: &str) -> String {
    use sha2::Digest as _;
    base64url(&sha2::Sha256::digest(verifier.as_bytes()))
}

// ---------------------------------------------------------------------------
// client registration (DCR, oauth.ts's clientInformation + the SDK's register)
// ---------------------------------------------------------------------------

async fn register_client(
    http: &reqwest::Client,
    discovery: &Discovery,
    redirect_url: &str,
    oauth: &OAuthConfig,
) -> Result<StoredClient, String> {
    let endpoint = discovery
        .registration_endpoint
        .as_deref()
        .ok_or("the authorization server publishes no registration_endpoint (configure oauth.client_id)")?;
    let mut metadata = serde_json::Map::new();
    metadata.insert("redirect_uris".to_owned(), Value::Array(vec![Value::String(redirect_url.to_owned())]));
    metadata.insert("client_name".to_owned(), Value::String(CLIENT_NAME.to_owned()));
    metadata.insert("client_uri".to_owned(), Value::String(CLIENT_URI.to_owned()));
    metadata.insert(
        "grant_types".to_owned(),
        Value::Array(
            ["authorization_code", "refresh_token"]
                .iter()
                .map(|grant| Value::String((*grant).to_owned()))
                .collect(),
        ),
    );
    metadata.insert("response_types".to_owned(), Value::Array(vec![Value::String("code".to_owned())]));
    metadata.insert(
        "token_endpoint_auth_method".to_owned(),
        Value::String(
            if oauth.client_secret.is_some() { "client_secret_post" } else { "none" }.to_owned(),
        ),
    );
    if let Some(scope) = &oauth.scope {
        metadata.insert("scope".to_owned(), Value::String(scope.clone()));
    }
    let response = http
        .post(endpoint)
        .json(&Value::Object(metadata))
        .send()
        .await
        .map_err(|error| format!("client registration failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("client registration rejected: HTTP {status} {text}"));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("client registration returned invalid JSON: {error}"))?;
    let client_id = body
        .get("client_id")
        .and_then(Value::as_str)
        .ok_or("client registration returned no client_id")?
        .to_owned();
    Ok(StoredClient {
        client_id,
        client_secret: body.get("client_secret").and_then(Value::as_str).map(str::to_owned),
    })
}

// ---------------------------------------------------------------------------
// the token endpoint
// ---------------------------------------------------------------------------

async fn post_token(
    http: &reqwest::Client,
    endpoint: &str,
    pairs: Vec<(&str, String)>,
) -> Result<StoredTokens, String> {
    let response = http
        .post(endpoint)
        .form(&pairs)
        .send()
        .await
        .map_err(|error| format!("token request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("token endpoint rejected the request: HTTP {status} {text}"));
    }
    response
        .json::<StoredTokens>()
        .await
        .map_err(|error| format!("token endpoint returned invalid JSON: {error}"))
}

// ---------------------------------------------------------------------------
// authorize (oauth.ts:330-532)
// ---------------------------------------------------------------------------

/// Everything the authorize flow needs.
pub struct AuthorizeInput {
    /// The configured server name (labels the instructions page).
    pub name: String,
    pub config: RemoteConfig,
    /// The integration method id (stored on the credential).
    pub method_id: String,
    pub http: reqwest::Client,
}

/// The OAuth config of a remote server (`oauth: false` disables).
fn oauth_config(config: &RemoteConfig) -> Option<OAuthConfig> {
    match &config.oauth {
        None => Some(OAuthConfig::default()),
        Some(SchemaOAuthValue::Config(config)) => Some(config.clone()),
        Some(SchemaOAuthValue::Disabled(_)) => None,
    }
}

/// `authorize` (oauth.ts:330-532): bind the loopback listener, discover,
/// register, build the authorization URL (PKCE + state), and return the
/// [`Authorization`] whose auto callback waits for the browser redirect,
/// exchanges the code, and yields the credential to persist.
///
/// # Errors
/// v2-shaped flow failures (no authorization endpoint, registration
/// rejected, non-http(s) authorization URL).
pub async fn authorize(input: AuthorizeInput) -> Result<Authorization, String> {
    let oauth = oauth_config(&input.config)
        .ok_or_else(|| format!("MCP server \"{}\" has OAuth disabled", input.name))?;
    let challenge = challenge(&input.http, &input.config).await;
    let discovery =
        discovery_for(&input.http, &oauth, challenge.resource_metadata_url.as_deref(), &input.config.url)
            .await
            .map_err(|error| {
                format!("MCP server \"{}\": OAuth discovery failed: {error}", input.name)
            })?;

    // callback_port, else the port pinned by redirect_uri, else ephemeral.
    let redirect = oauth.redirect_uri.as_deref().and_then(|raw| reqwest::Url::parse(raw).ok());
    let redirect_path = redirect
        .as_ref()
        .map(|url| url.path().to_owned())
        .unwrap_or_else(|| "/callback".to_owned());
    let redirect_port = redirect.as_ref().and_then(|url| url.port()).map(u16::from);
    let bind_port = oauth.callback_port.or(redirect_port).unwrap_or(0);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", bind_port))
        .await
        .map_err(|error| format!("cannot bind the OAuth callback listener on 127.0.0.1:{bind_port}: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("could not determine the MCP OAuth callback port: {error}"))?
        .port();
    let redirect_url =
        oauth.redirect_uri.clone().unwrap_or_else(|| format!("http://127.0.0.1:{port}{redirect_path}"));

    // Static client or DCR (a configured client_id always wins).
    let client = match (&oauth.client_id, &oauth.client_secret) {
        (Some(client_id), client_secret) => Some(StoredClient {
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
        }),
        _ => Some(
            register_client(&input.http, &discovery, &redirect_url, &oauth)
                .await
                .map_err(|error| format!("MCP server \"{}\": {error}", input.name))?,
        ),
    };

    let state = base64url(&random_bytes(32));
    let verifier = base64url(&random_bytes(64));
    let scope = oauth
        .scope
        .clone()
        .or_else(|| challenge.scope.clone())
        .or_else(|| discovery.scope.clone());

    let mut authorization_url = reqwest::Url::parse(&discovery.authorization_endpoint)
        .map_err(|error| {
            format!(
                "MCP server \"{}\" returned an invalid authorization URL ({}): {error}",
                input.name, discovery.authorization_endpoint
            )
        })?;
    if !matches!(authorization_url.scheme(), "http" | "https") {
        return Err(format!(
            "MCP server \"{}\" returned a {} authorization URL; only http and https are supported",
            input.name,
            authorization_url.scheme()
        ));
    }
    let client_id = client.as_ref().map(|client| client.client_id.clone()).unwrap_or_default();
    {
        let mut pairs = authorization_url.query_pairs_mut();
        pairs.append_pair("response_type", "code");
        pairs.append_pair("client_id", &client_id);
        pairs.append_pair("redirect_uri", &redirect_url);
        pairs.append_pair("state", &state);
        pairs.append_pair("code_challenge", &code_challenge(&verifier));
        pairs.append_pair("code_challenge_method", "S256");
        if let Some(scope) = &scope {
            pairs.append_pair("scope", scope);
        }
        if let Ok(identity) = reqwest::Url::parse(&input.config.url) {
            if let Some(resource) = identity.host_str() {
                pairs.append_pair(
                    "resource",
                    &identity
                        .port()
                        .map_or_else(|| resource.to_owned(), |port| format!("{resource}:{port}")),
                );
            }
        }
    }

    let name = input.name.clone();
    let method_id = input.method_id.clone();
    let server_url = input.config.url.clone();
    let token_endpoint = discovery.token_endpoint.clone();
    let expected_state = state.clone();
    let expected_path = redirect_path.clone();
    let verifier = verifier.clone();
    let redirect_for_exchange = redirect_url.clone();
    let client_for_exchange = client.clone();
    let http = input.http.clone();

    // The auto callback (oauth.ts:359-382 + 477-516): one loopback
    // request → validated code → token exchange → the credential.
    let callback: futures::future::BoxFuture<'static, Result<CredentialValue, String>> = {
        use futures::FutureExt as _;
        async move {
            loop {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .map_err(|error| format!("OAuth callback listener failed: {error}"))?;
                let request = read_http_request(&mut stream).await?;
                let url = reqwest::Url::parse(&format!("http://127.0.0.1{}", request.target))
                    .map_err(|error| format!("invalid OAuth callback request: {error}"))?;
                if url.path() != expected_path {
                    write_http_response(&mut stream, 404, "Not found").await;
                    continue;
                }
                let query: BTreeMap<String, String> =
                    url.query_pairs().map(|(key, value)| (key.into_owned(), value.into_owned())).collect();
                if let Some(error) = query
                    .get("error_description")
                    .or_else(|| query.get("error"))
                {
                    write_http_response(
                        &mut stream,
                        400,
                        &format!("Authorization failed: {error}"),
                    )
                    .await;
                    return Err(error.clone());
                }
                if query.get("state").map(String::as_str) != Some(expected_state.as_str()) {
                    write_http_response(&mut stream, 400, "OAuth state mismatch").await;
                    return Err("OAuth state mismatch".to_owned());
                }
                let Some(code) = query.get("code") else {
                    write_http_response(&mut stream, 400, "Missing authorization code").await;
                    return Err("Missing authorization code".to_owned());
                };
                // exchange, then answer the browser (the page waits for
                // the outcome so it never reports success for a rejected
                // code — oauth.ts:377-379).
                let tokens = exchange_code(&http, &token_endpoint, &redirect_for_exchange, code, &verifier, client_for_exchange.as_ref()).await;
                match tokens {
                    Ok(tokens) => {
                        write_http_response(&mut stream, 200, &format!(
                            "Authorized {name}. You can close this window."
                        ))
                        .await;
                        return Ok(to_credential_value(&method_id, &server_url, &tokens, client_for_exchange.as_ref()));
                    }
                    Err(error) => {
                        write_http_response(&mut stream, 400, &format!("Token exchange failed: {error}")).await;
                        return Err(error);
                    }
                }
            }
        }
        .boxed()
    };

    Ok(Authorization {
        url: authorization_url.to_string(),
        instructions: format!(
            "Authorize {} in your browser. This window will close automatically.",
            input.name
        ),
        expires_at: None,
        mode: AuthMode::Auto(callback),
    })
}

/// The authorization-code exchange (the SDK `auth(..., authorizationCode)`).
async fn exchange_code(
    http: &reqwest::Client,
    token_endpoint: &str,
    redirect_url: &str,
    code: &str,
    verifier: &str,
    client: Option<&StoredClient>,
) -> Result<StoredTokens, String> {
    let mut pairs = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("code", code.to_owned()),
        ("redirect_uri", redirect_url.to_owned()),
        ("code_verifier", verifier.to_owned()),
    ];
    if let Some(client) = client {
        pairs.push(("client_id", client.client_id.clone()));
        if let Some(secret) = &client.client_secret {
            pairs.push(("client_secret", secret.clone()));
        }
    }
    post_token(http, token_endpoint, pairs).await
}

// ---------------------------------------------------------------------------
// refresh (oauth.ts:28-53 + the SDK provider's refresh grant)
// ---------------------------------------------------------------------------

/// The refresh grant: rotates tokens for one stored sign-in. Discovery
/// re-runs at refresh time exactly like the SDK's connect-time provider.
///
/// # Errors
/// Flow failures (no token endpoint reachable, rejected grant).
pub async fn refresh(
    http: &reqwest::Client,
    config: &RemoteConfig,
    credential: &StoredCredential,
) -> Result<StoredTokens, String> {
    let oauth = oauth_config(config)
        .ok_or_else(|| format!("MCP server \"{}\" has OAuth disabled", config.url))?;
    // No stored discovery: re-run the resource server's challenge so the
    // well-known probe starts from the advertised metadata URL (the SDK
    // discovers at refresh time the same way).
    let challenge = challenge(http, config).await;
    let discovery = discovery_for(http, &oauth, challenge.resource_metadata_url.as_deref(), &config.url)
        .await
        .map_err(|error| format!("OAuth discovery failed: {error}"))?;
    let mut pairs = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", credential.refresh.clone()),
    ];
    match &credential.client {
        Some(client) => {
            pairs.push(("client_id", client.client_id.clone()));
            if let Some(secret) = &client.client_secret {
                pairs.push(("client_secret", secret.clone()));
            }
        }
        // A flow without a registered client (a foreign credential row)
        // names the integration method as its public client id.
        None => pairs.push(("client_id", credential.method_id.clone())),
    }
    post_token(http, &discovery.token_endpoint, pairs).await
}

// ---------------------------------------------------------------------------
// connect-time preparation (the connectProvider store's tokens() contract)
// ---------------------------------------------------------------------------

/// What [`prepare_token`] decided.
#[derive(Debug)]
pub enum PreparedToken {
    /// The stored access token is usable as-is.
    Fresh { access: String },
    /// The refresh grant rotated the tokens; persist them.
    Refreshed { access: String, tokens: StoredTokens },
}

/// The connect-time token decision: fresh credentials pass through;
/// tokens inside the 5-minute expiry margin (or past expiry) refresh
/// first (integration.ts:710). A credential with no refresh token past
/// expiry surfaces its (dead) access token — the 401 path then settles
/// `needs_auth`, exactly like the SDK refusing an unrefreshable token.
pub async fn prepare_token(
    http: &reqwest::Client,
    config: &RemoteConfig,
    credential: &StoredCredential,
) -> Result<PreparedToken, String> {
    let non_expiring = credential.expires == 0;
    if non_expiring || credential.expires - now_ms() > REFRESH_MARGIN_MS {
        return Ok(PreparedToken::Fresh { access: credential.access.clone() });
    }
    if credential.refresh.is_empty() {
        return Ok(PreparedToken::Fresh { access: credential.access.clone() });
    }
    let tokens = refresh(http, config, credential).await?;
    Ok(PreparedToken::Refreshed { access: tokens.access_token.clone(), tokens })
}

// ---------------------------------------------------------------------------
// the loopback callback listener's HTTP (minimal request read / response)
// ---------------------------------------------------------------------------

struct CallbackRequest {
    target: String,
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Result<CallbackRequest, String> {
    use tokio::io::AsyncReadExt as _;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| format!("reading the OAuth callback failed: {error}"))?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        // headers end at the blank line; the callback GET has no body.
        if buffer.windows(4).any(|window| window == b"\r\n\r\n")
            || buffer.windows(2).any(|window| window == b"\n\n")
        {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buffer);
    let request_line = text.lines().next().unwrap_or_default().to_owned();
    let target = request_line.split(' ').nth(1).unwrap_or("/").to_owned();
    Ok(CallbackRequest { target })
}

async fn write_http_response(stream: &mut tokio::net::TcpStream, status: u16, body: &str) {
    use tokio::io::AsyncWriteExt as _;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let page = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>opencode MCP OAuth</title>\
         <style>body{{font-family:system-ui;margin:3rem auto;max-width:32rem;text-align:center}}</style>\
         </head><body><h2>opencode</h2><p>{body}</p></body></html>",
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_round_trip_keeps_the_client_and_expiry() {
        let tokens = StoredTokens {
            access_token: "acc".into(),
            refresh_token: Some("ref".into()),
            token_type: "Bearer".into(),
            expires_in: Some(3600),
            scope: Some("mcp".into()),
            issuer: Some("https://iss".into()),
        };
        let client = StoredClient { client_id: "cid".into(), client_secret: Some("sec".into()) };
        let value = to_credential_value("mcp_abc", "https://srv/mcp", &tokens, Some(&client));
        let CredentialValue::OAuth(oauth) = &value else { panic!("oauth value") };
        assert!(oauth.expires > now_ms(), "expiry is an absolute deadline");
        let stored = stored_credential("cred_1", oauth);
        assert_eq!(stored.access, "acc");
        assert_eq!(stored.refresh, "ref");
        assert_eq!(stored.client.as_ref(), Some(&client));
        let back = tokens_from_value(oauth);
        assert_eq!(back.access_token, "acc");
        assert_eq!(back.token_type, "Bearer");
    }

    #[test]
    fn rotation_keeps_the_previous_refresh_when_the_answer_omits_it() {
        let previous = OAuthValue {
            method_id: "mcp_abc".into(),
            refresh: "old-refresh".into(),
            access: "old-access".into(),
            expires: now_ms() - 1000,
            metadata: Some(BTreeMap::new()),
        };
        let rotated = value_with_tokens(
            &previous,
            &StoredTokens {
                access_token: "new-access".into(),
                refresh_token: None,
                token_type: "Bearer".into(),
                expires_in: Some(600),
                ..StoredTokens::default()
            },
        );
        assert_eq!(rotated.access, "new-access");
        assert_eq!(rotated.refresh, "old-refresh", "refresh tokens rotate, never vanish");
        assert!(rotated.expires > now_ms());
    }

    #[test]
    fn www_authenticate_params_parse_quoted_and_bare() {
        let params = www_authenticate_params(
            "Bearer resource_metadata=\"https://srv/.well-known/oauth-protected-resource\", scope=\"mcp:read mcp:write\"",
        );
        assert_eq!(
            params.get("resource_metadata").map(String::as_str),
            Some("https://srv/.well-known/oauth-protected-resource")
        );
        assert_eq!(params.get("scope").map(String::as_str), Some("mcp:read mcp:write"));
        // The scheme may carry a realm before the flow's params.
        let params = www_authenticate_params(
            "Bearer realm=\"mcp\", resource_metadata=\"https://srv/.well-known/oauth-protected-resource\"",
        );
        assert_eq!(
            params.get("resource_metadata").map(String::as_str),
            Some("https://srv/.well-known/oauth-protected-resource")
        );
    }

    #[test]
    fn well_known_urls_derive_from_the_resource_url() {
        assert_eq!(
            protected_resource_url("https://example.com:8443/mcp").as_deref(),
            Some("https://example.com:8443/.well-known/oauth-protected-resource/mcp")
        );
        assert_eq!(
            authorization_server_metadata_url("https://sso.example.com").as_deref(),
            Some("https://sso.example.com/.well-known/oauth-authorization-server")
        );
    }

    #[test]
    fn pkce_challenge_is_s256_of_the_verifier() {
        let verifier = base64url(&random_bytes(64));
        let challenge = code_challenge(&verifier);
        assert_eq!(challenge.len(), 43, "base64url(sha256) is 43 chars");
        assert_ne!(challenge, verifier);
    }
}
