//! V1 → V2 theme migration via OKLCH hue inference, ported from
//! `packages/theme/src/tui/v1-migrate.ts`.
//!
//! The v1 flat token map is classified into seven chromatic hues plus a
//! neutral gray scale interpolated between the v1 text/background anchors;
//! matched colors are then rewritten as `$hue.<name>.<step>` references.

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Map, Value as Json};

use crate::color::{ints_to_oklch, oklch_to_hex, Oklch, Rgba};
use crate::resolve::ThemeDocument;
use crate::schema::ThemeError;
use crate::types::Mode;
use crate::v1::{resolve_v1, V1Theme};

/// Chromatic hue names, in the canonical order used for classification,
/// reference indexing and categorical ordering.
pub const CHROMATIC_HUES: [&str; 7] =
    ["red", "orange", "yellow", "green", "cyan", "blue", "purple"];

/// V1 tokens that feed the categorical palette, in selection order.
const CATEGORICAL_TOKENS: [&str; 7] =
    ["secondary", "accent", "success", "warning", "primary", "error", "info"];

const MINIMUM_CHROMA: f64 = 0.03;
const LIGHT_THRESHOLD: f64 = 0.6;

/// Canonical swatches the hue classifier measures against (light/dark).
const HUE_REFERENCES: [[&str; 7]; 2] = [
    ["#fca5a5", "#fdba74", "#fde047", "#86efac", "#67e8f9", "#93c5fd", "#d8b4fe"],
    ["#b91c1c", "#c2410c", "#a16207", "#15803d", "#0e7490", "#1d4ed8", "#7e22ce"],
];

/// OKLCH hue angles of [`HUE_REFERENCES`] per mode (`[light, dark]` rows).
fn hue_angles(mode: Mode) -> [f64; 7] {
    let row = usize::from(mode != Mode::Light);
    let mut angles = [0.0; 7];
    for (index, hex) in HUE_REFERENCES[row].iter().enumerate() {
        let color = Rgba::from_hex(hex).expect("reference swatch");
        let [r, g, b, _] = color.to_ints();
        angles[index] = ints_to_oklch(r, g, b).h;
    }
    angles
}

/// Migrate a v1 theme JSON into a v2 `ThemeDocument` (TS `migrateV1`).
pub fn migrate_v1(source: &Json) -> Result<ThemeDocument, ThemeError> {
    let light = resolve_v1(source, Mode::Light)?;
    let dark = resolve_v1(source, Mode::Dark)?;
    let light_background = light.color("background");
    let dark_background = dark.color("background");
    if light_background.a > 0 && dark_background.a > 0 && light_background == dark_background {
        let light_mode = detect_mode(&light);
        if light_mode == detect_mode(&dark) {
            let theme = if light_mode == Mode::Light { &light } else { &dark };
            let definition = migrate_mode(theme, light_mode);
            let mut document = Map::new();
            document.insert("base".into(), Json::Object(base_of(&definition)));
            document.insert(
                light_mode.as_str().into(),
                json!({ "hue": definition.get("hue").cloned().unwrap_or(Json::Null) }),
            );
            return Ok(ThemeDocument::new(document));
        }
    }
    let light_definition = migrate_mode(&light, Mode::Light);
    let dark_definition = migrate_mode(&dark, Mode::Dark);
    let mut document = Map::new();
    document.insert("base".into(), Json::Object(base_of(&light_definition)));
    document.insert(
        "light".into(),
        json!({ "hue": light_definition.get("hue").cloned().unwrap_or(Json::Null) }),
    );
    document.insert("dark".into(), Json::Object(dark_definition));
    Ok(ThemeDocument::new(document))
}

fn base_of(definition: &Map<String, Json>) -> Map<String, Json> {
    let mut out = definition.clone();
    out.remove("hue");
    out
}

fn detect_mode(theme: &V1Theme) -> Mode {
    if luminance(theme.color("text")) > luminance(theme.color("background")) {
        Mode::Dark
    } else {
        Mode::Light
    }
}

fn luminance(color: Rgba) -> f64 {
    let [r, g, b, _] = color.floats();
    0.299 * r + 0.587 * g + 0.114 * b
}

fn selected_foreground(theme: &V1Theme, background: Rgba) -> Rgba {
    if theme.has_selected_list_item_text {
        return theme.color("selectedListItemText");
    }
    let theme_background = theme.color("background");
    if theme_background.a != 0 {
        return theme_background;
    }
    if luminance(background) > 0.5 {
        Rgba::from_ints(0, 0, 0, 255)
    } else {
        Rgba::from_ints(255, 255, 255, 255)
    }
}

/// Build a complete v2 definition from a resolved v1 theme (TS `migrateMode`).
fn migrate_mode(theme: &V1Theme, mode: Mode) -> Map<String, Json> {
    let color = |token: &str| Json::String(theme.color(token).hex());
    let selected = Json::String(selected_foreground(theme, theme.color("primary")).hex());
    let destructive = Json::String(selected_foreground(theme, theme.color("error")).hex());
    let hues = infer_hues(theme);

    let mut categorical: Vec<String> = Vec::new();
    for token in CATEGORICAL_TOKENS {
        if let Some(index) = hues.by_token.get(token) {
            let name = CHROMATIC_HUES[*index].to_string();
            if !categorical.contains(&name) {
                categorical.push(name);
            }
        }
    }
    if categorical.is_empty() {
        categorical.push("neutral".into());
    }

    let text = "$hue.neutral.200";
    let text_muted = "$hue.neutral.400";
    let primary = "$hue.interactive.200";

    let mut hue = Map::new();
    hue.insert("gray".into(), Json::Object(neutral_scale(theme)));
    for (index, name) in CHROMATIC_HUES.iter().enumerate() {
        if let Some((color, _)) = hues.by_hue[index] {
            hue.insert((*name).into(), Json::Object(hue_scale(color, mode)));
        }
    }
    let alias = |token: &str| -> Json {
        match hues.by_token.get(token) {
            Some(index) => Json::String(format!("$hue.{}", CHROMATIC_HUES[*index])),
            None => Json::String("$hue.gray".into()),
        }
    };
    hue.insert("accent".into(), alias("accent"));
    hue.insert("interactive".into(), alias("primary"));
    hue.insert("neutral".into(), Json::String("$hue.gray".into()));

    let mut definition = Map::new();
    definition.insert("hue".into(), Json::Object(hue));
    definition.insert(
        "categorical".into(),
        Json::Array(categorical.into_iter().map(Json::String).collect()),
    );
    definition.insert(
        "text".into(),
        json!({
            "base": text,
            "muted": text_muted,
            "action": {
                "primary": { "base": "$text.base", "$disabled": text_muted, "$focused": selected, "$selected": primary },
                "secondary": { "base": "$text.muted", "$hovered": "$text.base" },
                "destructive": { "base": destructive, "$disabled": text_muted }
            },
            "formfield": {
                "base": text, "$hovered": primary, "$focused": primary, "$pressed": primary,
                "$disabled": text_muted, "$selected": primary
            },
            "feedback": {
                "error": { "base": color("error") },
                "warning": { "base": color("warning") },
                "success": { "base": color("success") },
                "info": { "base": color("info") }
            }
        }),
    );
    definition.insert(
        "background".into(),
        json!({
            "base": "$hue.neutral.800",
            "raised": { "base": "$hue.neutral.700", "high": "$hue.neutral.600", "max": "$hue.neutral.500" },
            "action": {
                "primary": { "base": "transparent", "$hovered": "$hue.neutral.700", "$focused": primary, "$selected": "transparent" },
                "secondary": { "base": "transparent" },
                "destructive": { "base": color("error") }
            },
            "formfield": { "base": "$background.base" },
            "feedback": {
                "error": { "base": "$background.base" },
                "warning": { "base": "$background.base" },
                "success": { "base": "$background.base" },
                "info": { "base": "$background.base" }
            }
        }),
    );
    definition.insert("border".into(), json!({ "base": color("border") }));
    definition.insert("scrollbar".into(), json!({ "base": color("borderActive") }));
    definition.insert(
        "diff".into(),
        json!({
            "text": {
                "added": color("diffAdded"), "removed": color("diffRemoved"),
                "context": color("diffContext"), "hunkHeader": color("diffHunkHeader")
            },
            "background": {
                "added": color("diffAddedBg"), "removed": color("diffRemovedBg"), "context": color("diffContextBg")
            },
            "highlight": { "added": color("diffHighlightAdded"), "removed": color("diffHighlightRemoved") },
            "lineNumber": {
                "text": color("diffLineNumber"),
                "background": { "added": color("diffAddedLineNumberBg"), "removed": color("diffRemovedLineNumberBg") }
            }
        }),
    );
    definition.insert(
        "syntax".into(),
        json!({
            "comment": color("syntaxComment"), "keyword": color("syntaxKeyword"), "function": color("syntaxFunction"),
            "variable": color("syntaxVariable"), "string": color("syntaxString"), "number": color("syntaxNumber"),
            "type": color("syntaxType"), "operator": color("syntaxOperator"), "punctuation": color("syntaxPunctuation")
        }),
    );
    definition.insert(
        "markdown".into(),
        json!({
            "text": color("markdownText"), "heading": color("markdownHeading"), "link": color("markdownLink"),
            "linkText": color("markdownLinkText"), "code": color("markdownCode"), "blockQuote": color("markdownBlockQuote"),
            "emphasis": color("markdownEmph"), "strong": color("markdownStrong"),
            "horizontalRule": color("markdownHorizontalRule"), "listItem": color("markdownListItem"),
            "listEnumeration": color("markdownListEnumeration"), "image": color("markdownImage"),
            "imageText": color("markdownImageText"), "codeBlock": color("markdownCodeBlock")
        }),
    );
    definition.insert(
        "@dialog".into(),
        json!({
            "background": {
                "base": "$background.raised.base",
                "action": { "primary": { "$hovered": "$background.raised.high" } }
            }
        }),
    );
    reference_hues(&definition)
}

/// Rewrite hex strings that appear in generated hue scales as
/// `$hue.<name>.<step>` references (TS `referenceHues`).
fn reference_hues(theme: &Map<String, Json>) -> Map<String, Json> {
    let Some(definitions) = theme.get("hue").and_then(Json::as_object) else {
        return theme.clone();
    };
    let mut scales: HashMap<&str, Map<String, Json>> = HashMap::new();
    let mut references: BTreeMap<String, String> = BTreeMap::new();

    for name in CHROMATIC_HUES {
        index_scale(name, false, definitions, &mut scales, &mut references);
    }
    index_scale("gray", false, definitions, &mut scales, &mut references);
    index_scale("accent", true, definitions, &mut scales, &mut references);
    index_scale("interactive", true, definitions, &mut scales, &mut references);
    index_scale("neutral", true, definitions, &mut scales, &mut references);

    theme
        .iter()
        .map(|(key, value)| {
            if key == "hue" || key == "categorical" {
                (key.clone(), value.clone())
            } else {
                (key.clone(), replace_reference(value, &references))
            }
        })
        .collect()
}

/// Replace hex strings with their `$hue.<name>.<step>` reference, recursing
/// into objects (arrays are left alone, matching TS `replace`).
fn replace_reference(value: &Json, references: &BTreeMap<String, String>) -> Json {
    match value {
        Json::String(s) => match references.get(&s.to_lowercase()) {
            Some(reference) => Json::String(reference.clone()),
            None => Json::String(s.clone()),
        },
        Json::Object(map) => Json::Object(
            map.iter().map(|(k, v)| (k.clone(), replace_reference(v, references))).collect(),
        ),
        other => other.clone(),
    }
}

/// Follow a hue entry (scale or `$hue.<name>` alias) to its scale.
fn resolve_scale<'a>(
    name: &'a str,
    definitions: &'a Map<String, Json>,
    scales: &mut HashMap<&'a str, Map<String, Json>>,
    chain: &mut Vec<&'a str>,
) -> Option<Map<String, Json>> {
    if let Some(cached) = scales.get(name) {
        return Some(cached.clone());
    }
    if chain.contains(&name) {
        return None;
    }
    let value = definitions.get(name)?;
    if !value.is_string() {
        let scale = value.as_object()?.clone();
        scales.insert(name, scale.clone());
        return Some(scale);
    }
    let target = value.as_str()?.strip_prefix("$hue.")?;
    if target.contains('.') || target.is_empty() {
        return None;
    }
    chain.push(name);
    let scale = resolve_scale(target, definitions, scales, chain);
    chain.pop();
    if let Some(scale) = &scale {
        scales.insert(name, scale.clone());
    }
    scale
}

/// Map every hex of a hue scale to `$hue.<name>.<step>`, optionally
/// overwriting colors claimed by an earlier hue.
fn index_scale<'a>(
    name: &'a str,
    overwrite: bool,
    definitions: &'a Map<String, Json>,
    scales: &mut HashMap<&'a str, Map<String, Json>>,
    references: &mut BTreeMap<String, String>,
) {
    let Some(scale) = resolve_scale(name, definitions, scales, &mut Vec::new()) else {
        return;
    };
    for step in crate::schema::HUE_STEPS {
        let Some(Json::String(color)) = scale.get(step) else { continue };
        let key = color.to_lowercase();
        if !overwrite && references.contains_key(&key) {
            continue;
        }
        references.insert(key, format!("$hue.{name}.{step}"));
    }
}

#[derive(Default)]
struct InferredHues {
    by_hue: [Option<(Rgba, f64)>; 7],
    by_token: BTreeMap<&'static str, usize>,
}

/// Classify v1 accent tokens into chromatic hues (TS `inferHues`).
fn infer_hues(theme: &V1Theme) -> InferredHues {
    let mut inferred = InferredHues::default();
    let first_pass: [(&'static str, Rgba); 7] = [
        ("accent", theme.color("accent")),
        ("success", theme.color("success")),
        ("warning", theme.color("warning")),
        ("primary", theme.color("primary")),
        ("error", theme.color("error")),
        ("info", theme.color("info")),
        ("secondary", theme.color("secondary")),
    ];
    for (token, color) in first_pass {
        let Some((index, distance)) = infer_hue(color) else { continue };
        let keep = inferred.by_hue[index].is_some_and(|(_, current)| current <= distance);
        if !keep {
            inferred.by_hue[index] = Some((color, distance));
        }
        inferred.by_token.insert(token, index);
    }
    for (token, color) in [("accent", theme.color("accent")), ("primary", theme.color("primary"))] {
        let Some((index, distance)) = infer_hue(color) else { continue };
        inferred.by_hue[index] = Some((color, distance));
        inferred.by_token.insert(token, index);
    }
    inferred
}

fn infer_hue(color: Rgba) -> Option<(usize, f64)> {
    let value = to_oklch(color);
    if ambiguous(color, value.c) {
        return None;
    }
    let reference =
        if value.l >= LIGHT_THRESHOLD { hue_angles(Mode::Light) } else { hue_angles(Mode::Dark) };
    let mut best: Option<(usize, f64)> = None;
    for (index, angle) in reference.iter().enumerate() {
        let distance = hue_distance(value.h, *angle);
        if best.is_none_or(|(_, current)| distance < current) {
            best = Some((index, distance));
        }
    }
    best
}

fn hue_distance(first: f64, second: f64) -> f64 {
    let difference = (first - second).abs();
    difference.min(360.0 - difference)
}

fn ambiguous(color: Rgba, chroma: f64) -> bool {
    color.a == 0 || chroma < MINIMUM_CHROMA
}

/// Generate a nine-step scale around a v1 color (TS `hueScale`).
fn hue_scale(color: Rgba, mode: Mode) -> Map<String, Json> {
    let value = to_oklch(color);
    let anchor: i32 = 200;
    let endpoint = if mode == Mode::Light { value.l.max(0.97) } else { value.l.min(0.18) };
    let alpha = color.a;
    let mut scale = Map::new();
    for step in [100, 200, 300, 400, 500, 600, 700, 800, 900] {
        let hex = if step == anchor {
            color.hex()
        } else {
            let progress = f64::from(step - 200) / f64::from(900 - 200);
            let generated = oklch_to_hex(&Oklch {
                l: value.l + (endpoint - value.l) * progress,
                c: value.c * (1.0 - progress * 0.5),
                h: value.h,
            });
            if alpha == 255 {
                generated
            } else {
                format!("{generated}{alpha:02x}")
            }
        };
        scale.insert(step.to_string(), Json::String(hex));
    }
    scale
}

/// The neutral scale, interpolated between v1 text/background anchors
/// (TS `neutralScale` + `neutralAnchors`).
fn neutral_scale(theme: &V1Theme) -> Map<String, Json> {
    // neutralAnchors builds a light-mode list and mirrors it (1000 - step).
    let anchors: [(i32, Rgba); 5] = [
        (200, theme.color("text")),
        (400, theme.color("textMuted")),
        (600, theme.color("backgroundElement")),
        (700, theme.color("backgroundPanel")),
        (800, theme.color("background")),
    ];
    let mut scale = Map::new();
    for step in [100, 200, 300, 400, 500, 600, 700, 800, 900] {
        let target = step;
        let hex = if let Some((_, color)) = anchors.iter().find(|(anchor, _)| *anchor == target) {
            color.hex()
        } else {
            let first = anchors[0];
            let last = anchors[anchors.len() - 1];
            let (lower, upper) = if target < first.0 {
                (anchors[0], anchors[1])
            } else if target > last.0 {
                (anchors[anchors.len() - 2], anchors[anchors.len() - 1])
            } else {
                let lower = anchors
                    .iter()
                    .rev()
                    .find(|(anchor, _)| *anchor < target)
                    .expect("bracketed anchor");
                let upper =
                    anchors.iter().find(|(anchor, _)| *anchor > target).expect("bracketed anchor");
                (*lower, *upper)
            };
            let amount = f64::from(target - lower.0) / f64::from(upper.0 - lower.0);
            interpolate(lower.1, upper.1, amount)
        };
        scale.insert(step.to_string(), Json::String(hex));
    }
    scale
}

/// Shortest-path OKLCH interpolation between two colors (TS `interpolate`).
fn interpolate(first: Rgba, second: Rgba, amount: f64) -> String {
    let start = to_oklch(first);
    let end = to_oklch(second);
    let start_hue = if start.h.is_finite() {
        start.h
    } else if end.h.is_finite() {
        end.h
    } else {
        0.0
    };
    let end_hue = if end.h.is_finite() { end.h } else { start_hue };
    let hue = ((end_hue - start_hue) % 360.0 + 540.0) % 360.0 - 180.0;
    let generated = oklch_to_hex(&Oklch {
        l: start.l + (end.l - start.l) * amount,
        c: start.c + (end.c - start.c) * amount,
        h: start_hue + hue * amount,
    });
    let alpha = (f64::from(first.a) + (f64::from(second.a) - f64::from(first.a)) * amount)
        .round()
        .clamp(0.0, 255.0) as u8;
    if alpha == 255 {
        generated
    } else {
        format!("{generated}{alpha:02x}")
    }
}

fn to_oklch(color: Rgba) -> Oklch {
    let [r, g, b, _] = color.to_ints();
    ints_to_oklch(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_angles_match_reference() {
        // Produced by the TS pipeline (`hueAngles` in v1-migrate.ts).
        let light = hue_angles(Mode::Light);
        let expected_light = [19.571, 66.29, 98.111, 154.449, 207.078, 251.813, 306.383];
        for (actual, expected) in light.iter().zip(expected_light) {
            assert!((actual - expected).abs() < 0.001, "{actual} vs {expected}");
        }
        let dark = hue_angles(Mode::Dark);
        let expected_dark = [27.518, 38.402, 66.442, 150.069, 223.128, 264.376, 301.924];
        for (actual, expected) in dark.iter().zip(expected_dark) {
            assert!((actual - expected).abs() < 0.001, "{actual} vs {expected}");
        }
    }

    #[test]
    fn hue_scale_anchors_color_at_200() {
        let scale = hue_scale(Rgba::from_ints(0xff, 0x55, 0x55, 255), Mode::Dark);
        assert_eq!(scale.get("200").unwrap(), "#ff5555");
        assert!(scale.get("100").unwrap().as_str().unwrap().starts_with('#'));
        let with_alpha = hue_scale(Rgba::from_ints(0xff, 0x55, 0x55, 0x80), Mode::Dark);
        assert_eq!(with_alpha.get("200").unwrap(), "#ff555580");
    }

    #[test]
    fn neutral_scale_uses_anchors_and_interpolation() {
        let mut colors = BTreeMap::new();
        let [r, g, b] = [0x20u8, 0x21, 0x30];
        for token in ["text", "textMuted", "backgroundElement", "backgroundPanel", "background"] {
            colors.insert(token.to_string(), Rgba::from_ints(r, g, b, 255));
        }
        let theme = V1Theme { colors, thinking_opacity: 0.6, has_selected_list_item_text: false };
        let scale = neutral_scale(&theme);
        assert_eq!(scale.get("200").unwrap(), "#202130");
        assert_eq!(scale.get("800").unwrap(), "#202130");
        // Identical anchors interpolate to themselves.
        for step in ["100", "300", "400", "500", "600", "700", "900"] {
            assert_eq!(scale.get(step).unwrap(), "#202130");
        }
    }
}
