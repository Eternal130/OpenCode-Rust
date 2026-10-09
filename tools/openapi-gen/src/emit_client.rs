//! Emits `crates/oc-client/src/methods.gen.rs`: namespaced `Client` methods.

use std::collections::BTreeMap;

use crate::emit_routes::client_path;
use crate::model::Operation;
use crate::naming::{pascal_ident, snake_ident};
use crate::tygen::{BodyOut, FieldRender, OpTypes, QueryStruct, RespOut};

pub struct OpInfo {
    pub op: Operation,
    pub types: OpTypes,
    pub module: Vec<String>,
    pub method: String,
}

struct Node {
    methods: Vec<usize>,
    children: BTreeMap<String, Node>,
}

fn insert(node: &mut Node, module: &[String], index: usize) {
    if module.is_empty() {
        node.methods.push(index);
        return;
    }
    let child = node
        .children
        .entry(module[0].clone())
        .or_insert(Node { methods: Vec::new(), children: BTreeMap::new() });
    insert(child, &module[1..], index);
}

fn method_enum(method: &str) -> &'static str {
    match method {
        "get" => "reqwest::Method::GET",
        "post" => "reqwest::Method::POST",
        "put" => "reqwest::Method::PUT",
        "delete" => "reqwest::Method::DELETE",
        "patch" => "reqwest::Method::PATCH",
        other => panic!("unknown method {other}"),
    }
}

fn query_rendering(q: &QueryStruct) -> String {
    let mut code = String::from("        let mut pairs: Vec<(String, String)> = Vec::new();\n");
    let mut hdrs =
        String::from("        let mut hdrs: Vec<(&'static str, String)> = Vec::new();\n");
    for f in &q.fields {
        let optional = matches!(f.ty, crate::tygen::Ty::Opt(_));
        let target = if f.header { &mut hdrs } else { &mut code };
        let vec = if f.header { "hdrs" } else { "pairs" };
        // Header keys are `&'static str`; query keys are owned `String`s.
        let key =
            if f.header { format!("\"{}\"", f.wire) } else { format!("\"{}\".to_owned()", f.wire) };
        match f.render {
            FieldRender::Str => {
                if optional {
                    target.push_str(&format!(
                        "        if let Some(v) = &query.{} {{\n            {}.push(({}, v.clone()));\n        }}\n",
                        f.ident, vec, key
                    ));
                } else {
                    target.push_str(&format!(
                        "        {}.push(({}, query.{}.clone()));\n",
                        vec, key, f.ident
                    ));
                }
            }
            FieldRender::EnumAsStr => {
                if optional {
                    target.push_str(&format!(
                        "        if let Some(v) = &query.{} {{\n            {}.push(({}, v.as_str().to_owned()));\n        }}\n",
                        f.ident, vec, key
                    ));
                } else {
                    target.push_str(&format!(
                        "        {}.push(({}, query.{}.as_str().to_owned()));\n",
                        vec, key, f.ident
                    ));
                }
            }
            FieldRender::LocationObject => code.push_str(
                "        if let Some(loc) = &query.location {\n            if let Some(dir) = &loc.directory {\n                pairs.push((\"location[directory]\".to_owned(), dir.clone()));\n            }\n        }\n",
            ),
        }
    }
    code.push_str(&hdrs);
    code
}

fn error_doc(op: &Operation) -> String {
    op.errors
        .iter()
        .map(|e| {
            let name = e.ref_name.clone().unwrap_or_else(|| "unknown".to_owned());
            format!("`{}` {}", e.status, name)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// (rust ident, wire name) for path params, in template order.
fn path_args(op: &Operation) -> Vec<(String, String)> {
    let mut args: Vec<(String, String)> = op
        .path
        .split('/')
        .filter(|s| s.starts_with('{'))
        .map(|s| s.trim_matches(['{', '}']).to_owned())
        .map(|name| (snake_ident(&name), name))
        .collect();
    if op.path.ends_with('*') && !args.iter().any(|(ident, _)| ident == "path") {
        args.push(("path".to_owned(), "path".to_owned()));
    }
    args
}

pub fn emit(infos: &[OpInfo]) -> String {
    let mut root = Node { methods: Vec::new(), children: BTreeMap::new() };
    for (i, info) in infos.iter().enumerate() {
        insert(&mut root, &info.module, i);
    }
    assert_method_names_unique(&root, infos, &[]);

    let mut out = String::new();
    out.push_str(crate::emit_routes::ROUTES_HEADER);
    out.push_str("//! Namespaced oc-client methods for all HttpApi operations.\n");
    out.push_str("//! Mirrors the v2 SDK namespaces; every method maps 1:1 onto a route in\n");
    out.push_str("//! `oc_server::routes_gen::ROUTES` (operationId parity, 141/141).\n");
    out.push_str("#![allow(clippy::too_many_lines, clippy::struct_excessive_bools)]\n");
    out.push_str("#![allow(unused_mut, clippy::must_use_candidate, clippy::vec_init_then_push)]\n");
    out.push_str("#![allow(clippy::doc_markdown)]\n\n");
    out.push_str("/// Every operationId covered by this client (sorted).\n");
    out.push_str("pub static OPERATION_IDS: &[&str] = &[\n");
    let mut ids: Vec<&str> = infos.iter().map(|i| i.op.operation_id.as_str()).collect();
    ids.sort_unstable();
    for id in ids {
        out.push_str(&format!("    \"{id}\",\n"));
    }
    out.push_str("];\n\n");

    emit_node(&mut out, &root, infos, &[]);
    out
}

fn assert_method_names_unique(node: &Node, infos: &[OpInfo], prefix: &[String]) {
    let mut seen: Vec<&str> = node.methods.iter().map(|&i| infos[i].method.as_str()).collect();
    seen.sort_unstable();
    let mut sorted = seen.clone();
    sorted.dedup();
    assert_eq!(seen, sorted, "duplicate method names in namespace {}", prefix.join("::"));
    for (name, child) in &node.children {
        let mut p = prefix.to_vec();
        p.push(name.clone());
        assert_method_names_unique(child, infos, &p);
    }
}

fn api_struct_name(module: &[String]) -> String {
    format!("{}Api", pascal_ident(&module.join("_")))
}

fn emit_node(out: &mut String, node: &Node, infos: &[OpInfo], prefix: &[String]) {
    for (name, child) in &node.children {
        let mut module_path = prefix.to_vec();
        module_path.push(name.clone());
        out.push_str(&format!(
            "// Namespace `{}` (mirrors the v2 SDK method group).\npub mod {} {{\n",
            module_path.join("::"),
            snake_ident(name)
        ));
        emit_node(out, child, infos, &module_path);
        out.push_str("}\n\n");
    }
    if prefix.is_empty() {
        return;
    }
    let struct_name = api_struct_name(prefix);
    out.push_str(&format!(
        "pub struct {struct_name}<'a> {{\n    pub(crate) client: &'a crate::Client,\n}}\n\n"
    ));
    out.push_str(&format!("impl {struct_name}<'_> {{\n"));
    for &index in &node.methods {
        emit_method(out, &infos[index]);
    }
    for name in node.children.keys() {
        let mut child_module = prefix.to_vec();
        child_module.push(name.clone());
        let child_struct = api_struct_name(&child_module);
        out.push_str(&format!(
            "    /// Nested namespace `{}::{}`.\n    #[must_use]\n    pub fn {}(&self) -> {}::{}<'_> {{\n        {}::{} {{ client: self.client }}\n    }}\n",
            prefix.join("::"),
            name,
            snake_ident(name),
            snake_ident(name),
            child_struct,
            snake_ident(name),
            child_struct
        ));
    }
    out.push_str("}\n\n");
    if prefix.len() == 1 {
        let module_name = snake_ident(&prefix[0]);
        out.push_str(&format!(
            "impl crate::Client {{\n    /// Namespace `{}` operations.\n    #[must_use]\n    pub fn {}(&self) -> {}<'_> {{\n        {} {{ client: self }}\n    }}\n}}\n\n",
            prefix.join("::"),
            module_name,
            struct_name,
            struct_name
        ));
    }
}

fn emit_method(out: &mut String, info: &OpInfo) {
    let op = &info.op;
    let types = &info.types;
    let path_args = path_args(op);

    let mut sig = String::new();
    for (ident, _wire) in &path_args {
        sig.push_str(&format!("        {ident}: &str,\n"));
    }
    if let Some(q) = &types.query_struct {
        sig.push_str(&format!("        query: &crate::types::{},\n", q.rust_name));
    }
    match &types.body {
        BodyOut::None => {}
        BodyOut::Binary => sig.push_str("        payload: &[u8],\n"),
        BodyOut::Json(ty) => {
            sig.push_str(&format!("        body: &{},\n", ty.render(true)));
        }
    }

    let (ret, exec, unwrap, sse) = match &types.response {
        RespOut::Unit => ("()".to_owned(), "execute_empty".to_owned(), false, None),
        RespOut::Json(ty) => (ty.render(true), "execute_json".to_owned(), false, None),
        RespOut::Unwrap(ty) => (ty.render(true), "execute_json".to_owned(), true, None),
        RespOut::Binary => ("Vec<u8>".to_owned(), "execute_bytes".to_owned(), false, None),
        RespOut::Sse { double_decode } => (
            "crate::sse::SseStream".to_owned(),
            "execute_sse".to_owned(),
            false,
            Some(*double_decode),
        ),
    };

    let mut notes = vec![format!(
        "{} {} — operationId `{}`",
        op.method.to_uppercase(),
        op.path,
        op.operation_id
    )];
    if !op.errors.is_empty() {
        notes.push(format!("Declared errors: {}", error_doc(op)));
    }
    if unwrap {
        notes.push("The `{{ data }}` envelope is unwrapped (v2 client parity).".to_owned());
    }
    match op.operation_id.as_str() {
        "pty.connect" | "persistentPty.connect" => notes.push(
            "TODO(pty-ws): schema models this as a boolean GET; the real endpoint is a\n    /// WebSocket upgrade — WS clients live outside this crate (server-deep-map §3).".to_owned(),
        ),
        _ => {}
    }
    if matches!(types.response, RespOut::Sse { double_decode: true }) {
        notes.push(
            "SSE stream; `data` frames are JSON-in-string payloads, double-decoded by\n    /// `SseStream` (server-deep-map §7).".to_owned(),
        );
    }
    if op.path.ends_with('*')
        || matches!(types.body, BodyOut::Binary)
        || matches!(types.response, RespOut::Binary)
    {
        notes.push(
            "Binary/octet-stream endpoint: bytes pass through untyped (TODO: mime codecs)."
                .to_owned(),
        );
    }

    out.push_str(&format!("    /// {}\n", notes.join("\n    /// ")));
    // rustfmt drops the trailing comma for zero-arg signatures; match it so
    // regeneration is as close to formatted output as practical.
    let self_arg = if sig.is_empty() { "        &self\n" } else { "        &self,\n" };
    out.push_str(&format!(
        "    pub async fn {}(\n{}{}    ) -> Result<{}, crate::ClientError> {{\n",
        info.method, self_arg, sig, ret
    ));

    if let Some(q) = &types.query_struct {
        out.push_str(&query_rendering(q));
    } else {
        out.push_str("        let pairs: Vec<(String, String)> = Vec::new();\n");
        out.push_str("        let hdrs: Vec<(&'static str, String)> = Vec::new();\n");
    }
    let body_expr = match &types.body {
        BodyOut::None => "crate::transport::Body::None",
        BodyOut::Binary => "crate::transport::Body::Bytes(payload)",
        BodyOut::Json(_) => {
            out.push_str("        let body_value = crate::transport::encode_body(body)?;\n");
            "crate::transport::Body::Json(&body_value)"
        }
    };

    let path_params: Vec<String> =
        path_args.iter().map(|(ident, wire)| format!("(\"{wire}\", {ident})")).collect();
    let declared: Vec<String> = op.errors.iter().map(|e| format!("{}", e.status)).collect();
    let sse_arg = match sse {
        Some(double) => format!(",\n            {double}"),
        None => String::new(),
    };
    out.push_str(&format!(
        "        self.client.{}(\n            &crate::transport::Call {{\n                method: {},\n                path: \"{}\",\n                path_params: &[{}],\n                query: &pairs,\n                headers: &hdrs,\n                body: {},\n                success: {},\n                declared: &[{}],\n            }}{}\n        )\n        .await\n",
        if unwrap {
            format!("execute_json::<crate::transport::Envelope<{ret}>>")
        } else {
            exec
        },
        method_enum(&op.method),
        client_path(&op.path),
        path_params.join(", "),
        body_expr,
        op.success.status,
        declared.join(", "),
        sse_arg,
    ));
    if unwrap {
        out.push_str("        .map(|env| env.data)\n");
    }
    out.push_str("    }\n\n");
}
