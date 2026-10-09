//! MCP elicitation — port of the elicitation halves of `mcp/index.ts`
//! (`elicitation` handler, `GLOBAL_ELICITATION_SESSION_ID`,
//! `URL_ELICITATION_FIELD_KEY`, `toElicitationField`) plus the
//! `ElicitationHandler` implementation from `mcp/client.ts`.
//!
//! MCP elicitations are **location-scoped**, not session-scoped: the server
//! cannot attribute one to a persisted session row, so their forms are
//! owned by the opaque sentinel session id `"global"`.
//!
//! Two modes exist:
//!
//! - **form** — the server sends a JSON-Schema (`requestedSchema`); the
//!   client converts properties to form fields ([`to_elicitation_field`]),
//!   asks through the form service, and returns the answers as
//!   `content`. A schema with zero properties accepts immediately without
//!   creating a form ("accepts empty MCP elicitations without creating
//!   forms").
//! - **url** (legacy era) — the server points the user at an external URL;
//!   the form settles when the user confirms, or early when the server
//!   sends `notifications/elicitation/complete` carrying the
//!   `elicitationId` it announced ("settles modern MCP URL elicitations
//!   when the user confirms").
//!
//! The form service itself is a seam ([`FormService`]): the C-lane forms
//! module will implement it; tests use a scripted fake.

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_schema::form;
use oc_schema::id::FormId;
use serde_json::json;

fn record(value: serde_json::Value) -> oc_schema::common::Record {
    serde_json::from_value(value).unwrap_or_default()
}
use parking_lot::Mutex;

/// `elicitation/create` request params (client.ts `ElicitationParams`).
#[derive(Debug, Clone)]
pub enum ElicitationParams {
    /// Form mode: JSON-Schema properties + required keys.
    Form { message: String, requested_schema: serde_json::Value },
    /// Legacy URL mode.
    Url { message: String, url: String, elicitation_id: Option<String> },
}

/// Input handed to [`ElicitationHandler::create`].
pub struct ElicitationCreate<'a> {
    pub server: &'a str,
    pub params: ElicitationParams,
}

/// `ElicitationResult`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ElicitationResult {
    /// `"accept"` | `"cancel"`.
    pub action: ElicitationAction,
    /// Filled field values on accept.
    pub content: BTreeMap<String, ElicitationValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElicitationAction {
    #[default]
    Accept,
    Cancel,
}

impl ElicitationAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ElicitationAction::Accept => "accept",
            ElicitationAction::Cancel => "cancel",
        }
    }
}

/// One elicited field value (scalar or string list).
#[derive(Debug, Clone, PartialEq)]
pub enum ElicitationValue {
    String(String),
    Number(f64),
    Bool(bool),
    Strings(Vec<String>),
}

/// Elicitation handler seam (`ElicitationHandler`, client.ts:96-111).
/// rmcp 0.5 cannot dispatch `elicitation/create` requests yet; the registry
/// installs the implementation and the mock-transport tests exercise it.
pub trait ElicitationHandler: Send + Sync {
    fn create(
        &self,
        input: ElicitationCreate<'_>,
    ) -> futures::future::BoxFuture<'static, ElicitationResult>;
    /// Legacy era only: the server announces a URL-mode elicitation finished.
    fn complete(&self, server: &str, elicitation_id: &str);
}

/// Forms are owned by this opaque sentinel session id (index.ts:93).
pub const GLOBAL_ELICITATION_SESSION_ID: &str = "global";
/// The single field key of a URL-mode elicitation form (index.ts:94).
pub const URL_ELICITATION_FIELD_KEY: &str = "elicitation";

/// The form-service seam (`Form.Service`): ask a question, answer a parked
/// form. The C-lane forms module implements this; tests script it.
pub trait FormService: Send + Sync {
    /// `forms.ask` — resolves when answered; `Err` when dismissed/cancelled.
    fn ask(
        &self,
        form: form::Info,
    ) -> futures::future::BoxFuture<'static, Result<form::Answer, FormCancelled>>;

    /// `forms.reply` — settles a parked form by id (best effort).
    fn reply(&self, id: &FormId, answer: form::Answer);
}

/// The form was dismissed rather than answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("form dismissed")]
pub struct FormDismissed;

/// `Effect.raceFirst(ask, waitForAbort(signal))` — an aborted elicitation
/// settles as cancelled, not as an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("form cancelled")]
pub struct FormCancelled;

/// Legacy-era pending URL elicitations, settled by
/// `notifications/elicitation/complete` (index.ts:171).
type PendingUrls = BTreeMap<String, FormId>;

fn pending_key(server: &str, elicitation_id: &str) -> String {
    format!("{server}\u{0}{elicitation_id}")
}

/// The registry-bound elicitation handler (index.ts:231-302).
pub struct RegistryElicitation {
    forms: Arc<dyn FormService>,
    pending_urls: Mutex<PendingUrls>,
}

impl RegistryElicitation {
    #[must_use]
    pub fn new(forms: Arc<dyn FormService>) -> Self {
        Self { forms, pending_urls: Mutex::new(BTreeMap::new()) }
    }

    /// The handler entry point for `elicitation/create`.
    #[allow(clippy::too_many_lines)] // one match over the two elicitation modes
    pub async fn create(&self, input: ElicitationCreate<'_>) -> ElicitationResult {
        match &input.params {
            ElicitationParams::Url { message, url, elicitation_id } => {
                let form_id = FormId::new();
                if let Some(elicitation_id) = elicitation_id {
                    self.pending_urls
                        .lock()
                        .insert(pending_key(input.server, elicitation_id), form_id.clone());
                }
                let mut metadata = record(json!({
                    "kind": "mcp-elicitation",
                    "server": input.server,
                    "message": message,
                }));
                if let Some(elicitation_id) = elicitation_id {
                    metadata.insert("elicitationID".to_string(), json!(elicitation_id));
                }
                let info = form::Info {
                    id: form_id,
                    session_id: GLOBAL_ELICITATION_SESSION_ID.to_string(),
                    title: format!("{} is requesting input", input.server),
                    metadata: Some(metadata),
                    fields: vec![form::Field::External {
                        key: URL_ELICITATION_FIELD_KEY.to_string(),
                        url: url.clone(),
                        title: None,
                        description: None,
                    }],
                };
                let answer = self.forms.ask(info).await;
                if let Some(elicitation_id) = elicitation_id {
                    self.pending_urls.lock().remove(&pending_key(input.server, elicitation_id));
                }
                match answer {
                    Ok(_) => ElicitationResult {
                        action: ElicitationAction::Accept,
                        content: BTreeMap::new(),
                    },
                    Err(_) => ElicitationResult {
                        action: ElicitationAction::Cancel,
                        content: BTreeMap::new(),
                    },
                }
            }
            ElicitationParams::Form { message, requested_schema } => {
                let properties = requested_schema
                    .get("properties")
                    .and_then(serde_json::Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let required: Vec<String> = requested_schema
                    .get("required")
                    .and_then(serde_json::Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut fields = Vec::new();
                for (key, property) in &properties {
                    fields.push(to_elicitation_field(key, property, required.contains(key)));
                }
                let Some(first) = fields.first().cloned() else {
                    // Empty schema: accept with no content, without a form.
                    return ElicitationResult::default();
                };
                let info = form::Info {
                    id: FormId::new(),
                    session_id: GLOBAL_ELICITATION_SESSION_ID.to_string(),
                    title: format!("{} is requesting input", input.server),
                    metadata: Some(record(json!({
                        "kind": "mcp-elicitation",
                        "server": input.server,
                        "message": message,
                    }))),
                    fields: {
                        let mut all = vec![first];
                        all.extend(fields.into_iter().skip(1));
                        all
                    },
                };
                match self.forms.ask(info).await {
                    Ok(answer) => ElicitationResult {
                        action: ElicitationAction::Accept,
                        content: answer
                            .into_iter()
                            .map(|(key, value)| {
                                let value = match value {
                                    form::FormValue::String(text) => ElicitationValue::String(text),
                                    form::FormValue::Number(number) => {
                                        ElicitationValue::Number(number)
                                    }
                                    form::FormValue::Bool(flag) => ElicitationValue::Bool(flag),
                                    form::FormValue::Strings(items) => {
                                        ElicitationValue::Strings(items)
                                    }
                                };
                                (key, value)
                            })
                            .collect(),
                    },
                    Err(_) => ElicitationResult {
                        action: ElicitationAction::Cancel,
                        content: BTreeMap::new(),
                    },
                }
            }
        }
    }

    /// Legacy `notifications/elicitation/complete` — settle the parked URL
    /// form without returning internal content (index.ts:296-301).
    pub fn complete(&self, server: &str, elicitation_id: &str) {
        let Some(form_id) =
            self.pending_urls.lock().get(&pending_key(server, elicitation_id)).cloned()
        else {
            return;
        };
        let mut answer = form::Answer::new();
        answer.insert(URL_ELICITATION_FIELD_KEY.to_string(), form::FormValue::Bool(true));
        self.forms.reply(&form_id, answer);
    }
}

/// `toElicitationField` (index.ts:786-844): one JSON-Schema property → one
/// form field. Machine titles like `"string with format email"` lose to the
/// description or the key.
#[must_use]
#[allow(clippy::too_many_lines)] // one match over the JSON-Schema property kinds
pub fn to_elicitation_field(
    key: &str,
    property: &serde_json::Value,
    required: bool,
) -> form::Field {
    let machine_title =
        regex::Regex::new(r"(?i)^(boolean|string|number|integer|array)(\s+with\b.*|\s+in\b.*)?$")
            .expect("static regex");
    let title = property
        .get("title")
        .and_then(serde_json::Value::as_str)
        .filter(|title| !machine_title.is_match(title.trim()))
        .map(str::to_string)
        .or_else(|| {
            property.get("description").and_then(serde_json::Value::as_str).map(str::to_string)
        })
        .unwrap_or_else(|| key.to_string());
    let description = property
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .filter(|description| *description != title);
    let property_type =
        property.get("type").and_then(serde_json::Value::as_str).unwrap_or("string");
    match property_type {
        "boolean" => form::Field::Boolean {
            key: key.to_string(),
            title: Some(title),
            description,
            required: required.then_some(true),
            hidden: None,
            when: None,
            default: property.get("default").and_then(serde_json::Value::as_bool),
        },
        "number" | "integer" => form::Field::Integer {
            key: key.to_string(),
            title: Some(title),
            description,
            required: required.then_some(true),
            hidden: None,
            when: None,
            minimum: property.get("minimum").and_then(serde_json::Value::as_f64),
            maximum: property.get("maximum").and_then(serde_json::Value::as_f64),
            default: property.get("default").and_then(value_as_f64),
        },
        "array" => {
            let items = property.get("items").cloned().unwrap_or(serde_json::Value::Null);
            let options = items
                .get("anyOf")
                .and_then(serde_json::Value::as_array)
                .map(|variants| {
                    variants
                        .iter()
                        .map(|variant| form::Option_ {
                            value: variant
                                .get("const")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            label: variant
                                .get("title")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            description: None,
                        })
                        .collect::<Vec<_>>()
                })
                .or_else(|| {
                    items.get("enum").and_then(serde_json::Value::as_array).map(|values| {
                        values
                            .iter()
                            .map(|value| form::Option_ {
                                value: value.as_str().unwrap_or_default().to_string(),
                                label: value.as_str().unwrap_or_default().to_string(),
                                description: None,
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .unwrap_or_default();
            form::Field::Multiselect {
                key: key.to_string(),
                title: Some(title),
                description,
                required: required.then_some(true),
                hidden: None,
                when: None,
                options,
                min_items: property.get("minItems").and_then(serde_json::Value::as_u64),
                max_items: property.get("maxItems").and_then(serde_json::Value::as_u64),
                custom: Some(false),
                default: property.get("default").and_then(serde_json::Value::as_array).map(
                    |values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    },
                ),
            }
        }
        // "string" and anything unrecognized
        _ => {
            let options = property
                .get("oneOf")
                .and_then(serde_json::Value::as_array)
                .map(|variants| {
                    variants
                        .iter()
                        .map(|variant| form::Option_ {
                            value: variant
                                .get("const")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            label: variant
                                .get("title")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            description: None,
                        })
                        .collect::<Vec<_>>()
                })
                .or_else(|| enum_options(property));
            form::Field::String {
                key: key.to_string(),
                title: Some(title),
                description,
                required: required.then_some(true),
                hidden: None,
                when: None,
                format: property
                    .get("format")
                    .and_then(serde_json::Value::as_str)
                    .and_then(parse_string_format),
                min_length: property.get("minLength").and_then(serde_json::Value::as_u64),
                max_length: property.get("maxLength").and_then(serde_json::Value::as_u64),
                pattern: None,
                placeholder: None,
                default: property
                    .get("default")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                options,
                custom: None,
            }
        }
    }
}

fn value_as_f64(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        // JSON integers stay within the f64 mantissa for every
        // practical bound; the widened cast documents it.
        #[allow(clippy::cast_precision_loss)]
        value.as_i64().map(|number| number as f64)
    })
}

fn parse_string_format(format: &str) -> Option<form::StringFormat> {
    match format {
        "email" => Some(form::StringFormat::Email),
        "uri" => Some(form::StringFormat::Uri),
        "date" => Some(form::StringFormat::Date),
        "date-time" => Some(form::StringFormat::DateTime),
        _ => None,
    }
}

fn enum_options(property: &serde_json::Value) -> Option<Vec<form::Option_>> {
    let values = property.get("enum")?.as_array()?;
    let names = property.get("enumNames").and_then(serde_json::Value::as_array);
    Some(
        values
            .iter()
            .enumerate()
            .map(|(index, value)| form::Option_ {
                value: value.as_str().unwrap_or_default().to_string(),
                label: names
                    .and_then(|names| names.get(index))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| value.as_str().unwrap_or_default())
                    .to_string(),
                description: None,
            })
            .collect(),
    )
}

/// The client-bound [`super::client::ElicitationHandler`] adapter (see the
/// rmcp 0.5 capability gap documented on the client module).
pub struct ClientElicitation {
    inner: Arc<RegistryElicitation>,
}

impl ClientElicitation {
    #[must_use]
    pub fn new(inner: Arc<RegistryElicitation>) -> Self {
        Self { inner }
    }
}

impl ElicitationHandler for ClientElicitation {
    fn create(
        &self,
        input: ElicitationCreate<'_>,
    ) -> futures::future::BoxFuture<'static, ElicitationResult> {
        let inner = Arc::clone(&self.inner);
        let server = input.server.to_string();
        let params = input.params.clone();
        Box::pin(async move {
            inner.create(ElicitationCreate { server: server.as_str(), params }).await
        })
    }

    fn complete(&self, server: &str, elicitation_id: &str) {
        self.inner.complete(server, elicitation_id);
    }
}
