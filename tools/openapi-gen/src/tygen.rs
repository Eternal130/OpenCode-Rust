//! JSON-Schema -> Rust type mapping for the pinned OpenAPI document.
//!
//! Deterministic: named schemas and operations are walked in sorted order and
//! every synthesized name goes through the [`Names`] allocator.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{Map, Value};

use crate::naming::{doc_line, pascal_ident, snake_ident, Names};

#[derive(Debug, Clone)]
pub enum Ty {
    Named(String),
    Str,
    I64,
    F64,
    Bool,
    Value,
    VecT(Box<Ty>),
    MapT(Box<Ty>),
    Opt(Box<Ty>),
}

impl Ty {
    pub fn render(&self, qualify: bool) -> String {
        match self {
            Ty::Named(n) => {
                if qualify {
                    format!("crate::types::{n}")
                } else {
                    n.clone()
                }
            }
            Ty::Str => "String".to_owned(),
            Ty::I64 => "i64".to_owned(),
            Ty::F64 => "f64".to_owned(),
            Ty::Bool => "bool".to_owned(),
            Ty::Value => "serde_json::Value".to_owned(),
            Ty::VecT(t) => format!("Vec<{}>", t.render(qualify)),
            Ty::MapT(t) => format!("std::collections::BTreeMap<String, {}>", t.render(qualify)),
            Ty::Opt(t) => format!("Option<{}>", t.render(qualify)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Struct,
    Enum,
    Union,
    Alias,
}

pub struct Def {
    pub name: String,
    pub kind: DefKind,
    pub code: String,
}

/// Per-operation type decisions consumed by the method / route emitters.
pub struct OpTypes {
    pub query_struct: Option<QueryStruct>,
    pub body: BodyOut,
    pub response: RespOut,
}

pub struct QueryField {
    pub wire: String,
    pub ident: String,
    pub ty: Ty,
    pub header: bool,
    /// How the value is rendered into query pairs / headers.
    pub render: FieldRender,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldRender {
    /// `Option<String>` (or alias-of-string) -> direct clone.
    Str,
    /// Enum -> `as_str()`.
    EnumAsStr,
    /// Shared `QueryLocation` struct -> expands to `location[directory]`.
    LocationObject,
}

pub struct QueryStruct {
    pub rust_name: String,
    pub fields: Vec<QueryField>,
}

pub enum BodyOut {
    None,
    Json(Ty),
    Binary,
}

pub enum RespOut {
    /// 204 No Content.
    Unit,
    Json(Ty),
    /// `{ data: T }` envelope unwrapped client-side (mirrors the v2 client).
    Unwrap(Ty),
    Binary,
    Sse {
        double_decode: bool,
    },
}

pub struct TyGen<'a> {
    schemas: &'a Map<String, Value>,
    names: Names,
    defs: Vec<Def>,
    def_index: HashMap<String, DefKind>,
    defaultable: HashSet<String>,
    schema_names: HashMap<String, String>,
    pub warnings: Vec<String>,
}

fn is_null_schema(s: &Value) -> bool {
    s.get("type").and_then(Value::as_str) == Some("null")
        || s == &Value::Bool(false)
        || (s
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|e| !e.is_empty() && e.iter().all(Value::is_null)))
}

fn singleton_string_enum(s: &Value) -> Option<String> {
    let vals = s.get("enum")?.as_array()?;
    if vals.len() != 1 {
        return None;
    }
    vals[0].as_str().map(str::to_owned)
}

impl<'a> TyGen<'a> {
    pub fn new(schemas: &'a Map<String, Value>) -> Self {
        let mut gen = TyGen {
            schemas,
            names: Names::default(),
            defs: Vec::new(),
            def_index: HashMap::new(),
            defaultable: HashSet::new(),
            schema_names: HashMap::new(),
            warnings: Vec::new(),
        };
        // Shared query-param struct for the structured `location` parameter
        // (v2 serializes it as `location[directory]=...`).
        gen.names.reserve("QueryLocation");
        gen.def_index.insert("QueryLocation".to_owned(), DefKind::Struct);
        gen.defaultable.insert("QueryLocation".to_owned());
        gen.defs.push(Def {
            name: "QueryLocation".to_owned(),
            kind: DefKind::Struct,
            code: concat!(
                "#[derive(Debug, Clone, Default, PartialEq)]\n",
                "pub struct QueryLocation {\n",
                "    /// Wire field: `directory`.\n",
                "    pub directory: Option<String>,\n",
                "}\n",
            )
            .to_owned(),
        });
        gen
    }

    pub fn defs(&self) -> &[Def] {
        &self.defs
    }

    pub fn def_kind(&self, name: &str) -> Option<DefKind> {
        self.def_index.get(name).copied()
    }

    fn is_defaultable(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Named(n) => self.defaultable.contains(n),
            Ty::Str | Ty::I64 | Ty::F64 | Ty::Bool | Ty::Value => true,
            Ty::VecT(_) | Ty::MapT(_) | Ty::Opt(_) => true,
        }
    }

    /// Emits (once) the Rust definition for a named component schema.
    pub fn named_type(&mut self, schema_name: &str) -> Ty {
        if let Some(rust) = self.schema_names.get(schema_name) {
            return Ty::Named(rust.clone());
        }
        let rust = self.names.alloc(pascal_ident(schema_name));
        self.schema_names.insert(schema_name.to_owned(), rust.clone());
        let schema = self
            .schemas
            .get(schema_name)
            .unwrap_or_else(|| panic!("unresolved schema ref {schema_name}"))
            .clone();
        let doc = schema.get("description").and_then(Value::as_str).map(str::to_owned);
        self.emit_root_def(&schema, &rust, schema_name, doc.as_deref());
        Ty::Named(rust)
    }

    fn emit_root_def(&mut self, schema: &Value, rust: &str, schema_name: &str, doc: Option<&str>) {
        let header = Self::doc_header(doc, &format!("Wire schema: `{schema_name}`."));
        if let Some(members) = schema.get("anyOf").and_then(Value::as_array) {
            let non_null = members.iter().filter(|m| !is_null_schema(m)).count();
            if non_null == 1 && members.len() > non_null {
                let only =
                    members.iter().find(|m| !is_null_schema(m)).cloned().unwrap_or(Value::Null);
                let inner = self.ty_for(&only, rust.to_owned());
                self.push_alias(rust, format!("Option<{}>", inner.render(false)), header);
                return;
            }
        }
        if schema.get("properties").is_some() {
            self.emit_struct_from(schema, rust, header, None);
            return;
        }
        if let Some(values) = string_enum_values(schema) {
            self.emit_enum(rust, &values, header);
            return;
        }
        if schema.get("allOf").is_some() {
            match merge_all_of(self.schemas, schema, schema_name, &mut self.warnings) {
                Some(m) => {
                    self.emit_struct_parts(
                        rust,
                        m.props,
                        m.required,
                        header,
                        m.extra_value_ty.is_some(),
                    );
                }
                None => self.push_alias(rust, "serde_json::Value".to_owned(), header),
            }
            return;
        }
        if schema.get("anyOf").is_some() || schema.get("oneOf").is_some() {
            self.emit_union(schema, rust, header, schema_name);
            return;
        }
        // Trivial named schemas become type aliases (mirrors v2 `type X = ...`).
        let ty = self.ty_for(schema, rust.to_owned());
        self.push_alias(rust, ty.render(false), header);
    }

    fn doc_header(doc: Option<&str>, fallback: &str) -> String {
        let line = doc_line(doc, fallback);
        format!("/// {line}\n")
    }

    fn push_alias(&mut self, rust: &str, target: String, header: String) {
        self.def_index.insert(rust.to_owned(), DefKind::Alias);
        self.defaultable.insert(rust.to_owned());
        let code = format!("{header}pub type {rust} = {target};\n");
        self.defs.push(Def { name: rust.to_owned(), kind: DefKind::Alias, code });
    }

    fn emit_struct_from(
        &mut self,
        schema: &Value,
        rust: &str,
        header: String,
        drop_prop: Option<&str>,
    ) {
        let props =
            schema.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
        let required: BTreeSet<String> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        let mut filtered = Map::new();
        for (k, v) in props {
            if Some(k.as_str()) == drop_prop {
                continue;
            }
            filtered.insert(k, v);
        }
        let required = required.into_iter().filter(|r| Some(r.as_str()) != drop_prop).collect();
        let flatten = schema
            .get("additionalProperties")
            .map(|ap| ap.as_bool() != Some(false) && !ap.is_null())
            .unwrap_or(false);
        self.emit_struct_parts(rust, filtered, required, header, flatten);
    }

    fn emit_struct_parts(
        &mut self,
        rust: &str,
        props: Map<String, Value>,
        required: BTreeSet<String>,
        header: String,
        flatten_extra: bool,
    ) {
        let mut fields = String::new();
        let mut field_names = Names::default();
        let mut all_default = true;
        for (wire, prop) in &props {
            let ident = field_names.alloc(snake_ident(wire));
            let suggest = format!("{rust}{}", pascal_ident(wire));
            let inner = self.ty_for(prop, suggest);
            let nullable = matches!(inner, Ty::Opt(_));
            let is_required = required.contains(wire);
            let ty = if is_required || nullable { inner } else { Ty::Opt(Box::new(inner)) };
            if !self.is_defaultable(&ty) {
                all_default = false;
            }
            let doc = doc_line(
                prop.get("description").and_then(Value::as_str),
                &format!("Wire field: `{wire}`."),
            );
            fields.push_str(&format!("    /// {doc}\n"));
            let rendered = ty.render(false);
            if is_required {
                // Required-but-nullable serializes `null` explicitly (never skipped).
                fields.push_str(&format!(
                    "    #[serde(rename = \"{wire}\")]\n    pub {ident}: {rendered},\n"
                ));
            } else {
                fields.push_str(&format!(
                    "    #[serde(default, rename = \"{wire}\", skip_serializing_if = \"Option::is_none\")]\n    pub {ident}: {rendered},\n"
                ));
            }
        }
        if flatten_extra {
            let ident = field_names.alloc("extra".to_owned());
            fields.push_str(&format!(
                "    /// Catch-all for schemas with open `additionalProperties`.\n    #[serde(flatten)]\n    pub {ident}: std::collections::BTreeMap<String, serde_json::Value>,\n"
            ));
        }
        let derives = if all_default {
            "Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize"
        } else {
            "Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize"
        };
        let code = format!("{header}#[derive({derives})]\npub struct {rust} {{\n{fields}}}\n");
        self.def_index.insert(rust.to_owned(), DefKind::Struct);
        if all_default {
            self.defaultable.insert(rust.to_owned());
        }
        self.defs.push(Def { name: rust.to_owned(), kind: DefKind::Struct, code });
    }

    fn emit_enum(&mut self, rust: &str, values: &[String], header: String) {
        let mut variant_names = Names::default();
        let mut variants = String::new();
        let mut arms = String::new();
        for (i, value) in values.iter().enumerate() {
            let variant = variant_names.alloc(pascal_ident(value));
            if i == 0 && values.len() == 1 {
                // Singleton enums double as discriminators; a Default makes
                // the carrying structs constructible with `..Default::default()`.
                variants.push_str("    #[default]\n");
            }
            variants.push_str(&format!("    #[serde(rename = \"{value}\")]\n    {variant},\n"));
            arms.push_str(&format!("            Self::{variant} => \"{value}\",\n"));
        }
        let derives = if values.len() == 1 {
            "Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize"
        } else {
            "Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize"
        };
        let code = format!(
            "{header}#[derive({derives})]\npub enum {rust} {{\n{variants}}}\n\nimpl {rust} {{\n    pub fn as_str(&self) -> &'static str {{\n        match self {{\n{arms}        }}\n    }}\n}}\n"
        );
        self.def_index.insert(rust.to_owned(), DefKind::Enum);
        if values.len() == 1 {
            self.defaultable.insert(rust.to_owned());
        }
        self.defs.push(Def { name: rust.to_owned(), kind: DefKind::Enum, code });
    }

    fn emit_union(&mut self, schema: &Value, rust: &str, header: String, schema_name: &str) {
        let key = if schema.get("anyOf").is_some() { "anyOf" } else { "oneOf" };
        let members: Vec<Value> =
            schema.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        let non_null: Vec<Value> = members.iter().filter(|m| !is_null_schema(m)).cloned().collect();
        if non_null.is_empty() {
            self.push_alias(rust, "serde_json::Value".to_owned(), header);
            return;
        }
        // Discriminated unions stay `untagged`: every variant carries its own
        // singleton-enum tag field (refs keep theirs from the component
        // schema), which serde matches exactly without `tag` collisions.
        let tag = detect_discriminator(self.schemas, &non_null);
        let mut variant_names = Names::default();
        let mut variants = String::new();
        for (i, member) in non_null.iter().enumerate() {
            let suggest = format!("{rust}Variant{}", i + 1);
            let vty = self.variant_ty(member, suggest);
            let name_hint = match &tag {
                Some(t) if member.get("$ref").is_none() => t
                    .members_by_value
                    .iter()
                    .find(|(_v, m)| **m == *member)
                    .map(|(v, _)| pascal_ident(v))
                    .unwrap_or_else(|| variant_name_hint(member, &vty)),
                _ => variant_name_hint(member, &vty),
            };
            let variant = variant_names.alloc(name_hint);
            variants.push_str(&format!("    {variant}({}),\n", vty.render(false)));
        }
        let note = if tag.is_some() {
            format!(
                "/// Discriminated union (each variant carries its tag field);\n/// serde matches variants in declared order (wire schema `{schema_name}`).\n"
            )
        } else {
            format!(
                "/// Untagged union; serde matches variants in declared order (wire schema `{schema_name}`).\n"
            )
        };
        let code = format!(
            "{header}{note}#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]\n#[serde(untagged)]\npub enum {rust} {{\n{variants}}}\n"
        );
        self.def_index.insert(rust.to_owned(), DefKind::Union);
        self.defs.push(Def { name: rust.to_owned(), kind: DefKind::Union, code });
    }

    fn variant_ty(&mut self, member: &Value, suggest: String) -> Ty {
        if member.get("$ref").is_some() {
            return self.ty_for(member, suggest);
        }
        if member.get("properties").is_some() {
            // Inline variant struct keeps every field, including its
            // singleton-enum tag (the union itself is untagged).
            let rust = self.names.alloc(suggest);
            self.emit_struct_from(member, &rust, String::new(), None);
            return Ty::Named(rust);
        }
        self.ty_for(member, suggest)
    }

    /// Core schema -> [`Ty`] mapping for nested / inline schemas.
    pub fn ty_for(&mut self, schema: &Value, suggest: String) -> Ty {
        if schema.is_null() || schema == &Value::Bool(true) {
            return Ty::Value;
        }
        if schema.as_object().is_some_and(Map::is_empty) {
            return Ty::Value;
        }
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = reference.rsplit('/').next().unwrap_or(reference);
            return self.named_type(name);
        }
        if let Some(values) = string_enum_values(schema) {
            let rust = self.names.alloc(suggest);
            self.emit_enum(&rust, &values, String::new());
            return Ty::Named(rust);
        }
        if let Some(members) = schema.get("anyOf").and_then(Value::as_array).cloned() {
            let non_null: Vec<Value> =
                members.iter().filter(|m| !is_null_schema(m)).cloned().collect();
            if non_null.is_empty() {
                return Ty::Value;
            }
            if non_null.len() == 1 {
                return Ty::Opt(Box::new(self.ty_for(&non_null[0], suggest)));
            }
            // Unions of string-ish schemas collapse to `String`
            // (e.g. `string | "null"` filters such as session parentID).
            if non_null.iter().all(|m| stringish(m)) {
                return Ty::Str;
            }
            let rust = self.names.alloc(suggest.clone());
            self.emit_union(schema, &rust, String::new(), &suggest);
            return Ty::Named(rust);
        }
        if let Some(members) = schema.get("oneOf").and_then(Value::as_array).cloned() {
            let non_null: Vec<Value> =
                members.iter().filter(|m| !is_null_schema(m)).cloned().collect();
            if non_null.len() <= 1 {
                if let Some(only) = non_null.first() {
                    return Ty::Opt(Box::new(self.ty_for(only, suggest)));
                }
                return Ty::Value;
            }
            let rust = self.names.alloc(suggest.clone());
            self.emit_union(schema, &rust, String::new(), &suggest);
            return Ty::Named(rust);
        }
        if schema.get("allOf").is_some() {
            return match merge_all_of(self.schemas, schema, &suggest, &mut self.warnings) {
                Some(m) => {
                    let rust = self.names.alloc(suggest);
                    self.emit_struct_parts(
                        &rust,
                        m.props,
                        m.required,
                        String::new(),
                        m.extra_value_ty.is_some(),
                    );
                    Ty::Named(rust)
                }
                None => Ty::Value,
            };
        }
        match schema.get("type").and_then(Value::as_str) {
            Some("object") => {
                if schema.get("properties").is_some() {
                    let rust = self.names.alloc(suggest);
                    self.emit_struct_from(schema, &rust, String::new(), None);
                    return Ty::Named(rust);
                }
                let value_ty = schema
                    .get("additionalProperties")
                    .and_then(|ap| {
                        if ap.as_bool() == Some(true) || ap.is_null() || is_null_schema(ap) {
                            None
                        } else {
                            Some(ap.clone())
                        }
                    })
                    .map(|ap| self.ty_for(&ap, format!("{suggest}Value")))
                    .unwrap_or(Ty::Value);
                Ty::MapT(Box::new(value_ty))
            }
            Some("array") => {
                let items = schema.get("items").cloned().unwrap_or(Value::Null);
                if let Some(prefix) = schema.get("prefixItems").and_then(Value::as_array) {
                    // Tuple arrays are only representable when the single
                    // prefix item matches `items` (min-length homogeneous array).
                    if !(prefix.len() == 1 && prefix[0] == items) {
                        self.warnings.push(format!(
                            "tuple array not representable at `{suggest}`; emitted serde_json::Value (TODO)"
                        ));
                        return Ty::Value;
                    }
                }
                Ty::VecT(Box::new(self.ty_for(&items, format!("{suggest}Item"))))
            }
            Some("string") => Ty::Str,
            Some("integer") => Ty::I64,
            Some("number") => Ty::F64,
            Some("boolean") => Ty::Bool,
            Some("null") => Ty::Value,
            _ => Ty::Value,
        }
    }

    // ---- operation-level passes ------------------------------------------

    pub fn op_types(&mut self, op: &crate::model::Operation) -> OpTypes {
        let op_pascal = pascal_ident(&op.operation_id);

        // Query struct: query + header params.
        let mut qfields: Vec<QueryField> = Vec::new();
        let mut field_names = Names::default();
        for param in &op.params {
            if param.location == crate::model::ParamLoc::Path {
                continue;
            }
            let suggest = format!("{op_pascal}Query{}", pascal_ident(&param.name));
            let (ty, render) = if param.name == "location" && is_query_location(&param.schema) {
                (
                    Ty::Opt(Box::new(Ty::Named("QueryLocation".to_owned()))),
                    FieldRender::LocationObject,
                )
            } else {
                let ty = if param.required {
                    self.ty_for(&param.schema, suggest)
                } else {
                    match self.ty_for(&param.schema, suggest) {
                        Ty::Opt(inner) => Ty::Opt(inner),
                        inner => Ty::Opt(Box::new(inner)),
                    }
                };
                let render = match &ty {
                    Ty::Opt(inner) => match inner.as_ref() {
                        Ty::Named(n) if self.def_kind(n) == Some(DefKind::Enum) => {
                            FieldRender::EnumAsStr
                        }
                        _ => FieldRender::Str,
                    },
                    Ty::Named(n) if self.def_kind(n) == Some(DefKind::Enum) => {
                        FieldRender::EnumAsStr
                    }
                    _ => FieldRender::Str,
                };
                (ty, render)
            };
            qfields.push(QueryField {
                wire: param.name.clone(),
                ident: field_names.alloc(snake_ident(&param.name)),
                ty,
                header: param.location == crate::model::ParamLoc::Header,
                render,
            });
        }
        let query_struct = if qfields.is_empty() {
            None
        } else {
            let rust_name = format!("{op_pascal}Query");
            let mut fields = String::new();
            let mut all_default = true;
            for f in &qfields {
                if !self.is_defaultable(&f.ty) {
                    all_default = false;
                }
                let loc = if f.header { "header" } else { "query" };
                let wire = &f.wire;
                let doc = doc_line(None, &format!("Wire param: `{wire}` ({loc})."));
                fields.push_str(&format!(
                    "    /// {doc}\n    pub {}: {},\n",
                    f.ident,
                    f.ty.render(false)
                ));
            }
            let derives = if all_default {
                "Debug, Clone, Default, PartialEq"
            } else {
                "Debug, Clone, PartialEq"
            };
            self.def_index.insert(rust_name.clone(), DefKind::Struct);
            if all_default {
                self.defaultable.insert(rust_name.clone());
            }
            self.defs.push(Def {
                name: rust_name.clone(),
                kind: DefKind::Struct,
                code: format!("#[derive({derives})]\npub struct {rust_name} {{\n{fields}}}\n"),
            });
            Some(QueryStruct { rust_name, fields: qfields })
        };

        // Body.
        let body = match &op.body {
            None => BodyOut::None,
            Some(b) if b.binary => BodyOut::Binary,
            Some(b) => match &b.schema {
                Some(schema) if schema.get("properties").is_some() && b.ref_name.is_none() => {
                    let rust = format!("{op_pascal}Body");
                    let header = format!("/// Request body for `{}`.\n", op.operation_id);
                    self.emit_struct_from(schema, &rust, header, None);
                    BodyOut::Json(Ty::Named(rust))
                }
                Some(schema) => BodyOut::Json(self.ty_for(schema, format!("{op_pascal}Body"))),
                None => BodyOut::None,
            },
        };

        // Response.
        let response = self.response_out(op, &op_pascal);

        OpTypes { query_struct, body, response }
    }

    fn response_out(&mut self, op: &crate::model::Operation, op_pascal: &str) -> RespOut {
        let success = &op.success;
        if success.status == 204 || success.content_type == crate::model::ContentType::None {
            return RespOut::Unit;
        }
        match success.content_type {
            crate::model::ContentType::OctetStream => RespOut::Binary,
            crate::model::ContentType::EventStream => {
                RespOut::Sse { double_decode: success.data_json_string }
            }
            crate::model::ContentType::Json => {
                let schema = success.schema.clone().unwrap_or(Value::Null);
                // `{ data }`-only envelope: unwrap (v2 client parity).
                if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                    let required: Vec<&str> = schema
                        .get("required")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    if props.len() == 1 && required.as_slice() == ["data"] {
                        if let Some(data) = props.get("data") {
                            let inner = self.ty_for(data, format!("{op_pascal}ResponseData"));
                            return RespOut::Unwrap(inner);
                        }
                    }
                    let rust = format!("{op_pascal}Response");
                    let header = format!("/// Response envelope for `{}`.\n", op.operation_id);
                    self.emit_struct_from(&schema, &rust, header, None);
                    return RespOut::Json(Ty::Named(rust));
                }
                RespOut::Json(self.ty_for(&schema, format!("{op_pascal}Response")))
            }
            crate::model::ContentType::None => RespOut::Unit,
        }
    }
}

fn variant_name_hint(member: &Value, vty: &Ty) -> String {
    if let Some(reference) = member.get("$ref").and_then(Value::as_str) {
        let tail = reference.rsplit('/').next().unwrap_or(reference);
        return pascal_ident(tail.rsplit('.').next().unwrap_or(tail));
    }
    match vty {
        Ty::Named(n) => n.clone(),
        Ty::Str => "String".to_owned(),
        Ty::I64 => "Integer".to_owned(),
        Ty::F64 => "Number".to_owned(),
        Ty::Bool => "Boolean".to_owned(),
        Ty::Value => "Any".to_owned(),
        Ty::VecT(_) => "Array".to_owned(),
        Ty::MapT(_) => "Map".to_owned(),
        Ty::Opt(_) => "Nullable".to_owned(),
    }
}

fn is_query_location(schema: &Value) -> bool {
    let Some(members) = schema.get("anyOf").and_then(Value::as_array) else {
        return false;
    };
    let non_null: Vec<&Value> = members.iter().filter(|m| !is_null_schema(m)).collect();
    if non_null.len() != 1 {
        return false;
    }
    let Some(props) = non_null[0].get("properties").and_then(Value::as_object) else {
        return false;
    };
    let Some(directory) = props.get("directory") else {
        return false;
    };
    props.len() == 1 && nullable_string(directory)
}

fn nullable_string(schema: &Value) -> bool {
    if schema.get("type").and_then(Value::as_str) == Some("string") {
        return true;
    }
    let Some(members) = schema.get("anyOf").and_then(Value::as_array) else {
        return false;
    };
    let non_null: Vec<&Value> = members.iter().filter(|m| !is_null_schema(m)).collect();
    non_null.len() == 1 && non_null[0].get("type").and_then(Value::as_str) == Some("string")
}

fn stringish(s: &Value) -> bool {
    if s.get("$ref").is_some() {
        return false;
    }
    if s.get("type").and_then(Value::as_str) == Some("string") {
        return true;
    }
    string_enum_values(s).is_some()
}

fn string_enum_values(schema: &Value) -> Option<Vec<String>> {
    let values = schema.get("enum")?.as_array()?;
    if values.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(values.len());
    for v in values {
        out.push(v.as_str()?.to_owned());
    }
    Some(out)
}

struct Merged {
    props: Map<String, Value>,
    required: BTreeSet<String>,
    extra_value_ty: Option<Value>,
}

fn merge_all_of(
    schemas: &Map<String, Value>,
    schema: &Value,
    at: &str,
    warnings: &mut Vec<String>,
) -> Option<Merged> {
    let members = schema.get("allOf")?.as_array()?;
    let mut props = Map::new();
    let mut required = BTreeSet::new();
    let mut extra: Option<Value> = None;
    let mut queue: Vec<Value> = members.clone();
    while let Some(member) = queue.pop() {
        let member = if let Some(reference) = member.get("$ref").and_then(Value::as_str) {
            let name = reference.rsplit('/').next().unwrap_or(reference);
            schemas.get(name)?.clone()
        } else {
            member.clone()
        };
        if member.get("properties").is_some() {
            if let Some(p) = member.get("properties").and_then(Value::as_object) {
                for (k, v) in p {
                    props.insert(k.clone(), v.clone());
                }
            }
            if let Some(req) = member.get("required").and_then(Value::as_array) {
                for r in req {
                    if let Some(r) = r.as_str() {
                        required.insert(r.to_owned());
                    }
                }
            }
        }
        if let Some(ap) = member.get("additionalProperties") {
            if ap.as_bool() != Some(false) && !ap.is_null() && !is_null_schema(ap) {
                extra = Some(ap.clone());
            }
        }
        if let Some(nested) = member.get("allOf").and_then(Value::as_array) {
            queue.extend(nested.iter().cloned());
        }
    }
    if props.is_empty() && extra.is_none() {
        warnings.push(format!("unmergeable allOf at `{at}`; emitted serde_json::Value (TODO)"));
        return None;
    }
    // The catch-all `additionalProperties` member may be `anyOf: [{}, {type: null}]`.
    let extra_value_ty = extra.map(|ap| {
        if let Some(members) = ap.get("anyOf").and_then(Value::as_array) {
            let non_null: Vec<&Value> = members.iter().filter(|m| !is_null_schema(m)).collect();
            if non_null.len() == 1 && non_null[0].as_object().is_some_and(Map::is_empty) {
                return Value::Bool(true);
            }
        }
        ap
    });
    Some(Merged { props, required, extra_value_ty })
}

struct Discriminator {
    members_by_value: BTreeMap<String, Value>,
}

fn detect_discriminator(schemas: &Map<String, Value>, members: &[Value]) -> Option<Discriminator> {
    let mut resolved: Vec<Value> = Vec::with_capacity(members.len());
    for m in members {
        let r = if let Some(reference) = m.get("$ref").and_then(Value::as_str) {
            let name = reference.rsplit('/').next().unwrap_or(reference);
            schemas.get(name)?.clone()
        } else {
            m.clone()
        };
        r.get("properties")?;
        resolved.push(r);
    }
    let first_props = resolved[0].get("properties")?.as_object()?.clone();
    'prop: for (name, prop_schema) in &first_props {
        let Some(first_value) = singleton_string_enum(prop_schema) else {
            continue;
        };
        let mut by_value = BTreeMap::new();
        by_value.insert(first_value, members[0].clone());
        for (member, schema) in members.iter().skip(1).zip(resolved.iter().skip(1)) {
            let value = singleton_string_enum(schema.get("properties")?.get(name)?)?;
            if by_value.contains_key(&value) {
                continue 'prop;
            }
            by_value.insert(value, member.clone());
        }
        return Some(Discriminator { members_by_value: by_value });
    }
    None
}
