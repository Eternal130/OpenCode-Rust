//! The `opencode.mcp.codemode.defaults` transform (v2
//! `packages/core/src/plugin/mcp-codemode-defaults.ts`) — defaults that
//! make known MCP servers work well with OpenCode Code Mode, so a
//! server's own Code Mode never ends up nested inside OpenCode's. A
//! server with `codemode` set in config is left exactly as configured.
//!
//! v2 ships this as an internal Effect plugin over `ctx.mcp.transform`;
//! the Rust host applies the same table to the config-sourced server
//! table at seed time (registration-time pre-resolution — the guest's
//! transform editors snapshot the defaulted base).

use serde_json::Value;

/// Servers that ARE Code Mode themselves — OpenCode's turns off for
/// them (`codemode: false`): executor.sh remote endpoints.
fn is_executor_url(endpoint: &str) -> bool {
    // /^https:\/\/executor\.sh\/[^/]+\/mcp$/ over
    // `${url.origin}${url.pathname.replace(/\/+$/, "")}`.
    let Some(rest) = endpoint.strip_prefix("https://executor.sh/") else {
        return false;
    };
    let Some(segment) = rest.strip_suffix("/mcp") else {
        return false;
    };
    !segment.is_empty() && !segment.contains('/')
}

/// PostHog wraps all of its tools in one server-side Code Mode tool
/// unless the client asks for "tools" mode, so OpenCode Code Mode stays
/// on and PostHog's is turned off via the header (which wins over
/// `?mode=` and leaves the URL — the credential key — unchanged).
fn is_posthog_host(hostname: &str) -> bool {
    // /^mcp(?:-eu|\.us|\.eu)?\.posthog\.com$/
    let tail = ".posthog.com";
    let Some(prefix) = hostname.strip_suffix(tail) else {
        return false;
    };
    matches!(prefix, "mcp" | "mcp-eu" | "mcp.us" | "mcp.eu")
}

/// One `[origin, pathname]` pair from a URL (no query/fragment). URL
/// parsing failures are skipped like v2's `URL.canParse` guard.
fn url_endpoint(url: &str) -> Option<(String, String, String)> {
    // Split scheme://rest, then the first '/', '?', or '#'.
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let after = &rest[authority_end..];
    let path_end = after.find(['?', '#']).unwrap_or(after.len());
    let pathname = &after[..path_end];
    // The origin for these purposes: scheme + authority (userinfo
    // stripped — none of the matched hosts use it).
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let default_port = match scheme {
        "https" => ":443",
        _ => ":80",
    };
    let host = host.strip_suffix(default_port).unwrap_or(host);
    Some((format!("{scheme}://{host}"), pathname.to_string(), host.to_string()))
}

/// The query string (without `?`, up to `#`) when present.
fn url_endpoint_query(url: &str) -> Option<&str> {
    let start = url.find('?')?;
    let rest = &url[start + 1..];
    Some(rest.split('#').next().unwrap_or(rest))
}

/// Apply the defaults to one server config (in place). Returns whether
/// the config changed.
pub fn apply_to_server(config: &mut Value) -> bool {
    let Some(object) = config.as_object_mut() else { return false };
    if object.get("codemode").is_some() {
        return false; // a configured value is left exactly as configured
    }
    match object.get("type").and_then(Value::as_str) {
        Some("local") => {
            let command = object.get("command").and_then(Value::as_array);
            let is_executor = command.is_some_and(|argv| {
                argv.first().and_then(Value::as_str) == Some("executor")
                    && argv.get(1).and_then(Value::as_str) == Some("mcp")
            });
            if is_executor {
                object.insert("codemode".into(), Value::Bool(false));
                return true;
            }
            false
        }
        Some("remote") => {
            let Some(url) = object.get("url").and_then(Value::as_str) else { return false };
            let Some((origin, pathname, host)) = url_endpoint(url) else { return false };
            let endpoint = format!("{origin}{}", pathname.trim_end_matches('/'));
            let mut changed = false;
            if is_executor_url(&endpoint) {
                object.insert("codemode".into(), Value::Bool(false));
                changed = true;
            }
            // PostHog: the header unless `?mode=` or any casing of the
            // header is already present.
            let has_mode_param = url_endpoint_query(url)
                .is_some_and(|query| query.split('&').any(|pair| pair.split('=').next() == Some("mode")));
            let has_header = object
                .get("headers")
                .and_then(Value::as_object)
                .is_some_and(|headers| {
                    headers.keys().any(|key| key.eq_ignore_ascii_case("x-posthog-mcp-mode"))
                });
            if is_posthog_host(&host) && !has_mode_param && !has_header {
                let mut headers = object
                    .get("headers")
                    .and_then(Value::as_object)
                    .map(|map| map.iter().map(|(key, value)| (key.clone(), value.clone())).collect::<serde_json::Map<_, _>>())
                    .unwrap_or_default();
                headers.insert("x-posthog-mcp-mode".into(), Value::String("tools".into()));
                object.insert("headers".into(), Value::Object(headers));
                changed = true;
            }
            changed
        }
        _ => false,
    }
}

/// Apply the defaults to a whole seeded server table. Returns the
/// number of touched configs.
pub fn apply_to_servers(servers: &mut serde_json::Map<String, Value>) -> usize {
    servers.values_mut().filter(|config| apply_to_server(config)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn server(config: Value) -> Value {
        config
    }

    /// The v2 case table (mcp-codemode-defaults.test.ts) verbatim:
    /// name → (config, expected codemode, expected headers).
    #[test]
    fn v2_case_table() {
        struct Case {
            name: &'static str,
            config: Value,
            codemode: Option<bool>,
            headers: Option<Value>,
        }
        let cases = vec![
            Case {
                name: "executor remote",
                config: server(json!({ "type": "remote", "url": "https://executor.sh/example/mcp?source=opencode" })),
                codemode: Some(false),
                headers: None,
            },
            Case {
                name: "executor local",
                config: server(json!({ "type": "local", "command": ["executor", "mcp"] })),
                codemode: Some(false),
                headers: None,
            },
            Case {
                name: "cloudflare code mode",
                config: server(json!({ "type": "remote", "url": "https://mcp.cloudflare.com/mcp/" })),
                codemode: None,
                headers: None,
            },
            Case {
                name: "cloudflare raw tools",
                config: server(json!({ "type": "remote", "url": "https://mcp.cloudflare.com/mcp?codemode=false" })),
                codemode: None,
                headers: None,
            },
            Case {
                name: "cloudflare docs",
                config: server(json!({ "type": "remote", "url": "https://docs.mcp.cloudflare.com/mcp" })),
                codemode: None,
                headers: None,
            },
            Case {
                name: "explicit true",
                config: server(json!({ "type": "remote", "url": "https://mcp.cloudflare.com/mcp", "codemode": true })),
                codemode: Some(true),
                headers: None,
            },
            Case {
                name: "explicit false",
                config: server(json!({ "type": "remote", "url": "https://executor.sh/example/mcp", "codemode": false })),
                codemode: Some(false),
                headers: None,
            },
            Case {
                name: "exa",
                config: server(json!({ "type": "remote", "url": "https://mcp.exa.ai/mcp" })),
                codemode: None,
                headers: None,
            },
            Case {
                name: "unrelated",
                config: server(json!({ "type": "remote", "url": "https://example.com/mcp" })),
                codemode: None,
                headers: None,
            },
            Case {
                name: "posthog",
                config: server(json!({ "type": "remote", "url": "https://mcp.posthog.com/mcp" })),
                codemode: None,
                headers: Some(json!({ "x-posthog-mcp-mode": "tools" })),
            },
            Case {
                name: "posthog eu with auth",
                config: server(json!({ "type": "remote", "url": "https://mcp-eu.posthog.com/mcp", "headers": { "Authorization": "Bearer phx_test" } })),
                codemode: None,
                headers: Some(json!({ "Authorization": "Bearer phx_test", "x-posthog-mcp-mode": "tools" })),
            },
            Case {
                name: "posthog explicit code mode",
                config: server(json!({ "type": "remote", "url": "https://mcp.us.posthog.com/mcp", "codemode": true })),
                codemode: Some(true),
                headers: None,
            },
            Case {
                name: "posthog mode in url",
                config: server(json!({ "type": "remote", "url": "https://mcp.posthog.com/mcp?mode=cli" })),
                codemode: None,
                headers: None,
            },
            Case {
                name: "posthog mode header",
                config: server(json!({ "type": "remote", "url": "https://mcp.posthog.com/mcp", "headers": { "X-PostHog-MCP-Mode": "cli" } })),
                codemode: None,
                headers: Some(json!({ "X-PostHog-MCP-Mode": "cli" })),
            },
            Case {
                name: "posthog without code mode",
                config: server(json!({ "type": "remote", "url": "https://mcp.posthog.com/mcp", "codemode": false })),
                codemode: Some(false),
                headers: None,
            },
        ];
        for case in cases {
            let mut config = case.config;
            apply_to_server(&mut config);
            assert_eq!(
                config.get("codemode").and_then(Value::as_bool),
                case.codemode,
                "{}: codemode",
                case.name
            );
            let headers = if config.get("type") == Some(&json!("remote")) {
                config.get("headers").cloned()
            } else {
                None
            };
            assert_eq!(headers, case.headers.map_or(Value::Null, |value| value), "{}: headers", case.name);
        }
    }

    #[test]
    fn posthog_with_codemode_false_gets_no_header() {
        // "posthog without code mode": explicit false → untouched apart
        // from the configured value (the header would turn PostHog's
        // OWN codemode off, pointless when ours is off).
        let mut config = json!({ "type": "remote", "url": "https://mcp.posthog.com/mcp", "codemode": false });
        assert!(!apply_to_server(&mut config));
        assert!(config.get("headers").is_none());
    }

    #[test]
    fn apply_to_servers_counts_touches() {
        let mut servers = serde_json::Map::new();
        servers.insert("a".into(), json!({ "type": "local", "command": ["executor", "mcp"] }));
        servers.insert("b".into(), json!({ "type": "remote", "url": "https://mcp.exa.ai/mcp" }));
        assert_eq!(apply_to_servers(&mut servers), 1);
        assert_eq!(servers["a"]["codemode"], json!(false));
    }
}
