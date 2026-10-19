// Generated file — DO NOT EDIT by hand.
// Regenerate with: cargo run -p openapi-gen
// Source: tools/openapi-gen/openapi.json (sha256 0612418392961ef22f0337ce0e1e520270917665fc85f29c4af1c94d1e1873ee)
//! Request-validation constraint table for the 141-op HttpApi surface
//! (HTTP-H0): one row per operation mapping its request body to a
//! JSON-shape constraint. DATA ONLY — the interpreter/seam lives in
//! `request_validation.rs`, which enforces a rollout subset (the rest
//! of the table is emitted for review/coverage but stays pass-through).
//! Arena: every schema node is one `NODES` entry; object fields are
//! inline slices; children are indices. Field order is the spec's
//! DECLARATION order (Effect reports first-field-wins). Unsupported
//! constructs degrade to `Any` (accept) — counted in CAPPED_NODES,
//! never a silent false-reject.
#![allow(clippy::must_use_candidate, clippy::doc_markdown)]

pub const OPENAPI_SHA256: &str = "0612418392961ef22f0337ce0e1e520270917665fc85f29c4af1c94d1e1873ee";
pub const OPERATION_COUNT: usize = 141;
/// `$ref` back-edges / nests beyond the generator cap (8) —
/// emitted as `Any` (accept), counted here so drift is reviewable.
pub const CAPPED_NODES: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SchemaNode {
    /// Accept anything (unsupported/unmapped construct — never reject).
    Any,
    /// JSON string.
    Str,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteValidation {
    pub operation_id: &'static str,
    pub body: BodyValidation,
}

pub static NODES: &[SchemaNode] = &[
    SchemaNode::Str,                                        // 0
    SchemaNode::Any,                                        // 1
    SchemaNode::Ref { label: "Credential.Value", node: 1 }, // 2
    SchemaNode::Bool,                                       // 3
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 0 },
            FieldSpec { name: "integrationID", required: true, ty: 0 },
            FieldSpec { name: "label", required: false, ty: 0 },
            FieldSpec { name: "value", required: true, ty: 2 },
            FieldSpec { name: "activate", required: false, ty: 3 },
        ],
    }, // 4
    SchemaNode::Ref { label: "Credential.CreateInput", node: 4 }, // 5
    SchemaNode::Obj { fields: &[FieldSpec { name: "label", required: true, ty: 0 }] }, // 6
    SchemaNode::Nullable(0),                                // 7
    SchemaNode::Obj { fields: &[FieldSpec { name: "shell", required: true, ty: 7 }] }, // 8
    SchemaNode::Ref { label: "Config.Patch", node: 8 },     // 9
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: true, ty: 0 },
            FieldSpec { name: "providerID", required: true, ty: 0 },
            FieldSpec { name: "variant", required: false, ty: 0 },
        ],
    }, // 10
    SchemaNode::Ref { label: "Model.Ref", node: 10 },       // 11
    SchemaNode::Nullable(11),                               // 12
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "prompt", required: true, ty: 0 },
            FieldSpec { name: "model", required: false, ty: 12 },
        ],
    }, // 13
    SchemaNode::Obj { fields: &[FieldSpec { name: "url", required: true, ty: 0 }] }, // 14
    SchemaNode::Obj { fields: &[FieldSpec { name: "config", required: true, ty: 1 }] }, // 15
    SchemaNode::Int { minimum: None, exclusive_minimum: Some(0.0) }, // 16
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "cols", required: true, ty: 16 },
            FieldSpec { name: "rows", required: true, ty: 16 },
        ],
    }, // 17
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "attachmentID", required: false, ty: 0 },
            FieldSpec { name: "size", required: true, ty: 17 },
        ],
    }, // 18
    SchemaNode::Ref { label: "PersistentPty.UpdateInput", node: 18 }, // 19
    SchemaNode::Ref { label: "Session.ForkBoundary", node: 1 }, // 20
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "sessionID", required: true, ty: 0 },
            FieldSpec { name: "boundary", required: true, ty: 20 },
        ],
    }, // 21
    SchemaNode::Num,                                        // 22
    SchemaNode::Ref { label: "Money.USD", node: 22 },       // 23
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "read", required: true, ty: 22 },
            FieldSpec { name: "write", required: true, ty: 22 },
        ],
    }, // 24
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "input", required: true, ty: 22 },
            FieldSpec { name: "output", required: true, ty: 22 },
            FieldSpec { name: "reasoning", required: true, ty: 22 },
            FieldSpec { name: "cache", required: true, ty: 24 },
        ],
    }, // 25
    SchemaNode::Ref { label: "TokenUsage.Info", node: 25 }, // 26
    SchemaNode::Literal(&["succeeded", "failed", "interrupted"]), // 27
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "created", required: true, ty: 22 },
            FieldSpec { name: "updated", required: true, ty: 22 },
            FieldSpec { name: "idle", required: false, ty: 22 },
            FieldSpec { name: "viewed", required: false, ty: 22 },
            FieldSpec { name: "archived", required: false, ty: 22 },
        ],
    }, // 28
    SchemaNode::Record(1),                                  // 29
    SchemaNode::Ref { label: "Session.Metadata", node: 29 }, // 30
    SchemaNode::Literal(&["allow", "deny", "ask"]),         // 31
    SchemaNode::Ref { label: "Permission.Effect", node: 31 }, // 32
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "action", required: true, ty: 0 },
            FieldSpec { name: "resource", required: true, ty: 0 },
            FieldSpec { name: "effect", required: true, ty: 32 },
        ],
    }, // 33
    SchemaNode::Ref { label: "Permission.Rule", node: 33 }, // 34
    SchemaNode::Arr(34),                                    // 35
    SchemaNode::Ref { label: "Permission.Ruleset", node: 35 }, // 36
    SchemaNode::Int { minimum: Some(0.0), exclusive_minimum: None }, // 37
    SchemaNode::Literal(&["added", "deleted", "modified"]), // 38
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "file", required: true, ty: 0 },
            FieldSpec { name: "patch", required: true, ty: 0 },
            FieldSpec { name: "additions", required: true, ty: 37 },
            FieldSpec { name: "deletions", required: true, ty: 37 },
            FieldSpec { name: "status", required: true, ty: 38 },
        ],
    }, // 39
    SchemaNode::Ref { label: "FileDiff.Info", node: 39 },   // 40
    SchemaNode::Arr(40),                                    // 41
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "messageID", required: true, ty: 0 },
            FieldSpec { name: "partID", required: false, ty: 0 },
            FieldSpec { name: "snapshot", required: false, ty: 0 },
            FieldSpec { name: "files", required: false, ty: 41 },
        ],
    }, // 42
    SchemaNode::Ref { label: "Session.Revert", node: 42 },  // 43
    SchemaNode::Obj { fields: &[FieldSpec { name: "directory", required: true, ty: 0 }] }, // 44
    SchemaNode::Ref { label: "Location.PublicRef", node: 44 }, // 45
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: true, ty: 0 },
            FieldSpec { name: "parentID", required: false, ty: 0 },
            FieldSpec { name: "fork", required: false, ty: 21 },
            FieldSpec { name: "projectID", required: true, ty: 0 },
            FieldSpec { name: "agent", required: false, ty: 0 },
            FieldSpec { name: "model", required: false, ty: 11 },
            FieldSpec { name: "cost", required: true, ty: 23 },
            FieldSpec { name: "tokens", required: true, ty: 26 },
            FieldSpec { name: "outcome", required: false, ty: 27 },
            FieldSpec { name: "time", required: true, ty: 28 },
            FieldSpec { name: "title", required: false, ty: 0 },
            FieldSpec { name: "subpath", required: false, ty: 0 },
            FieldSpec { name: "metadata", required: false, ty: 30 },
            FieldSpec { name: "permissions", required: false, ty: 36 },
            FieldSpec { name: "revert", required: false, ty: 43 },
            FieldSpec { name: "location", required: true, ty: 45 },
        ],
    }, // 46
    SchemaNode::Ref { label: "Session.Info", node: 46 },    // 47
    SchemaNode::Ref { label: "Session.Message.Info", node: 1 }, // 48
    SchemaNode::Arr(48),                                    // 49
    SchemaNode::Nullable(45),                               // 50
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "info", required: true, ty: 47 },
            FieldSpec { name: "messages", required: true, ty: 49 },
            FieldSpec { name: "location", required: false, ty: 50 },
        ],
    }, // 51
    SchemaNode::Obj { fields: &[FieldSpec { name: "value", required: true, ty: 1 }] }, // 52
    SchemaNode::Nullable(3),                                // 53
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: true, ty: 0 },
            FieldSpec { name: "resume", required: false, ty: 53 },
        ],
    }, // 54
    SchemaNode::Arr(0),                                     // 55
    SchemaNode::Record(0),                                  // 56
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "command", required: false, ty: 0 },
            FieldSpec { name: "args", required: true, ty: 55 },
            FieldSpec { name: "cwd", required: false, ty: 0 },
            FieldSpec { name: "title", required: true, ty: 0 },
            FieldSpec { name: "env", required: true, ty: 56 },
            FieldSpec { name: "size", required: false, ty: 17 },
        ],
    }, // 57
    SchemaNode::Ref { label: "PersistentPty.CreateInput", node: 57 }, // 58
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "methodID", required: true, ty: 0 },
            FieldSpec { name: "label", required: false, ty: 7 },
        ],
    }, // 59
    SchemaNode::Ref { label: "Form.Value", node: 1 },       // 60
    SchemaNode::Record(60),                                 // 61
    SchemaNode::Ref { label: "Form.Answer", node: 61 },     // 62
    SchemaNode::Nullable(62),                               // 63
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "key", required: true, ty: 0 },
            FieldSpec { name: "answer", required: false, ty: 63 },
            FieldSpec { name: "label", required: false, ty: 7 },
        ],
    }, // 64
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "methodID", required: true, ty: 0 },
            FieldSpec { name: "answer", required: false, ty: 63 },
            FieldSpec { name: "label", required: false, ty: 7 },
        ],
    }, // 65
    SchemaNode::Obj { fields: &[FieldSpec { name: "code", required: false, ty: 7 }] }, // 66
    SchemaNode::Obj { fields: &[FieldSpec { name: "target", required: false, ty: 7 }] }, // 67
    SchemaNode::Obj { fields: &[FieldSpec { name: "targets", required: true, ty: 55 }] }, // 68
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "url", required: false, ty: 0 },
            FieldSpec { name: "override", required: false, ty: 0 },
            FieldSpec { name: "color", required: false, ty: 0 },
        ],
    }, // 69
    SchemaNode::Ref { label: "Project.Icon", node: 69 },    // 70
    SchemaNode::Obj { fields: &[FieldSpec { name: "start", required: false, ty: 0 }] }, // 71
    SchemaNode::Ref { label: "Project.Commands", node: 71 }, // 72
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "canonical", required: false, ty: 0 },
            FieldSpec { name: "name", required: false, ty: 0 },
            FieldSpec { name: "icon", required: false, ty: 70 },
            FieldSpec { name: "commands", required: false, ty: 72 },
        ],
    }, // 73
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "command", required: false, ty: 0 },
            FieldSpec { name: "args", required: false, ty: 55 },
            FieldSpec { name: "cwd", required: false, ty: 0 },
            FieldSpec { name: "title", required: false, ty: 0 },
            FieldSpec { name: "env", required: false, ty: 56 },
        ],
    }, // 74
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "rows", required: true, ty: 16 },
            FieldSpec { name: "cols", required: true, ty: 16 },
        ],
    }, // 75
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "title", required: false, ty: 0 },
            FieldSpec { name: "size", required: false, ty: 75 },
        ],
    }, // 76
    SchemaNode::Obj { fields: &[FieldSpec { name: "input", required: false, ty: 1 }] }, // 77
    SchemaNode::Ref { label: "Rpc.Input", node: 77 },       // 78
    SchemaNode::Nullable(30),                               // 79
    SchemaNode::Nullable(36),                               // 80
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 7 },
            FieldSpec { name: "parentID", required: false, ty: 7 },
            FieldSpec { name: "title", required: false, ty: 7 },
            FieldSpec { name: "agent", required: false, ty: 7 },
            FieldSpec { name: "model", required: false, ty: 12 },
            FieldSpec { name: "location", required: false, ty: 50 },
            FieldSpec { name: "metadata", required: false, ty: 79 },
            FieldSpec { name: "permissions", required: false, ty: 80 },
        ],
    }, // 81
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "title", required: false, ty: 7 },
            FieldSpec { name: "metadata", required: false, ty: 79 },
            FieldSpec { name: "permissions", required: false, ty: 80 },
        ],
    }, // 82
    SchemaNode::Obj { fields: &[FieldSpec { name: "agent", required: true, ty: 0 }] }, // 83
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "start", required: true, ty: 22 },
            FieldSpec { name: "end", required: true, ty: 22 },
            FieldSpec { name: "text", required: true, ty: 0 },
        ],
    }, // 84
    SchemaNode::Ref { label: "Prompt.Mention", node: 84 },  // 85
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "uri", required: true, ty: 0 },
            FieldSpec { name: "name", required: false, ty: 0 },
            FieldSpec { name: "description", required: false, ty: 0 },
            FieldSpec { name: "mention", required: false, ty: 85 },
        ],
    }, // 86
    SchemaNode::Ref { label: "PromptInput.FileAttachment", node: 86 }, // 87
    SchemaNode::Arr(87),                                    // 88
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "name", required: true, ty: 0 },
            FieldSpec { name: "mention", required: false, ty: 85 },
        ],
    }, // 89
    SchemaNode::Ref { label: "Prompt.AgentAttachment", node: 89 }, // 90
    SchemaNode::Arr(90),                                    // 91
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: true, ty: 0 },
            FieldSpec { name: "mention", required: false, ty: 85 },
        ],
    }, // 92
    SchemaNode::Ref { label: "PromptInput.SkillAttachment", node: 92 }, // 93
    SchemaNode::Arr(93),                                    // 94
    SchemaNode::Literal(&["steer", "queue"]),               // 95
    SchemaNode::Ref { label: "Session.Inbox.Delivery", node: 95 }, // 96
    SchemaNode::Nullable(96),                               // 97
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "name", required: true, ty: 0 },
            FieldSpec { name: "text", required: true, ty: 0 },
            FieldSpec { name: "files", required: false, ty: 88 },
            FieldSpec { name: "agents", required: false, ty: 91 },
            FieldSpec { name: "skills", required: false, ty: 94 },
            FieldSpec { name: "delivery", required: false, ty: 97 },
        ],
    }, // 98
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 7 },
            FieldSpec { name: "delivery", required: false, ty: 97 },
        ],
    }, // 99
    SchemaNode::Obj { fields: &[FieldSpec { name: "variables", required: true, ty: 56 }] }, // 100
    SchemaNode::Obj { fields: &[FieldSpec { name: "before", required: false, ty: 7 }] }, // 101
    SchemaNode::Ref { label: "Form.Metadata", node: 29 },   // 102
    SchemaNode::Ref { label: "Form.Field", node: 1 },       // 103
    SchemaNode::Arr(103),                                   // 104
    SchemaNode::Ref { label: "Form.Fields", node: 104 },    // 105
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 7 },
            FieldSpec { name: "title", required: true, ty: 0 },
            FieldSpec { name: "metadata", required: false, ty: 102 },
            FieldSpec { name: "fields", required: true, ty: 105 },
        ],
    }, // 106
    SchemaNode::Ref { label: "Form.CreatePayload", node: 106 }, // 107
    SchemaNode::Obj { fields: &[FieldSpec { name: "answer", required: true, ty: 62 }] }, // 108
    SchemaNode::Ref { label: "Form.Reply", node: 108 },     // 109
    SchemaNode::Obj { fields: &[FieldSpec { name: "prompt", required: true, ty: 0 }] }, // 110
    SchemaNode::Obj { fields: &[FieldSpec { name: "delivery", required: true, ty: 96 }] }, // 111
    SchemaNode::Obj { fields: &[FieldSpec { name: "model", required: true, ty: 11 }] }, // 112
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "directory", required: true, ty: 0 },
            FieldSpec { name: "delivery", required: false, ty: 97 },
        ],
    }, // 113
    SchemaNode::Ref { label: "Permission.Source", node: 1 }, // 114
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 7 },
            FieldSpec { name: "action", required: true, ty: 0 },
            FieldSpec { name: "resources", required: true, ty: 55 },
            FieldSpec { name: "save", required: false, ty: 55 },
            FieldSpec { name: "metadata", required: false, ty: 29 },
            FieldSpec { name: "source", required: false, ty: 114 },
            FieldSpec { name: "agent", required: false, ty: 7 },
        ],
    }, // 115
    SchemaNode::Literal(&["once", "always", "reject"]),     // 116
    SchemaNode::Ref { label: "Permission.Reply", node: 116 }, // 117
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "decision", required: true, ty: 117 },
            FieldSpec { name: "message", required: false, ty: 7 },
        ],
    }, // 118
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 7 },
            FieldSpec { name: "text", required: true, ty: 0 },
            FieldSpec { name: "files", required: false, ty: 88 },
            FieldSpec { name: "agents", required: false, ty: 91 },
            FieldSpec { name: "skills", required: false, ty: 94 },
            FieldSpec { name: "metadata", required: false, ty: 29 },
            FieldSpec { name: "delivery", required: false, ty: 97 },
            FieldSpec { name: "resume", required: false, ty: 53 },
        ],
    }, // 119
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "messageID", required: true, ty: 0 },
            FieldSpec { name: "files", required: false, ty: 53 },
        ],
    }, // 120
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 7 },
            FieldSpec { name: "command", required: true, ty: 0 },
        ],
    }, // 121
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "id", required: false, ty: 7 },
            FieldSpec { name: "text", required: true, ty: 0 },
            FieldSpec { name: "description", required: false, ty: 7 },
            FieldSpec { name: "metadata", required: false, ty: 29 },
            FieldSpec { name: "delivery", required: false, ty: 97 },
            FieldSpec { name: "resume", required: false, ty: 53 },
        ],
    }, // 122
    SchemaNode::Obj { fields: &[FieldSpec { name: "idle", required: true, ty: 22 }] }, // 123
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "command", required: true, ty: 0 },
            FieldSpec { name: "cwd", required: false, ty: 0 },
            FieldSpec { name: "timeout", required: false, ty: 37 },
            FieldSpec { name: "metadata", required: false, ty: 29 },
        ],
    }, // 124
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "query", required: true, ty: 0 },
            FieldSpec { name: "providerID", required: false, ty: 0 },
        ],
    }, // 125
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "projectID", required: true, ty: 0 },
            FieldSpec { name: "directory", required: true, ty: 0 },
            FieldSpec { name: "force", required: true, ty: 3 },
        ],
    }, // 126
    SchemaNode::Ref { label: "Worktree.RemoveInput", node: 126 }, // 127
    SchemaNode::Obj {
        fields: &[
            FieldSpec { name: "projectID", required: true, ty: 0 },
            FieldSpec { name: "from", required: false, ty: 0 },
            FieldSpec { name: "branch", required: false, ty: 0 },
            FieldSpec { name: "directory", required: false, ty: 0 },
            FieldSpec { name: "name", required: false, ty: 0 },
        ],
    }, // 128
    SchemaNode::Ref { label: "Worktree.CreateInput", node: 128 }, // 129
    SchemaNode::Obj { fields: &[FieldSpec { name: "projectID", required: true, ty: 0 }] }, // 130
];

pub static ROUTES: &[RouteValidation] = &[
    RouteValidation { operation_id: "agent.get", body: BodyValidation::None },
    RouteValidation { operation_id: "agent.list", body: BodyValidation::None },
    RouteValidation { operation_id: "command.list", body: BodyValidation::None },
    RouteValidation { operation_id: "config.get", body: BodyValidation::None },
    RouteValidation { operation_id: "config.shells", body: BodyValidation::None },
    RouteValidation { operation_id: "credential.activate", body: BodyValidation::None },
    RouteValidation { operation_id: "credential.create", body: BodyValidation::Json(5) },
    RouteValidation { operation_id: "credential.list", body: BodyValidation::None },
    RouteValidation { operation_id: "credential.remove", body: BodyValidation::None },
    RouteValidation { operation_id: "credential.update", body: BodyValidation::Json(6) },
    RouteValidation { operation_id: "debug.location.evict", body: BodyValidation::None },
    RouteValidation { operation_id: "debug.location.list", body: BodyValidation::None },
    RouteValidation { operation_id: "event.subscribe", body: BodyValidation::None },
    RouteValidation { operation_id: "experimental.config.update", body: BodyValidation::Json(9) },
    RouteValidation { operation_id: "experimental.fs.write", body: BodyValidation::Binary },
    RouteValidation { operation_id: "experimental.generate.text", body: BodyValidation::Json(13) },
    RouteValidation {
        operation_id: "experimental.integration.wellknown.add",
        body: BodyValidation::Json(14),
    },
    RouteValidation { operation_id: "experimental.mcp.add", body: BodyValidation::Json(15) },
    RouteValidation { operation_id: "experimental.mcp.connect", body: BodyValidation::None },
    RouteValidation { operation_id: "experimental.mcp.disconnect", body: BodyValidation::None },
    RouteValidation { operation_id: "experimental.mcp.remove", body: BodyValidation::None },
    RouteValidation {
        operation_id: "experimental.migration.v1.status",
        body: BodyValidation::None,
    },
    RouteValidation { operation_id: "experimental.session.export", body: BodyValidation::None },
    RouteValidation { operation_id: "experimental.session.import", body: BodyValidation::Json(51) },
    RouteValidation {
        operation_id: "experimental.session.instructions.entry.list",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "experimental.session.instructions.entry.put",
        body: BodyValidation::Json(52),
    },
    RouteValidation {
        operation_id: "experimental.session.instructions.entry.remove",
        body: BodyValidation::None,
    },
    RouteValidation { operation_id: "experimental.session.skill", body: BodyValidation::Json(54) },
    RouteValidation { operation_id: "experimental.session.stats", body: BodyValidation::None },
    RouteValidation { operation_id: "experimental.session.wait", body: BodyValidation::None },
    RouteValidation { operation_id: "form.list", body: BodyValidation::None },
    RouteValidation { operation_id: "fs.find", body: BodyValidation::None },
    RouteValidation { operation_id: "fs.list", body: BodyValidation::None },
    RouteValidation { operation_id: "fs.read", body: BodyValidation::None },
    RouteValidation { operation_id: "integration.command.cancel", body: BodyValidation::None },
    RouteValidation { operation_id: "integration.command.connect", body: BodyValidation::Json(59) },
    RouteValidation { operation_id: "integration.command.status", body: BodyValidation::None },
    RouteValidation { operation_id: "integration.connect.key", body: BodyValidation::Json(64) },
    RouteValidation { operation_id: "integration.get", body: BodyValidation::None },
    RouteValidation { operation_id: "integration.list", body: BodyValidation::None },
    RouteValidation { operation_id: "integration.oauth.cancel", body: BodyValidation::None },
    RouteValidation { operation_id: "integration.oauth.complete", body: BodyValidation::Json(66) },
    RouteValidation { operation_id: "integration.oauth.connect", body: BodyValidation::Json(65) },
    RouteValidation { operation_id: "integration.oauth.status", body: BodyValidation::None },
    RouteValidation { operation_id: "location.get", body: BodyValidation::None },
    RouteValidation { operation_id: "location.reload", body: BodyValidation::None },
    RouteValidation { operation_id: "mcp.list", body: BodyValidation::None },
    RouteValidation { operation_id: "mcp.resource.catalog", body: BodyValidation::None },
    RouteValidation { operation_id: "model.default", body: BodyValidation::None },
    RouteValidation { operation_id: "model.list", body: BodyValidation::None },
    RouteValidation { operation_id: "permission.request.list", body: BodyValidation::None },
    RouteValidation { operation_id: "permission.saved.list", body: BodyValidation::None },
    RouteValidation { operation_id: "permission.saved.remove", body: BodyValidation::None },
    RouteValidation { operation_id: "persistentPty.connect", body: BodyValidation::None },
    RouteValidation { operation_id: "plugin.check", body: BodyValidation::Json(67) },
    RouteValidation { operation_id: "plugin.list", body: BodyValidation::None },
    RouteValidation { operation_id: "plugin.update", body: BodyValidation::Json(68) },
    RouteValidation { operation_id: "project.list", body: BodyValidation::None },
    RouteValidation { operation_id: "project.update", body: BodyValidation::Json(73) },
    RouteValidation { operation_id: "provider.get", body: BodyValidation::None },
    RouteValidation { operation_id: "provider.list", body: BodyValidation::None },
    RouteValidation { operation_id: "pty.connect", body: BodyValidation::None },
    RouteValidation { operation_id: "pty.connect.token", body: BodyValidation::None },
    RouteValidation { operation_id: "pty.create", body: BodyValidation::Json(74) },
    RouteValidation { operation_id: "pty.get", body: BodyValidation::None },
    RouteValidation { operation_id: "pty.list", body: BodyValidation::None },
    RouteValidation { operation_id: "pty.remove", body: BodyValidation::None },
    RouteValidation { operation_id: "pty.update", body: BodyValidation::Json(76) },
    RouteValidation { operation_id: "reference.list", body: BodyValidation::None },
    RouteValidation { operation_id: "rpc.call", body: BodyValidation::Json(78) },
    RouteValidation { operation_id: "server.connect", body: BodyValidation::None },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.connectToken",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.create",
        body: BodyValidation::Json(58),
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.get",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.handoff",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.list",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.read",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.remove",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.shutdown",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.snapshot",
        body: BodyValidation::None,
    },
    RouteValidation {
        operation_id: "server.experimental.persistentPty.update",
        body: BodyValidation::Json(19),
    },
    RouteValidation { operation_id: "server.info", body: BodyValidation::None },
    RouteValidation { operation_id: "server.pair", body: BodyValidation::None },
    RouteValidation { operation_id: "session.active", body: BodyValidation::None },
    RouteValidation { operation_id: "session.background", body: BodyValidation::None },
    RouteValidation { operation_id: "session.command", body: BodyValidation::Json(98) },
    RouteValidation { operation_id: "session.compact", body: BodyValidation::Json(99) },
    RouteValidation { operation_id: "session.context", body: BodyValidation::None },
    RouteValidation { operation_id: "session.create", body: BodyValidation::Json(81) },
    RouteValidation { operation_id: "session.diff", body: BodyValidation::None },
    RouteValidation { operation_id: "session.environment", body: BodyValidation::Json(100) },
    RouteValidation { operation_id: "session.fork", body: BodyValidation::Json(101) },
    RouteValidation { operation_id: "session.form.cancel", body: BodyValidation::None },
    RouteValidation { operation_id: "session.form.create", body: BodyValidation::Json(107) },
    RouteValidation { operation_id: "session.form.get", body: BodyValidation::None },
    RouteValidation { operation_id: "session.form.list", body: BodyValidation::None },
    RouteValidation { operation_id: "session.form.reply", body: BodyValidation::Json(109) },
    RouteValidation { operation_id: "session.generate", body: BodyValidation::Json(110) },
    RouteValidation { operation_id: "session.get", body: BodyValidation::None },
    RouteValidation { operation_id: "session.inbox.cancel", body: BodyValidation::None },
    RouteValidation { operation_id: "session.inbox.list", body: BodyValidation::None },
    RouteValidation { operation_id: "session.inbox.update", body: BodyValidation::Json(111) },
    RouteValidation { operation_id: "session.interrupt", body: BodyValidation::None },
    RouteValidation { operation_id: "session.list", body: BodyValidation::None },
    RouteValidation { operation_id: "session.log", body: BodyValidation::None },
    RouteValidation { operation_id: "session.message.get", body: BodyValidation::None },
    RouteValidation { operation_id: "session.message.list", body: BodyValidation::None },
    RouteValidation { operation_id: "session.move", body: BodyValidation::Json(113) },
    RouteValidation { operation_id: "session.permission.create", body: BodyValidation::Json(115) },
    RouteValidation { operation_id: "session.permission.get", body: BodyValidation::None },
    RouteValidation { operation_id: "session.permission.list", body: BodyValidation::None },
    RouteValidation { operation_id: "session.permission.reply", body: BodyValidation::Json(118) },
    RouteValidation { operation_id: "session.prompt", body: BodyValidation::Json(119) },
    RouteValidation { operation_id: "session.remove", body: BodyValidation::None },
    RouteValidation { operation_id: "session.revert.clear", body: BodyValidation::None },
    RouteValidation { operation_id: "session.revert.commit", body: BodyValidation::None },
    RouteValidation { operation_id: "session.revert.stage", body: BodyValidation::Json(120) },
    RouteValidation { operation_id: "session.shell", body: BodyValidation::Json(121) },
    RouteValidation { operation_id: "session.switchAgent", body: BodyValidation::Json(83) },
    RouteValidation { operation_id: "session.switchModel", body: BodyValidation::Json(112) },
    RouteValidation { operation_id: "session.synthetic", body: BodyValidation::Json(122) },
    RouteValidation { operation_id: "session.update", body: BodyValidation::Json(82) },
    RouteValidation { operation_id: "session.view", body: BodyValidation::Json(123) },
    RouteValidation { operation_id: "shell.create", body: BodyValidation::Json(124) },
    RouteValidation { operation_id: "shell.get", body: BodyValidation::None },
    RouteValidation { operation_id: "shell.list", body: BodyValidation::None },
    RouteValidation { operation_id: "shell.output", body: BodyValidation::None },
    RouteValidation { operation_id: "shell.remove", body: BodyValidation::None },
    RouteValidation { operation_id: "skill.list", body: BodyValidation::None },
    RouteValidation { operation_id: "vcs.base", body: BodyValidation::None },
    RouteValidation { operation_id: "vcs.branch.list", body: BodyValidation::None },
    RouteValidation { operation_id: "vcs.diff", body: BodyValidation::None },
    RouteValidation { operation_id: "vcs.get", body: BodyValidation::None },
    RouteValidation { operation_id: "vcs.init", body: BodyValidation::None },
    RouteValidation { operation_id: "vcs.status", body: BodyValidation::None },
    RouteValidation { operation_id: "websearch.providers", body: BodyValidation::None },
    RouteValidation { operation_id: "websearch.query", body: BodyValidation::Json(125) },
    RouteValidation { operation_id: "worktree.create", body: BodyValidation::Json(129) },
    RouteValidation { operation_id: "worktree.list", body: BodyValidation::None },
    RouteValidation { operation_id: "worktree.refresh", body: BodyValidation::Json(130) },
    RouteValidation { operation_id: "worktree.remove", body: BodyValidation::Json(127) },
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every generated route must have exactly one validation row.
    #[test]
    fn covers_all_operations() {
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

        // session.create: title nullable string (Effect `| null` label).
        match &NODES[field_of("session.create", "title").ty as usize] {
            SchemaNode::Nullable(inner) => assert_eq!(NODES[*inner as usize], SchemaNode::Str),
            other => panic!("title not nullable: {other:?}"),
        }

        // Routes without JSON bodies stay pass-through.
        assert_eq!(find("session.list").map(|r| r.body), Some(BodyValidation::None));
        assert_eq!(find("fs.read").map(|r| r.body), Some(BodyValidation::None));
    }
}
