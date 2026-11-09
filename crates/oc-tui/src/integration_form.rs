//! G-271④ — the integration connect methods' `form` answer flow
//! (`formAnswer`, dialog-integration.tsx:777-787 + util/form.ts).
//!
//! A key/oauth method may declare form fields; the ref walks them
//! BEFORE the connect (beginKey :312-317 / beginOAuth :513-518):
//! each ACTIVE field prompts (select for boolean/option/multiselect
//! rows, a text prompt otherwise; hidden fields take their default),
//! and the collected answer rides the `answer` body field of
//! `integration.connect.key` / `integration.oauth.connect`.
//!
//! This module holds the pure halves (the field shape off the wire,
//! `formLabel`/`active`/`formValidateValue`/`formToggleMultiselect`
//! and the walk state machine); the store drives it through
//! [`crate::store::Store`] and the `DialogKind::IntegrationForm`
//! dialog renders the current field.

use oc_client::types::{FormField, FormValue};

/// The collected answer (`Form.Answer`) — the wire body's `answer`.
pub type IntegrationFormAnswer = std::collections::BTreeMap<String, FormValue>;

/// One select option (`Form.Option`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationFieldOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

/// `when` guard (`Form.When`): the answer's `key` must (not) equal
/// `value` (an array answer tests membership, `String(when.value)`).
#[derive(Debug, Clone, PartialEq)]
pub struct IntegrationWhen {
    pub key: String,
    pub value: FormValue,
    /// `eq` (true) / `neq` (false).
    pub equal: bool,
}

/// One form field (the store-side shape of `Form.Field`).
#[derive(Debug, Clone, PartialEq)]
pub struct IntegrationFormField {
    pub key: String,
    /// `formLabel` (util/form.ts:9): `title ?? key` (`title ?? url`
    /// for external).
    pub label: String,
    pub kind: IntegrationFieldKind,
    pub description: Option<String>,
    pub placeholder: Option<String>,
    pub required: bool,
    pub hidden: bool,
    /// `custom` allows typing an own answer (string-with-options +
    /// multiselect).
    pub custom: bool,
    /// The select rows (string-with-options + multiselect).
    pub options: Vec<IntegrationFieldOption>,
    pub default_value: Option<FormValue>,
    /// string format: `email` | `uri` | `date` | `date-time`.
    pub format: Option<&'static str>,
    pub min_length: Option<i64>,
    pub max_length: Option<i64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub min_items: Option<i64>,
    pub max_items: Option<i64>,
    pub when: Vec<IntegrationWhen>,
    /// external only: the link the field opens.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationFieldKind {
    Text,
    Number,
    Integer,
    Boolean,
    Multiselect,
    External,
}

impl IntegrationFieldKind {
    /// The interactive style: textual fields prompt, the rest select.
    #[must_use]
    pub fn is_textual(&self) -> bool {
        matches!(self, Self::Text | Self::Number | Self::Integer)
    }
}

fn options_from_wire(options: &[oc_client::types::FormOption]) -> Vec<IntegrationFieldOption> {
    options
        .iter()
        .map(|option| IntegrationFieldOption {
            value: option.value.clone(),
            label: option.label.clone(),
            description: option.description.clone(),
        })
        .collect()
}

fn when_from_wire(when: Option<&Vec<oc_client::types::FormWhen>>) -> Vec<IntegrationWhen> {
    when.map(|entries| {
        entries
            .iter()
            .map(|entry| IntegrationWhen {
                key: entry.key.clone(),
                value: match &entry.value {
                    oc_client::types::FormWhenValue::String(value) => {
                        FormValue::String(value.clone())
                    }
                    oc_client::types::FormWhenValue::Boolean(value) => FormValue::Boolean(*value),
                    oc_client::types::FormWhenValue::FormWhenValueVariant2(
                        oc_client::types::FormWhenValueVariant2::Number(value),
                    ) => FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(
                        *value,
                    )),
                    oc_client::types::FormWhenValue::FormWhenValueVariant2(_) => {
                        FormValue::String(String::new())
                    }
                },
                equal: entry.op == oc_client::types::FormWhenOp::Eq,
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The number-typed wire unions (`Infinity`/`NaN` literals collapse to
/// `None` — they never validate as finite numbers anyway).
fn finite(value: Option<&oc_client::types::FormNumberFieldDefault>) -> Option<f64> {
    match value {
        Some(oc_client::types::FormNumberFieldDefault::Number(number)) => Some(*number),
        _ => None,
    }
}

fn finite_minimum(value: Option<&oc_client::types::FormNumberFieldMinimum>) -> Option<f64> {
    match value {
        Some(oc_client::types::FormNumberFieldMinimum::Number(number)) => Some(*number),
        _ => None,
    }
}

fn finite_maximum(value: Option<&oc_client::types::FormNumberFieldMaximum>) -> Option<f64> {
    match value {
        Some(oc_client::types::FormNumberFieldMaximum::Number(number)) => Some(*number),
        _ => None,
    }
}

#[allow(clippy::too_many_lines)] // monolithic 1:1 ref port; helper split deferred
/// Shape one wire field (the dialog's row source).
#[must_use]
pub fn field_from_wire(field: &FormField) -> IntegrationFormField {
    match field {
        FormField::StringField(inner) => IntegrationFormField {
            key: inner.key.clone(),
            label: inner.title.clone().unwrap_or_else(|| inner.key.clone()),
            kind: IntegrationFieldKind::Text,
            description: inner.description.clone(),
            placeholder: inner.placeholder.clone(),
            required: inner.required.unwrap_or(false),
            hidden: inner.hidden.unwrap_or(false),
            custom: inner.custom.unwrap_or(false),
            options: options_from_wire(inner.options.as_deref().unwrap_or(&[])),
            default_value: inner.default.clone().map(FormValue::String),
            format: inner.format.as_ref().map(|format| match format {
                oc_client::types::FormStringFieldFormat::Email => "email",
                oc_client::types::FormStringFieldFormat::Uri => "uri",
                oc_client::types::FormStringFieldFormat::Date => "date",
                oc_client::types::FormStringFieldFormat::DateTime => "date-time",
            }),
            min_length: inner.min_length,
            max_length: inner.max_length,
            minimum: None,
            maximum: None,
            min_items: None,
            max_items: None,
            when: when_from_wire(inner.when.as_ref()),
            url: None,
        },
        FormField::NumberField(inner) => IntegrationFormField {
            key: inner.key.clone(),
            label: inner.title.clone().unwrap_or_else(|| inner.key.clone()),
            kind: IntegrationFieldKind::Number,
            description: inner.description.clone(),
            placeholder: None,
            required: inner.required.unwrap_or(false),
            hidden: inner.hidden.unwrap_or(false),
            custom: false,
            options: Vec::new(),
            default_value: finite(inner.default.as_ref()).map(|number| {
                FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(number))
            }),
            format: None,
            min_length: None,
            max_length: None,
            minimum: finite_minimum(inner.minimum.as_ref()),
            maximum: finite_maximum(inner.maximum.as_ref()),
            min_items: None,
            max_items: None,
            when: when_from_wire(inner.when.as_ref()),
            url: None,
        },
        FormField::IntegerField(inner) => IntegrationFormField {
            key: inner.key.clone(),
            label: inner.title.clone().unwrap_or_else(|| inner.key.clone()),
            kind: IntegrationFieldKind::Integer,
            description: inner.description.clone(),
            placeholder: None,
            required: inner.required.unwrap_or(false),
            hidden: inner.hidden.unwrap_or(false),
            custom: false,
            options: Vec::new(),
            default_value: match &inner.default {
                Some(oc_client::types::FormIntegerFieldDefault::Number(number)) => {
                    Some(FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(
                        *number,
                    )))
                }
                _ => None,
            },
            format: None,
            min_length: None,
            max_length: None,
            minimum: match &inner.minimum {
                Some(oc_client::types::FormIntegerFieldMinimum::Number(number)) => Some(*number),
                _ => None,
            },
            maximum: match &inner.maximum {
                Some(oc_client::types::FormIntegerFieldMaximum::Number(number)) => Some(*number),
                _ => None,
            },
            min_items: None,
            max_items: None,
            when: when_from_wire(inner.when.as_ref()),
            url: None,
        },
        FormField::BooleanField(inner) => IntegrationFormField {
            key: inner.key.clone(),
            label: inner.title.clone().unwrap_or_else(|| inner.key.clone()),
            kind: IntegrationFieldKind::Boolean,
            description: inner.description.clone(),
            placeholder: None,
            required: inner.required.unwrap_or(false),
            hidden: inner.hidden.unwrap_or(false),
            custom: false,
            options: Vec::new(),
            default_value: inner.default.map(FormValue::Boolean),
            format: None,
            min_length: None,
            max_length: None,
            minimum: None,
            maximum: None,
            min_items: None,
            max_items: None,
            when: when_from_wire(inner.when.as_ref()),
            url: None,
        },
        FormField::MultiselectField(inner) => IntegrationFormField {
            key: inner.key.clone(),
            label: inner.title.clone().unwrap_or_else(|| inner.key.clone()),
            kind: IntegrationFieldKind::Multiselect,
            description: inner.description.clone(),
            placeholder: None,
            required: inner.required.unwrap_or(false),
            hidden: inner.hidden.unwrap_or(false),
            custom: inner.custom.unwrap_or(false),
            options: options_from_wire(&inner.options),
            default_value: inner
                .default
                .clone()
                .map(|items| FormValue::Array(items.into_iter().collect())),
            format: None,
            min_length: None,
            max_length: None,
            minimum: None,
            maximum: None,
            min_items: inner.min_items,
            max_items: inner.max_items,
            when: when_from_wire(inner.when.as_ref()),
            url: None,
        },
        FormField::ExternalField(inner) => IntegrationFormField {
            key: inner.key.clone(),
            label: inner.title.clone().unwrap_or_else(|| inner.url.clone()),
            kind: IntegrationFieldKind::External,
            description: inner.description.clone(),
            placeholder: None,
            required: false,
            hidden: false,
            custom: false,
            options: Vec::new(),
            default_value: None,
            format: None,
            min_length: None,
            max_length: None,
            minimum: None,
            maximum: None,
            min_items: None,
            max_items: None,
            when: Vec::new(),
            url: Some(inner.url.clone()),
        },
    }
}

/// `active` (dialog-integration.tsx:788-798): every `when` guard must
/// hold on the answer so far (undefined answer never matches).
#[must_use]
pub fn field_active(field: &IntegrationFormField, answer: &IntegrationFormAnswer) -> bool {
    field.when.iter().all(|when| {
        let Some(value) = answer.get(&when.key) else {
            return false;
        };
        let hit = match (value, &when.value) {
            (FormValue::Array(items), FormValue::String(expected)) => {
                items.iter().any(|item| item == expected)
            }
            (FormValue::Array(items), other) => {
                // `value.includes(String(when.value))` — non-string
                // guards stringify (JS template semantics: booleans
                // lowercase, numbers plain).
                let expected = value_to_string(other);
                items.contains(&expected)
            }
            (value, expected) => value == expected,
        };
        when.equal == hit
    })
}

/// JS `String(value)` for the guard comparison (String(other)).
fn value_to_string(value: &FormValue) -> String {
    match value {
        FormValue::String(text) => text.clone(),
        FormValue::Boolean(flag) => flag.to_string(),
        FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(number)) => {
            format_js_number(*number)
        }
        FormValue::FormValueVariant2(_) => String::new(),
        FormValue::Array(items) => items.join(","),
    }
}

#[allow(clippy::cast_possible_truncation)] // rendering geometry/size math: values are terminal-bounded; the ref computes in JS number space
/// JS number → string (integers print bare, fractions as-is).
pub(crate) fn format_js_number(number: f64) -> String {
    if number.fract() == 0.0 && number.abs() < 1e15 {
        format!("{}", number as i64)
    } else {
        format!("{number}")
    }
}

#[allow(clippy::cast_possible_wrap)] // rendering geometry/size math: values are terminal-bounded; the ref computes in JS number space
#[allow(clippy::too_many_lines)] // monolithic 1:1 ref port; helper split deferred
/// `formValidateValue` (util/form.ts:66-105) — the port skips the
/// `pattern` check (a `RegExp` the TUI crate has no engine for; the
/// server re-validates) — bounded deviation, G-271④.
#[must_use]
pub fn validate_value(field: &IntegrationFormField, value: Option<&FormValue>) -> Option<String> {
    let Some(value) = value else {
        return field.required.then(|| "Answer required".to_owned());
    };
    if field.required
        && (value == &FormValue::String(String::new())
            || matches!(value, FormValue::Array(items) if items.is_empty()))
    {
        return Some(
            if field.kind == IntegrationFieldKind::Multiselect {
                "Select at least one option"
            } else {
                "Answer required"
            }
            .to_owned(),
        );
    }
    match field.kind {
        IntegrationFieldKind::Text => {
            let FormValue::String(text) = value else {
                return Some("Expected text".to_owned());
            };
            if let Some(min) = field.min_length {
                if text.chars().count() < usize::try_from(min).unwrap_or(0) {
                    return Some(format!("Must be at least {min} characters"));
                }
            }
            if let Some(max) = field.max_length {
                let max = usize::try_from(max).unwrap_or(usize::MAX);
                if text.chars().count() > max {
                    return Some(format!("Must be at most {max} characters"));
                }
            }
            match field.format {
                Some("email") => {
                    if !valid_email(text) {
                        return Some("Expected an email address".to_owned());
                    }
                }
                Some("uri") => {
                    if !valid_url(text) {
                        return Some("Expected a URL".to_owned());
                    }
                }
                Some("date") => {
                    if !valid_date(text) {
                        return Some("Expected a date (YYYY-MM-DD)".to_owned());
                    }
                }
                Some("date-time") if !valid_date_time(text) => {
                    return Some("Expected a date and time".to_owned());
                }
                _ => {}
            }
            if !field.options.is_empty()
                && !field.custom
                && !field.options.iter().any(|option| &option.value == text)
            {
                return Some("Select an available option".to_owned());
            }
            None
        }
        IntegrationFieldKind::Number | IntegrationFieldKind::Integer => {
            let FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(number)) =
                value
            else {
                return Some("Expected a number".to_owned());
            };
            if !number.is_finite() {
                return Some("Expected a number".to_owned());
            }
            if field.kind == IntegrationFieldKind::Integer && number.fract() != 0.0 {
                return Some("Expected an integer".to_owned());
            }
            if let Some(min) = field.minimum {
                if *number < min {
                    return Some(format!("Must be at least {}", format_js_number(min)));
                }
            }
            if let Some(max) = field.maximum {
                if *number > max {
                    return Some(format!("Must be at most {}", format_js_number(max)));
                }
            }
            None
        }
        IntegrationFieldKind::Boolean => match value {
            FormValue::Boolean(_) => None,
            _ => Some("Expected yes or no".to_owned()),
        },
        IntegrationFieldKind::Multiselect => {
            let FormValue::Array(items) = value else {
                return Some("Expected selections".to_owned());
            };
            if let Some(min) = field.min_items {
                if (items.len() as i64) < min {
                    return Some(format!("Select at least {min}"));
                }
            }
            if let Some(max) = field.max_items {
                if (items.len() as i64) > max {
                    return Some(format!("Select at most {max}"));
                }
            }
            if !field.custom
                && items
                    .iter()
                    .any(|item| !field.options.iter().any(|option| &option.value == item))
            {
                return Some("Select only available options".to_owned());
            }
            None
        }
        // External fields never validate (the answer is `true` once
        // the user confirms the link flow).
        IntegrationFieldKind::External => None,
    }
}

/// `/^[^\s@]+@[^\s@]+\.[^\s@]+$/` without the regex engine.
fn valid_email(text: &str) -> bool {
    let at = text.matches('@').count();
    if at != 1 {
        return false;
    }
    let (local, domain) = text.split_once('@').unwrap_or(("", ""));
    !local.is_empty()
        && !local.contains(char::is_whitespace)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains(char::is_whitespace)
}

/// `new URL(value)` — <scheme://rest> with a non-empty rest.
fn valid_url(text: &str) -> bool {
    text.split_once("://").is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.')
            && !rest.is_empty()
            && !rest.contains(char::is_whitespace)
    })
}

/// `^\d{4}-\d{2}-\d{2}$` + a real calendar date.
fn valid_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let digit = |slice: &str| slice.chars().all(|ch| ch.is_ascii_digit()) && !slice.is_empty();
    let (year, rest) = text.split_at(4);
    let (_, rest) = rest.split_at(1);
    let (month, day) = rest.split_at(2);
    let (month, day) = (&month[..2], &day[1..]);
    if !digit(year) || !digit(month) || !digit(day) {
        return false;
    }
    let Ok(year) = year.parse::<i32>() else { return false };
    let Ok(month) = month.parse::<u32>() else { return false };
    let Ok(day) = day.parse::<u32>() else { return false };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=12).contains(&month) && (1..=max_day).contains(&day)
}

/// `!Number.isNaN(new Date(value).getTime())` — a coarse ISO-ish
/// check (`YYYY-MM-DDTHH:MM(:SS)?`).
fn valid_date_time(text: &str) -> bool {
    let Some((date, time)) = text.split_once(['T', 't']) else {
        return false;
    };
    if !valid_date(date) {
        return false;
    }
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() < 2 {
        return false;
    }
    let numeric = |part: &str| {
        part.trim_end_matches('Z')
            .trim_end_matches(|ch: char| ch.is_ascii_digit())
            .chars()
            .all(|ch| ch.is_ascii_digit())
    };
    parts.iter().all(|part| part.len() == 2 && numeric(part))
}

/// `formToggleMultiselect` (util/form.ts:120-126).
#[must_use]
pub fn toggle_multiselect(selected: &[String], item: &str) -> Vec<String> {
    match selected.iter().position(|entry| entry == item) {
        None => {
            let mut next = selected.to_vec();
            next.push(item.to_owned());
            next
        }
        Some(at) => {
            let mut next = selected.to_vec();
            next.remove(at);
            next
        }
    }
}

/// The walk's state machine (`formAnswer`'s loop as a dialog-driven
/// state: one field mounted at a time, the tail answered).
#[derive(Debug, Clone, PartialEq)]
pub struct IntegrationFormState {
    pub integration_id: String,
    /// `key` for the key method, else the oauth method id.
    pub method_id: String,
    /// `key` | `oauth` — the completion's connect route.
    pub method_kind: &'static str,
    pub fields: Vec<IntegrationFormField>,
    /// The index awaiting an answer.
    pub index: usize,
    pub answer: IntegrationFormAnswer,
    /// The multiselect's working set while one is mounted.
    pub multiselect: Vec<String>,
    /// The prompt-style override (the custom row's "Type your own
    /// answer" arm).
    pub custom_active: bool,
    /// The external field's open-the-link latch ("I finished" needs
    /// a prior open).
    pub external_opened: bool,
    /// The current prompt's inline validation error.
    pub error: Option<String>,
}

impl IntegrationFormState {
    /// Begin the walk at the first active promptable field.
    #[must_use]
    pub fn new(
        integration_id: &str,
        method_id: &str,
        method_kind: &'static str,
        fields: Vec<IntegrationFormField>,
    ) -> Self {
        let mut state = Self {
            integration_id: integration_id.to_owned(),
            method_id: method_id.to_owned(),
            method_kind,
            fields,
            index: 0,
            answer: IntegrationFormAnswer::new(),
            multiselect: Vec::new(),
            custom_active: false,
            external_opened: false,
            error: None,
        };
        state.advance();
        state
    }

    /// The field awaiting an answer (`None` = the walk is done).
    #[must_use]
    pub fn current(&self) -> Option<&IntegrationFormField> {
        self.fields.get(self.index)
    }

    /// Whether the mounted field renders as the text prompt (the
    /// textual kinds + the custom arm).
    #[must_use]
    pub fn prompt_style(&self) -> bool {
        self.current().is_some_and(|field| {
            self.custom_active || (field.kind.is_textual() && field.options.is_empty())
        })
    }

    /// Move to the next field that needs an answer: inactive fields
    /// skip, hidden (non-external) fields take their default. Returns
    /// `false` when the walk completed.
    pub fn advance(&mut self) -> bool {
        self.custom_active = false;
        self.external_opened = false;
        self.error = None;
        while self.index < self.fields.len() {
            let field = &self.fields[self.index];
            if !field_active(field, &self.answer) {
                self.index += 1;
                continue;
            }
            if field.kind != IntegrationFieldKind::External && field.hidden {
                // `field.hidden ? field.default` — an undefined
                // default leaves the key ABSENT (the ref assigns
                // `undefined`, which the `!== undefined` gate drops).
                match field.default_value.clone() {
                    Some(default) => {
                        self.answer.insert(field.key.clone(), default);
                    }
                    None => {
                        self.answer.remove(&field.key);
                    }
                }
                self.index += 1;
                continue;
            }
            if field.kind == IntegrationFieldKind::Multiselect {
                self.multiselect = match &field.default_value {
                    Some(FormValue::Array(items)) => items.clone(),
                    _ => Vec::new(),
                };
            }
            return true;
        }
        false
    }

    /// Record one field's answer (`Some`) or skip it (`None` — the
    /// key stays absent, `answer[field.key] = value`'s undefined
    /// gate) and move PAST the field (the walk continues with
    /// [`Self::advance`]).
    pub fn record(&mut self, key: &str, value: Option<FormValue>) {
        match value {
            Some(value) => {
                self.answer.insert(key.to_owned(), value);
            }
            None => {
                self.answer.remove(key);
            }
        }
        self.index = (self.index + 1).min(self.fields.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_field(key: &str, required: bool) -> IntegrationFormField {
        IntegrationFormField {
            key: key.to_owned(),
            label: key.to_owned(),
            kind: IntegrationFieldKind::Text,
            description: None,
            placeholder: None,
            required,
            hidden: false,
            custom: false,
            options: Vec::new(),
            default_value: None,
            format: None,
            min_length: None,
            max_length: None,
            minimum: None,
            maximum: None,
            min_items: None,
            max_items: None,
            when: Vec::new(),
            url: None,
        }
    }

    #[test]
    fn validation_mirrors_form_validate_value() {
        let mut field = text_field("name", true);
        // required + missing / empty.
        assert_eq!(validate_value(&field, None).as_deref(), Some("Answer required"));
        assert_eq!(
            validate_value(&field, Some(&FormValue::String(String::new()))).as_deref(),
            Some("Answer required")
        );
        // Optional + empty string is undefined-shaped → valid.
        field.required = false;
        assert_eq!(validate_value(&field, None), None);
        // Lengths (JS `value.length` is UTF-16 units; chars() is the
        // port's close equivalent — same count for BMP text).
        field.min_length = Some(3);
        assert_eq!(
            validate_value(&field, Some(&FormValue::String("ab".into()))).as_deref(),
            Some("Must be at least 3 characters")
        );
        // Email format.
        field.min_length = None;
        field.format = Some("email");
        assert_eq!(
            validate_value(&field, Some(&FormValue::String("nope".into()))).as_deref(),
            Some("Expected an email address")
        );
        assert_eq!(validate_value(&field, Some(&FormValue::String("a@b.co".into()))), None);
        // Integer.
        let mut number = text_field("n", false);
        number.kind = IntegrationFieldKind::Integer;
        assert_eq!(
            validate_value(&number, Some(&FormValue::String("3".into()))).as_deref(),
            Some("Expected a number")
        );
        let three = FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(3.0));
        assert_eq!(validate_value(&number, Some(&three)), None);
        let mut bounded = number.clone();
        bounded.maximum = Some(2.0);
        assert_eq!(validate_value(&bounded, Some(&three)).as_deref(), Some("Must be at most 2"));
    }

    #[test]
    fn when_guards_follow_the_ref_semantics() {
        let mut field = text_field("team", true);
        field.when = vec![IntegrationWhen {
            key: "mode".to_owned(),
            value: FormValue::String("org".to_owned()),
            equal: true,
        }];
        let mut answer = IntegrationFormAnswer::new();
        // No answer for the guard key → inactive.
        assert!(!field_active(&field, &answer));
        // A mismatched answer → inactive (eq).
        answer.insert("mode".to_owned(), FormValue::String("personal".into()));
        assert!(!field_active(&field, &answer));
        // A hit → active; an ARRAY answer tests membership.
        answer.insert("mode".to_owned(), FormValue::String("org".into()));
        assert!(field_active(&field, &answer));
        answer.insert("mode".to_owned(), FormValue::Array(vec!["x".into(), "org".into()]));
        assert!(field_active(&field, &answer));
        // neq flips the hit.
        field.when[0].equal = false;
        assert!(!field_active(&field, &answer));
    }

    #[test]
    fn the_walk_skips_hidden_and_inactive_fields() {
        let mut hidden = text_field("region", false);
        hidden.hidden = true;
        hidden.default_value = Some(FormValue::String("us".into()));
        let mut inactive = text_field("never", true);
        inactive.when = vec![IntegrationWhen {
            key: "mode".to_owned(),
            value: FormValue::Boolean(true),
            equal: true,
        }];
        let prompt = text_field("token", true);
        let mut state =
            IntegrationFormState::new("github", "key", "key", vec![hidden, inactive, prompt]);
        // The hidden field's default landed in the answer; the
        // inactive field skipped; the prompt field is current.
        assert_eq!(state.answer.get("region"), Some(&FormValue::String("us".into())));
        let current = state.current().expect("the token field");
        assert_eq!(current.key, "token");
        assert!(state.prompt_style());
        // Answering it completes the walk.
        state.record("token", Some(FormValue::String("sekrit".into())));
        assert!(!state.advance(), "the walk is done");
        assert_eq!(state.answer.get("token"), Some(&FormValue::String("sekrit".into())));
    }

    #[test]
    fn multiselect_toggles_and_seeds_from_default() {
        let mut field = text_field("scopes", false);
        field.kind = IntegrationFieldKind::Multiselect;
        field.options = vec![
            IntegrationFieldOption {
                value: "repo".into(),
                label: "Repo".into(),
                description: None,
            },
            IntegrationFieldOption { value: "org".into(), label: "Org".into(), description: None },
        ];
        field.default_value = Some(FormValue::Array(vec!["repo".into()]));
        let mut state = IntegrationFormState::new("github", "oauth-x", "oauth", vec![field]);
        assert!(!state.prompt_style());
        // The working set seeded from the default; a toggle flips.
        assert_eq!(state.multiselect, vec!["repo".to_owned()]);
        state.multiselect = toggle_multiselect(&state.multiselect, "org");
        assert_eq!(state.multiselect, vec!["repo".to_owned(), "org".to_owned()]);
        state.multiselect = toggle_multiselect(&state.multiselect, "repo");
        assert_eq!(state.multiselect, vec!["org".to_owned()]);
    }
}
