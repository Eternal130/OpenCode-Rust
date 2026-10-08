//! Declarative URL construction for one route (port of `src/route/endpoint.ts`).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::schema::LLMRequest;

/// Input to path/query rendering: the resolved request and the built body.
pub struct EndpointInput<'a> {
    pub request: &'a LLMRequest,
    pub body: &'a Value,
}

/// Path part: a static string or a function of the request/body (Gemini,
/// Bedrock embed the model id in the path).
#[derive(Clone)]
pub enum EndpointPart {
    Static(String),
    Dynamic(Arc<dyn Fn(&EndpointInput<'_>) -> String + Send + Sync>),
}

impl std::fmt::Debug for EndpointPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointPart::Static(path) => f.debug_tuple("Static").field(path).finish(),
            EndpointPart::Dynamic(_) => f.write_str("Dynamic(<fn>)"),
        }
    }
}

/// URL construction for one route (port of `Endpoint.Definition`).
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub base_url: Option<String>,
    pub path: EndpointPart,
    pub query: Option<BTreeMap<String, String>>,
}

/// Patch accepted by [`Endpoint::merge`]; `None` fields keep the base value.
#[derive(Default)]
pub struct EndpointPatch {
    pub base_url: Option<String>,
    pub path: Option<EndpointPart>,
    pub query: Option<BTreeMap<String, String>>,
}

impl Endpoint {
    pub fn path(value: impl Into<String>) -> Self {
        Self { base_url: None, path: EndpointPart::Static(value.into()), query: None }
    }

    pub fn dynamic_path<F>(render: F) -> Self
    where
        F: Fn(&EndpointInput<'_>) -> String + Send + Sync + 'static,
    {
        Self { base_url: None, path: EndpointPart::Dynamic(Arc::new(render)), query: None }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    #[must_use]
    pub fn with_query(mut self, query: BTreeMap<String, String>) -> Self {
        self.query = Some(query);
        self
    }

    /// Merge a patch: base/path replace wholesale, query merges per key
    /// (patch wins), so provider overrides can keep inherited params.
    #[must_use]
    pub fn merge(base: &Endpoint, patch: EndpointPatch) -> Endpoint {
        Endpoint {
            base_url: patch.base_url.or_else(|| base.base_url.clone()),
            path: patch.path.unwrap_or_else(|| base.path.clone()),
            query: match (base.query.clone(), patch.query) {
                (None, None) => None,
                (None, Some(query)) => Some(query),
                (Some(query), None) => Some(query),
                (Some(mut base_query), Some(patch_query)) => {
                    base_query.extend(patch_query);
                    Some(base_query)
                }
            },
        }
    }
}

/// Strip trailing slashes from a base URL (port of `trimBaseUrl`).
#[must_use]
pub fn trim_base_url(value: &str) -> &str {
    value.trim_end_matches('/')
}

fn percent_encode_path_segment(value: &str) -> String {
    // encodeURIComponent parity for the characters that appear in model ids
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Percent-encode a path segment like `encodeURIComponent`
/// (exported for endpoint path functions).
#[must_use]
pub fn encode_segment(value: &str) -> String {
    percent_encode_path_segment(value)
}

/// Minimal URL shape produced by endpoint rendering: scheme//host + path +
/// query. This exists so the route layer never depends on a URL crate; the
/// transport layer renders it into its own request type.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedUrl {
    pub base: String,
    pub query: Vec<(String, String)>,
}

impl std::fmt::Display for RenderedUrl {
    /// Full URL string with query parameters in insertion order
    /// (`searchParams.set` semantics).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.base)?;
        if self.query.is_empty() {
            return Ok(());
        }
        let query =
            self.query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
        write!(f, "?{query}")
    }
}

/// Render the endpoint for one request (port of `Endpoint.render`).
///
/// Path-embedded query strings (`/chat?alt=sse`) are parsed into the query
/// set; endpoint `query` entries then overwrite per key (URL
/// `searchParams.set` semantics). Caller `http.query` is applied later by the
/// transport and wins over both.
#[must_use]
pub fn render(endpoint: &Endpoint, input: &EndpointInput<'_>) -> RenderedUrl {
    let raw_path = match &endpoint.path {
        EndpointPart::Static(path) => path.clone(),
        EndpointPart::Dynamic(render) => render(input),
    };
    let (path, path_query) = match raw_path.split_once('?') {
        Some((path, query)) => (path.to_string(), query),
        None => (raw_path, ""),
    };
    let base = format!("{}{}", trim_base_url(endpoint.base_url.as_deref().unwrap_or("")), path);
    let mut query: Vec<(String, String)> = Vec::new();
    let set = |key: String, value: String, query: &mut Vec<(String, String)>| {
        if let Some(entry) = query.iter_mut().find(|(k, _)| *k == key) {
            entry.1 = value;
        } else {
            query.push((key, value));
        }
    };
    for pair in path_query.split('&').filter(|p| !p.is_empty()) {
        if let Some((k, v)) = pair.split_once('=') {
            set(k.to_string(), v.to_string(), &mut query);
        }
    }
    if let Some(endpoint_query) = &endpoint.query {
        for (k, v) in endpoint_query {
            set(k.clone(), v.clone(), &mut query);
        }
    }
    RenderedUrl { base, query }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::LanguageModel;
    use std::sync::Arc;

    fn request() -> LLMRequest {
        let route = crate::test_support::static_route_for_tests();
        LLMRequest::new(LanguageModel::new("model-1", "test", Arc::clone(&route)))
    }

    #[test]
    fn appends_a_static_path_to_the_base_url() {
        let endpoint = Endpoint::path("/chat").with_base_url("https://api.example.test/v1/");
        let url =
            render(&endpoint, &EndpointInput { request: &request(), body: &serde_json::json!({}) });
        assert_eq!(url.to_string(), "https://api.example.test/v1/chat");
    }

    #[test]
    fn endpoint_query_params_are_appended_to_the_rendered_url() {
        let mut query = BTreeMap::new();
        query.insert("api-version".to_string(), "2026-01-01".to_string());
        query.insert("alt".to_string(), "json".to_string());
        let endpoint = Endpoint {
            base_url: Some("https://custom.example.test/root/".into()),
            path: EndpointPart::Static("/chat?alt=sse".into()),
            query: Some(query),
        };
        let url =
            render(&endpoint, &EndpointInput { request: &request(), body: &serde_json::json!({}) });
        // `alt=sse` embedded in the path is overwritten by the endpoint query
        assert_eq!(
            url.to_string(),
            "https://custom.example.test/root/chat?alt=json&api-version=2026-01-01"
        );
    }

    #[test]
    fn path_may_be_a_function_of_the_validated_body() {
        let endpoint = Endpoint::dynamic_path(|input| {
            let model = input.body.get("modelId").and_then(|v| v.as_str()).unwrap_or_default();
            format!("/model/{}/converse-stream", encode_segment(model))
        })
        .with_base_url("https://bedrock-runtime.us-east-1.amazonaws.com");
        let url = render(
            &endpoint,
            &EndpointInput {
                request: &request(),
                body: &serde_json::json!({"modelId": "us.amazon.nova-micro-v1:0"}),
            },
        );
        assert_eq!(
            url.to_string(),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.amazon.nova-micro-v1%3A0/converse-stream"
        );
    }

    #[test]
    fn merge_replaces_base_and_path_but_merges_query() {
        let base = Endpoint {
            base_url: Some("https://api.example.test/v1".into()),
            path: EndpointPart::Static("/chat/completions".into()),
            query: Some(BTreeMap::from([
                ("keep".to_string(), "base".into()),
                ("base".to_string(), "1".into()),
            ])),
        };
        let merged = Endpoint::merge(
            &base,
            EndpointPatch {
                base_url: None,
                path: None,
                query: Some(BTreeMap::from([
                    ("keep".to_string(), "patch".into()),
                    ("patch".to_string(), "1".into()),
                ])),
            },
        );
        assert_eq!(merged.base_url.as_deref(), Some("https://api.example.test/v1"));
        assert!(matches!(merged.path, EndpointPart::Static(ref p) if p == "/chat/completions"));
        let query = merged.query.unwrap();
        assert_eq!(query["keep"], "patch");
        assert_eq!(query["base"], "1");
        assert_eq!(query["patch"], "1");
    }
}
