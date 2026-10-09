//! `opencode api` — `OpenAPI` operation-id / method-path calls against the
//! resolved server (handlers/api.ts). Uses a plain fetch (v2 semantics:
//! print any body; non-2xx mirrors `HTTP <status>` and exits 1).

use serde_json::Value;

use crate::cli::ServerParams;
use crate::handlers::connect;

const METHODS: [&str; 7] = ["delete", "get", "head", "options", "patch", "post", "put"];

struct Request {
    method: String,
    path: String,
}

fn raw_request(input: &[String]) -> Option<Request> {
    if input.len() != 2
        || !METHODS.contains(&input[0].to_ascii_lowercase().as_str())
        || !input[1].starts_with('/')
    {
        return None;
    }
    Some(Request { method: input[0].to_ascii_uppercase(), path: input[1].clone() })
}

/// `resolveOperation`: find the operation id in /openapi.json.
fn resolve_operation(
    spec: &Value,
    operation_id: &str,
    params: &std::collections::BTreeMap<String, String>,
) -> Result<Request, String> {
    let paths = spec.get("paths").and_then(Value::as_object).ok_or("Operation not found")?;
    for (path, operations) in paths {
        let Some(operations) = operations.as_object() else { continue };
        for (method, operation) in operations {
            if !METHODS.contains(&method.as_str()) {
                continue;
            }
            if operation.get("operationId").and_then(Value::as_str) == Some(operation_id) {
                return Ok(Request {
                    method: method.to_ascii_uppercase(),
                    path: interpolate(path, params)?,
                });
            }
        }
    }
    Err(format!("Operation not found: {operation_id}"))
}

fn encode_component(value: &str) -> String {
    // encodeURIComponent.
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
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
            _ => {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0xf) as usize] as char);
            }
        }
    }
    out
}

fn interpolate(
    path: &str,
    params: &std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    let mut used = std::collections::HashSet::new();
    let mut pathname = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        pathname.push_str(&rest[..start]);
        let end =
            rest[start..].find('}').map(|index| start + index).ok_or("malformed path template")?;
        let name = &rest[start + 1..end];
        let value = params.get(name).ok_or_else(|| format!("Missing path parameter: {name}"))?;
        used.insert(name.to_owned());
        pathname.push_str(&encode_component(value));
        rest = &rest[end + 1..];
    }
    pathname.push_str(rest);
    let query: Vec<String> = params
        .iter()
        .filter(|(name, _)| !used.contains(*name))
        .map(|(name, value)| format!("{}={}", encode_component(name), encode_component(value)))
        .collect();
    if query.is_empty() {
        Ok(pathname)
    } else {
        Ok(format!("{pathname}?{}", query.join("&")))
    }
}

fn basic_auth_header(password: Option<&str>) -> Option<String> {
    use base64::Engine as _;
    let password = password?;
    Some(format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
    ))
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run(
    server: &ServerParams,
    request: &[String],
    data: Option<&str>,
    headers: &[String],
    params: &[String],
) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let endpoint = &resolved.endpoint;
    let mut params_map = std::collections::BTreeMap::new();
    for param in params {
        if let Some((key, value)) = param.split_once('=') {
            params_map.insert(key.to_owned(), value.to_owned());
        } else {
            eprintln!("Error: Invalid parameter, expected name=value: {param}");
            return 1;
        }
    }

    let resolved_request = if let Some(raw) = raw_request(request) {
        raw
    } else if request.len() != 1 {
        eprintln!("Error: Expected an operation name or an HTTP method and path");
        return 1;
    } else {
        let mut spec_url = reqwest::Url::parse(&endpoint.url).expect("valid endpoint url");
        spec_url.set_path("/openapi.json");
        let spec_request = reqwest::Client::new().get(spec_url);
        let spec_request = match basic_auth_header(endpoint.password.as_deref()) {
            Some(auth) => spec_request.header("authorization", auth),
            None => spec_request,
        };
        let spec = match spec_request.send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                eprintln!("Error: Failed to load OpenAPI document: HTTP {}", response.status());
                return 1;
            }
            Err(error) => {
                eprintln!("Error: Failed to load OpenAPI document: {error}");
                return 1;
            }
        };
        let spec: Value = match spec.json().await {
            Ok(spec) => spec,
            Err(error) => {
                eprintln!("Error: Failed to load OpenAPI document: {error}");
                return 1;
            }
        };
        match resolve_operation(&spec, &request[0], &params_map) {
            Ok(found) => found,
            Err(message) => {
                eprintln!("Error: {message}");
                return 1;
            }
        }
    };

    let mut header_map: Vec<(String, String)> = Vec::new();
    let mut has_content_type = false;
    for header in headers {
        let Some((name, value)) = header.split_once(':').filter(|(name, _)| !name.is_empty())
        else {
            eprintln!("Error: Invalid header, expected name:value: {header}");
            return 1;
        };
        if name.trim().eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        header_map.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    if data.is_some() && !has_content_type {
        header_map.push(("content-type".to_owned(), "application/json".to_owned()));
    }

    let mut url = reqwest::Url::parse(&endpoint.url).expect("valid endpoint url");
    // Replace (not join) so absolute-looking paths stay rooted at the server.
    let (path, query) =
        resolved_request.path.split_once('?').unwrap_or((resolved_request.path.as_str(), ""));
    url.set_path(path);
    url.set_query(if query.is_empty() { None } else { Some(query) });

    let method =
        reqwest::Method::from_bytes(resolved_request.method.as_bytes()).expect("valid method");
    let http = reqwest::Client::new();
    let mut request = http.request(method, url);
    if let Some(auth) = basic_auth_header(endpoint.password.as_deref()) {
        request = request.header("authorization", auth);
    }
    request = request.headers({
        let mut map = reqwest::header::HeaderMap::new();
        for (name, value) in &header_map {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::try_from(name.as_str()),
                reqwest::header::HeaderValue::try_from(value.as_str()),
            ) {
                map.insert(name, value);
            }
        }
        map
    });
    if let Some(data) = data {
        request = request.body(data.to_owned());
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    let status = response.status();
    let output = response.text().await.unwrap_or_default();
    if !output.is_empty() {
        println!("{}", output.trim_end_matches('\n'));
    }
    if !status.is_success() {
        eprintln!("HTTP {} {}", status.as_u16(), status.canonical_reason().unwrap_or_default());
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn interpolation_path_and_query() {
        let map = params(&[("sessionID", "ses 1"), ("limit", "5")]);
        assert_eq!(
            interpolate("/api/session/{sessionID}", &map).expect("interpolate"),
            "/api/session/ses%201?limit=5"
        );
        assert!(interpolate("/api/session/{missing}", &map).is_err());
    }

    #[test]
    fn raw_request_shapes() {
        assert!(raw_request(&["get".into(), "/x".into()]).is_some());
        assert!(raw_request(&["GET".into(), "/x".into()]).is_some());
        assert!(raw_request(&["fetch".into(), "/x".into()]).is_none());
        assert!(raw_request(&["get".into(), "x".into()]).is_none());
    }
}
