//! Theme document schema: a faithful port of the `effect` Schema definitions in
//! `packages/theme/src/tui/schema.ts`.
//!
//! Validation accepts unknown JSON values and returns a *cleaned* object with
//! unknown keys dropped (the effect `Struct`/`Record` default), mirroring the
//! TS decode step. Required/optional rules follow schema.ts exactly:
//!
//! * `ThemeDocument` = `{ $schema?, base }` plus at least one of `light`/`dark`
//!   (union member order: `light`-required first).
//! * `BaseThemeDefinition` = `categorical` + **complete** tokens + `@dialog?`
//!   (no `hue`; a stray `hue` key is dropped as unknown).
//! * `ModeDefinition` = `hue` (with the three semantic hues) + optional
//!   `categorical` + **sparse** tokens + `@dialog?`.
//! * `ThemeDefinition` = `hue` + `categorical` + complete tokens + `@dialog?`.
//! * Hue scales require all nine steps `"100".."900"`; hue values may also be
//!   `"$hue.<name>"` aliases. Syntax/markdown values are hex or
//!   `"$hue.<name>.<step>"` references only.

use serde_json::{Map, Value};

pub type Json = Value;

pub const HUE_STEPS: [&str; 9] = ["100", "200", "300", "400", "500", "600", "700", "800", "900"];
pub const SEMANTIC_HUES: [&str; 3] = ["accent", "interactive", "neutral"];
pub const ACTION_VARIANTS: [&str; 3] = ["primary", "secondary", "destructive"];
pub const ACTION_STATES: [&str; 5] = ["disabled", "pressed", "focused", "selected", "hovered"];
pub const FEEDBACK_KINDS: [&str; 4] = ["error", "warning", "success", "info"];
pub const SYNTAX_TOKENS: [&str; 9] = [
    "comment",
    "keyword",
    "function",
    "variable",
    "string",
    "number",
    "type",
    "operator",
    "punctuation",
];
pub const MARKDOWN_TOKENS: [&str; 14] = [
    "text",
    "heading",
    "link",
    "linkText",
    "code",
    "blockQuote",
    "emphasis",
    "strong",
    "horizontalRule",
    "listItem",
    "listEnumeration",
    "image",
    "imageText",
    "codeBlock",
];

/// A theme validation/decoding failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError(pub String);

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ThemeError {}

impl From<String> for ThemeError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for ThemeError {
    fn from(message: &str) -> Self {
        Self(message.to_string())
    }
}

fn err(path: &str, detail: &str) -> ThemeError {
    ThemeError(format!("Invalid theme value at \"{path}\": {detail}"))
}

fn object<'a>(value: &'a Json, path: &str) -> Result<&'a Map<String, Json>, ThemeError> {
    value.as_object().ok_or_else(|| err(path, "expected an object"))
}

fn optional_str<'a>(
    map: &'a Map<String, Json>,
    key: &str,
    path: &str,
) -> Result<Option<&'a str>, ThemeError> {
    match map.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(err(path, "expected a string")),
    }
}

fn require<'a>(map: &'a Map<String, Json>, key: &str, path: &str) -> Result<&'a Json, ThemeError> {
    map.get(key)
        .filter(|v| !v.is_null())
        .ok_or_else(|| err(path, &format!("missing required key \"{key}\"")))
}

/// `^#(?:[\da-f]{3}|[\da-f]{4}|[\da-f]{6}|[\da-f]{8})$` (case-insensitive).
pub fn is_hex_color(value: &str) -> bool {
    let body = value.strip_prefix('#').unwrap_or("");
    let hex_digits = |s: &str| s.bytes().all(|b| b.is_ascii_hexdigit());
    matches!(body.len(), 3 | 4 | 6 | 8) && hex_digits(body)
}

/// `ColorValue` = hex | `"transparent"` | `"$" + NonEmptyString`.
pub fn is_color_value(value: &Json) -> bool {
    let Some(s) = value.as_str() else { return false };
    is_hex_color(s) || s == "transparent" || (s.starts_with('$') && s.len() > 1)
}

/// `$hue.<name>.<step>` reference (the `HueColorValue` union member).
pub fn is_hue_step_ref(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("$hue.") else { return false };
    let Some((name, step)) = rest.rsplit_once('.') else { return false };
    !name.is_empty() && HUE_STEPS.contains(&step)
}

fn check_color(value: &Json, path: &str) -> Result<(), ThemeError> {
    if is_color_value(value) {
        Ok(())
    } else {
        Err(err(path, &format!("invalid color value {value:?}")))
    }
}

fn check_hue_color(value: &Json, path: &str) -> Result<(), ThemeError> {
    match value.as_str() {
        Some(s) if is_hex_color(s) || is_hue_step_ref(s) => Ok(()),
        _ => Err(err(path, &format!("invalid hue color value {value:?}"))),
    }
}

fn stateful_color(
    map: &Map<String, Json>,
    path: &str,
    base_required: bool,
) -> Result<Map<String, Json>, ThemeError> {
    let mut out = Map::new();
    if base_required {
        let base = require(map, "base", path)?;
        check_color(base, &format!("{path}.base"))?;
        out.insert("base".into(), base.clone());
    } else if let Some(base) = map.get("base").filter(|v| !v.is_null()) {
        check_color(base, &format!("{path}.base"))?;
        out.insert("base".into(), base.clone());
    }
    for state in ACTION_STATES {
        let key = format!("${state}");
        if let Some(value) = map.get(&key).filter(|v| !v.is_null()) {
            check_color(value, &format!("{path}.{key}"))?;
            out.insert(key, value.clone());
        }
    }
    Ok(out)
}

fn action_colors(
    map: &Map<String, Json>,
    path: &str,
    complete: bool,
) -> Result<Map<String, Json>, ThemeError> {
    let mut out = Map::new();
    for variant in ACTION_VARIANTS {
        let variant_path = format!("{path}.{variant}");
        match map.get(variant).filter(|v| !v.is_null()) {
            Some(value) => {
                out.insert(
                    variant.into(),
                    Json::Object(stateful_color(
                        object(value, &variant_path)?,
                        &variant_path,
                        complete,
                    )?),
                );
            }
            None if complete => return Err(err(&variant_path, "missing required action variant")),
            None => {}
        }
    }
    Ok(out)
}

fn feedback_colors(
    map: &Map<String, Json>,
    path: &str,
    complete: bool,
    with_muted: bool,
) -> Result<Map<String, Json>, ThemeError> {
    let mut out = Map::new();
    for kind in FEEDBACK_KINDS {
        let kind_path = format!("{path}.{kind}");
        match map.get(kind).filter(|v| !v.is_null()) {
            Some(value) => {
                let entry = object(value, &kind_path)?;
                let mut cleaned = Map::new();
                let base = require(entry, "base", &kind_path)?;
                check_color(base, &format!("{kind_path}.base"))?;
                cleaned.insert("base".into(), base.clone());
                if with_muted {
                    if let Some(muted) = entry.get("muted").filter(|v| !v.is_null()) {
                        check_color(muted, &format!("{kind_path}.muted"))?;
                        cleaned.insert("muted".into(), muted.clone());
                    }
                }
                out.insert(kind.into(), Json::Object(cleaned));
            }
            None if complete => return Err(err(&kind_path, "missing required feedback kind")),
            None => {}
        }
    }
    Ok(out)
}

fn text_definition(
    map: &Map<String, Json>,
    path: &str,
    complete: bool,
) -> Result<Map<String, Json>, ThemeError> {
    let mut out = Map::new();
    let color_key = |out: &mut Map<String, Json>, key: &str| -> Result<(), ThemeError> {
        if complete {
            let value = require(map, key, path)?;
            check_color(value, &format!("{path}.{key}"))?;
            out.insert(key.into(), value.clone());
        } else if let Some(value) = map.get(key).filter(|v| !v.is_null()) {
            check_color(value, &format!("{path}.{key}"))?;
            out.insert(key.into(), value.clone());
        }
        Ok(())
    };
    color_key(&mut out, "base")?;
    color_key(&mut out, "muted")?;
    if complete || map.get("action").is_some_and(|v| !v.is_null()) {
        let action_path = format!("{path}.action");
        let action = match map.get("action").filter(|v| !v.is_null()) {
            Some(value) => object(value, &action_path)?,
            None => return Err(err(&action_path, "missing required key")),
        };
        out.insert("action".into(), Json::Object(action_colors(action, &action_path, complete)?));
    }
    if complete || map.get("formfield").is_some_and(|v| !v.is_null()) {
        let formfield_path = format!("{path}.formfield");
        let formfield = match map.get("formfield").filter(|v| !v.is_null()) {
            Some(value) => object(value, &formfield_path)?,
            None => return Err(err(&formfield_path, "missing required key")),
        };
        out.insert(
            "formfield".into(),
            Json::Object(stateful_color(formfield, &formfield_path, complete)?),
        );
    }
    if complete || map.get("feedback").is_some_and(|v| !v.is_null()) {
        let feedback_path = format!("{path}.feedback");
        let feedback = match map.get("feedback").filter(|v| !v.is_null()) {
            Some(value) => object(value, &feedback_path)?,
            None => return Err(err(&feedback_path, "missing required key")),
        };
        out.insert(
            "feedback".into(),
            Json::Object(feedback_colors(feedback, &feedback_path, complete, true)?),
        );
    }
    Ok(out)
}

fn background_definition(
    map: &Map<String, Json>,
    path: &str,
    complete: bool,
) -> Result<Map<String, Json>, ThemeError> {
    let mut out = Map::new();
    if complete {
        let base = require(map, "base", path)?;
        check_color(base, &format!("{path}.base"))?;
        out.insert("base".into(), base.clone());
    } else if let Some(base) = map.get("base").filter(|v| !v.is_null()) {
        check_color(base, &format!("{path}.base"))?;
        out.insert("base".into(), base.clone());
    }
    if complete || map.get("raised").is_some_and(|v| !v.is_null()) {
        let raised_path = format!("{path}.raised");
        let raised = match map.get("raised").filter(|v| !v.is_null()) {
            Some(value) => object(value, &raised_path)?,
            None => return Err(err(&raised_path, "missing required key")),
        };
        let mut cleaned = Map::new();
        for level in ["base", "high", "max"] {
            let level_path = format!("{raised_path}.{level}");
            match raised.get(level).filter(|v| !v.is_null()) {
                Some(value) => {
                    check_color(value, &level_path)?;
                    cleaned.insert(level.into(), value.clone());
                }
                None if complete => return Err(err(&level_path, "missing required raised level")),
                None => {}
            }
        }
        out.insert("raised".into(), Json::Object(cleaned));
    }
    if complete || map.get("action").is_some_and(|v| !v.is_null()) {
        let action_path = format!("{path}.action");
        let action = match map.get("action").filter(|v| !v.is_null()) {
            Some(value) => object(value, &action_path)?,
            None => return Err(err(&action_path, "missing required key")),
        };
        out.insert("action".into(), Json::Object(action_colors(action, &action_path, complete)?));
    }
    if complete || map.get("formfield").is_some_and(|v| !v.is_null()) {
        let formfield_path = format!("{path}.formfield");
        let formfield = match map.get("formfield").filter(|v| !v.is_null()) {
            Some(value) => object(value, &formfield_path)?,
            None => return Err(err(&formfield_path, "missing required key")),
        };
        out.insert(
            "formfield".into(),
            Json::Object(stateful_color(formfield, &formfield_path, complete)?),
        );
    }
    if complete || map.get("feedback").is_some_and(|v| !v.is_null()) {
        let feedback_path = format!("{path}.feedback");
        let feedback = match map.get("feedback").filter(|v| !v.is_null()) {
            Some(value) => object(value, &feedback_path)?,
            None => return Err(err(&feedback_path, "missing required key")),
        };
        out.insert(
            "feedback".into(),
            Json::Object(feedback_colors(feedback, &feedback_path, complete, false)?),
        );
    }
    Ok(out)
}

fn diff_definition(
    map: &Map<String, Json>,
    path: &str,
    complete: bool,
) -> Result<Map<String, Json>, ThemeError> {
    let mut out = Map::new();
    let section = |out: &mut Map<String, Json>,
                   key: &str,
                   fields: &[&str]|
     -> Result<(), ThemeError> {
        let section_path = format!("{path}.{key}");
        if !complete && map.get(key).is_none_or(Json::is_null) {
            return Ok(());
        }
        let section = match map.get(key).filter(|v| !v.is_null()) {
            Some(value) => object(value, &section_path)?,
            None => return Err(err(&section_path, "missing required key")),
        };
        let mut cleaned = Map::new();
        for field in fields {
            let field_path = format!("{section_path}.{field}");
            match section.get(*field).filter(|v| !v.is_null()) {
                Some(value) => {
                    check_color(value, &field_path)?;
                    cleaned.insert((*field).into(), value.clone());
                }
                None if complete => return Err(err(&field_path, "missing required diff field")),
                None => {}
            }
        }
        out.insert(key.into(), Json::Object(cleaned));
        Ok(())
    };
    section(&mut out, "text", &["added", "removed", "context", "hunkHeader"])?;
    section(&mut out, "background", &["added", "removed", "context"])?;
    section(&mut out, "highlight", &["added", "removed"])?;
    // lineNumber: { text: ColorValue, background: { added, removed } }
    let line_path = format!("{path}.lineNumber");
    if complete || map.get("lineNumber").is_some_and(|v| !v.is_null()) {
        let line = match map.get("lineNumber").filter(|v| !v.is_null()) {
            Some(value) => object(value, &line_path)?,
            None => return Err(err(&line_path, "missing required key")),
        };
        let mut cleaned = Map::new();
        match line.get("text").filter(|v| !v.is_null()) {
            Some(value) => {
                check_color(value, &format!("{line_path}.text"))?;
                cleaned.insert("text".into(), value.clone());
            }
            None if complete => {
                return Err(err(&format!("{line_path}.text"), "missing required diff field"))
            }
            None => {}
        }
        let line_bg_path = format!("{line_path}.background");
        let mut line_bg = Map::new();
        if complete || line.get("background").is_some_and(|v| !v.is_null()) {
            let bg = match line.get("background").filter(|v| !v.is_null()) {
                Some(value) => object(value, &line_bg_path)?,
                None => return Err(err(&line_bg_path, "missing required key")),
            };
            for field in ["added", "removed"] {
                let field_path = format!("{line_bg_path}.{field}");
                match bg.get(field).filter(|v| !v.is_null()) {
                    Some(value) => {
                        check_color(value, &field_path)?;
                        line_bg.insert(field.into(), value.clone());
                    }
                    None if complete => {
                        return Err(err(&field_path, "missing required diff field"))
                    }
                    None => {}
                }
            }
        }
        cleaned.insert("background".into(), Json::Object(line_bg));
        out.insert("lineNumber".into(), Json::Object(cleaned));
    }
    Ok(out)
}

fn hue_scoped_tokens(
    map: &Map<String, Json>,
    path: &str,
    tokens: &[&str],
    complete: bool,
) -> Result<Map<String, Json>, ThemeError> {
    let mut out = Map::new();
    for token in tokens {
        let token_path = format!("{path}.{token}");
        match map.get(*token).filter(|v| !v.is_null()) {
            Some(value) => {
                check_hue_color(value, &token_path)?;
                out.insert((*token).into(), value.clone());
            }
            None if complete => return Err(err(&token_path, "missing required token")),
            None => {}
        }
    }
    Ok(out)
}

/// `ThemeTokensDefinition` — the seven token sections (sparse).
pub fn tokens_definition(value: &Json, path: &str) -> Result<Map<String, Json>, ThemeError> {
    let map = object(value, path)?;
    let mut out = Map::new();
    let text = match map.get("text").filter(|v| !v.is_null()) {
        Some(v) => Json::Object(text_definition(
            object(v, &format!("{path}.text"))?,
            &format!("{path}.text"),
            false,
        )?),
        None => Json::Null,
    };
    if !text.is_null() {
        out.insert("text".into(), text);
    }
    let background = match map.get("background").filter(|v| !v.is_null()) {
        Some(v) => Json::Object(background_definition(
            object(v, &format!("{path}.background"))?,
            &format!("{path}.background"),
            false,
        )?),
        None => Json::Null,
    };
    if !background.is_null() {
        out.insert("background".into(), background);
    }
    for key in ["border", "scrollbar"] {
        if let Some(value) = map.get(key).filter(|v| !v.is_null()) {
            let key_path = format!("{path}.{key}");
            let section = object(value, &key_path)?;
            let mut cleaned = Map::new();
            if let Some(base) = section.get("base").filter(|v| !v.is_null()) {
                check_color(base, &format!("{key_path}.base"))?;
                cleaned.insert("base".into(), base.clone());
            }
            out.insert(key.into(), Json::Object(cleaned));
        }
    }
    if let Some(value) = map.get("diff").filter(|v| !v.is_null()) {
        out.insert(
            "diff".into(),
            Json::Object(diff_definition(
                object(value, &format!("{path}.diff"))?,
                &format!("{path}.diff"),
                false,
            )?),
        );
    }
    if let Some(value) = map.get("syntax").filter(|v| !v.is_null()) {
        out.insert(
            "syntax".into(),
            Json::Object(hue_scoped_tokens(
                object(value, &format!("{path}.syntax"))?,
                &format!("{path}.syntax"),
                &SYNTAX_TOKENS,
                false,
            )?),
        );
    }
    if let Some(value) = map.get("markdown").filter(|v| !v.is_null()) {
        out.insert(
            "markdown".into(),
            Json::Object(hue_scoped_tokens(
                object(value, &format!("{path}.markdown"))?,
                &format!("{path}.markdown"),
                &MARKDOWN_TOKENS,
                false,
            )?),
        );
    }
    Ok(out)
}

/// The seven complete token sections of `CompleteThemeTokensDefinition`.
fn complete_tokens(map: &Map<String, Json>, path: &str) -> Result<Map<String, Json>, ThemeError> {
    let mut out = Map::new();
    let text = require(map, "text", path)?;
    out.insert(
        "text".into(),
        Json::Object(text_definition(
            object(text, &format!("{path}.text"))?,
            &format!("{path}.text"),
            true,
        )?),
    );
    let background = require(map, "background", path)?;
    out.insert(
        "background".into(),
        Json::Object(background_definition(
            object(background, &format!("{path}.background"))?,
            &format!("{path}.background"),
            true,
        )?),
    );
    for key in ["border", "scrollbar"] {
        let key_path = format!("{path}.{key}");
        let section = object(require(map, key, path)?, &key_path)?;
        let base = require(section, "base", &key_path)?;
        check_color(base, &format!("{key_path}.base"))?;
        let mut cleaned = Map::new();
        cleaned.insert("base".into(), base.clone());
        out.insert(key.into(), Json::Object(cleaned));
    }
    let diff = require(map, "diff", path)?;
    out.insert(
        "diff".into(),
        Json::Object(diff_definition(
            object(diff, &format!("{path}.diff"))?,
            &format!("{path}.diff"),
            true,
        )?),
    );
    let syntax = require(map, "syntax", path)?;
    out.insert(
        "syntax".into(),
        Json::Object(hue_scoped_tokens(
            object(syntax, &format!("{path}.syntax"))?,
            &format!("{path}.syntax"),
            &SYNTAX_TOKENS,
            true,
        )?),
    );
    let markdown = require(map, "markdown", path)?;
    out.insert(
        "markdown".into(),
        Json::Object(hue_scoped_tokens(
            object(markdown, &format!("{path}.markdown"))?,
            &format!("{path}.markdown"),
            &MARKDOWN_TOKENS,
            true,
        )?),
    );
    Ok(out)
}

/// `HueDefinition` — non-empty hue names mapping to scales or aliases,
/// with `accent`/`interactive`/`neutral` required.
pub fn hue_definition(value: &Json, path: &str) -> Result<Map<String, Json>, ThemeError> {
    let map = object(value, path)?;
    let mut out = Map::new();
    for (name, entry) in map {
        if name.is_empty() {
            // Empty keys are silently dropped, matching the effect record
            // decode of `Record(NonEmptyString, ...)`.
            continue;
        }
        let entry_path = format!("{path}.{name}");
        let entry = match entry.as_str() {
            Some(s) if s.starts_with("$hue.") && s.len() > "$hue.".len() => {
                Json::String(s.to_string())
            }
            Some(_) => return Err(err(&entry_path, "invalid hue alias")),
            None => {
                let scale = object(entry, &entry_path)?;
                let mut cleaned = Map::new();
                for step in HUE_STEPS {
                    let color = require(scale, step, &entry_path)?;
                    let color =
                        color.as_str().ok_or_else(|| err(&entry_path, "expected a hex color"))?;
                    if !is_hex_color(color) {
                        return Err(err(&format!("{entry_path}.{step}"), "expected a hex color"));
                    }
                    cleaned.insert(step.into(), Json::String(color.to_string()));
                }
                Json::Object(cleaned)
            }
        };
        out.insert(name.clone(), entry);
    }
    let missing: Vec<&str> =
        SEMANTIC_HUES.iter().filter(|hue| !out.contains_key(**hue)).copied().collect();
    if !missing.is_empty() {
        return Err(ThemeError(format!(
            "Invalid theme value at \"{path}\": missing required semantic hues: {}",
            missing.join(", ")
        )));
    }
    Ok(out)
}

fn categorical(value: &Json, path: &str) -> Result<Vec<Json>, ThemeError> {
    let list =
        value.as_array().ok_or_else(|| err(path, "expected a non-empty array of hue names"))?;
    if list.is_empty() {
        return Err(err(path, "expected a non-empty array of hue names"));
    }
    for item in list {
        if item.as_str().is_none_or(str::is_empty) {
            return Err(err(path, "expected a non-empty array of hue names"));
        }
    }
    Ok(list.clone())
}

/// `BaseThemeDefinition` — complete tokens plus categorical, no hue.
pub fn base_theme_definition(value: &Json, path: &str) -> Result<Map<String, Json>, ThemeError> {
    let map = object(value, path)?;
    let mut out = complete_tokens(map, path)?;
    let list = categorical(require(map, "categorical", path)?, &format!("{path}.categorical"))?;
    out.insert("categorical".into(), Json::Array(list));
    if let Some(dialog) = map.get("@dialog").filter(|v| !v.is_null()) {
        out.insert(
            "@dialog".into(),
            Json::Object(tokens_definition(dialog, &format!("{path}.@dialog"))?),
        );
    }
    Ok(out)
}

/// `ModeDefinition` — required hue plus optional sparse overrides.
pub fn mode_definition(value: &Json, path: &str) -> Result<Map<String, Json>, ThemeError> {
    let map = object(value, path)?;
    let mut out = Map::new();
    out.insert(
        "hue".into(),
        Json::Object(hue_definition(require(map, "hue", path)?, &format!("{path}.hue"))?),
    );
    if let Some(list) = map.get("categorical").filter(|v| !v.is_null()) {
        out.insert(
            "categorical".into(),
            Json::Array(categorical(list, &format!("{path}.categorical"))?),
        );
    }
    for (key, value) in tokens_definition(&Json::Object(map.clone()), path)? {
        out.insert(key, value);
    }
    if let Some(dialog) = map.get("@dialog").filter(|v| !v.is_null()) {
        out.insert(
            "@dialog".into(),
            Json::Object(tokens_definition(dialog, &format!("{path}.@dialog"))?),
        );
    }
    Ok(out)
}

/// `ThemeDefinition` — a complete, standalone theme (hue + categorical + tokens).
pub fn theme_definition(value: &Json, path: &str) -> Result<Map<String, Json>, ThemeError> {
    let map = object(value, path)?;
    let mut out = Map::new();
    out.insert(
        "hue".into(),
        Json::Object(hue_definition(require(map, "hue", path)?, &format!("{path}.hue"))?),
    );
    let list = categorical(require(map, "categorical", path)?, &format!("{path}.categorical"))?;
    out.insert("categorical".into(), Json::Array(list));
    for (key, value) in complete_tokens(map, path)? {
        out.insert(key, value);
    }
    if let Some(dialog) = map.get("@dialog").filter(|v| !v.is_null()) {
        out.insert(
            "@dialog".into(),
            Json::Object(tokens_definition(dialog, &format!("{path}.@dialog"))?),
        );
    }
    Ok(out)
}

/// `ThemeDocument` — `{ $schema?, base }` with at least one of `light`/`dark`.
pub fn theme_document(value: &Json, path: &str) -> Result<Map<String, Json>, ThemeError> {
    let map = object(value, path)?;
    let base = base_theme_definition(require(map, "base", path)?, &format!("{path}.base"))?;
    let light = map.get("light").filter(|v| !v.is_null());
    let dark = map.get("dark").filter(|v| !v.is_null());
    if light.is_none() && dark.is_none() {
        return Err(err(&format!("{path}.light"), "theme must provide at least one mode"));
    }
    let mut out = Map::new();
    if let Some(schema) = optional_str(map, "$schema", path)? {
        out.insert("$schema".into(), Json::String(schema.to_string()));
    }
    out.insert("base".into(), Json::Object(base));
    if let Some(light) = light {
        out.insert("light".into(), Json::Object(mode_definition(light, &format!("{path}.light"))?));
    }
    if let Some(dark) = dark {
        out.insert("dark".into(), Json::Object(mode_definition(dark, &format!("{path}.dark"))?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hex_color() -> Json {
        json!({
            "100": "#000000", "200": "#111111", "300": "#222222", "400": "#333333",
            "500": "#444444", "600": "#555555", "700": "#666666", "800": "#777777", "900": "#888888"
        })
    }

    fn document() -> Json {
        json!({
            "base": {
                "categorical": ["a"],
                "text": { "base": "#000000", "muted": "#111111",
                    "action": { "primary": {"base": "#000"}, "secondary": {"base": "$text.muted"}, "destructive": {"base": "transparent"} },
                    "formfield": {"base": "#000000"},
                    "feedback": { "error": {"base": "#000"}, "warning": {"base": "#000"}, "success": {"base": "#000"}, "info": {"base": "#000"} } },
                "background": { "base": "#000000",
                    "raised": {"base": "#000", "high": "#000", "max": "#000"},
                    "action": { "primary": {"base": "#000"}, "secondary": {"base": "#000"}, "destructive": {"base": "#000"} },
                    "formfield": {"base": "#000"},
                    "feedback": { "error": {"base": "#000"}, "warning": {"base": "#000"}, "success": {"base": "#000"}, "info": {"base": "#000"} } },
                "border": {"base": "#000"}, "scrollbar": {"base": "#000"},
                "diff": {
                    "text": {"added": "#000", "removed": "#000", "context": "#000", "hunkHeader": "#000"},
                    "background": {"added": "#000", "removed": "#000", "context": "#000"},
                    "highlight": {"added": "#000", "removed": "#000"},
                    "lineNumber": {"text": "#000", "background": {"added": "#000", "removed": "#000"}} },
                "syntax": { "comment": "#000", "keyword": "#000", "function": "#000", "variable": "#000", "string": "#000",
                    "number": "#000", "type": "#000", "operator": "#000", "punctuation": "#000" },
                "markdown": { "text": "#000", "heading": "#000", "link": "#000", "linkText": "#000", "code": "#000",
                    "blockQuote": "#000", "emphasis": "#000", "strong": "#000", "horizontalRule": "#000",
                    "listItem": "#000", "listEnumeration": "#000", "image": "#000", "imageText": "#000", "codeBlock": "#000" }
            },
            "light": { "hue": { "gray": hex_color(), "accent": "$hue.gray", "interactive": "$hue.gray", "neutral": "$hue.gray" } },
            "dark": { "hue": { "gray": hex_color(), "accent": "$hue.gray", "interactive": "$hue.gray", "neutral": "$hue.gray" } }
        })
    }

    #[test]
    fn validates_and_strips_unknown_keys() {
        let mut input = document();
        input["$schema"] = json!("https://opencode.ai/theme.json");
        input["stray"] = json!(true);
        input["base"]["hue"] = json!({"gray": hex_color()});
        input["base"]["text"]["stray"] = json!("#fff");
        let cleaned = theme_document(&input, "theme").unwrap();
        assert_eq!(cleaned.len(), 4);
        assert!(cleaned.contains_key("$schema"));
        assert!(!cleaned.contains_key("stray"));
        assert!(!cleaned["base"].as_object().unwrap().contains_key("hue"));
        assert!(!cleaned["base"]["text"].as_object().unwrap().contains_key("stray"));
    }

    #[test]
    fn requires_at_least_one_mode() {
        let mut input = document();
        input.as_object_mut().unwrap().remove("light");
        input.as_object_mut().unwrap().remove("dark");
        assert!(theme_document(&input, "theme").is_err());
    }

    #[test]
    fn requires_semantic_hues() {
        let mut input = document();
        input["light"]["hue"].as_object_mut().unwrap().remove("accent");
        assert!(theme_document(&input, "theme").is_err());
    }

    #[test]
    fn hue_scales_require_all_steps_and_strip_extras() {
        let mut scale = hex_color();
        scale["50"] = json!("#fff");
        scale.as_object_mut().unwrap().remove("900");
        let value = json!({ "gray": scale, "accent": "$hue.gray", "interactive": "$hue.gray", "neutral": "$hue.gray" });
        let error = hue_definition(&value, "hue").unwrap_err();
        assert!(error.0.contains("900"));
    }

    #[test]
    fn color_value_rules() {
        assert!(is_color_value(&json!("#abc")));
        assert!(is_color_value(&json!("#abcd")));
        assert!(is_color_value(&json!("#aabbcc")));
        assert!(is_color_value(&json!("#aabbccdd")));
        assert!(is_color_value(&json!("transparent")));
        assert!(is_color_value(&json!("$text.base")));
        assert!(!is_color_value(&json!("$")));
        assert!(!is_color_value(&json!("red")));
        assert!(!is_color_value(&json!("#12345")));
        assert!(is_hue_step_ref("$hue.gray.200"));
        assert!(!is_hue_step_ref("$hue.gray"));
        assert!(!is_hue_step_ref("$hue.gray.250"));
    }
}
