//! Release-channel types (lane C): version ordering for the Rust port's
//! tag scheme, GitHub-releases feed parsing, and candidate selection.
//!
//! Pure functions ([`ReleaseVer::parse`], [`parse_releases`],
//! [`select_candidate`]) are split from the network touch
//! ([`fetch_latest`]) so tests can drive everything through fixtures
//! and localhost listeners — no test ever reaches the real feed.
//!
//! Channel rule (v2 semantics): a build from a beta tag (`.beta.N`)
//! sees ALL releases (betas + finals); a build from a final tag sees
//! only `prerelease == false` releases. Among parseable, asset-bearing
//! candidates the max version wins; `max <= current` means no update.

use std::cmp::Ordering;
use std::time::Duration;

/// The Rust port's own releases feed (array of release objects).
pub const FEED_URL: &str = "https://api.github.com/repos/Eternal130/OpenCode-Rust/releases";

/// How many releases one feed query asks for (GitHub returns newest
/// first; 20 comfortably covers the port's cadence).
pub const FEED_PAGE_SIZE: usize = 20;

/// The single release target published today (release.yml packages
/// `linux-x64` tarballs); kept a constant until more targets ship.
pub const RELEASE_TARGET: &str = "linux-x64";

/// Tarball asset name for one version: `opencode-<version>-linux-x64.tar.gz`.
#[must_use]
pub fn asset_name(version: &ReleaseVer) -> String {
    format!("opencode-{version}-{RELEASE_TARGET}.tar.gz")
}

/// `User-Agent` GitHub requires on api.github.com requests.
const USER_AGENT: &str = "opencode-rust-updater";

/// One parsed release tag: `v?<major>.<minor>.<patch>[.beta.<n>]`.
///
/// Ordering: the numeric triple first, then channel — a final release
/// sorts ABOVE any beta of the same triple, and beta numbers order
/// numerically (`beta.2` > `beta.1`). Unparseable tags are `None` at
/// parse time and never become update candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseVer {
    major: u64,
    minor: u64,
    patch: u64,
    beta: Option<u32>,
}

impl ReleaseVer {
    /// Parse a tag like `v2.0.24`, `2.0.24.beta.1`. Anything else
    /// (`dev`, `latest`, partial or suffixed strings) is `None`.
    #[must_use]
    pub fn parse(tag: &str) -> Option<Self> {
        let body = tag.trim();
        let body = body.strip_prefix('v').unwrap_or(body);
        let mut parts = body.split('.');
        let major = digits(parts.next()?)?;
        let minor = digits(parts.next()?)?;
        let patch = digits(parts.next()?)?;
        let beta = match parts.next() {
            None => None,
            Some("beta") => Some(u32::try_from(digits(parts.next()?)?).ok()?),
            Some(_) => return None,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self { major, minor, patch, beta })
    }

    /// Whether this version came from a beta tag (the channel selector).
    #[must_use]
    pub fn is_beta(&self) -> bool {
        self.beta.is_some()
    }
}

impl std::fmt::Display for ReleaseVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.beta {
            Some(beta) => {
                write!(f, "{}.{}.{}.beta.{}", self.major, self.minor, self.patch, beta)
            }
            None => write!(f, "{}.{}.{}", self.major, self.minor, self.patch),
        }
    }
}

impl Ord for ReleaseVer {
    fn cmp(&self, other: &Self) -> Ordering {
        let triple = (self.major, self.minor, self.patch);
        let other_triple = (other.major, other.minor, other.patch);
        triple.cmp(&other_triple).then_with(|| {
            match (self.beta, other.beta) {
                // Final > any beta of the same triple; betas order by n.
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(&b),
            }
        })
    }
}

impl PartialOrd for ReleaseVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A whole, strictly-numeric path segment (no `+`, no spaces, no
/// leading zeros rejected — `02` still parses as 2).
fn digits(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// Failures of the feed query. The automatic check treats ANY of
/// these as a silent no-update; the manual `upgrade` command prints
/// them clearly and exits non-zero.
#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    /// Transport-level failure (DNS, connect refused, TLS, timeout).
    #[error("release feed unreachable: {0}")]
    Transport(String),
    /// A non-rate-limit HTTP error status.
    #[error("release feed returned {0}")]
    Http(reqwest::StatusCode),
    /// A 2xx body that is not a releases array.
    #[error("release feed parse failure: {0}")]
    Parse(String),
}

/// One downloadable asset of a release (unknown fields tolerated).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

/// One release entry of the feed (unknown fields tolerated; a release
/// missing `tag_name` is dropped at parse time).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// A newer release this build should move to, with both download URLs
/// fully resolved (tarball + `.sha256` sidecar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The exact tag on the feed (`v2.0.24.beta.2`).
    pub tag: String,
    /// Parsed version of `tag`.
    pub version: ReleaseVer,
    /// Tarball download URL (asset `opencode-<version>-<target>.tar.gz`).
    pub tarball_url: String,
    /// Sidecar URL — the tarball asset URL + `.sha256`.
    pub sha256_url: String,
}

/// Parse a releases-array JSON body. Unparseable ENTRIES are skipped
/// (feeds gain fields; one odd release must not kill the channel);
/// a body that is not an array at all is [`FeedError::Parse`].
pub fn parse_releases(body: &str) -> Result<Vec<Release>, FeedError> {
    let values: Vec<serde_json::Value> =
        serde_json::from_str(body).map_err(|error| FeedError::Parse(error.to_string()))?;
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Release>(value).ok())
        .collect())
}

/// Resolve the tarball + sidecar URLs for one release, `None` when the
/// expected asset is absent (that release cannot be installed).
#[must_use]
pub fn resolve_asset_urls(release: &Release) -> Option<(String, String)> {
    let version = ReleaseVer::parse(&release.tag_name)?;
    let expected = asset_name(&version);
    let asset = release.assets.iter().find(|asset| asset.name == expected)?;
    Some((asset.browser_download_url.clone(), format!("{}.sha256", asset.browser_download_url)))
}

/// Pure selection: the best installable candidate newer than
/// `current`, honoring the channel rule. `None` = no update.
#[must_use]
pub fn select_candidate(releases: &[Release], current: &ReleaseVer) -> Option<Candidate> {
    releases
        .iter()
        // Channel rule: beta builds see everything; final builds see finals only.
        .filter(|release| current.is_beta() || !release.prerelease)
        .filter_map(|release| {
            let version = ReleaseVer::parse(&release.tag_name)?;
            let (tarball_url, sha256_url) = resolve_asset_urls(release)?;
            Some(Candidate { tag: release.tag_name.clone(), version, tarball_url, sha256_url })
        })
        .filter(|candidate| candidate.version > *current)
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// Query the feed (5 s connect / 15 s total, one retry after 2 s for
/// transient DNS blips). GitHub rate-limit responses (403/429) are
/// `Ok(None)` — "cannot know" is not an error worth surfacing.
///
/// `feed` is the base URL (no query string); tests pass localhost
/// listeners, production passes [`FEED_URL`].
pub async fn fetch_latest(
    feed: &str,
    current: &ReleaseVer,
) -> Result<Option<Candidate>, FeedError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| FeedError::Transport(error.to_string()))?;
    let separator = if feed.contains('?') { '&' } else { '?' };
    let url = format!("{feed}{separator}per_page={FEED_PAGE_SIZE}");

    let mut retried = false;
    loop {
        let attempt = client
            .get(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .timeout(Duration::from_secs(15))
            .send()
            .await;
        let response = match attempt {
            Ok(response) => response,
            Err(_) if !retried => {
                retried = true;
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(error) => return Err(FeedError::Transport(error.to_string())),
        };
        let status = response.status();
        if status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            // Rate limited: not an error, just no answer this round.
            return Ok(None);
        }
        if status.is_server_error() && !retried {
            retried = true;
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }
        if !status.is_success() {
            return Err(FeedError::Http(status));
        }
        let body =
            response.text().await.map_err(|error| FeedError::Transport(error.to_string()))?;
        let releases = parse_releases(&body)?;
        return Ok(select_candidate(&releases, current));
    }
}

/// Production wrapper: query the real feed ([`FEED_URL`]).
pub async fn latest_release(current: &ReleaseVer) -> Result<Option<Candidate>, FeedError> {
    fetch_latest(FEED_URL, current).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    // ------------------------------------------------------------------
    // Version comparator
    // ------------------------------------------------------------------

    #[test]
    fn version_ordering_table() {
        let beta1 = ReleaseVer::parse("v2.0.24.beta.1").expect("beta.1");
        let beta2 = ReleaseVer::parse("v2.0.24.beta.2").expect("beta.2");
        let beta10 = ReleaseVer::parse("v2.0.24.beta.10").expect("beta.10");
        let final_ = ReleaseVer::parse("v2.0.24").expect("final");
        let next = ReleaseVer::parse("v2.0.25").expect("next");
        let major = ReleaseVer::parse("v3.0.0").expect("major");

        assert!(beta1 < beta2);
        assert!(beta2 < beta10, "beta numbers order numerically, not lexically");
        assert!(beta10 < final_, "final > any beta of the same triple");
        assert!(final_ < next);
        assert!(next < major);
        assert_eq!(beta1, ReleaseVer::parse("2.0.24.beta.1").expect("no-v"));
        assert_eq!(final_, ReleaseVer::parse("  v2.0.24  ").expect("whitespace"));
        assert_eq!(beta1.to_string(), "2.0.24.beta.1");
        assert_eq!(final_.to_string(), "2.0.24");
        assert!(beta1.is_beta());
        assert!(!final_.is_beta());
    }

    #[test]
    fn unparseable_tags_are_none() {
        for garbage in
            ["dev", "latest", "2.0", "v2.0.24.beta", "2.0.24.beta.x", "2.0.24-rc.1", "v2..24", ""]
        {
            assert!(ReleaseVer::parse(garbage).is_none(), "{garbage:?} must not parse");
        }
    }

    // ------------------------------------------------------------------
    // Feed parsing + selection (fixtures, no network)
    // ------------------------------------------------------------------

    fn fixture_body() -> String {
        r#"[
          {
            "tag_name": "v2.0.25.beta.1",
            "prerelease": true,
            "assets": [
              {"name": "opencode-2.0.25.beta.1-linux-x64.tar.gz",
               "browser_download_url": "https://example.com/dl/opencode-2.0.25.beta.1-linux-x64.tar.gz"},
              {"name": "opencode-2.0.25.beta.1-linux-x64.tar.gz.sha256",
               "browser_download_url": "https://example.com/dl/opencode-2.0.25.beta.1-linux-x64.tar.gz.sha256"}
            ],
            "published_at": "2026-01-02T00:00:00Z",
            "unknown_future_field": {"nested": true}
          },
          {
            "tag_name": "v2.0.24.beta.2",
            "name": "opencode-rust v2.0.24.beta.2",
            "prerelease": true,
            "assets": [
              {"name": "opencode-2.0.24.beta.2-linux-x64.tar.gz",
               "browser_download_url": "https://example.com/dl/opencode-2.0.24.beta.2-linux-x64.tar.gz"}
            ]
          },
          {
            "tag_name": "v2.0.24.beta.1",
            "prerelease": true,
            "assets": [
              {"name": "opencode-2.0.24.beta.1-linux-x64.tar.gz",
               "browser_download_url": "https://example.com/dl/opencode-2.0.24.beta.1-linux-x64.tar.gz"}
            ]
          },
          {
            "tag_name": "not-a-version",
            "prerelease": false,
            "assets": []
          },
          {"prerelease": false, "assets": []}
        ]"#
        .to_owned()
    }

    #[test]
    fn parse_releases_tolerates_unknown_fields_and_bad_entries() {
        let releases = parse_releases(&fixture_body()).expect("fixture parses");
        assert_eq!(releases.len(), 4, "the tag-less entry is dropped, the rest survive");
        assert_eq!(releases[0].tag_name, "v2.0.25.beta.1");
        assert!(releases[0].prerelease);
        assert_eq!(releases[0].assets.len(), 2);
        assert!(parse_releases("not json").is_err());
        assert!(parse_releases(r#"{"object":"not an array"}"#).is_err());
        assert!(parse_releases("[]").expect("empty").is_empty());
    }

    #[test]
    fn sidecar_url_is_asset_url_plus_suffix() {
        let releases = parse_releases(&fixture_body()).expect("fixture");
        let (tarball, sha256) =
            resolve_asset_urls(&releases[0]).expect("asset resolves for a parseable tag");
        assert_eq!(tarball, "https://example.com/dl/opencode-2.0.25.beta.1-linux-x64.tar.gz");
        assert_eq!(sha256, format!("{tarball}.sha256"));
        // A release without the expected asset is not installable.
        assert!(resolve_asset_urls(&releases[3]).is_none(), "unparseable tag");
    }

    #[test]
    fn beta_channel_sees_prereleases_and_picks_the_max() {
        let releases = parse_releases(&fixture_body()).expect("fixture");
        let current = ReleaseVer::parse("v2.0.24.beta.1").expect("current");
        let candidate = select_candidate(&releases, &current).expect("beta sees betas");
        assert_eq!(candidate.tag, "v2.0.25.beta.1");
        assert_eq!(candidate.version, ReleaseVer::parse("v2.0.25.beta.1").expect("ver"));
    }

    #[test]
    fn stable_channel_filters_prereleases() {
        let releases = parse_releases(&fixture_body()).expect("fixture");
        let current = ReleaseVer::parse("v2.0.24").expect("current");
        // Every parseable asset-bearing release in the fixture is a
        // prerelease → a final build sees no update at all.
        assert!(select_candidate(&releases, &current).is_none());
    }

    #[test]
    fn max_not_above_current_means_no_update() {
        let releases = parse_releases(&fixture_body()).expect("fixture");
        let current = ReleaseVer::parse("v2.0.25.beta.3").expect("current");
        assert!(select_candidate(&releases, &current).is_none());
        let equal = ReleaseVer::parse("v2.0.25.beta.1").expect("equal");
        assert!(select_candidate(&releases, &equal).is_none(), "equal tag is not an update");
    }

    #[test]
    fn candidate_requires_the_expected_asset() {
        let body = r#"[
          {"tag_name": "v9.9.9", "prerelease": false, "assets": [
             {"name": "opencode-9.9.9-linux-arm64.tar.gz", "browser_download_url": "https://x/arm64"}
          ]},
          {"tag_name": "v8.8.8", "prerelease": false, "assets": [
             {"name": "opencode-8.8.8-linux-x64.tar.gz", "browser_download_url": "https://x/x64"}
          ]}
        ]"#;
        let releases = parse_releases(body).expect("fixture");
        let current = ReleaseVer::parse("v2.0.24").expect("current");
        // v9.9.9 lacks the linux-x64 asset → the installable v8.8.8 wins.
        let candidate = select_candidate(&releases, &current).expect("fallback");
        assert_eq!(candidate.tag, "v8.8.8");
    }

    // ------------------------------------------------------------------
    // fetch_latest over localhost fixtures (never the real network)
    // ------------------------------------------------------------------

    /// One-shot local HTTP server: accept a single connection, write
    /// `response` bytes, close. Returns the base URL to hit.
    fn serve_once(response: Vec<u8>, tls: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Drain (part of) the request so the client is not racing us.
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.read(&mut [0u8; 1024]);
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            }
        });
        let scheme = if tls { "https" } else { "http" };
        format!("{scheme}://127.0.0.1:{port}")
    }

    fn http_response(status: &str, body: &str) -> Vec<u8> {
        format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).into_bytes()
    }

    #[tokio::test]
    async fn fetch_latest_selects_from_a_served_fixture() {
        let url = serve_once(http_response("200 OK", &fixture_body()), false);
        let current = ReleaseVer::parse("v2.0.24.beta.1").expect("current");
        let candidate = fetch_latest(&url, &current).await.expect("ok").expect("candidate");
        assert_eq!(candidate.tag, "v2.0.25.beta.1");
        assert_eq!(candidate.sha256_url, format!("{}.sha256", candidate.tarball_url));
    }

    #[tokio::test]
    async fn fetch_latest_maps_rate_limit_to_no_update() {
        for status in ["403 Forbidden", "429 Too Many Requests"] {
            let url = serve_once(http_response(status, "{}"), false);
            let current = ReleaseVer::parse("v2.0.24").expect("current");
            let result = fetch_latest(&url, &current).await;
            assert!(matches!(result, Ok(None)), "{status} must mean no-update, not an error");
        }
    }

    #[tokio::test]
    async fn fetch_latest_garbage_body_is_a_parse_error() {
        let url = serve_once(http_response("200 OK", "<html>garbage</html>"), false);
        let current = ReleaseVer::parse("v2.0.24").expect("current");
        let result = fetch_latest(&url, &current).await;
        assert!(matches!(result, Err(FeedError::Parse(_))), "{result:?}");
    }

    #[tokio::test]
    async fn fetch_latest_plain_404_is_an_http_error() {
        let url = serve_once(http_response("404 Not Found", "{}"), false);
        let current = ReleaseVer::parse("v2.0.24").expect("current");
        let result = fetch_latest(&url, &current).await;
        assert!(matches!(result, Err(FeedError::Http(_))), "{result:?}");
    }

    #[tokio::test]
    async fn fetch_latest_connection_refused_is_a_fast_transport_error() {
        let start = std::time::Instant::now();
        let current = ReleaseVer::parse("v2.0.24.beta.1").expect("current");
        let result = fetch_latest("http://127.0.0.1:1", &current).await;
        assert!(matches!(result, Err(FeedError::Transport(_))), "{result:?}");
        // Refused connects fail instantly; even with the 2 s retry
        // backoff this must stay far below the 15 s budget.
        assert!(start.elapsed() < Duration::from_secs(10), "took {:?}", start.elapsed());
    }

    #[tokio::test]
    async fn fetch_latest_tls_garbage_is_a_transport_error() {
        // A plain-TCP listener speaking nonsense where the client
        // expects a TLS handshake = the invalid-TLS flavor of dead feed.
        let url = serve_once(b"this is not tls at all\r\n\r\n".to_vec(), true);
        let current = ReleaseVer::parse("v2.0.24").expect("current");
        let result = fetch_latest(&url, &current).await;
        assert!(matches!(result, Err(FeedError::Transport(_))), "{result:?}");
    }
}
