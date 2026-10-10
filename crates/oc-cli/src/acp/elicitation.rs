//! Form elicitation (port of `acp/elicitation.ts`): opencode forms →
//! ACP `elicitation/create` for clients that advertise form support,
//! with the credential-free gate.

use agent_client_protocol::schema::v1::{
    CreateElicitationRequest, CreateElicitationResponse, ElicitationAction, ElicitationFormMode,
    ElicitationMode, ElicitationSchema, ElicitationScope, ElicitationSessionScope, EnumOption,
    StringPropertySchema,
};
use serde_json::{Map, Value};

use crate::acp::capabilities::Capabilities;
use crate::acp::child::{self, ChildSession};
use crate::acp::error::classify;
use crate::acp::Conn;

const QUESTION_KIND: &str = "question";

/// `UnshownQuestionMessage` (elicitation.ts:35-36).
pub const UNSHOWN_QUESTION_MESSAGE: &str = "The question couldn't be shown to the user in this client. Continue without an answer: make reasonable assumptions and state them, or ask the user in your reply if you can't proceed.";

/// Credential-ish text never elicits (elicitation.ts:22).
fn credential_like(text: &str) -> bool {
    let lowered = text.to_lowercase();
    ["password", "passphrase", "secret", "token", "api_key", "apikey", "credential", "private_key"]
        .iter()
        .any(|needle| lowered.contains(needle))
        || {
            // api[_ -]?key / private[_ -]?key variants.
            lowered.contains("api key")
                || lowered.contains("api-key")
                || lowered.contains("private key")
                || lowered.contains("private-key")
        }
}

/// The field facts the schema builder needs.
#[allow(clippy::struct_excessive_bools)]
struct Field<'a> {
    key: &'a str,
    r#type: &'a str,
    title: Option<&'a str>,
    description: Option<&'a str>,
    required: bool,
    hidden: bool,
    custom: bool,
    when: bool,
    default: Option<&'a Value>,
    min_length: Option<i64>,
    max_length: Option<i64>,
    pattern: Option<&'a str>,
    format: Option<&'a str>,
    minimum: Option<&'a Value>,
    maximum: Option<&'a Value>,
    min_items: Option<i64>,
    max_items: Option<i64>,
    options: Vec<(&'a str, &'a str, Option<&'a str>)>,
}

fn field_of(raw: &Value) -> Option<Field<'_>> {
    let r#type = raw.get("type")?.as_str()?;
    if r#type == "external" {
        return None;
    }
    Some(Field {
        key: raw.get("key")?.as_str()?,
        r#type,
        title: raw.get("title").and_then(Value::as_str),
        description: raw.get("description").and_then(Value::as_str),
        required: raw.get("required").and_then(Value::as_bool).unwrap_or(false),
        hidden: raw.get("hidden").and_then(Value::as_bool).unwrap_or(false),
        custom: raw.get("custom").and_then(Value::as_bool).unwrap_or(false),
        when: raw.get("when").and_then(Value::as_array).is_some_and(|when| !when.is_empty()),
        default: raw.get("default"),
        min_length: raw.get("minLength").and_then(Value::as_i64),
        max_length: raw.get("maxLength").and_then(Value::as_i64),
        pattern: raw.get("pattern").and_then(Value::as_str),
        format: raw.get("format").and_then(Value::as_str),
        minimum: raw.get("minimum"),
        maximum: raw.get("maximum"),
        min_items: raw.get("minItems").and_then(Value::as_i64),
        max_items: raw.get("maxItems").and_then(Value::as_i64),
        options: raw
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| {
                        Some((
                            option.get("value")?.as_str()?,
                            option.get("label")?.as_str()?,
                            option.get("description").and_then(Value::as_str),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// `requestedSchema` (elicitation.ts:50-65): `None` when the form is
/// not elicitable.
#[must_use]
pub fn requested_schema(form: &Value, capabilities: Capabilities) -> Option<ElicitationSchema> {
    if !capabilities.form_elicitation {
        return None;
    }
    // ElicitedKind: only question and websearch.provider forms (elicitation.ts:21).
    let kind = form.pointer("/metadata/kind").and_then(Value::as_str)?;
    if kind != QUESTION_KIND && kind != "websearch.provider" {
        return None;
    }
    let fields: Vec<&Value> = form
        .get("fields")
        .and_then(Value::as_array)
        .map(|fields: &Vec<Value>| fields.iter().collect())
        .unwrap_or_default();
    if fields.iter().any(|field| {
        field.get("key").and_then(Value::as_str).is_some_and(credential_like)
            || field.get("title").and_then(Value::as_str).is_some_and(credential_like)
            || field.get("description").and_then(Value::as_str).is_some_and(credential_like)
            || field.get("options").and_then(Value::as_array).is_some_and(|options| {
                options.iter().any(|option| {
                    option.get("label").and_then(Value::as_str).is_some_and(credential_like)
                })
            })
    }) {
        return None;
    }
    let parsed: Vec<Field<'_>> = fields.iter().filter_map(|field| field_of(field)).collect();
    // External fields and conditional fields never elicit (elicitation.ts:54-55).
    if parsed.len() != fields.len() || parsed.iter().any(|field| field.when) {
        return None;
    }
    if parsed.iter().any(|field| field.hidden && field.required && field.default.is_none()) {
        return None;
    }
    let keys: Vec<&str> = parsed.iter().map(|field| field.key).collect();
    let visible: Vec<&Field<'_>> = parsed.iter().filter(|field| !field.hidden).collect();
    if !visible.iter().all(|field| representable(field, &keys)) {
        return None;
    }
    let mut schema = ElicitationSchema::new();
    for field in visible {
        let required = field.required;
        for (name, property) in properties(field) {
            if name == field.key {
                schema = schema.property(name, property, required);
            } else {
                // The custom-answer companion of a select is optional.
                schema = schema.property(name, property, false);
            }
        }
    }
    Some(schema)
}

/// `representable` (elicitation.ts:119-129).
fn representable(field: &Field<'_>, keys: &[&str]) -> bool {
    if field.r#type != "string" && field.r#type != "multiselect" {
        return true;
    }
    if field.options.is_empty() {
        return true;
    }
    let values: Vec<&str> = field.options.iter().map(|(value, _, _)| *value).collect();
    let defaults: Vec<String> = match field.default {
        Some(Value::String(single)) => vec![single.clone()],
        Some(Value::Array(items)) => {
            items.iter().filter_map(Value::as_str).map(str::to_owned).collect()
        }
        _ => Vec::new(),
    };
    if defaults.iter().any(|value| !values.contains(&value.as_str())) {
        return false;
    }
    if !field.custom {
        return true;
    }
    if field.required || keys.contains(&custom_key(field).as_str()) {
        return false;
    }
    field.r#type == "string" || (field.min_items.is_none() && field.max_items.is_none())
}

/// `properties` (elicitation.ts:131-167).
fn properties(
    field: &Field<'_>,
) -> Vec<(String, agent_client_protocol::schema::v1::ElicitationPropertySchema)> {
    let mut result = Vec::new();
    let base = |schema: StringPropertySchema| -> StringPropertySchema {
        let mut schema = schema;
        if let Some(title) = field.title {
            schema.title = Some(title.to_owned());
        }
        if let Some(description) = field.description {
            schema.description = Some(description.to_owned());
        }
        schema
    };
    match field.r#type {
        "string" => {
            if field.options.is_empty() {
                let mut property = base(StringPropertySchema::new());
                if let Some(default) = field.default.and_then(Value::as_str) {
                    property.default = Some(default.to_owned());
                }
                apply_string_constraints(&mut property, field);
                result.push((field.key.to_owned(), property.into()));
            } else {
                let mut property = base(StringPropertySchema::new());
                property.one_of = Some(enum_options(field));
                if let Some(default) = field.default.and_then(Value::as_str) {
                    property.default = Some(default.to_owned());
                }
                result.push((field.key.to_owned(), property.into()));
                if field.custom {
                    result.push(other_field(field, "Type your own answer"));
                }
            }
        }
        "multiselect" => {
            let mut array = agent_client_protocol::schema::v1::MultiSelectPropertySchema::titled(
                enum_options(field),
            );
            if let Some(title) = field.title {
                array.title = Some(title.to_owned());
            }
            if let Some(description) = field.description {
                array.description = Some(description.to_owned());
            }
            array.min_items = field.min_required().map(u64::from);
            array.max_items =
                field.max_items.map(|value| u64::try_from(value.max(0)).unwrap_or(u32::MAX.into()));
            match field.default {
                Some(Value::Array(items)) => {
                    array.default = Some(
                        items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect(),
                    );
                }
                Some(Value::String(single)) => {
                    array.default = Some(vec![single.clone()]);
                }
                _ => array.default = None,
            }
            result.push((field.key.to_owned(), array.into()));
            if field.custom {
                let mut other = other_field(field, "Add your own answer");
                if let agent_client_protocol::schema::v1::ElicitationPropertySchema::String(
                    property,
                ) = &mut other.1
                {
                    apply_string_constraints(property, field);
                }
                result.push(other);
            }
        }
        "number" | "integer" => {
            if field.r#type == "number" {
                let mut property = agent_client_protocol::schema::v1::NumberPropertySchema::new();
                property.title = field.title.map(str::to_owned);
                property.description = field.description.map(str::to_owned);
                property.minimum = field.minimum.and_then(Value::as_f64);
                property.maximum = field.maximum.and_then(Value::as_f64);
                property.default = field.default.and_then(Value::as_f64);
                result.push((field.key.to_owned(), property.into()));
            } else {
                let mut property = agent_client_protocol::schema::v1::IntegerPropertySchema::new();
                property.title = field.title.map(str::to_owned);
                property.description = field.description.map(str::to_owned);
                property.minimum = field.minimum.and_then(Value::as_i64);
                property.maximum = field.maximum.and_then(Value::as_i64);
                property.default = field.default.and_then(Value::as_i64);
                result.push((field.key.to_owned(), property.into()));
            }
        }
        "boolean" => {
            let mut property = agent_client_protocol::schema::v1::BooleanPropertySchema::new();
            property.title = field.title.map(str::to_owned);
            property.description = field.description.map(str::to_owned);
            property.default = field.default.and_then(Value::as_bool);
            result.push((field.key.to_owned(), property.into()));
        }
        _ => {}
    }
    result
}

impl Field<'_> {
    fn max_length_val(&self) -> Option<u32> {
        self.max_length.map(|value| u32::try_from(value.max(0)).unwrap_or(u32::MAX))
    }

    fn min_required(&self) -> Option<u32> {
        if self.required {
            Some(self.min_items.map_or(1, |value| u32::try_from(value.max(1)).unwrap_or(u32::MAX)))
        } else {
            self.min_items.map(|value| u32::try_from(value.max(0)).unwrap_or(u32::MAX))
        }
    }
}

/// `stringConstraints` (elicitation.ts:182-189): core rejects an empty
/// string for a required field.
fn apply_string_constraints(property: &mut StringPropertySchema, field: &Field<'_>) {
    property.min_length = Some(if field.required {
        u32::try_from(field.min_length.unwrap_or(0).max(1)).unwrap_or(u32::MAX)
    } else {
        u32::try_from(field.min_length.unwrap_or(0).max(0)).unwrap_or(u32::MAX)
    });
    property.max_length = field.max_length_val();
    property.pattern = field.pattern.map(str::to_owned);
    if let Some(format) = field.format {
        property.format = parse_format(format);
    }
}

fn parse_format(format: &str) -> Option<agent_client_protocol::schema::v1::StringFormat> {
    match format {
        "email" => Some(agent_client_protocol::schema::v1::StringFormat::Email),
        "uri" => Some(agent_client_protocol::schema::v1::StringFormat::Uri),
        "date" => Some(agent_client_protocol::schema::v1::StringFormat::Date),
        "date-time" => Some(agent_client_protocol::schema::v1::StringFormat::DateTime),
        _ => None,
    }
}

/// `other` (elicitation.ts:169-179).
fn other_field(
    field: &Field<'_>,
    description: &str,
) -> (String, agent_client_protocol::schema::v1::ElicitationPropertySchema) {
    let mut property = StringPropertySchema::new();
    property.title = Some(format!("{} (other)", field.title.unwrap_or(field.key)));
    property.description = Some(description.to_owned());
    (custom_key(field), property.into())
}

fn custom_key(field: &Field<'_>) -> String {
    format!("{}_custom", field.key)
}

/// `options` (elicitation.ts:191-197).
fn enum_options(field: &Field<'_>) -> Vec<EnumOption> {
    field
        .options
        .iter()
        .map(|(value, label, description)| {
            let option = EnumOption::new(*value, *label);
            match description {
                Some(description) => option.description(*description),
                None => option,
            }
        })
        .collect()
}

/// The elicitation input (`Input`, elicitation.ts:25-33).
pub struct ElicitationInput<'a> {
    pub client: &'a oc_client::Client,
    pub conn: &'a Conn,
    /// The raw form object (`form.created`'s `form`).
    pub form: &'a Value,
    pub requested_schema: ElicitationSchema,
    #[allow(dead_code)]
    pub client_session_id: String,
    pub child: Option<ChildSession>,
    pub tool_call_sent: bool,
}

/// The reply outcome (`Form.Answer | "cancel"`).
pub type Outcome = Result<Map<String, Value>, ()>;

/// `ask` (elicitation.ts:86-97).
pub async fn ask(
    input: &ElicitationInput<'_>,
    cancelled: &mut (impl std::future::Future<Output = ()> + Unpin),
) -> Outcome {
    // The tool source rides the elicitation when its tool call went to
    // the client (elicitation.ts:87-88).
    let tool_call_id = if input.tool_call_sent {
        input.form.pointer("/metadata/tool/id").and_then(Value::as_str)
    } else {
        None
    };
    let mut scope = ElicitationSessionScope::new(string(input.form, "sessionID"));
    if let Some(tool_call_id) = tool_call_id {
        scope =
            scope.tool_call_id(child::tool_call_id(input.child.as_ref(), tool_call_id).as_str());
    }
    let message = match &input.child {
        Some(child) => child::prefix_title(Some(child), &string(input.form, "title")),
        None => string(input.form, "title"),
    };
    let request = CreateElicitationRequest::new(
        ElicitationMode::Form(ElicitationFormMode::new(
            ElicitationScope::Session(scope),
            input.requested_schema.clone(),
        )),
        message,
    );
    let sent = input.conn.send_request(request);
    let response = tokio::select! {
        () = &mut *cancelled => None,
        response = sent.block_task() => response.ok(),
    };
    let Some(response) = response else { return Err(()) };
    answer(input.form, response)
}

/// `answer` (elicitation.ts:74-84).
fn answer(form: &Value, response: CreateElicitationResponse) -> Outcome {
    let ElicitationAction::Accept(accept) = response.action else {
        return Err(());
    };
    let Some(content) = accept.content else {
        return Err(());
    };
    let mut result = Map::new();
    let empty: Vec<Value> = Vec::new();
    for raw in form.get("fields").and_then(Value::as_array).unwrap_or(&empty) {
        let Some(field) = field_of(raw) else { continue };
        let value = field_answer(&field, &content);
        if let Some(value) = value {
            result.insert(field.key.to_owned(), value);
        }
    }
    Ok(result)
}

/// The elicitation response content (`ElicitationContentValue` map).
type Content =
    std::collections::BTreeMap<String, agent_client_protocol::schema::v1::ElicitationContentValue>;

/// `fieldAnswer` (elicitation.ts:199-207).
fn field_answer(field: &Field<'_>, content: &Content) -> Option<Value> {
    use agent_client_protocol::schema::v1::ElicitationContentValue as ContentValue;
    if field.hidden {
        return field.default.cloned();
    }
    let value = content.get(field.key)?;
    let value = match value {
        ContentValue::String(text) => Value::String(text.clone()),
        ContentValue::Boolean(flag) => Value::Bool(*flag),
        ContentValue::Integer(int) => Value::Number((*int).into()),
        ContentValue::Number(number) => Value::Number(serde_json::Number::from_f64(*number)?),
        ContentValue::StringArray(items) => {
            Value::Array(items.iter().map(|item| Value::String(item.clone())).collect())
        }
        _ => return None,
    };
    if (field.r#type != "string" && field.r#type != "multiselect")
        || !field.custom
        || field.options.is_empty()
    {
        return Some(value);
    }
    let custom = content
        .get(custom_key(field).as_str())
        .and_then(|value| match value {
            ContentValue::String(text) => Some(text.trim()),
            _ => None,
        })
        .unwrap_or("");
    if custom.is_empty() {
        return Some(value);
    }
    if field.r#type == "string" {
        return Some(Value::String(custom.to_owned()));
    }
    match value {
        Value::Array(items) => {
            let mut items = items;
            items.push(Value::String(custom.to_owned()));
            Some(Value::Array(items))
        }
        other => Some(Value::Array(vec![other, Value::String(custom.to_owned())])),
    }
}

/// `cancelUnshown` (elicitation.ts:68-72).
pub async fn cancel_unshown(client: &oc_client::Client, form: &Value) {
    let message = if form.pointer("/metadata/kind").and_then(Value::as_str) == Some(QUESTION_KIND) {
        Some(UNSHOWN_QUESTION_MESSAGE)
    } else {
        None
    };
    cancel(client, form, message).await;
}

/// `cancel` (elicitation.ts:38-48).
pub async fn cancel(client: &oc_client::Client, form: &Value, message: Option<&str>) {
    let session_id = string(form, "sessionID");
    let form_id = string(form, "id");
    let result = client
        .session()
        .form()
        .cancel(
            &session_id,
            &form_id,
            &oc_client::types::SessionFormCancelQuery { message: message.map(str::to_owned) },
        )
        .await;
    match result {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.api_name(),
                Some("FormAlreadySettledError" | "FormNotFoundError")
            ) => {}
        Err(_) => {
            // Fall back to interrupting the session (elicitation.ts:41-47).
            if let Ok(id) = crate::acp::error::decode_session_id(&session_id) {
                let _ = client
                    .session()
                    .interrupt(&id, &oc_client::types::SessionInterruptQuery { resume: None })
                    .await;
            }
        }
    }
}

/// `respond` (elicitation.ts:99-109).
pub async fn respond(input: &ElicitationInput<'_>, outcome: Outcome) {
    match outcome {
        Err(()) => cancel(input.client, input.form, None).await,
        Ok(answer) => {
            let mapped: Option<std::collections::BTreeMap<String, oc_client::types::FormValue>> =
                answer
                    .into_iter()
                    .map(|(key, value)| match value {
                        Value::String(text) => {
                            Some((key, oc_client::types::FormValue::String(text)))
                        }
                        Value::Bool(flag) => {
                            Some((key, oc_client::types::FormValue::Boolean(flag)))
                        }
                        Value::Array(items) => Some((
                            key,
                            oc_client::types::FormValue::Array(
                                items
                                    .into_iter()
                                    .filter_map(|item| item.as_str().map(str::to_owned))
                                    .collect(),
                            ),
                        )),
                        Value::Number(number) => number.as_f64().map(|number| {
                            (
                                key,
                                oc_client::types::FormValue::FormValueVariant2(
                                    oc_client::types::FormValueVariant2::Number(number),
                                ),
                            )
                        }),
                        _ => None,
                    })
                    .collect();
            let result = input
                .client
                .session()
                .form()
                .reply(
                    &string(input.form, "sessionID"),
                    &string(input.form, "id"),
                    &oc_client::types::FormReply { answer: mapped.unwrap_or_default() },
                )
                .await;
            match result {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.api_name(),
                        Some("FormAlreadySettledError" | "FormNotFoundError")
                    ) => {}
                Err(error) => {
                    let _ = classify(&error);
                    cancel(input.client, input.form, None).await;
                }
            }
        }
    }
}

fn string(value: &Value, field: &str) -> String {
    value.get(field).and_then(Value::as_str).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn capabilities() -> Capabilities {
        Capabilities { form_elicitation: true, ..Capabilities::default() }
    }

    // (the helper is consumed by value via requested_schema)

    fn question_form(fields: &Value) -> Value {
        serde_json::json!({
            "id": "frm_1",
            "sessionID": "ses_1",
            "title": "Pick one",
            "metadata": { "kind": "question" },
            "fields": fields,
        })
    }

    #[test]
    fn questions_without_form_support_never_elicit() {
        let form = question_form(&json!([{ "type": "string", "key": "a", "required": true }]));
        assert!(requested_schema(&form, Capabilities::default()).is_none());
    }

    #[test]
    fn simple_questions_map_to_object_schemas() {
        let form = question_form(&json!([
            { "type": "string", "key": "note", "required": true, "title": "Note" },
            { "type": "boolean", "key": "ok", "default": true },
        ]));
        let schema = requested_schema(&form, capabilities()).expect("schema");
        assert_eq!(schema.properties.len(), 2);
        assert!(schema.required.unwrap_or_default().contains(&"note".to_owned()));
    }

    #[test]
    fn non_question_kinds_are_gated() {
        let mut form = question_form(&json!([{ "type": "string", "key": "a" }]));
        form["metadata"]["kind"] = json!("credential");
        assert!(requested_schema(&form, capabilities()).is_none());
        form["metadata"]["kind"] = json!("websearch.provider");
        assert!(requested_schema(&form, capabilities()).is_some());
    }

    #[test]
    fn credential_like_fields_are_rejected() {
        let form = question_form(&json!([
            { "type": "string", "key": "api_key", "required": true },
        ]));
        assert!(requested_schema(&form, capabilities()).is_none());
        let form = question_form(&json!([
            { "type": "string", "key": "a", "title": "Your password" },
        ]));
        assert!(requested_schema(&form, capabilities()).is_none());
    }

    #[test]
    fn external_and_conditional_fields_never_elicit() {
        let form = question_form(&json!([
            { "type": "external", "key": "a" },
            { "type": "string", "key": "b" },
        ]));
        assert!(requested_schema(&form, capabilities()).is_none());
        let form = question_form(&json!([
            { "type": "string", "key": "b", "when": [{ "key": "x", "equals": 1 }] },
        ]));
        assert!(requested_schema(&form, capabilities()).is_none());
    }

    #[test]
    fn selects_gain_a_custom_answer_companion() {
        let form = question_form(&json!([
            { "type": "string", "key": "flavor", "custom": true,
              "options": [
                { "value": "sweet", "label": "Sweet" },
                { "value": "sour", "label": "Sour" },
              ] },
        ]));
        let schema = requested_schema(&form, capabilities()).expect("schema");
        assert!(schema.properties.contains_key("flavor"));
        assert!(schema.properties.contains_key("flavor_custom"));
        let property = &schema.properties["flavor"];
        let encoded = serde_json::to_value(property).unwrap();
        assert_eq!(encoded["oneOf"][0]["const"], "sweet");
        assert_eq!(encoded["oneOf"][1]["title"], "Sour");
    }

    #[test]
    fn hidden_required_fields_without_defaults_never_elicit() {
        let form = question_form(&json!([
            { "type": "string", "key": "a", "hidden": true, "required": true },
        ]));
        assert!(requested_schema(&form, capabilities()).is_none());
    }
}
