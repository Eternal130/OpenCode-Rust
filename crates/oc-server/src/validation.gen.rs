// Generated file — DO NOT EDIT by hand.
// Regenerate with: cargo run -p openapi-gen
// Source: tools/openapi-gen/openapi.json (sha256 0612418392961ef22f0337ce0e1e520270917665fc85f29c4af1c94d1e1873ee)
//! Request-validation constraint table for the 141-op HttpApi surface
//! (HTTP-H0): one row per operation mapping its request body AND its
//! declared query parameters to JSON-shape constraints. DATA ONLY —
//! the interpreter/seam lives in `request_validation.rs`, which
//! enforces a rollout subset (the rest of the table is emitted for
//! review/coverage but stays pass-through).
//! Arena: every schema node is one `NODES` entry; object fields are
//! inline slices; children are indices. Field order is the spec's
//! DECLARATION order (Effect reports first-field-wins). Unsupported
//! constructs degrade to `Any` (accept) — counted in CAPPED_NODES,
//! never a silent false-reject.
//! Query rows: URL strings are never JSON null, so optional
//! `anyOf [X, null]` wrappers are UNWRAPPED to X (kind-mismatch
//! suffixes are interpreter-owned, probe-pinned to
//! `Expected … | undefined`); NumberFromString integer filters
//! (limit/context/lines) are decode-time in v2 and absent from the
//! spec — the seam carries those probe-pinned bounds separately.
#![allow(clippy::must_use_candidate, clippy::doc_markdown)]

pub const OPENAPI_SHA256: &str = "0612418392961ef22f0337ce0e1e520270917665fc85f29c4af1c94d1e1873ee";
pub const OPERATION_COUNT: usize = 141;
/// Operations declaring at least one query parameter.
pub const QUERY_PARAM_ROUTES: usize = 71;

/// `$ref` back-edges / nests beyond the generator cap (8) —
/// emitted as `Any` (accept), counted here so drift is reviewable.
pub const CAPPED_NODES: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SchemaNode {
    /// Accept anything (unsupported/unmapped construct — never reject).
    Any,
    /// JSON string.
    Str,
    /// JSON string that must start with `prefix` (the spec's `^literal`
    /// patterns — Effect's `startsWith` filter; message-pinned:
    /// `Expected a string starting with "ses"`).
    Pattern { prefix: &'static str },
    /// JSON number, integral; `minimum`/`exclusive_minimum` bounds.
    Int { minimum: Option<f64>, exclusive_minimum: Option<f64> },
    /// JSON number (any).
    Num,
    /// JSON boolean.
    Bool,
    /// Inline string-literal union (`Expected "a" | "b"`).
    Literal(&'static [&'static str]),
    /// Annotated schema ($ref): `label` is the Effect identifier printed
    /// for whole-field mismatches; `node` is the wrapped constraint.
    Ref { label: &'static str, node: u32 },
    /// `anyOf [X, null]` — null passes, else `X` (label gains `| null`).
    Nullable(u32),
    /// JSON array of the item node.
    Arr(u32),
    /// JSON object with declared fields (declaration order preserved).
    Obj { fields: &'static [FieldSpec] },
    /// JSON object with arbitrary string keys of the value node (also the
    /// plain `{"type":"object"}` shape: value node `Any`).
    Record(u32),
    /// Multi-member `anyOf` without a null member. Only emitted for the
    /// two probe-pinned shapes: string-unions (query filters — the seam
    /// accepts on any member and reports the FIRST member's message) and
    /// discriminator-struct unions (mcp.add `config` — the seam delegates
    /// to the member whose `type` literal matches, else renders the union
    /// label `{ readonly "type": "local", ... } | …`).
    Union(&'static [u32]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// Wire name (exact spelling).
    pub name: &'static str,
    /// In the spec's `required` list.
    pub required: bool,
    /// Index into `NODES`.
    pub ty: u32,
}

/// What a route's request body must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyValidation {
    /// No request body.
    None,
    /// `application/octet-stream` (bytes; not JSON-validated).
    Binary,
    /// `application/json` constrained by `NODES[index]`.
    Json(u32),
}

/// What a route's query string must satisfy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryValidation {
    /// No query parameters declared (unknown keys are stripped upstream).
    None,
    /// Declared parameters as an `Obj` node (`NODES[index]`), spec
    /// declaration order; URL values are strings — see the seam for the
    /// string-semantics interpretation.
    Params(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteValidation {
    pub operation_id: &'static str,
    pub body: BodyValidation,
    pub query: QueryValidation,
}

pub static NODES: &[SchemaNode] = &[
    SchemaNode::Str,         // 0
    SchemaNode::Nullable(0), // 1
    SchemaNode::Obj { fields: &[FieldSpec { name: "directory", required: false, ty: 1 }] }, // 2
    SchemaNode::Obj { fields: &[FieldSpec { name: "location", required: false, ty: 2 }] }, // 3
    SchemaNode::Any,         // 4
    SchemaNode::Ref { label: "Credential.Value", node: 4 }, // 5
    SchemaNode::Bool,        // 6
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 0 },
            FieldSpec { name: "integrationID", required: true, ty: 0 },
            FieldSpec { name: "label", required: false, ty: 0 },
            FieldSpec { name: "value", required: true, ty: 5 },
            FieldSpec { name: "activate", required: false, ty: 6 },
        ],
    }, // 7
    SchemaNode::Ref { label: "Credential.CreateInput", node: 7 }, // 8
    SchemaNode::Obj { fields: &[FieldSpec { name: "label", required: true, ty: 0 }] }, // 9
    SchemaNode::Obj { fields: &[FieldSpec { name: "shell", required: true, ty: 1 }] }, // 10
    SchemaNode::Ref { label: "Config.Patch", node: 10 }, // 11
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "location", required: false, ty: 2 },
            FieldSpec { name: "path", required: true, ty: 0 },
        ],
    }, // 12
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: true, ty: 0 },
            FieldSpec { name: "providerID", required: true, ty: 0 },
            FieldSpec { name: "variant", required: false, ty: 0 },
        ],
    }, // 13
    SchemaNode::Ref { label: "Model.Ref", node: 13 }, // 14
    SchemaNode::Nullable(14), // 15
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "prompt", required: true, ty: 0 },
            FieldSpec { name: "model", required: false, ty: 15 },
        ],
    }, // 16
    SchemaNode::Obj { fields: &[FieldSpec { name: "url", required: true, ty: 0 }] }, // 17
    SchemaNode::Literal(&["local"]), // 18
    SchemaNode::Arr(0),      // 19
    SchemaNode::Record(0),   // 20
    SchemaNode::Int { minimum: None, exclusive_minimum: Some(0.0) }, // 21
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "startup", required: false, ty: 21 },
            FieldSpec { name: "catalog", required: false, ty: 21 },
            FieldSpec { name: "execution", required: false, ty: 21 },
        ],
    }, // 22
    SchemaNode::Literal(&["legacy", "auto", "2026-07-28"]), // 23
    SchemaNode::Ref { label: "Mcp.Protocol", node: 23 }, // 24
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "type", required: true, ty: 18 },
            FieldSpec { name: "command", required: true, ty: 19 },
            FieldSpec { name: "cwd", required: false, ty: 0 },
            FieldSpec { name: "environment", required: false, ty: 20 },
            FieldSpec { name: "disabled", required: false, ty: 6 },
            FieldSpec { name: "codemode", required: false, ty: 6 },
            FieldSpec { name: "timeout", required: false, ty: 22 },
            FieldSpec { name: "protocol", required: false, ty: 24 },
        ],
    }, // 25
    SchemaNode::Ref { label: "Mcp.LocalConfigEncoded", node: 25 }, // 26
    SchemaNode::Literal(&["remote"]), // 27
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "type", required: true, ty: 27 },
            FieldSpec { name: "url", required: true, ty: 0 },
            FieldSpec { name: "headers", required: false, ty: 20 },
            FieldSpec { name: "oauth", required: false, ty: 4 },
            FieldSpec { name: "disabled", required: false, ty: 6 },
            FieldSpec { name: "codemode", required: false, ty: 6 },
            FieldSpec { name: "timeout", required: false, ty: 22 },
            FieldSpec { name: "protocol", required: false, ty: 24 },
        ],
    }, // 28
    SchemaNode::Ref { label: "Mcp.RemoteConfigEncoded", node: 28 }, // 29
    SchemaNode::Union(&[26, 29]), // 30
    SchemaNode::Obj { fields: &[FieldSpec { name: "config", required: true, ty: 30 }] }, // 31
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "cols", required: true, ty: 21 },
            FieldSpec { name: "rows", required: true, ty: 21 },
        ],
    }, // 32
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "attachmentID", required: false, ty: 0 },
            FieldSpec { name: "size", required: true, ty: 32 },
        ],
    }, // 33
    SchemaNode::Ref { label: "PersistentPty.UpdateInput", node: 33 }, // 34
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "cursor", required: false, ty: 0 },
            FieldSpec { name: "role", required: false, ty: 0 },
            FieldSpec { name: "attachment_id", required: false, ty: 0 },
            FieldSpec { name: "takeover", required: false, ty: 0 },
            FieldSpec { name: "input_protocol", required: false, ty: 0 },
            FieldSpec { name: "ticket", required: false, ty: 0 },
        ],
    }, // 35
    SchemaNode::Pattern { prefix: "ses" }, // 36
    SchemaNode::Ref { label: "Session.ForkBoundary", node: 4 }, // 37
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "sessionID", required: true, ty: 36 },
            FieldSpec { name: "boundary", required: true, ty: 37 },
        ],
    }, // 38
    SchemaNode::Num,         // 39
    SchemaNode::Ref { label: "Money.USD", node: 39 }, // 40
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "read", required: true, ty: 39 },
            FieldSpec { name: "write", required: true, ty: 39 },
        ],
    }, // 41
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "input", required: true, ty: 39 },
            FieldSpec { name: "output", required: true, ty: 39 },
            FieldSpec { name: "reasoning", required: true, ty: 39 },
            FieldSpec { name: "cache", required: true, ty: 41 },
        ],
    }, // 42
    SchemaNode::Ref { label: "TokenUsage.Info", node: 42 }, // 43
    SchemaNode::Literal(&["succeeded", "failed", "interrupted"]), // 44
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "created", required: true, ty: 39 },
            FieldSpec { name: "updated", required: true, ty: 39 },
            FieldSpec { name: "idle", required: false, ty: 39 },
            FieldSpec { name: "viewed", required: false, ty: 39 },
            FieldSpec { name: "archived", required: false, ty: 39 },
        ],
    }, // 45
    SchemaNode::Record(4),   // 46
    SchemaNode::Ref { label: "Session.Metadata", node: 46 }, // 47
    SchemaNode::Literal(&["allow", "deny", "ask"]), // 48
    SchemaNode::Ref { label: "Permission.Effect", node: 48 }, // 49
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "action", required: true, ty: 0 },
            FieldSpec { name: "resource", required: true, ty: 0 },
            FieldSpec { name: "effect", required: true, ty: 49 },
        ],
    }, // 50
    SchemaNode::Ref { label: "Permission.Rule", node: 50 }, // 51
    SchemaNode::Arr(51),     // 52
    SchemaNode::Ref { label: "Permission.Ruleset", node: 52 }, // 53
    SchemaNode::Pattern { prefix: "msg_" }, // 54
    SchemaNode::Int { minimum: Some(0.0), exclusive_minimum: None }, // 55
    SchemaNode::Literal(&["added", "deleted", "modified"]), // 56
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "file", required: true, ty: 0 },
            FieldSpec { name: "patch", required: true, ty: 0 },
            FieldSpec { name: "additions", required: true, ty: 55 },
            FieldSpec { name: "deletions", required: true, ty: 55 },
            FieldSpec { name: "status", required: true, ty: 56 },
        ],
    }, // 57
    SchemaNode::Ref { label: "FileDiff.Info", node: 57 }, // 58
    SchemaNode::Arr(58),     // 59
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "messageID", required: true, ty: 54 },
            FieldSpec { name: "partID", required: false, ty: 0 },
            FieldSpec { name: "snapshot", required: false, ty: 0 },
            FieldSpec { name: "files", required: false, ty: 59 },
        ],
    }, // 60
    SchemaNode::Ref { label: "Session.Revert", node: 60 }, // 61
    SchemaNode::Obj { fields: &[FieldSpec { name: "directory", required: true, ty: 0 }] }, // 62
    SchemaNode::Ref { label: "Location.PublicRef", node: 62 }, // 63
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: true, ty: 36 },
            FieldSpec { name: "parentID", required: false, ty: 36 },
            FieldSpec { name: "fork", required: false, ty: 38 },
            FieldSpec { name: "projectID", required: true, ty: 0 },
            FieldSpec { name: "agent", required: false, ty: 0 },
            FieldSpec { name: "model", required: false, ty: 14 },
            FieldSpec { name: "cost", required: true, ty: 40 },
            FieldSpec { name: "tokens", required: true, ty: 43 },
            FieldSpec { name: "outcome", required: false, ty: 44 },
            FieldSpec { name: "time", required: true, ty: 45 },
            FieldSpec { name: "title", required: false, ty: 0 },
            FieldSpec { name: "subpath", required: false, ty: 0 },
            FieldSpec { name: "metadata", required: false, ty: 47 },
            FieldSpec { name: "permissions", required: false, ty: 53 },
            FieldSpec { name: "revert", required: false, ty: 61 },
            FieldSpec { name: "location", required: true, ty: 63 },
        ],
    }, // 64
    SchemaNode::Ref { label: "Session.Info", node: 64 }, // 65
    SchemaNode::Ref { label: "Session.Message.Info", node: 4 }, // 66
    SchemaNode::Arr(66),     // 67
    SchemaNode::Nullable(63), // 68
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "info", required: true, ty: 65 },
            FieldSpec { name: "messages", required: true, ty: 67 },
            FieldSpec { name: "location", required: false, ty: 68 },
        ],
    }, // 69
    SchemaNode::Literal(&["none", "summary", "detail"]), // 70
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "from", required: false, ty: 0 },
            FieldSpec { name: "to", required: false, ty: 0 },
            FieldSpec { name: "project", required: false, ty: 0 },
            FieldSpec { name: "timezone", required: false, ty: 0 },
            FieldSpec { name: "tools", required: false, ty: 70 },
        ],
    }, // 71
    SchemaNode::Literal(&["true", "false"]), // 72
    SchemaNode::Obj { fields: &[FieldSpec { name: "sanitize", required: false, ty: 72 }] }, // 73
    SchemaNode::Obj { fields: &[FieldSpec { name: "value", required: true, ty: 4 }] }, // 74
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "after", required: false, ty: 0 },
            FieldSpec { name: "follow", required: false, ty: 72 },
        ],
    }, // 75
    SchemaNode::Nullable(6), // 76
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: true, ty: 0 },
            FieldSpec { name: "resume", required: false, ty: 76 },
        ],
    }, // 77
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "command", required: false, ty: 0 },
            FieldSpec { name: "args", required: true, ty: 19 },
            FieldSpec { name: "cwd", required: false, ty: 0 },
            FieldSpec { name: "title", required: true, ty: 0 },
            FieldSpec { name: "env", required: true, ty: 20 },
            FieldSpec { name: "size", required: false, ty: 32 },
        ],
    }, // 78
    SchemaNode::Ref { label: "PersistentPty.CreateInput", node: 78 }, // 79
    SchemaNode::Ref { label: "PersistentPty.ReadLinesEncoded", node: 0 }, // 80
    SchemaNode::Obj { fields: &[FieldSpec { name: "lines", required: false, ty: 80 }] }, // 81
    SchemaNode::Literal(&["file", "directory"]), // 82
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "location", required: false, ty: 2 },
            FieldSpec { name: "query", required: true, ty: 0 },
            FieldSpec { name: "type", required: false, ty: 82 },
            FieldSpec { name: "limit", required: false, ty: 0 },
        ],
    }, // 83
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "location", required: false, ty: 2 },
            FieldSpec { name: "path", required: false, ty: 0 },
        ],
    }, // 84
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "methodID", required: true, ty: 0 },
            FieldSpec { name: "label", required: false, ty: 1 },
        ],
    }, // 85
    SchemaNode::Ref { label: "Form.Value", node: 4 }, // 86
    SchemaNode::Record(86),  // 87
    SchemaNode::Ref { label: "Form.Answer", node: 87 }, // 88
    SchemaNode::Nullable(88), // 89
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "key", required: true, ty: 0 },
            FieldSpec { name: "answer", required: false, ty: 89 },
            FieldSpec { name: "label", required: false, ty: 1 },
        ],
    }, // 90
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "methodID", required: true, ty: 0 },
            FieldSpec { name: "answer", required: false, ty: 89 },
            FieldSpec { name: "label", required: false, ty: 1 },
        ],
    }, // 91
    SchemaNode::Obj { fields: &[FieldSpec { name: "code", required: false, ty: 1 }] }, // 92
    SchemaNode::Obj { fields: &[FieldSpec { name: "projectID", required: false, ty: 0 }] }, // 93
    SchemaNode::Obj { fields: &[FieldSpec { name: "target", required: false, ty: 1 }] }, // 94
    SchemaNode::Obj { fields: &[FieldSpec { name: "targets", required: true, ty: 19 }] }, // 95
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "url", required: false, ty: 0 },
            FieldSpec { name: "override", required: false, ty: 0 },
            FieldSpec { name: "color", required: false, ty: 0 },
        ],
    }, // 96
    SchemaNode::Ref { label: "Project.Icon", node: 96 }, // 97
    SchemaNode::Obj { fields: &[FieldSpec { name: "start", required: false, ty: 0 }] }, // 98
    SchemaNode::Ref { label: "Project.Commands", node: 98 }, // 99
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "canonical", required: false, ty: 0 },
            FieldSpec { name: "name", required: false, ty: 0 },
            FieldSpec { name: "icon", required: false, ty: 97 },
            FieldSpec { name: "commands", required: false, ty: 99 },
        ],
    }, // 100
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "command", required: false, ty: 0 },
            FieldSpec { name: "args", required: false, ty: 19 },
            FieldSpec { name: "cwd", required: false, ty: 0 },
            FieldSpec { name: "title", required: false, ty: 0 },
            FieldSpec { name: "env", required: false, ty: 20 },
        ],
    }, // 101
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "rows", required: true, ty: 21 },
            FieldSpec { name: "cols", required: true, ty: 21 },
        ],
    }, // 102
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "title", required: false, ty: 0 },
            FieldSpec { name: "size", required: false, ty: 102 },
        ],
    }, // 103
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "location[directory]", required: false, ty: 0 },
            FieldSpec { name: "cursor", required: false, ty: 0 },
            FieldSpec { name: "ticket", required: false, ty: 0 },
        ],
    }, // 104
    SchemaNode::Obj { fields: &[FieldSpec { name: "input", required: false, ty: 4 }] }, // 105
    SchemaNode::Ref { label: "Rpc.Input", node: 105 }, // 106
    SchemaNode::Literal(&["asc", "desc"]), // 107
    SchemaNode::Literal(&["null"]), // 108
    SchemaNode::Union(&[36, 108]), // 109
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "limit", required: false, ty: 0 },
            FieldSpec { name: "order", required: false, ty: 107 },
            FieldSpec { name: "search", required: false, ty: 0 },
            FieldSpec { name: "parentID", required: false, ty: 109 },
            FieldSpec { name: "directory", required: false, ty: 0 },
            FieldSpec { name: "project", required: false, ty: 0 },
            FieldSpec { name: "subpath", required: false, ty: 0 },
            FieldSpec { name: "cursor", required: false, ty: 0 },
        ],
    }, // 110
    SchemaNode::Nullable(36), // 111
    SchemaNode::Nullable(47), // 112
    SchemaNode::Nullable(53), // 113
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 111 },
            FieldSpec { name: "parentID", required: false, ty: 111 },
            FieldSpec { name: "title", required: false, ty: 1 },
            FieldSpec { name: "agent", required: false, ty: 1 },
            FieldSpec { name: "model", required: false, ty: 15 },
            FieldSpec { name: "location", required: false, ty: 68 },
            FieldSpec { name: "metadata", required: false, ty: 112 },
            FieldSpec { name: "permissions", required: false, ty: 113 },
        ],
    }, // 114
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "title", required: false, ty: 1 },
            FieldSpec { name: "metadata", required: false, ty: 112 },
            FieldSpec { name: "permissions", required: false, ty: 113 },
        ],
    }, // 115
    SchemaNode::Obj { fields: &[FieldSpec { name: "agent", required: true, ty: 0 }] }, // 116
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "start", required: true, ty: 39 },
            FieldSpec { name: "end", required: true, ty: 39 },
            FieldSpec { name: "text", required: true, ty: 0 },
        ],
    }, // 117
    SchemaNode::Ref { label: "Prompt.Mention", node: 117 }, // 118
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "uri", required: true, ty: 0 },
            FieldSpec { name: "name", required: false, ty: 0 },
            FieldSpec { name: "description", required: false, ty: 0 },
            FieldSpec { name: "mention", required: false, ty: 118 },
        ],
    }, // 119
    SchemaNode::Ref { label: "PromptInput.FileAttachment", node: 119 }, // 120
    SchemaNode::Arr(120),    // 121
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "name", required: true, ty: 0 },
            FieldSpec { name: "mention", required: false, ty: 118 },
        ],
    }, // 122
    SchemaNode::Ref { label: "Prompt.AgentAttachment", node: 122 }, // 123
    SchemaNode::Arr(123),    // 124
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: true, ty: 0 },
            FieldSpec { name: "mention", required: false, ty: 118 },
        ],
    }, // 125
    SchemaNode::Ref { label: "PromptInput.SkillAttachment", node: 125 }, // 126
    SchemaNode::Arr(126),    // 127
    SchemaNode::Literal(&["steer", "queue"]), // 128
    SchemaNode::Ref { label: "Session.Inbox.Delivery", node: 128 }, // 129
    SchemaNode::Nullable(129), // 130
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "name", required: true, ty: 0 },
            FieldSpec { name: "text", required: true, ty: 0 },
            FieldSpec { name: "files", required: false, ty: 121 },
            FieldSpec { name: "agents", required: false, ty: 124 },
            FieldSpec { name: "skills", required: false, ty: 127 },
            FieldSpec { name: "delivery", required: false, ty: 130 },
        ],
    }, // 131
    SchemaNode::Nullable(54), // 132
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 132 },
            FieldSpec { name: "delivery", required: false, ty: 130 },
        ],
    }, // 133
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "from", required: false, ty: 54 },
            FieldSpec { name: "to", required: false, ty: 54 },
            FieldSpec { name: "context", required: false, ty: 0 },
        ],
    }, // 134
    SchemaNode::Obj { fields: &[FieldSpec { name: "variables", required: true, ty: 20 }] }, // 135
    SchemaNode::Obj { fields: &[FieldSpec { name: "before", required: false, ty: 132 }] }, // 136
    SchemaNode::Pattern { prefix: "frm_" }, // 137
    SchemaNode::Nullable(137), // 138
    SchemaNode::Ref { label: "Form.Metadata", node: 46 }, // 139
    SchemaNode::Ref { label: "Form.Field", node: 4 }, // 140
    SchemaNode::Arr(140),    // 141
    SchemaNode::Ref { label: "Form.Fields", node: 141 }, // 142
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 138 },
            FieldSpec { name: "title", required: true, ty: 0 },
            FieldSpec { name: "metadata", required: false, ty: 139 },
            FieldSpec { name: "fields", required: true, ty: 142 },
        ],
    }, // 143
    SchemaNode::Ref { label: "Form.CreatePayload", node: 143 }, // 144
    SchemaNode::Obj { fields: &[FieldSpec { name: "message", required: false, ty: 0 }] }, // 145
    SchemaNode::Obj { fields: &[FieldSpec { name: "answer", required: true, ty: 88 }] }, // 146
    SchemaNode::Ref { label: "Form.Reply", node: 146 }, // 147
    SchemaNode::Obj { fields: &[FieldSpec { name: "prompt", required: true, ty: 0 }] }, // 148
    SchemaNode::Obj { fields: &[FieldSpec { name: "delivery", required: true, ty: 129 }] }, // 149
    SchemaNode::Obj { fields: &[FieldSpec { name: "resume", required: false, ty: 72 }] }, // 150
    SchemaNode::Literal(&[
        "agent-switched",
        "model-switched",
        "location-switched",
        "user",
        "synthetic",
        "system",
        "skill",
        "shell",
        "assistant",
        "compaction",
    ]), // 151
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "limit", required: false, ty: 0 },
            FieldSpec { name: "order", required: false, ty: 107 },
            FieldSpec { name: "cursor", required: false, ty: 0 },
            FieldSpec { name: "type", required: false, ty: 151 },
        ],
    }, // 152
    SchemaNode::Obj { fields: &[FieldSpec { name: "model", required: true, ty: 14 }] }, // 153
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "directory", required: true, ty: 0 },
            FieldSpec { name: "delivery", required: false, ty: 130 },
        ],
    }, // 154
    SchemaNode::Pattern { prefix: "per" }, // 155
    SchemaNode::Nullable(155), // 156
    SchemaNode::Literal(&["tool"]), // 157
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "type", required: true, ty: 157 },
            FieldSpec { name: "messageID", required: true, ty: 0 },
            FieldSpec { name: "id", required: true, ty: 0 },
        ],
    }, // 158
    SchemaNode::Ref { label: "Permission.Source", node: 158 }, // 159
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 156 },
            FieldSpec { name: "action", required: true, ty: 0 },
            FieldSpec { name: "resources", required: true, ty: 19 },
            FieldSpec { name: "save", required: false, ty: 19 },
            FieldSpec { name: "metadata", required: false, ty: 46 },
            FieldSpec { name: "source", required: false, ty: 159 },
            FieldSpec { name: "agent", required: false, ty: 1 },
        ],
    }, // 160
    SchemaNode::Literal(&["once", "always", "reject"]), // 161
    SchemaNode::Ref { label: "Permission.Reply", node: 161 }, // 162
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "decision", required: true, ty: 162 },
            FieldSpec { name: "message", required: false, ty: 1 },
        ],
    }, // 163
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 132 },
            FieldSpec { name: "text", required: true, ty: 0 },
            FieldSpec { name: "files", required: false, ty: 121 },
            FieldSpec { name: "agents", required: false, ty: 124 },
            FieldSpec { name: "skills", required: false, ty: 127 },
            FieldSpec { name: "metadata", required: false, ty: 46 },
            FieldSpec { name: "delivery", required: false, ty: 130 },
            FieldSpec { name: "resume", required: false, ty: 76 },
        ],
    }, // 164
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "messageID", required: true, ty: 54 },
            FieldSpec { name: "files", required: false, ty: 76 },
        ],
    }, // 165
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 132 },
            FieldSpec { name: "command", required: true, ty: 0 },
        ],
    }, // 166
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 132 },
            FieldSpec { name: "text", required: true, ty: 0 },
            FieldSpec { name: "description", required: false, ty: 1 },
            FieldSpec { name: "metadata", required: false, ty: 46 },
            FieldSpec { name: "delivery", required: false, ty: 130 },
            FieldSpec { name: "resume", required: false, ty: 76 },
        ],
    }, // 167
    SchemaNode::Obj { fields: &[FieldSpec { name: "idle", required: true, ty: 39 }] }, // 168
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "command", required: true, ty: 0 },
            FieldSpec { name: "cwd", required: false, ty: 0 },
            FieldSpec { name: "timeout", required: false, ty: 55 },
            FieldSpec { name: "metadata", required: false, ty: 46 },
        ],
    }, // 169
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "location", required: false, ty: 2 },
            FieldSpec { name: "cursor", required: false, ty: 0 },
            FieldSpec { name: "limit", required: false, ty: 0 },
        ],
    }, // 170
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "location", required: false, ty: 2 },
            FieldSpec { name: "search", required: false, ty: 0 },
            FieldSpec { name: "limit", required: false, ty: 0 },
        ],
    }, // 171
    SchemaNode::Literal(&["working", "branch", "committed"]), // 172
    SchemaNode::Ref { label: "Vcs.Mode", node: 172 }, // 173
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "location", required: false, ty: 2 },
            FieldSpec { name: "mode", required: true, ty: 173 },
            FieldSpec { name: "base", required: false, ty: 0 },
            FieldSpec { name: "context", required: false, ty: 0 },
        ],
    }, // 174
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "location", required: false, ty: 2 },
            FieldSpec { name: "provider", required: false, ty: 0 },
        ],
    }, // 175
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "query", required: true, ty: 0 },
            FieldSpec { name: "providerID", required: false, ty: 0 },
        ],
    }, // 176
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "projectID", required: true, ty: 0 },
            FieldSpec { name: "directory", required: true, ty: 0 },
            FieldSpec { name: "force", required: true, ty: 6 },
        ],
    }, // 177
    SchemaNode::Ref { label: "Worktree.RemoveInput", node: 177 }, // 178
    SchemaNode::Obj { fields: &[FieldSpec { name: "projectID", required: true, ty: 0 }] }, // 179
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "projectID", required: true, ty: 0 },
            FieldSpec { name: "from", required: false, ty: 0 },
            FieldSpec { name: "branch", required: false, ty: 0 },
            FieldSpec { name: "directory", required: false, ty: 0 },
            FieldSpec { name: "name", required: false, ty: 0 },
        ],
    }, // 180
    SchemaNode::Ref { label: "Worktree.CreateInput", node: 180 }, // 181
];

pub static ROUTES: &[RouteValidation] = &[
    RouteValidation {
        operation_id: "agent.get",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "agent.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "command.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "config.get",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "config.shells",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "credential.activate",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "credential.create",
        body: BodyValidation::Json(8),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "credential.list",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "credential.remove",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "credential.update",
        body: BodyValidation::Json(9),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "debug.location.evict",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "debug.location.list",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "event.subscribe",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.config.update",
        body: BodyValidation::Json(11),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.fs.write",
        body: BodyValidation::Binary,
        query: QueryValidation::Params(12),
    },
    RouteValidation {
        operation_id: "experimental.generate.text",
        body: BodyValidation::Json(16),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.integration.wellknown.add",
        body: BodyValidation::Json(17),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "experimental.mcp.add",
        body: BodyValidation::Json(31),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "experimental.mcp.connect",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "experimental.mcp.disconnect",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "experimental.mcp.remove",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "experimental.migration.v1.status",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.session.export",
        body: BodyValidation::None,
        query: QueryValidation::Params(73),
    },
    RouteValidation {
        operation_id: "experimental.session.import",
        body: BodyValidation::Json(69),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.session.instructions.entry.list",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.session.instructions.entry.put",
        body: BodyValidation::Json(74),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.session.instructions.entry.remove",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.session.skill",
        body: BodyValidation::Json(77),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.session.stats",
        body: BodyValidation::None,
        query: QueryValidation::Params(71),
    },
    RouteValidation {
        operation_id: "experimental.session.wait",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "form.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "fs.find",
        body: BodyValidation::None,
        query: QueryValidation::Params(83),
    },
    RouteValidation {
        operation_id: "fs.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(84),
    },
    RouteValidation {
        operation_id: "fs.read",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.command.cancel",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.command.connect",
        body: BodyValidation::Json(85),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.command.status",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.connect.key",
        body: BodyValidation::Json(90),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.get",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.oauth.cancel",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.oauth.complete",
        body: BodyValidation::Json(92),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.oauth.connect",
        body: BodyValidation::Json(91),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "integration.oauth.status",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "location.get",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "location.reload",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "mcp.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "mcp.resource.catalog",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "model.default",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "model.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "permission.request.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "permission.saved.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(93),
    },
    RouteValidation {
        operation_id: "permission.saved.remove",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "persistentPty.connect",
        body: BodyValidation::None,
        query: QueryValidation::Params(35),
    },
    RouteValidation {
        operation_id: "plugin.check",
        body: BodyValidation::Json(94),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "plugin.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "plugin.update",
        body: BodyValidation::Json(95),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "project.list",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "project.update",
        body: BodyValidation::Json(100),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "provider.get",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "provider.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "pty.connect",
        body: BodyValidation::None,
        query: QueryValidation::Params(104),
    },
    RouteValidation {
        operation_id: "pty.connect.token",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "pty.create",
        body: BodyValidation::Json(101),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "pty.get",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "pty.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "pty.remove",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "pty.update",
        body: BodyValidation::Json(103),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "reference.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "rpc.call",
        body: BodyValidation::Json(106),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "server.connect",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.connectToken",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.create",
        body: BodyValidation::Json(79),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.get",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.handoff",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.list",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.read",
        body: BodyValidation::None,
        query: QueryValidation::Params(81),
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.remove",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.shutdown",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.snapshot",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.update",
        body: BodyValidation::Json(34),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.info",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "server.pair",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.active",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.background",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.command",
        body: BodyValidation::Json(131),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.compact",
        body: BodyValidation::Json(133),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.context",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.create",
        body: BodyValidation::Json(114),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.diff",
        body: BodyValidation::None,
        query: QueryValidation::Params(134),
    },
    RouteValidation {
        operation_id: "session.environment",
        body: BodyValidation::Json(135),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.fork",
        body: BodyValidation::Json(136),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.form.cancel",
        body: BodyValidation::None,
        query: QueryValidation::Params(145),
    },
    RouteValidation {
        operation_id: "session.form.create",
        body: BodyValidation::Json(144),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.form.get",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.form.list",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.form.reply",
        body: BodyValidation::Json(147),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.generate",
        body: BodyValidation::Json(148),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.get",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.inbox.cancel",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.inbox.list",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.inbox.update",
        body: BodyValidation::Json(149),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.interrupt",
        body: BodyValidation::None,
        query: QueryValidation::Params(150),
    },
    RouteValidation {
        operation_id: "session.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(110),
    },
    RouteValidation {
        operation_id: "session.log",
        body: BodyValidation::None,
        query: QueryValidation::Params(75),
    },
    RouteValidation {
        operation_id: "session.message.get",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.message.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(152),
    },
    RouteValidation {
        operation_id: "session.move",
        body: BodyValidation::Json(154),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.permission.create",
        body: BodyValidation::Json(160),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.permission.get",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.permission.list",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.permission.reply",
        body: BodyValidation::Json(163),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.prompt",
        body: BodyValidation::Json(164),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.remove",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.revert.clear",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.revert.commit",
        body: BodyValidation::None,
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.revert.stage",
        body: BodyValidation::Json(165),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.shell",
        body: BodyValidation::Json(166),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.switchAgent",
        body: BodyValidation::Json(116),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.switchModel",
        body: BodyValidation::Json(153),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.synthetic",
        body: BodyValidation::Json(167),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.update",
        body: BodyValidation::Json(115),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "session.view",
        body: BodyValidation::Json(168),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "shell.create",
        body: BodyValidation::Json(169),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "shell.get",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "shell.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "shell.output",
        body: BodyValidation::None,
        query: QueryValidation::Params(170),
    },
    RouteValidation {
        operation_id: "shell.remove",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "skill.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "vcs.base",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "vcs.branch.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(171),
    },
    RouteValidation {
        operation_id: "vcs.diff",
        body: BodyValidation::None,
        query: QueryValidation::Params(174),
    },
    RouteValidation {
        operation_id: "vcs.get",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "vcs.init",
        body: BodyValidation::None,
        query: QueryValidation::Params(175),
    },
    RouteValidation {
        operation_id: "vcs.status",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "websearch.providers",
        body: BodyValidation::None,
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "websearch.query",
        body: BodyValidation::Json(176),
        query: QueryValidation::Params(3),
    },
    RouteValidation {
        operation_id: "worktree.create",
        body: BodyValidation::Json(181),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "worktree.list",
        body: BodyValidation::None,
        query: QueryValidation::Params(179),
    },
    RouteValidation {
        operation_id: "worktree.refresh",
        body: BodyValidation::Json(179),
        query: QueryValidation::None,
    },
    RouteValidation {
        operation_id: "worktree.remove",
        body: BodyValidation::Json(178),
        query: QueryValidation::None,
    },
];

/// Looks a row up by operation id (binary search; `ROUTES` is sorted).
pub fn find(operation_id: &str) -> Option<&'static RouteValidation> {
    ROUTES
        .binary_search_by(|row| row.operation_id.as_bytes().cmp(operation_id.as_bytes()))
        .ok()
        .map(|index| &ROUTES[index])
}

/// Resolves the JSON-body node index for an operation, if it declares one.
pub fn body_node(operation_id: &str) -> Option<u32> {
    match find(operation_id)?.body {
        BodyValidation::Json(node) => Some(node),
        _ => None,
    }
}

/// Resolves the query-params `Obj` node index for an operation, if it
/// declares any query parameters.
pub fn query_node(operation_id: &str) -> Option<u32> {
    match find(operation_id)?.query {
        QueryValidation::Params(node) => Some(node),
        QueryValidation::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every generated route must have exactly one validation row.
    #[test]
    fn covers_all_operations() {
        // No schema degraded to `Any` by the generator caps (oracle
        // condition 3) — any drift here must be reviewed with the
        // regeneration commit.
        assert_eq!(CAPPED_NODES, 0);
        assert_eq!(ROUTES.len(), OPERATION_COUNT);
        assert_eq!(ROUTES.len(), crate::routes_gen::ROUTES.len());
        let table: std::collections::BTreeSet<&str> =
            ROUTES.iter().map(|r| r.operation_id).collect();
        let routes: std::collections::BTreeSet<&str> =
            crate::routes_gen::ROUTES.iter().map(|r| r.operation_id).collect();
        assert_eq!(table, routes, "validation rows must mirror routes.gen.rs");
    }

    #[test]
    fn rows_sorted_for_binary_search() {
        assert!(ROUTES.windows(2).all(|w| w[0].operation_id < w[1].operation_id));
        assert!(find("shell.create").is_some());
        assert!(find("no.such.op").is_none());
    }

    /// All child indices (and field ty indices) must point into NODES.
    #[test]
    fn arena_indices_in_range() {
        fn visit(node: u32, seen: &mut std::collections::BTreeSet<u32>) {
            assert!((node as usize) < NODES.len(), "node index {node} out of range");
            if !seen.insert(node) {
                return; // shared subgraph
            }
            match &NODES[node as usize] {
                SchemaNode::Ref { node, .. }
                | SchemaNode::Nullable(node)
                | SchemaNode::Arr(node)
                | SchemaNode::Record(node) => visit(*node, seen),
                SchemaNode::Union(members) => {
                    for member in *members {
                        visit(*member, seen);
                    }
                }
                SchemaNode::Obj { fields } => {
                    for field in *fields {
                        assert!(
                            (field.ty as usize) < NODES.len(),
                            "field {} ty out of range",
                            field.name
                        );
                        visit(field.ty, seen);
                    }
                }
                _ => {}
            }
        }
        for row in ROUTES {
            if let BodyValidation::Json(node) = row.body {
                visit(node, &mut std::collections::BTreeSet::new());
            }
            if let QueryValidation::Params(node) = row.query {
                visit(node, &mut std::collections::BTreeSet::new());
            }
        }
    }

    /// Slice pins: constraint shapes this lane's seam enforces, frozen
    /// against accidental regeneration drift.
    #[test]
    fn slice_pins() {
        fn fields(node: u32) -> &'static [FieldSpec] {
            match &NODES[node as usize] {
                SchemaNode::Obj { fields } => fields,
                other => panic!("node {node} is not an Obj: {other:?}"),
            }
        }
        fn field_of(op: &str, field: &str) -> &'static FieldSpec {
            let root = body_node(op).unwrap_or_else(|| panic!("{op} has no JSON body"));
            fields(root)
                .iter()
                .find(|f| f.name == field)
                .unwrap_or_else(|| panic!("{op} has no field {field}"))
        }

        // shell.create: command required string; timeout Int minimum 0.
        assert_eq!(NODES[field_of("shell.create", "command").ty as usize], SchemaNode::Str);
        assert!(field_of("shell.create", "command").required);
        assert_eq!(
            NODES[field_of("shell.create", "timeout").ty as usize],
            SchemaNode::Int { minimum: Some(0.0), exclusive_minimum: None }
        );

        // plugin.update: targets required array-of-string.
        assert_eq!(
            NODES[field_of("plugin.update", "targets").ty as usize],
            SchemaNode::Arr(
                u32::try_from(NODES.iter().position(|n| matches!(n, SchemaNode::Str)).unwrap())
                    .unwrap()
            )
        );
        assert!(field_of("plugin.update", "targets").required);

        // session.move: directory required; delivery Nullable(Ref labeled).
        assert!(field_of("session.move", "directory").required);
        let delivery = field_of("session.move", "delivery").ty;
        match &NODES[delivery as usize] {
            SchemaNode::Nullable(inner) => match &NODES[*inner as usize] {
                SchemaNode::Ref { label, .. } => assert_eq!(*label, "Session.Inbox.Delivery"),
                other => panic!("delivery inner not a Ref: {other:?}"),
            },
            other => panic!("delivery not Nullable: {other:?}"),
        }

        // pty.update: inline size struct, both members required integers.
        let size = field_of("pty.update", "size").ty;
        let size_fields = fields(size);
        assert_eq!(size_fields.len(), 2);
        assert!(size_fields.iter().all(|f| f.required));

        // session.create: title nullable string (Effect `| null` label);
        // id/parentID nullable `^ses` startsWith patterns.
        match &NODES[field_of("session.create", "title").ty as usize] {
            SchemaNode::Nullable(inner) => assert_eq!(NODES[*inner as usize], SchemaNode::Str),
            other => panic!("title not nullable: {other:?}"),
        }
        for pattern_field in ["id", "parentID"] {
            match &NODES[field_of("session.create", pattern_field).ty as usize] {
                SchemaNode::Nullable(inner) => assert_eq!(
                    NODES[*inner as usize],
                    SchemaNode::Pattern { prefix: "ses" },
                    "session.create {pattern_field} must be a nullable ^ses pattern"
                ),
                other => panic!("{pattern_field} not nullable: {other:?}"),
            }
        }

        // experimental.mcp.add: required config as a discriminator-struct
        // union (probe-pinned members local/remote).
        match &NODES[field_of("experimental.mcp.add", "config").ty as usize] {
            SchemaNode::Union(members) => {
                assert_eq!(members.len(), 2, "config union members: {members:?}");
                for member in *members {
                    match &NODES[*member as usize] {
                        SchemaNode::Ref { node, .. } => {
                            let type_field = fields(*node)
                                .iter()
                                .find(|f| f.name == "type")
                                .unwrap_or_else(|| panic!("union member has no type field"));
                            match &NODES[type_field.ty as usize] {
                                SchemaNode::Literal(values) => assert_eq!(values.len(), 1),
                                other => panic!("type field not a literal: {other:?}"),
                            }
                        }
                        other => panic!("config union member not a Ref: {other:?}"),
                    }
                }
            }
            other => panic!("config not a Union: {other:?}"),
        }

        // rpc.call: root is the annotated Rpc.Input (probe: whole-root
        // mismatch prints the identifier, not `Expected object`).
        match &NODES[body_node("rpc.call").unwrap() as usize] {
            SchemaNode::Ref { label, .. } => assert_eq!(*label, "Rpc.Input"),
            other => panic!("rpc.call root not a Ref: {other:?}"),
        }

        // Routes without JSON bodies stay pass-through.
        assert_eq!(find("session.list").map(|r| r.body), Some(BodyValidation::None));
        assert_eq!(find("fs.read").map(|r| r.body), Some(BodyValidation::None));
    }

    /// Query-table pins: declaration-order fields and the constraint
    /// classes the seam interprets (probe evidence rollout-probe*.json).
    #[test]
    fn query_slice_pins() {
        fn params(op: &str) -> &'static [FieldSpec] {
            let node = query_node(op).unwrap_or_else(|| panic!("{op} has no query row"));
            match &NODES[node as usize] {
                SchemaNode::Obj { fields } => fields,
                other => panic!("{op} query node not an Obj: {other:?}"),
            }
        }
        fn param_of(op: &str, name: &str) -> &'static FieldSpec {
            params(op)
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{op} has no query param {name}"))
        }

        // Count: 71 ops declare query parameters (QUERY_PARAM_ROUTES).
        assert_eq!(
            ROUTES.iter().filter(|r| matches!(r.query, QueryValidation::Params(_))).count(),
            QUERY_PARAM_ROUTES
        );

        // session.list: limit/order/search/parentID… in declaration order;
        // limit is a plain string in the spec (the NumberFromString
        // integer filter is seam-side, probe-pinned); order a literal
        // union; parentID the string-union [^ses, "null"].
        let names: Vec<&str> = params("session.list").iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            ["limit", "order", "search", "parentID", "directory", "project", "subpath", "cursor"]
        );
        assert_eq!(NODES[param_of("session.list", "limit").ty as usize], SchemaNode::Str);
        match &NODES[param_of("session.list", "order").ty as usize] {
            SchemaNode::Literal(values) => {
                assert_eq!(values, &["asc", "desc"]);
            }
            other => panic!("order not a Literal: {other:?}"),
        }
        match &NODES[param_of("session.list", "parentID").ty as usize] {
            SchemaNode::Union(members) => {
                assert_eq!(members.len(), 2);
                match &NODES[members[0] as usize] {
                    SchemaNode::Pattern { prefix } => assert_eq!(*prefix, "ses"),
                    other => panic!("parentID first member not ^ses: {other:?}"),
                }
                match &NODES[members[1] as usize] {
                    SchemaNode::Literal(values) => assert_eq!(values, &["null"]),
                    other => panic!("parentID second member not \"null\": {other:?}"),
                }
            }
            other => panic!("parentID not a Union: {other:?}"),
        }

        // vcs.diff: REQUIRED mode as a $ref'd literal union (probe: whole
        // mismatch prints the identifier `Expected Vcs.Mode`).
        let mode = param_of("vcs.diff", "mode");
        assert!(mode.required);
        match &NODES[mode.ty as usize] {
            SchemaNode::Ref { label, node } => {
                assert_eq!(*label, "Vcs.Mode");
                match &NODES[*node as usize] {
                    SchemaNode::Literal(values) => {
                        assert_eq!(values, &["working", "branch", "committed"]);
                    }
                    other => panic!("Vcs.Mode not a Literal: {other:?}"),
                }
            }
            other => panic!("mode not a Ref: {other:?}"),
        }

        // fs.find: required `query` string; `type` file/directory union.
        assert!(param_of("fs.find", "query").required);
        match &NODES[param_of("fs.find", "type").ty as usize] {
            SchemaNode::Literal(values) => assert_eq!(values, &["file", "directory"]),
            other => panic!("fs.find type not a Literal: {other:?}"),
        }

        // session.diff: `from`/`to` are `^msg_` patterns.
        for name in ["from", "to"] {
            match &NODES[param_of("session.diff", name).ty as usize] {
                SchemaNode::Pattern { prefix } => assert_eq!(*prefix, "msg_"),
                other => panic!("session.diff {name} not a ^msg_ pattern: {other:?}"),
            }
        }

        // worktree.list / experimental.fs.write: required plain strings.
        assert!(param_of("worktree.list", "projectID").required);
        assert!(param_of("experimental.fs.write", "path").required);
        assert!(query_node("session.list").is_some());
        assert_eq!(query_node("session.create"), None, "session.create declares no query");
        // pty.connect's raw flat params (location[directory]/cursor/ticket)
        // are plain strings — nothing can fail.
        for name in ["location[directory]", "cursor", "ticket"] {
            assert_eq!(
                NODES[param_of("pty.connect", name).ty as usize],
                SchemaNode::Str,
                "pty.connect {name}"
            );
        }
    }
}
