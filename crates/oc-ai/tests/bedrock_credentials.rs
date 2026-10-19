//! C7b AI-PROVIDERS — Bedrock `SigV4` default credential chain (ledger
//! `AI-M10`, cluster C7).
//!
//! v2 resolves Bedrock credentials through
//! `@aws-sdk/credential-providers` `fromNodeProviderChain`
//! (`packages/ai/src/protocols/utils/bedrock-auth.ts:36-55`) with the
//! priority static (route) → env → shared-config profile (static keys or
//! SSO session + cached token + federation portal) → EC2 IMDS, re-reading
//! the chain on every request.
//!
//! The file legs run against temp INI files; the SSO portal and IMDS legs
//! run against local HTTP mocks bound to `127.0.0.1:0` — no real endpoints
//! (ADR-0006). Process-env legs use injected env maps.

use std::collections::BTreeMap;
use std::path::PathBuf;

use oc_ai::protocols::bedrock::credentials::{
    resolve_default_chain, ChainEndpoints, ChainPaths, DefaultChainOptions, EnvSource,
};
use oc_ai::protocols::bedrock::sigv4::{self, Credentials};
use oc_ai::route::auth::{AuthInput, Headers};
use oc_ai::schema::AiErrorReason;

// ---------------------------------------------------------------------------
// temp config fixtures
// ---------------------------------------------------------------------------

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join("opencode");
        let dir = base.join(format!(
            "lane-ai-prov-bedrock-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("sandbox dir");
        Self { dir }
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(&path, contents).expect("write fixture");
        path
    }
}

/// A never-listening local endpoint: chain tests must never dial the real
/// link-local IMDS address (ADR-0006).
fn dead_imds() -> String {
    "http://127.0.0.1:9".to_string()
}

fn empty_env() -> EnvSource {
    EnvSource::Map(BTreeMap::new())
}

fn env_map(pairs: &[(&str, &str)]) -> EnvSource {
    EnvSource::Map(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

// ---------------------------------------------------------------------------
// Local HTTP mock (SSO portal / IMDS), 127.0.0.1:0
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "wire-record shape kept whole for assertions")]
struct RecordedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

struct HttpMock {
    port: u16,
    requests: std::sync::Arc<std::sync::Mutex<Vec<RecordedRequest>>>,
}

impl HttpMock {
    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

/// Handler: request → response, or `None` for a canned 404.
type MockHandler = std::sync::Arc<dyn Fn(&RecordedRequest) -> Option<(u16, String)> + Send + Sync>;

async fn http_mock(handler: MockHandler) -> HttpMock {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("port").port();
    let requests: std::sync::Arc<std::sync::Mutex<Vec<RecordedRequest>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    tokio::spawn({
        let requests = std::sync::Arc::clone(&requests);
        async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let requests = std::sync::Arc::clone(&requests);
                let handler = std::sync::Arc::clone(&handler);
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                    let mut buffer = [0u8; 16384];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let mut lines = text.lines();
                    let request_line = lines.next().unwrap_or_default().to_string();
                    let mut parts = request_line.split(' ');
                    let method = parts.next().unwrap_or_default().to_string();
                    let path = parts.next().unwrap_or_default().to_string();
                    let headers: Vec<(String, String)> = lines
                        .take_while(|line| !line.is_empty())
                        .filter_map(|line| line.split_once(':'))
                        .map(|(name, value)| {
                            (name.trim().to_ascii_lowercase(), value.trim().to_string())
                        })
                        .collect();
                    let body = text.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
                    let recorded = RecordedRequest {
                        method: method.clone(),
                        path: path.clone(),
                        headers: headers.clone(),
                        body: body.clone(),
                    };
                    requests
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(recorded);
                    let (status, body) = handler(&RecordedRequest { method, path, headers, body })
                        .unwrap_or((404, String::new()));
                    let reason = if status == 200 { "OK" } else { "Not Found" };
                    let reply = format!(
                        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        }
    });
    HttpMock { port, requests }
}

// ---------------------------------------------------------------------------
// Chain rows
// ---------------------------------------------------------------------------

/// Ledger repro: `profile_chain` — the shared-config profile leg and the
/// chain priority (env over profile; profile over IMDS).
#[tokio::test]
async fn profile_chain() {
    let sandbox = Sandbox::new("profile");
    let credentials_file = sandbox.write(
        "credentials",
        "[default]\naws_access_key_id = DEFAULT_AKID\naws_secret_access_key = DEFAULT_SECRET\n",
    );
    let config_file = sandbox.write(
        "config",
        "[profile prod]\naws_access_key_id = PROD_AKID\naws_secret_access_key = PROD_SECRET\naws_session_token = PROD_TOKEN\n",
    );
    let base = |env: EnvSource, profile: Option<String>| DefaultChainOptions {
        region: "us-east-1".into(),
        profile,
        env,
        paths: ChainPaths {
            credentials_file: Some(credentials_file.clone()),
            config_file: Some(config_file.clone()),
            aws_dir: None,
        },
        endpoints: ChainEndpoints { sso_portal: None, imds: Some(dead_imds()) },
    };

    // explicit profile → its static keys
    let credentials = resolve_default_chain(&base(empty_env(), Some("prod".into())))
        .await
        .expect("profile credentials");
    assert_eq!(credentials.access_key_id, "PROD_AKID");
    assert_eq!(credentials.secret_access_key, "PROD_SECRET");
    assert_eq!(credentials.session_token.as_deref(), Some("PROD_TOKEN"));

    // AWS_PROFILE selects the profile
    let credentials = resolve_default_chain(&base(env_map(&[("AWS_PROFILE", "prod")]), None))
        .await
        .expect("AWS_PROFILE credentials");
    assert_eq!(credentials.access_key_id, "PROD_AKID");

    // default profile without AWS_PROFILE
    let credentials =
        resolve_default_chain(&base(empty_env(), None)).await.expect("default profile credentials");
    assert_eq!(credentials.access_key_id, "DEFAULT_AKID");

    // env wins over the profile
    let credentials = resolve_default_chain(&base(
        env_map(&[("AWS_ACCESS_KEY_ID", "ENV_AKID"), ("AWS_SECRET_ACCESS_KEY", "ENV_SECRET")]),
        Some("prod".into()),
    ))
    .await
    .expect("env credentials");
    assert_eq!(credentials.access_key_id, "ENV_AKID");
}

#[tokio::test]
async fn profile_beats_imds() {
    let sandbox = Sandbox::new("profile-imds");
    let credentials_file = sandbox.write(
        "credentials",
        "[default]\naws_access_key_id = PROFILE_AKID\naws_secret_access_key = PROFILE_SECRET\n",
    );
    let imds = imds_mock("IMDS_AKID").await;
    let options = DefaultChainOptions {
        region: "us-east-1".into(),
        profile: None,
        env: empty_env(),
        paths: ChainPaths {
            credentials_file: Some(credentials_file),
            config_file: None,
            aws_dir: None,
        },
        endpoints: ChainEndpoints { sso_portal: None, imds: Some(imds_base(&imds)) },
    };
    let credentials = resolve_default_chain(&options).await.expect("profile wins");
    assert_eq!(credentials.access_key_id, "PROFILE_AKID");
    assert!(imds.requests().is_empty(), "IMDS never dialed");
}

/// SSO leg: profile `sso_session` → `[sso-session]` start URL/region →
/// cached token (`~/.aws/sso/cache/<sha1(start_url)>.json`) → federation
/// portal with the bearer header.
/// sha1(session name) names the sso-session token cache file (legacy
/// profiles key by the start URL instead).
fn corp_cache_name() -> String {
    use sha1::{Digest, Sha1};
    let digest = {
        let mut hasher = Sha1::new();
        hasher.update(b"corp");
        hasher.finalize()
    };
    format!("{digest:x}.json")
}

#[tokio::test]
async fn sso_chain_uses_cached_token_and_portal() {
    let sandbox = Sandbox::new("sso");
    let config_file = sandbox.write(
        "config",
        "[profile corp]\nsso_session = corp\nsso_account_id = 123456789012\nsso_role_name = Engineers\n[sso-session corp]\nsso_start_url = https://mycompany.awsapps.com/start\nsso_region = us-east-1\n",
    );
    let digest_file = corp_cache_name();
    sandbox.write(
        &format!("aws/sso/cache/{digest_file}"),
        "{\"accessToken\":\"sso-token-1\",\"expiresAt\":\"2030-01-01T00:00:00Z\"}",
    );
    let portal = http_mock(std::sync::Arc::new(|request| {
        if request.path.starts_with("/federation/credentials") {
            Some((
                200,
                "{\"roleCredentials\":{\"accessKeyId\":\"SSO_AKID\",\"secretAccessKey\":\"SSO_SECRET\",\"sessionToken\":\"SSO_TOKEN\",\"expiration\":1893456000000}}"
                    .into(),
            ))
        } else {
            None
        }
    }))
    .await;

    let options = DefaultChainOptions {
        region: "us-east-1".into(),
        profile: Some("corp".into()),
        env: empty_env(),
        paths: ChainPaths {
            credentials_file: None,
            config_file: Some(config_file),
            aws_dir: Some(sandbox.dir.join("aws")),
        },
        endpoints: ChainEndpoints {
            sso_portal: Some(format!("http://127.0.0.1:{}", portal.port)),
            imds: Some(dead_imds()),
        },
    };
    let credentials = resolve_default_chain(&options).await.expect("SSO credentials");
    assert_eq!(credentials.access_key_id, "SSO_AKID");
    assert_eq!(credentials.secret_access_key, "SSO_SECRET");
    assert_eq!(credentials.session_token.as_deref(), Some("SSO_TOKEN"));

    // the portal call wire shape
    let requests = portal.requests();
    assert_eq!(requests.len(), 1, "one portal call");
    assert_eq!(requests[0].method, "GET");
    assert!(
        requests[0]
            .path
            .starts_with("/federation/credentials?account_id=123456789012&role_name=Engineers"),
        "portal path: {}",
        requests[0].path
    );
    assert!(
        requests[0]
            .headers
            .iter()
            .any(|(name, value)| name == "x-amz-sso_bearer_token" && value == "sso-token-1"),
        "bearer token header"
    );

    // the resolved credentials sign a request with the SSO session token
    let signed = sign_with(&credentials);
    assert!(signed.contains("Credential=SSO_AKID/"), "{signed}");
    assert!(signed.contains("x-amz-security-token"), "session token signed");
}

/// IMDS leg: `IMDSv2` token (PUT + TTL header), role name, role credentials
/// with the token header on both GETs.
#[tokio::test]
async fn imds_chain_imdsv2_token_then_role() {
    let imds = imds_mock("IMDS_AKID").await;
    let options = DefaultChainOptions {
        region: "us-east-1".into(),
        profile: None,
        env: empty_env(),
        paths: ChainPaths::default(),
        endpoints: ChainEndpoints { sso_portal: None, imds: Some(imds_base(&imds)) },
    };
    let credentials = resolve_default_chain(&options).await.expect("IMDS credentials");
    assert_eq!(credentials.access_key_id, "IMDS_AKID");
    assert_eq!(credentials.session_token.as_deref(), Some("IMDS_TOKEN"));

    let requests = imds.requests();
    let token_request = requests
        .iter()
        .find(|request| request.path == "/latest/api/token")
        .expect("IMDSv2 token request");
    assert_eq!(token_request.method, "PUT");
    assert!(
        token_request
            .headers
            .iter()
            .any(|(name, value)| name == "x-aws-ec2-metadata-token-ttl-seconds" && value == "21600"),
        "token TTL header"
    );
    let role_request = requests
        .iter()
        .find(|request| request.path == "/latest/meta-data/iam/security-credentials/")
        .expect("role name request");
    assert!(
        role_request
            .headers
            .iter()
            .any(|(name, value)| name == "x-aws-ec2-metadata-token" && value == "token-xyz"),
        "role request carries the IMDSv2 token"
    );
    let credentials_request = requests
        .iter()
        .find(|request| request.path == "/latest/meta-data/iam/security-credentials/role-a")
        .expect("role credentials request");
    assert!(credentials_request
        .headers
        .iter()
        .any(|(name, _value)| name == "x-aws-ec2-metadata-token"));

    let signed = sign_with(&credentials);
    assert!(signed.contains("Credential=IMDS_AKID/"), "{signed}");
}

/// `IMDSv1` fallback when the token endpoint refuses (404).
#[tokio::test]
async fn imds_falls_back_to_v1_on_404() {
    let imds = http_mock(std::sync::Arc::new(|request| {
        if request.path == "/latest/api/token" {
            Some((404, String::new()))
        } else if request.path == "/latest/meta-data/iam/security-credentials/" {
            Some((200, "role-a\n".into()))
        } else if request.path == "/latest/meta-data/iam/security-credentials/role-a" {
            Some((
                200,
                "{\"Code\":\"Success\",\"AccessKeyId\":\"V1_AKID\",\"SecretAccessKey\":\"V1_SECRET\",\"Token\":\"V1_TOKEN\",\"Expiration\":\"2030-01-01T00:00:00Z\"}".into(),
            ))
        } else {
            None
        }
    }))
    .await;
    let options = DefaultChainOptions {
        region: "us-east-1".into(),
        profile: None,
        env: empty_env(),
        paths: ChainPaths::default(),
        endpoints: ChainEndpoints { sso_portal: None, imds: Some(imds_base(&imds)) },
    };
    let credentials = resolve_default_chain(&options).await.expect("v1 credentials");
    assert_eq!(credentials.access_key_id, "V1_AKID");
    // the GETs carry no token header
    let role_request = imds
        .requests()
        .into_iter()
        .find(|request| request.path == "/latest/meta-data/iam/security-credentials/")
        .expect("role request");
    assert!(!role_request.headers.iter().any(|(name, _)| name == "x-aws-ec2-metadata-token"));
}

/// Unsupported profile legs (`credential_process` / assume-role / web
/// identity) surface by name when the chain fails — the v3 chain itself
/// falls through to IMDS, so with no IMDS the aggregate error names the
/// leg instead of silently succeeding elsewhere.
#[tokio::test]
async fn unsupported_profile_legs_surface_in_chain_failure() {
    let sandbox = Sandbox::new("process");
    let config_file =
        sandbox.write("config", "[profile corp]\ncredential_process = /opt/fetch-creds.sh\n");
    let options = DefaultChainOptions {
        region: "us-east-1".into(),
        profile: Some("corp".into()),
        env: empty_env(),
        paths: ChainPaths { credentials_file: None, config_file: Some(config_file), aws_dir: None },
        endpoints: ChainEndpoints { sso_portal: None, imds: Some(dead_imds()) },
    };
    let error = resolve_default_chain(&options).await.unwrap_err();
    match &error.reason {
        AiErrorReason::Authentication { fields } => {
            assert!(
                fields.message.contains("default credential chain failed"),
                "message: {}",
                fields.message
            );
            assert!(
                fields.message.contains("credential_process"),
                "names the leg: {}",
                fields.message
            );
        }
        reason => panic!("expected Authentication, got {reason:?}"),
    }
}

/// Chain exhaustion surfaces the aggregate authentication error naming each
/// failed leg.
#[tokio::test]
async fn chain_exhaustion_message() {
    let dead_port = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("port").port();
        drop(listener);
        port
    };
    let sandbox = Sandbox::new("empty");
    let options = DefaultChainOptions {
        region: "us-east-1".into(),
        profile: None,
        env: empty_env(),
        paths: ChainPaths {
            credentials_file: Some(sandbox.dir.join("missing-credentials")),
            config_file: Some(sandbox.dir.join("missing-config")),
            aws_dir: Some(sandbox.dir.join("missing-aws")),
        },
        endpoints: ChainEndpoints {
            sso_portal: None,
            imds: Some(format!("http://127.0.0.1:{dead_port}")),
        },
    };
    let error = resolve_default_chain(&options).await.unwrap_err();
    match &error.reason {
        AiErrorReason::Authentication { fields } => {
            assert!(
                fields.message.contains("AWS default credential chain failed"),
                "{}",
                fields.message
            );
            assert!(fields.message.contains("env"), "{}", fields.message);
            assert!(fields.message.contains("profile"), "{}", fields.message);
            assert!(fields.message.contains("imds"), "{}", fields.message);
        }
        reason => panic!("expected Authentication, got {reason:?}"),
    }
}

/// Static route credentials still win before any chain leg (v2
/// `resolveAuth` precedence), through the production auth definition.
#[test]
fn static_credentials_sign_without_chain() {
    let definition = sigv4::resolve_auth(
        &sigv4::BedrockAuthOptions {
            credentials: Some(Credentials {
                region: "us-east-1".into(),
                access_key_id: "STATIC_AKID".into(),
                secret_access_key: "STATIC_SECRET".into(),
                session_token: None,
            }),
            ..Default::default()
        },
        "bedrock",
        "Bedrock Converse",
    );
    let headers = definition
        .apply(&AuthInput {
            request_http: None,
            method: "POST",
            url: "https://bedrock-runtime.us-east-1.amazonaws.com/model/x/converse-stream",
            body: "{}",
            headers: Headers::new(),
        })
        .expect("static signing");
    let authorization = headers.get("authorization").expect("authorization");
    assert!(authorization.contains("Credential=STATIC_AKID/"), "{authorization}");
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn imds_base(mock: &HttpMock) -> String {
    format!("http://127.0.0.1:{}", mock.port)
}

async fn imds_mock(access_key_unused: &str) -> HttpMock {
    let _ = access_key_unused;
    http_mock(std::sync::Arc::new(|request: &RecordedRequest| {
        if request.path == "/latest/api/token" {
            Some((200, "token-xyz".into()))
        } else if request.path == "/latest/meta-data/iam/security-credentials/" {
            Some((200, "role-a\n".into()))
        } else if request.path == "/latest/meta-data/iam/security-credentials/role-a" {
            Some((
                200,
                "{\"Code\":\"Success\",\"AccessKeyId\":\"IMDS_AKID\",\"SecretAccessKey\":\"IMDS_SECRET\",\"Token\":\"IMDS_TOKEN\",\"Expiration\":\"2030-01-01T00:00:00Z\"}".into(),
            ))
        } else {
            None
        }
    }))
    .await
}

fn sign_with(credentials: &Credentials) -> String {
    let input = AuthInput {
        request_http: None,
        method: "POST",
        url: "https://bedrock-runtime.us-east-1.amazonaws.com/model/x/converse-stream",
        body: "{}",
        headers: Headers::new(),
    };
    let signed = sigv4::sign(&input, credentials, "bedrock", 1_440_942_960).expect("sign");
    signed
        .headers
        .into_iter()
        .find(|(name, _)| name == "authorization")
        .map(|(_, value)| value)
        .unwrap_or_default()
}
