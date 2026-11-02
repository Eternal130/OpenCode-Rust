//! G-150 — the auth `--answer` form system (ref
//! commands/handlers/auth/form.ts `answerForm`): schema-driven answer
//! validation over the login method's form fields (text/boolean/
//! multiselect/external, `when` conditional activation, unknown/
//! inactive field rejection, pattern/format/range checks, JSON values
//! for boolean/numeric fields) plus interactive TTY prompting for
//! missing answers (dialoguer, the G-61 selector pattern).

use dialoguer::{Confirm, Input, MultiSelect, Select};
use oc_client::types::{FormAnswer, FormValue};
use serde_json::Value;

/// ref form.ts `answerForm`: build the form answer from the raw
/// `--answer key=value` inputs. `Ok(None)` = no form (nothing to
/// answer); `Err` = a validation failure (the caller prints + exits).
pub fn answer_form(
    fields: Option<&[Value]>,
    input: &[String],
) -> Result<Option<FormAnswer>, String> {
    let mut supplied: Vec<(String, String)> = Vec::new();
    for item in input {
        let Some(separator) = item.find('=') else {
            return Err("Expected --answer key=value".to_owned());
        };
        if separator < 1 {
            return Err("Expected --answer key=value".to_owned());
        }
        let key = item[..separator].to_owned();
        if let Some(fields) = fields {
            if !fields.iter().any(|field| field_key(field) == key) {
                return Err(format!("Unknown form field: {key}"));
            }
        }
        supplied.push((key, item[separator + 1..].to_owned()));
    }
    let Some(fields) = fields else { return Ok(None) };
    let supplied = supplied;
    let mut answer: FormAnswer = FormAnswer::new();
    for field in fields {
        let key = field_key(field);
        if !active(field, &answer) {
            if supplied.iter().any(|(supplied_key, _)| supplied_key == &key) {
                return Err(format!("Form field is not active: {key}"));
            }
            continue;
        }
        let provided = supplied.iter().find(|(supplied_key, _)| supplied_key == &key);
        let value = match provided {
            Some((_, provided)) => supplied_answer(field, &key, provided)?,
            // Hidden non-external fields take their default without
            // prompting (ref: `field.hidden ? field.default`).
            None if field_type(field) != "external"
                && field.get("hidden").and_then(Value::as_bool).unwrap_or(false) =>
            {
                json_to_form_value(field.get("default").unwrap_or(&Value::Null))
            }
            None => answer_field(field)?,
        };
        if field_type(field) != "external" {
            if let Some(invalid) = validate_value(field, &value) {
                return Err(format!("{key}: {invalid}"));
            }
        }
        answer.insert(key, value);
    }
    Ok(Some(answer))
}

fn field_key(field: &Value) -> String {
    field.get("key").and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn field_type(field: &Value) -> String {
    field.get("type").and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn field_title(field: &Value) -> String {
    field
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())
        .unwrap_or(&field_key(field))
        .to_owned()
}

/// ref form.ts `active`: `when` conditions gate the field on the
/// answer built SO FAR (fields before it).
fn active(field: &Value, answer: &FormAnswer) -> bool {
    let field_type = field_type(field);
    if field_type == "external" {
        return true;
    }
    let Some(conditions) = field.get("when").and_then(Value::as_array) else { return true };
    conditions.iter().all(|condition| {
        let Some(condition_key) = condition.get("key").and_then(Value::as_str) else {
            return false;
        };
        let Some(value) = answer.get(condition_key) else { return false };
        let expected = condition.get("value").cloned().unwrap_or(Value::Null);
        let matches = match value {
            FormValue::Array(values) => {
                values.iter().any(|item| Value::String(item.clone()) == expected)
            }
            FormValue::String(text) => Value::String(text.clone()) == expected,
            FormValue::Boolean(flag) => Value::Bool(*flag) == expected,
            FormValue::FormValueVariant2(_) => false,
        };
        if condition.get("op").and_then(Value::as_str) == Some("neq") {
            !matches
        } else {
            matches
        }
    })
}

/// ref form.ts `suppliedAnswer`: strings pass through; other types take
/// a JSON value; external fields cannot be supplied non-interactively.
fn supplied_answer(field: &Value, key: &str, input: &str) -> Result<FormValue, String> {
    if field_type(field) == "external" {
        return Err(format!("External form field requires interactive confirmation: {key}"));
    }
    if field_type(field) == "string" {
        return Ok(FormValue::String(input.to_owned()));
    }
    let parsed: Value = serde_json::from_str(input)
        .map_err(|_| format!("Expected a JSON value for form field: {key}"))?;
    if parsed.is_null() {
        return Err(format!("Expected a JSON value for form field: {key}"));
    }
    Ok(json_to_form_value(&parsed))
}

/// A JSON value → wire `Form.Value` (string/bool/number/string[]).
fn json_to_form_value(value: &Value) -> FormValue {
    match value {
        Value::String(text) => FormValue::String(text.clone()),
        Value::Bool(flag) => FormValue::Boolean(*flag),
        Value::Number(number) => FormValue::FormValueVariant2(
            oc_client::types::FormValueVariant2::Number(number.as_f64().unwrap_or(0.0)),
        ),
        Value::Array(items) => FormValue::Array(
            items.iter().map(|item| item.as_str().unwrap_or_default().to_owned()).collect(),
        ),
        Value::Null => FormValue::String(String::new()),
        Value::Object(_) => FormValue::String(value.to_string()),
    }
}

/// ref form.ts `answerField`: the interactive prompt per field type.
fn answer_field(field: &Value) -> Result<FormValue, String> {
    crate::handlers::auth::require_interactive(
        "Authentication form input requires an interactive terminal",
    )?;
    let message = field_title(field);
    if let Some(description) = field.get("description").and_then(Value::as_str) {
        println!("  {description}");
    }
    let field_type = field_type(field);
    let required = field.get("required").and_then(Value::as_bool).unwrap_or(false);
    match field_type.as_str() {
        "external" => {
            let url = field.get("url").and_then(Value::as_str).unwrap_or_default();
            println!("  {url}");
            crate::handlers::auth::open_url(url);
            let acknowledged = Confirm::new()
                .with_prompt(format!("{} Continue after completing this step?", message))
                .default(true)
                .interact()
                .map_err(|error| error.to_string())?;
            if !acknowledged {
                return Err(format!("{message} is required"));
            }
            Ok(FormValue::Boolean(true))
        }
        "boolean" => {
            if required {
                let default = field.get("default").and_then(Value::as_bool).unwrap_or(true);
                let value = Confirm::new()
                    .with_prompt(message)
                    .default(default)
                    .interact()
                    .map_err(|e| e.to_string())?;
                Ok(FormValue::Boolean(value))
            } else {
                // Yes / No / Skip (ref select with the skip symbol).
                let items = vec!["Yes".to_owned(), "No".to_owned(), "Skip".to_owned()];
                let default = match field.get("default").and_then(Value::as_bool) {
                    Some(true) => 0,
                    Some(false) => 1,
                    None => 2,
                };
                match Select::new()
                    .with_prompt(message)
                    .items(&items)
                    .default(default)
                    .interact_opt()
                {
                    Ok(Some(0)) => Ok(FormValue::Boolean(true)),
                    Ok(Some(1)) => Ok(FormValue::Boolean(false)),
                    Ok(_) => Ok(FormValue::String(String::new())), // skipped: no value
                    Err(error) => Err(error.to_string()),
                }
            }
        }
        "multiselect" => {
            let options = option_list(field);
            let mut items: Vec<String> =
                options.iter().map(|(label, _, _)| label.clone()).collect();
            let custom = field.get("custom").and_then(Value::as_bool).unwrap_or(false);
            if custom {
                items.push("Type another value".to_owned());
            }
            let preset: Vec<bool> = options
                .iter()
                .map(|(_, value, _)| {
                    field.get("default").and_then(Value::as_array).is_some_and(|defaults| {
                        defaults.iter().any(|d| d.as_str() == Some(value.as_str()))
                    })
                })
                .collect();
            let selection = MultiSelect::new()
                .with_prompt(message)
                .items(&items)
                .defaults(&preset)
                .interact()
                .map_err(|error| error.to_string())?;
            let mut selected: Vec<String> = selection
                .iter()
                .filter(|index| **index < options.len())
                .map(|index| options[*index].1.clone())
                .collect();
            if selection.iter().any(|index| *index >= options.len()) {
                let typed = Input::<String>::new()
                    .with_prompt("Enter value")
                    .validate_with(
                        |input: &String| {
                            if input.is_empty() {
                                Err("Required")
                            } else {
                                Ok(())
                            }
                        },
                    )
                    .interact_text()
                    .map_err(|error| error.to_string())?;
                selected.push(typed);
            }
            let min = field.get("minItems").and_then(Value::as_i64);
            let max = field.get("maxItems").and_then(Value::as_i64);
            if let Some(min) = min {
                if (selected.len() as i64) < min {
                    return Err(format!("Select at least {min}"));
                }
            }
            if let Some(max) = max {
                if (selected.len() as i64) > max {
                    return Err(format!("Select at most {max}"));
                }
            }
            Ok(FormValue::Array(selected))
        }
        // string-with-options: a picker (plus custom/skip).
        other
            if other == "string"
                && field
                    .get("options")
                    .and_then(Value::as_array)
                    .is_some_and(|options| !options.is_empty()) =>
        {
            let options = option_list(field);
            let mut items: Vec<String> =
                options.iter().map(|(label, _, _)| label.clone()).collect();
            let custom = field.get("custom").and_then(Value::as_bool).unwrap_or(false);
            if custom {
                items.push("Type your own answer".to_owned());
            }
            if !required {
                items.push("Skip".to_owned());
            }
            let default = options
                .iter()
                .position(|(_, value, _)| {
                    field.get("default").and_then(Value::as_str) == Some(value.as_str())
                })
                .unwrap_or(items.len().saturating_sub(1));
            let picked = Select::new()
                .with_prompt(message)
                .items(&items)
                .default(default)
                .interact_opt()
                .map_err(|error| error.to_string())?;
            match picked {
                Some(index) if index < options.len() => {
                    Ok(FormValue::String(options[index].1.clone()))
                }
                Some(index) if custom && index == options.len() => {
                    let typed = prompt_text(field)?;
                    Ok(FormValue::String(typed))
                }
                Some(_) => Ok(FormValue::String(String::new())), // skip
                None => Err("Cancelled".to_owned()),
            }
        }
        _ => {
            let typed = prompt_text(field)?;
            if typed.is_empty() && !required {
                return Ok(FormValue::String(String::new()));
            }
            if field_type == "string" {
                Ok(FormValue::String(typed))
            } else {
                // number/integer: convert like ref `Number(value)`.
                Ok(FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(
                    typed.parse::<f64>().map_err(|_| format!("{message}: Expected a number"))?,
                )))
            }
        }
    }
}

fn prompt_text(field: &Value) -> Result<String, String> {
    let message = field_title(field);
    let field_type = field_type(field);
    let mut input = Input::<String>::new().with_prompt(message);
    let _ = field_type;
    let required = field.get("required").and_then(Value::as_bool).unwrap_or(false);
    input
        .validate_with(move |text: &String| {
            if let Some(invalid) = validate_text(field, Some(text)) {
                Err(invalid)
            } else if text.is_empty() && required {
                Err("Required".to_owned())
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map_err(|error| error.to_string())
}

/// (label, value, hint) per form option.
fn option_list(field: &Value) -> Vec<(String, String, Option<String>)> {
    field
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .map(|option| {
                    let value =
                        option.get("value").and_then(Value::as_str).unwrap_or_default().to_owned();
                    let label =
                        option.get("label").and_then(Value::as_str).unwrap_or(&value).to_owned();
                    let hint = option.get("description").and_then(Value::as_str).map(str::to_owned);
                    (label, value, hint)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// ref `formValidateValue`/`validateText` over the ANSWERED value:
/// required-ness, numeric ranges, string lengths, pattern, formats.
fn validate_value(field: &Value, value: &FormValue) -> Option<String> {
    let required = field.get("required").and_then(Value::as_bool).unwrap_or(false);
    let field_type = field_type(field);
    match value {
        FormValue::String(text) if text.is_empty() => {
            if required {
                Some("Required".to_owned())
            } else {
                None
            }
        }
        FormValue::String(text) if field_type == "string" => validate_text(field, Some(text)),
        FormValue::Array(values) => {
            if required && values.is_empty() {
                return Some("Required".to_owned());
            }
            None
        }
        FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(number)) => {
            if field_type == "integer" && number.fract() != 0.0 {
                return Some("Expected an integer".to_owned());
            }
            if let Some(minimum) = field.get("minimum").and_then(Value::as_f64) {
                if *number < minimum {
                    return Some(format!("Must be at least {minimum}"));
                }
            }
            if let Some(maximum) = field.get("maximum").and_then(Value::as_f64) {
                if *number > maximum {
                    return Some(format!("Must be at most {maximum}"));
                }
            }
            None
        }
        _ => None,
    }
}

/// ref form.ts `validateText` (the string/number/integer text checks).
fn validate_text(field: &Value, value: Option<&str>) -> Option<String> {
    let field_type = field_type(field);
    let Some(text) = value else {
        return if field.get("required").and_then(Value::as_bool).unwrap_or(false) {
            Some("Required".to_owned())
        } else {
            None
        };
    };
    if text.is_empty() {
        return if field.get("required").and_then(Value::as_bool).unwrap_or(false) {
            Some("Required".to_owned())
        } else {
            None
        };
    }
    if field_type == "number" || field_type == "integer" {
        let Ok(number) = text.parse::<f64>() else {
            return Some("Expected a number".to_owned());
        };
        if field_type == "integer" && number.fract() != 0.0 {
            return Some("Expected an integer".to_owned());
        }
        if let Some(minimum) = field.get("minimum").and_then(Value::as_f64) {
            if number < minimum {
                return Some(format!("Must be at least {minimum}"));
            }
        }
        if let Some(maximum) = field.get("maximum").and_then(Value::as_f64) {
            if number > maximum {
                return Some(format!("Must be at most {maximum}"));
            }
        }
        return None;
    }
    if let Some(min_length) = field.get("minLength").and_then(Value::as_u64) {
        if (text.chars().count() as u64) < min_length {
            return Some(format!("Must be at least {min_length} characters"));
        }
    }
    if let Some(max_length) = field.get("maxLength").and_then(Value::as_u64) {
        if (text.chars().count() as u64) > max_length {
            return Some(format!("Must be at most {max_length} characters"));
        }
    }
    if let Some(pattern) = field.get("pattern").and_then(Value::as_str) {
        // ref wraps in `new RegExp(pattern).test(value)` — UNanchored
        // substring semantics.
        match regex::Regex::new(pattern) {
            Ok(matcher) => {
                if !matcher.is_match(text) {
                    return Some("Invalid format".to_owned());
                }
            }
            Err(_) => return Some("Invalid format".to_owned()),
        }
    }
    match field.get("format").and_then(Value::as_str) {
        Some("uri") if !text.contains("://") || text.split("://").count() != 2 => {
            return Some("Expected a URL".to_owned());
        }
        Some("email") if !is_email(text) => return Some("Expected an email address".to_owned()),
        Some("date") if !is_date(text) => return Some("Expected a date".to_owned()),
        Some("date-time") if !is_date_time(text) => {
            return Some("Expected a date and time".to_owned());
        }
        _ => {}
    }
    None
}

fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else { return false };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !local.contains(char::is_whitespace)
        && !domain.contains(char::is_whitespace)
}

fn is_date(text: &str) -> bool {
    let Some((year, month, day)) =
        text.split_once('-').and_then(|(y, rest)| rest.split_once('-').map(|(m, d)| (y, m, d)))
    else {
        return false;
    };
    let (Ok(year), Ok(month), Ok(day)) =
        (year.parse::<u32>(), month.parse::<u32>(), day.parse::<u32>())
    else {
        return false;
    };
    year >= 1 && (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn is_date_time(text: &str) -> bool {
    // `Date.parse`-lite: `<date>[T ]<clock>[Z|offset]` with a numeric
    // clock.
    let Some((date, _clock)) = text.split_once(['T', ' ']) else { return false };
    is_date(date) && text.len() > date.len() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn string_field(key: &str, mut extra: Value) -> Value {
        let mut field = json!({ "key": key, "type": "string", "title": key });
        if let (Some(field), Some(extra)) = (field.as_object_mut(), extra.as_object_mut()) {
            for (name, value) in extra {
                field.insert(name.clone(), value.take());
            }
        }
        field
    }

    /// ref answerForm's no-form passthrough.
    #[test]
    fn no_fields_yields_none_even_with_answers() {
        assert!(answer_form(None, &[]).unwrap().is_none());
        assert!(answer_form(None, &["key=value".to_owned()]).unwrap().is_none());
    }

    /// Unknown --answer keys are rejected (ref "Unknown form field").
    #[test]
    fn unknown_field_is_rejected() {
        let error = answer_form(Some(&[string_field("known", json!({}))]), &["other=x".to_owned()])
            .unwrap_err();
        assert_eq!(error, "Unknown form field: other");
    }

    /// Bad key=value syntax is rejected.
    #[test]
    fn malformed_answer_is_rejected() {
        let error = answer_form(None, &["novalue".to_owned()]).unwrap_err();
        assert_eq!(error, "Expected --answer key=value");
    }

    /// String fields take the raw text; boolean/numeric fields parse a
    /// JSON value (ref suppliedAnswer).
    #[test]
    fn supplied_answers_carry_typed_values() {
        let fields = vec![
            string_field("name", json!({})),
            json!({ "key": "enabled", "type": "boolean", "title": "enabled" }),
            json!({ "key": "count", "type": "integer", "title": "count" }),
        ];
        let answer = answer_form(
            Some(&fields),
            &["name=hello".to_owned(), "enabled=true".to_owned(), "count=3".to_owned()],
        )
        .unwrap()
        .expect("answer");
        assert_eq!(answer.get("name"), Some(&FormValue::String("hello".to_owned())));
        assert_eq!(answer.get("enabled"), Some(&FormValue::Boolean(true)));
        assert!(matches!(
            answer.get("count"),
            Some(FormValue::FormValueVariant2(oc_client::types::FormValueVariant2::Number(number))) if *number == 3.0
        ));
    }

    /// A non-JSON value for a numeric field is rejected.
    #[test]
    fn numeric_field_requires_json() {
        let fields = vec![json!({ "key": "count", "type": "integer", "title": "count" })];
        let error = answer_form(Some(&fields), &["count=abc".to_owned()]).unwrap_err();
        assert_eq!(error, "Expected a JSON value for form field: count");
    }

    /// Pattern violations are rejected (ref validateText).
    #[test]
    fn pattern_is_validated() {
        let fields =
            vec![string_field("slug", json!({ "pattern": "^[a-z-]+$", "required": true }))];
        let error = answer_form(Some(&fields), &["slug=Not_Valid".to_owned()]).unwrap_err();
        assert_eq!(error, "slug: Invalid format");
        assert!(answer_form(Some(&fields), &["slug=ok-value".to_owned()]).unwrap().is_some());
    }

    /// Format checks: uri/email/date.
    #[test]
    fn formats_are_validated() {
        let fields = vec![string_field("site", json!({ "format": "uri" }))];
        assert_eq!(
            answer_form(Some(&fields), &["site=notaurl".to_owned()]).unwrap_err(),
            "site: Expected a URL"
        );
        let fields = vec![string_field("date", json!({ "format": "date" }))];
        assert_eq!(
            answer_form(Some(&fields), &["date=2026-13-40".to_owned()]).unwrap_err(),
            "date: Expected a date"
        );
    }

    /// `when` conditions: an inactive field cannot be supplied, and a
    /// satisfied condition makes it required-validated (ref active()).
    #[test]
    fn when_conditions_gate_fields() {
        let fields = vec![
            json!({ "key": "mode", "type": "string", "title": "mode" }),
            json!({ "key": "extra", "type": "string", "title": "extra",
                    "when": [{ "key": "mode", "value": "advanced", "op": "eq" }] }),
        ];
        // mode=basic: extra is inactive — supplying it errors.
        assert_eq!(
            answer_form(Some(&fields.clone()), &["mode=basic".to_owned(), "extra=x".to_owned()])
                .unwrap_err(),
            "Form field is not active: extra"
        );
        // mode=advanced: extra is active and answerable.
        assert!(answer_form(Some(&fields), &["mode=advanced".to_owned(), "extra=x".to_owned()])
            .unwrap()
            .is_some());
    }

    /// External fields cannot be supplied non-interactively.
    #[test]
    fn external_field_rejects_supplied_answer() {
        let fields =
            vec![json!({ "key": "ext", "type": "external", "title": "ext", "url": "https://x" })];
        assert_eq!(
            answer_form(Some(&fields), &["ext=1".to_owned()]).unwrap_err(),
            "External form field requires interactive confirmation: ext"
        );
    }

    /// Hidden fields take their default without prompting.
    #[test]
    fn hidden_field_takes_its_default() {
        let fields = vec![string_field("secret", json!({ "hidden": true, "default": "dflt" }))];
        let answer = answer_form(Some(&fields), &[]).unwrap().expect("answer");
        assert_eq!(answer.get("secret"), Some(&FormValue::String("dflt".to_owned())));
    }

    /// Required empty string answers are rejected post-validate.
    #[test]
    fn required_empty_answer_is_rejected() {
        let fields = vec![string_field("must", json!({ "required": true }))];
        assert_eq!(
            answer_form(Some(&fields), &["must=".to_owned()]).unwrap_err(),
            "must: Required"
        );
    }
}
