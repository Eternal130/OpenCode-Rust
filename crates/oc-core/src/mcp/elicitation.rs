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
//! rmcp 3.7 wiring: [`ClientElicitation`] adapts rmcp's
//! `ClientHandler::create_elicitation` (`elicitation/create`,
//! [`rmcp::model::ElicitRequestParams`]) to the registry handler and maps
//! the answer back to [`rmcp::model::ElicitResult`]. Modern-era url
//! elicitations (`tools/call` answering `InputRequiredResult`) are a
//! different mechanism this port leaves single-round (see the client
//! module docs).
//!
//! The form service itself is a seam ([`FormService`]): the real forms
//! service implements it; tests use a scripted fake.

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

/// Input handed to [`RegistryElicitation::create`].
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

/// Elicitation handler seam (`ElicitationHandler`, client.ts:96-111): the
/// registry-bound [`RegistryElicitation`] answers `elicitation/create` and
/// the legacy `notifications/elicitation/complete`; the rmcp-bound
/// [`ClientElicitation`] adapts it to the SDK handler.
pub trait ElicitationHandler: Send + Sync {
    fn create(
        &self,
        input: ElicitationCreate<'_>,
    ) -> futures::future::BoxFuture<'static, ElicitationResult>;
    /// Legacy era only: the server announces a URL-mode elicitation finished.
    fn complete(&self, server: &str, elicitation_id: &str);
}

impl ElicitationHandler for Arc<RegistryElicitation> {
    fn create(
        &self,
        input: ElicitationCreate<'_>,
    ) -> futures::future::BoxFuture<'static, ElicitationResult> {
        let inner = Arc::clone(self);
        let server = input.server.to_string();
        let params = input.params.clone();
        Box::pin(async move {
            // Inherent method by path: `inner.create(..)` would resolve to
            // this trait impl (Arc's own candidate wins over autoderef).
            RegistryElicitation::create(
                &inner,
                ElicitationCreate { server: server.as_str(), params },
            )
            .await
        })
    }

    fn complete(&self, server: &str, elicitation_id: &str) {
        RegistryElicitation::complete(self, server, elicitation_id);
    }
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

/// Deletes a parked URL entry on every exit from
/// [`RegistryElicitation::create`] — the ask settling *or* the future
/// being dropped mid-ask (the client's abort path cancels the select
/// arm) — mirroring `Effect.ensuring` (index.ts:260): the ref deletes
/// the pending entry on ALL exits, not only after the ask resolves.
struct PendingUrlGuard<'a> {
    handler: &'a RegistryElicitation,
    key: Option<String>,
}

impl Drop for PendingUrlGuard<'_> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.handler.pending_urls.lock().remove(&key);
        }
    }
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

    /// Builds the rmcp-bound client adapter over a form service — the
    /// layer's one-line composition (index.ts:439-446 passes the handler
    /// into `McpClient.connect`).
    #[must_use]
    pub fn adapter(forms: Arc<dyn FormService>) -> Arc<ClientElicitation> {
        Arc::new(ClientElicitation::new(Arc::new(RegistryElicitation::new(forms))))
    }

    /// The handler entry point for `elicitation/create`.
    #[allow(clippy::too_many_lines)] // one match over the two elicitation modes
    pub async fn create(&self, input: ElicitationCreate<'_>) -> ElicitationResult {
        match &input.params {
            ElicitationParams::Url { message, url, elicitation_id } => {
                let form_id = FormId::new();
                let pending_entry = elicitation_id
                    .as_ref()
                    .map(|elicitation_id| pending_key(input.server, elicitation_id));
                if let Some(key) = &pending_entry {
                    self.pending_urls.lock().insert(key.clone(), form_id.clone());
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
                // Delete the parked entry on every exit: settling here or
                // the future being dropped when aborted (Effect.ensuring,
                // index.ts:260).
                let _pending = PendingUrlGuard { handler: self, key: pending_entry };
                let answer = self.forms.ask(info).await;
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

    /// Test seam: whether a URL elicitation id is currently parked in
    /// the pending map ("abort removes the pending entry").
    #[cfg(test)]
    pub(crate) fn is_pending_url(&self, server: &str, elicitation_id: &str) -> bool {
        self.pending_urls.lock().contains_key(&pending_key(server, elicitation_id))
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

/// The client-bound rmcp adapter (client.ts:207-214): answers rmcp's
/// `create_elicitation` (`elicitation/create`) through the registry
/// handler and maps the answer back to the SDK's `ElicitResult`.
///
/// Semantics ported from index.ts:231-302 + client.ts:207-214:
/// - the form ask races the request's cancellation token
///   (`Effect.raceFirst(ask, waitForAbort(ctx.mcpReq.signal))`) — an
///   aborted elicitation answers `cancel`, never errors;
/// - form mode answers `{action:"accept", content:{...}}` — `content` is
///   present even when empty (the empty-schema early return is
///   `{action:"accept", content:{}}`); anything not answered is
///   `{action:"cancel"}`;
/// - url mode answers `{action:"accept"}` / `{action:"cancel"}` with no
///   content key ("acknowledges completed MCP URL elicitations without
///   returning internal content");
/// - there is no dedicated elicitation timeout: the ask settles by user
///   answer or request cancellation, and the surrounding `tools/call`
///   owns the execution-timeout wall.
pub struct ClientElicitation {
    inner: Arc<RegistryElicitation>,
}

impl ClientElicitation {
    #[must_use]
    pub fn new(inner: Arc<RegistryElicitation>) -> Self {
        Self { inner }
    }

    /// rmcp `ElicitRequestParams` → the registry's params. The typed
    /// schema re-serializes to raw JSON (wire order preserved —
    /// `ElicitationSchema` round-trips through an ordered wire form);
    /// rmcp requires url-mode `elicitationId`, so it always maps `Some`.
    fn to_params(request: rmcp::model::ElicitRequestParams) -> ElicitationParams {
        match request {
            rmcp::model::ElicitRequestParams::FormElicitationParams {
                message,
                requested_schema,
                ..
            } => ElicitationParams::Form {
                message,
                requested_schema: serde_json::to_value(&requested_schema)
                    .unwrap_or(serde_json::Value::Null),
            },
            rmcp::model::ElicitRequestParams::UrlElicitationParams {
                message,
                url,
                elicitation_id,
                ..
            } => ElicitationParams::Url { message, url, elicitation_id: Some(elicitation_id) },
            // rmcp only models the two current modes.
            _ => ElicitationParams::Form {
                message: String::new(),
                requested_schema: serde_json::Value::Null,
            },
        }
    }

    /// The registry answer → rmcp `ElicitResult`. `url_mode` controls the
    /// content key: form mode always carries it, url mode never does.
    fn to_result(result: ElicitationResult, url_mode: bool) -> rmcp::model::ElicitResult {
        let action = match result.action {
            ElicitationAction::Accept => rmcp::model::ElicitationAction::Accept,
            ElicitationAction::Cancel => rmcp::model::ElicitationAction::Cancel,
        };
        if url_mode || result.action == ElicitationAction::Cancel {
            // index.ts:262-264, 283 — `{action:"accept"}` / `{action:"cancel"}`.
            return rmcp::model::ElicitResult::new(action);
        }
        let mut content = serde_json::Map::new();
        for (key, value) in result.content {
            let value = match value {
                ElicitationValue::String(text) => serde_json::Value::String(text),
                ElicitationValue::Number(number) => serde_json::Number::from_f64(number)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number),
                ElicitationValue::Bool(flag) => serde_json::Value::Bool(flag),
                // index.ts:289 — object answers map through `Array.from`.
                ElicitationValue::Strings(items) => serde_json::Value::Array(
                    items.into_iter().map(serde_json::Value::String).collect(),
                ),
            };
            content.insert(key, value);
        }
        rmcp::model::ElicitResult::new(action).with_content(serde_json::Value::Object(content))
    }

    /// Handles one `elicitation/create` request: ask through the form
    /// service, racing the request cancellation. The future is cancel-safe
    /// to drop: dropping it cancels the underlying form ask.
    pub async fn handle(
        &self,
        server: &str,
        request: rmcp::model::ElicitRequestParams,
        ct: tokio_util::sync::CancellationToken,
    ) -> rmcp::model::ElicitResult {
        let url_mode =
            matches!(request, rmcp::model::ElicitRequestParams::UrlElicitationParams { .. });
        let params = Self::to_params(request);
        // Inherent method by path — the `Arc` trait impl would otherwise
        // shadow it during method resolution.
        let ask = RegistryElicitation::create(&self.inner, ElicitationCreate { server, params });
        let result = tokio::select! {
            result = ask => result,
            // waitForAbort(signal): an aborted elicitation settles cancelled.
            () = ct.cancelled() => ElicitationResult {
                action: ElicitationAction::Cancel,
                content: BTreeMap::new(),
            },
        };
        Self::to_result(result, url_mode)
    }

    /// Legacy `notifications/elicitation/complete` (client.ts:211-213):
    /// the payload's `elicitationId` settles the parked URL form.
    pub fn complete_notification(&self, server: &str, params: Option<&serde_json::Value>) {
        let Some(id) = params
            .and_then(|params| params.get("elicitationId"))
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        self.inner.complete(server, id);
    }
}

/// The real form service answers the elicitation seam: `ask` parks on the
/// forms service's terminal state, `reply` settles best-effort
/// (`Effect.ignore`, index.ts:300).
impl FormService for crate::forms::Forms {
    fn ask(
        &self,
        form: form::Info,
    ) -> futures::future::BoxFuture<'static, Result<form::Answer, FormCancelled>> {
        let forms = self.clone();
        Box::pin(async move {
            let input = crate::forms::NewForm {
                session_id: form.session_id,
                title: form.title,
                metadata: form.metadata,
                fields: form.fields,
            };
            match forms.ask_with_id(form.id, input).await {
                Ok(form::State::Answered { answer }) => Ok(answer),
                // Cancelled, interrupted, or rejected — every non-answer
                // maps to cancel (index.ts:283: `state.status !== "answered"`).
                _ => Err(FormCancelled),
            }
        })
    }

    fn reply(&self, id: &FormId, answer: form::Answer) {
        let forms = self.clone();
        let id = id.clone();
        tokio::spawn(async move {
            let _ = forms.reply(&id, answer).await;
        });
    }
}
