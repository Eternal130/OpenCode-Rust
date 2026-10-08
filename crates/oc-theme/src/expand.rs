//! Theme expansion: `expandTheme`/`expandTokens`/`mergeTheme` ported from
//! `packages/theme/src/tui/expand.ts`.
//!
//! * `merge_theme` deep-merges objects (later values win), skipping `null`
//!   entries and replacing non-object values wholesale; arrays are values,
//!   not containers, and are replaced.
//! * `expand_theme` fills state defaults (`$hovered` etc. fall back to
//!   `<path>.base`) and text defaults (`muted` falls back to `$text.base`,
//!   feedback muted to `$text.feedback.<kind>.base`).

use serde_json::{Map, Value as Json};

/// Deep-merge `values` left to right (TS `mergeTheme`).
pub fn merge_theme<'a>(
    values: impl IntoIterator<Item = &'a Map<String, Json>>,
) -> Map<String, Json> {
    let mut result = Map::new();
    for value in values {
        merge_into(&mut result, value);
    }
    result
}

fn merge_into(result: &mut Map<String, Json>, value: &Map<String, Json>) {
    for (key, item) in value {
        if item.is_null() {
            continue;
        }
        let merged = match (result.get(key), item) {
            (Some(Json::Object(existing)), Json::Object(incoming)) => {
                let mut base = existing.clone();
                merge_into(&mut base, incoming);
                Json::Object(base)
            }
            (_, Json::Object(incoming)) => {
                let mut base = Map::new();
                merge_into(&mut base, incoming);
                Json::Object(base)
            }
            _ => item.clone(),
        };
        result.insert(key.clone(), merged);
    }
}

/// Expand a full definition (tokens plus `@dialog`), TS `expandTheme`.
pub fn expand_theme(definition: &Map<String, Json>) -> Map<String, Json> {
    let mut out = definition.clone();
    for (key, value) in expand_tokens(definition) {
        out.insert(key, value);
    }
    if let Some(dialog) = definition.get("@dialog") {
        let expanded =
            expand_tokens(dialog.as_object().expect("@dialog is an object post-validation"));
        out.insert("@dialog".into(), Json::Object(expanded));
    }
    out
}

/// Expand the token sections of a definition, TS `expandTokens`.
pub fn expand_tokens(definition: &Map<String, Json>) -> Map<String, Json> {
    let mut out = definition.clone();
    if let Some(text) = definition.get("text").and_then(Json::as_object) {
        out.insert("text".into(), Json::Object(expand_text(text)));
    }
    if let Some(background) = definition.get("background").and_then(Json::as_object) {
        out.insert("background".into(), Json::Object(expand_background(background)));
    }
    out
}

fn expand_text(definition: &Map<String, Json>) -> Map<String, Json> {
    let mut out = definition.clone();
    if !out.contains_key("muted") && definition.contains_key("base") {
        out.insert("muted".into(), Json::String("$text.base".into()));
        // Without a base the TS helper stores `undefined`, which merges drop;
        // leaving the key absent is equivalent.
    }
    if let Some(action) = definition.get("action").and_then(Json::as_object) {
        out.insert("action".into(), Json::Object(expand_actions(action, "text.action")));
    }
    if let Some(formfield) = definition.get("formfield").and_then(Json::as_object) {
        out.insert("formfield".into(), Json::Object(expand_formfield(formfield, "text.formfield")));
    }
    if let Some(feedback) = definition.get("feedback").and_then(Json::as_object) {
        let mut expanded = Map::new();
        for (kind, entry) in feedback {
            let mut cleaned = entry.as_object().cloned().unwrap_or_default();
            if !cleaned.contains_key("muted") && cleaned.contains_key("base") {
                cleaned.insert("muted".into(), Json::String(format!("$text.feedback.{kind}.base")));
            }
            expanded.insert(kind.clone(), Json::Object(cleaned));
        }
        out.insert("feedback".into(), Json::Object(expanded));
    }
    out
}

fn expand_background(definition: &Map<String, Json>) -> Map<String, Json> {
    let mut out = definition.clone();
    if let Some(action) = definition.get("action").and_then(Json::as_object) {
        out.insert("action".into(), Json::Object(expand_actions(action, "background.action")));
    }
    if let Some(formfield) = definition.get("formfield").and_then(Json::as_object) {
        out.insert(
            "formfield".into(),
            Json::Object(expand_formfield(formfield, "background.formfield")),
        );
    }
    out
}

fn expand_formfield(definition: &Map<String, Json>, path: &str) -> Map<String, Json> {
    if !definition.contains_key("base") {
        return definition.clone();
    }
    let mut out = definition.clone();
    for state in crate::schema::ACTION_STATES {
        let key = format!("${state}");
        if !out.contains_key(&key) {
            out.insert(key, Json::String(format!("${path}.base")));
        }
    }
    out
}

fn expand_actions(definition: &Map<String, Json>, path: &str) -> Map<String, Json> {
    let mut out = Map::new();
    for (variant, value) in definition {
        let Some(entry) = value.as_object() else {
            out.insert(variant.clone(), value.clone());
            continue;
        };
        if !entry.contains_key("base") {
            out.insert(variant.clone(), value.clone());
            continue;
        }
        let mut expanded = entry.clone();
        for state in crate::schema::ACTION_STATES {
            let key = format!("${state}");
            if !expanded.contains_key(&key) {
                expanded.insert(key, Json::String(format!("${path}.{variant}.base")));
            }
        }
        out.insert(variant.clone(), Json::Object(expanded));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_deeply_and_skips_nulls() {
        let base = json!({ "a": { "b": 1, "c": 2 }, "keep": "x" }).as_object().unwrap().clone();
        let override_ =
            json!({ "a": { "c": 3, "d": null }, "extra": [1, 2] }).as_object().unwrap().clone();
        let merged = merge_theme([&base, &override_]);
        assert_eq!(
            merged,
            json!({ "a": { "b": 1, "c": 3 }, "keep": "x", "extra": [1, 2] })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn objects_replace_scalar_earlier_values() {
        let first = json!({ "a": "scalar" }).as_object().unwrap().clone();
        let second = json!({ "a": { "b": 1 } }).as_object().unwrap().clone();
        let merged = merge_theme([&first, &second]);
        assert_eq!(merged, json!({ "a": { "b": 1 } }).as_object().unwrap().clone());
    }

    #[test]
    fn expands_state_defaults() {
        let definition = json!({
            "action": { "primary": { "base": "#111111", "$hovered": "#222222" } },
            "formfield": { "base": "#111111" }
        })
        .as_object()
        .unwrap()
        .clone();
        let expanded =
            expand_actions(definition.get("action").unwrap().as_object().unwrap(), "text.action");
        assert_eq!(expanded["primary"]["$hovered"], json!("#222222"));
        assert_eq!(expanded["primary"]["$focused"], json!("$text.action.primary.base"));
        assert_eq!(expanded["primary"]["$disabled"], json!("$text.action.primary.base"));

        let formfield = expand_formfield(
            definition.get("formfield").unwrap().as_object().unwrap(),
            "text.formfield",
        );
        assert_eq!(formfield["$pressed"], json!("$text.formfield.base"));
    }

    #[test]
    fn actions_without_base_are_untouched() {
        let definition =
            json!({ "primary": { "$hovered": "#222222" } }).as_object().unwrap().clone();
        let expanded = expand_actions(&definition, "background.action");
        assert_eq!(expanded, definition);
    }

    #[test]
    fn text_muted_defaults_to_text_base() {
        let text = json!({ "base": "#111111" }).as_object().unwrap().clone();
        let expanded = expand_text(&text);
        assert_eq!(expanded["muted"], json!("$text.base"));

        let text = json!({ "feedback": { "error": { "base": "#ff0000" }, "info": { "muted": "#00ff00" } } })
            .as_object()
            .unwrap()
            .clone();
        let expanded = expand_text(&text);
        assert_eq!(expanded["feedback"]["error"]["muted"], json!("$text.feedback.error.base"));
        assert_eq!(expanded["feedback"]["info"]["muted"], json!("#00ff00"));
    }
}
