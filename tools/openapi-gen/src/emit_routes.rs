//! Emits `crates/oc-server/src/routes.gen.rs`: the static route table.

use crate::emit_client::OpInfo;
use crate::model::Operation;
use crate::tygen::{BodyOut, OpTypes, RespOut};

pub const ROUTES_HEADER: &str =
    "// Generated file — DO NOT EDIT by hand.\n// Regenerate with: cargo run -p openapi-gen\n";

/// `/api/session/{sessionID}` -> `/api/session/:sessionID` (axum-style),
/// wildcard tails become `*name`.
pub fn axum_path(path: &str) -> String {
    if let Some(prefix) = path.strip_suffix('*') {
        return format!("{prefix}*path");
    }
    path.replace('{', ":").replace('}', "")
}

/// `/api/fs/read/*` -> `/api/fs/read/{*path}` for client-side substitution.
pub fn client_path(path: &str) -> String {
    if let Some(prefix) = path.strip_suffix('*') {
        return format!("{prefix}{{*path}}");
    }
    path.to_owned()
}

fn method_enum(method: &str) -> &'static str {
    match method {
        "get" => "Method::Get",
        "post" => "Method::Post",
        "put" => "Method::Put",
        "delete" => "Method::Delete",
        "patch" => "Method::Patch",
        other => panic!("unknown method {other}"),
    }
}

fn request_summary(op: &Operation, types: &OpTypes) -> String {
    match (&types.body, &op.body) {
        (BodyOut::None, _) => String::new(),
        (BodyOut::Binary, _) => "bytes (application/octet-stream)".to_owned(),
        (BodyOut::Json(ty), Some(b)) => {
            if let Some(reference) = &b.ref_name {
                format!("{} (`{reference}`)", ty.render(false))
            } else {
                ty.render(false)
            }
        }
        (BodyOut::Json(_), None) => String::new(),
    }
}

fn response_summary(types: &OpTypes) -> String {
    match &types.response {
        RespOut::Unit => "()".to_owned(),
        RespOut::Binary => "bytes (application/octet-stream)".to_owned(),
        RespOut::Sse { double_decode } => {
            if *double_decode {
                "text/event-stream (JSON-in-string data)".to_owned()
            } else {
                "text/event-stream".to_owned()
            }
        }
        RespOut::Unwrap(ty) => format!("{} [unwrapped {{data}}]", ty.render(false)),
        RespOut::Json(ty) => ty.render(false),
    }
}

fn path_param_names(op: &Operation) -> Vec<String> {
    let mut names: Vec<String> = op
        .params
        .iter()
        .filter(|p| p.location == crate::model::ParamLoc::Path)
        .map(|p| p.name.clone())
        .collect();
    names.sort();
    names.dedup();
    if op.path.ends_with('*') && !names.iter().any(|n| n == "path") {
        names.push("path".to_owned());
    }
    names
}

pub fn emit(infos: &[OpInfo], sha256: &str) -> String {
    let mut out = String::new();
    out.push_str(ROUTES_HEADER);
    out.push_str(&format!("// Source: tools/openapi-gen/openapi.json (sha256 {sha256})\n"));
    out.push_str(
        "//! Static route table for the opencode HttpApi surface (117 paths / 141 operations).\n",
    );
    out.push_str("//! Paths are axum-style templates: `:param` captures one segment, `*name`\n");
    out.push_str("//! captures the remainder (axum 0.8 spellings: `{param}` / `{*name}`).\n");
    out.push_str("//! Consumed by the oc-server dispatch; mirrors oc-client namespaces 1:1.\n");
    out.push_str("#![allow(clippy::struct_excessive_bools)]\n\n");
    out.push_str("pub const OPENAPI_SHA256: &str = \"");
    out.push_str(sha256);
    out.push_str("\";\n");
    out.push_str(&format!("pub const OPERATION_COUNT: usize = {};\n\n", infos.len()));
    out.push_str(
        r#"#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathParam {
    pub name: &'static str,
    pub rust_type: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryParam {
    pub name: &'static str,
    pub rust_type: &'static str,
    pub required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub method: Method,
    /// axum-style template (`:param` / `*name`).
    pub path: &'static str,
    pub operation_id: &'static str,
    /// oc-client namespace path (e.g. `session::revert`).
    pub group: &'static str,
    /// Matching `oc_client` method (e.g. `session::revert::clear`).
    pub client_method: &'static str,
    pub path_params: &'static [PathParam],
    pub query_params: &'static [QueryParam],
    /// Resolved request-body type name (empty = no body).
    pub request_schema: &'static str,
    /// Resolved 2xx response type summary.
    pub response_schema: &'static str,
    pub success_status: u16,
    pub error_statuses: &'static [u16],
}

"#,
    );
    out.push_str("pub static ROUTES: &[Route] = &[\n");
    for info in infos {
        let op = &info.op;
        let types = &info.types;
        let (module, method) = (info.module.clone(), info.method.clone());
        let path_params = path_param_names(op)
            .iter()
            .map(|n| format!("PathParam {{ name: \"{n}\", rust_type: \"String\" }}"))
            .collect::<Vec<_>>()
            .join(", ");
        let query_params = types
            .query_struct
            .as_ref()
            .map(|q| {
                q.fields
                    .iter()
                    .filter(|f| !f.header)
                    .map(|f| {
                        format!(
                            "QueryParam {{ name: \"{}\", rust_type: \"{}\", required: false }}",
                            f.wire,
                            f.ty.render(false)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let error_statuses =
            op.errors.iter().map(|e| format!("{}", e.status)).collect::<Vec<_>>().join(", ");
        let request_schema = request_summary(op, types).replace('"', "\\\"");
        let response_schema = response_summary(types).replace('"', "\\\"");
        let group = module.join("::");
        let client_method = format!("{group}::{method}");
        out.push_str(&format!(
            "    Route {{\n        method: {},\n        path: \"{}\",\n        operation_id: \"{}\",\n        group: \"{}\",\n        client_method: \"{}\",\n        path_params: &[{}],\n        query_params: &[{}],\n        request_schema: \"{}\",\n        response_schema: \"{}\",\n        success_status: {},\n        error_statuses: &[{}],\n    }},\n",
            method_enum(&op.method),
            axum_path(&op.path),
            op.operation_id,
            group,
            client_method,
            path_params,
            query_params,
            request_schema,
            response_schema,
            op.success.status,
            error_statuses,
        ));
    }
    out.push_str("];\n\n");
    out.push_str(MATCHER);
    out
}

const MATCHER: &str = r#"/// Matches a concrete request path against the route table.
///
/// Returns the route plus captured path parameters (wildcard captures the
/// remainder verbatim, i.e. still percent-encoded).
pub fn find(method: Method, path: &str) -> Option<(&'static Route, Vec<(&'static str, String)>)> {
    let segments: Vec<&str> = path.split('/').collect();
    'routes: for route in ROUTES {
        if route.method != method {
            continue;
        }
        let template: Vec<&str> = route.path.split('/').collect();
        let mut params = Vec::new();
        for (i, seg) in template.iter().enumerate() {
            if let Some(name) = seg.strip_prefix('*') {
                let rest = segments.get(i..).unwrap_or(&[]).join("/");
                if rest.is_empty() {
                    continue 'routes;
                }
                params.push((name, rest));
                return Some((route, params));
            }
            if let Some(name) = seg.strip_prefix(':') {
                match segments.get(i) {
                    Some(actual) if !actual.is_empty() => params.push((name, (*actual).to_owned())),
                    _ => continue 'routes,
                }
            } else if segments.get(i) != Some(seg) {
                continue 'routes;
            }
        }
        if template.len() == segments.len() {
            return Some((route, params));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_all_operations() {
        assert_eq!(ROUTES.len(), OPERATION_COUNT);
        let mut ids: Vec<&str> = ROUTES.iter().map(|r| r.operation_id).collect();
        ids.sort_unstable();
        let mut sorted = ids.clone();
        sorted.dedup();
        assert_eq!(ids, sorted, "operation ids must be unique");
    }

    #[test]
    fn matches_concrete_paths() {
        let (route, params) = find(Method::Get, "/api/session/ses_123/message/msg_1").expect("match");
        assert_eq!(route.operation_id, "session.message.get");
        let mut names: Vec<(&str, String)> = params;
        names.sort_unstable();
        assert_eq!(
            names,
            vec![("messageID", "msg_1".to_owned()), ("sessionID", "ses_123".to_owned())]
        );

        let (route, params) = find(Method::Get, "/api/fs/read/a/b/c.txt").expect("wildcard match");
        assert_eq!(route.operation_id, "fs.read");
        assert_eq!(params, vec![("path", "a/b/c.txt".to_owned())]);

        assert!(find(Method::Delete, "/api/session/ses_1/message/msg_1").is_none());
        assert!(find(Method::Get, "/api/session/").is_none());
        assert!(find(Method::Get, "/api/definitely/not/a/route").is_none());
    }

    #[test]
    fn wildcard_requires_tail() {
        // The wildcard route must not swallow its prefix without a tail.
        assert!(find(Method::Get, "/api/fs/read").is_none());
    }
}
"#;
