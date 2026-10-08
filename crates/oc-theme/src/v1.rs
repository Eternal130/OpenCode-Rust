//! V1 theme parsing: the flat ~60-token theme JSON used by opencode ≤ v1
//! (`packages/theme/src/tui/v1.ts`), including dark/light variants, `defs`
//! references and ANSI-256 palette numbers.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};

use crate::color::Rgba;
use crate::schema::ThemeError;
use crate::types::Mode;

/// A v1 theme resolved for one mode: token name → color, plus the two
/// synthesized fields the migration reads.
#[derive(Debug, Clone, PartialEq)]
pub struct V1Theme {
    pub colors: BTreeMap<String, Rgba>,
    pub thinking_opacity: f64,
    pub has_selected_list_item_text: bool,
}

impl V1Theme {
    pub fn color(&self, token: &str) -> Rgba {
        self.colors.get(token).copied().unwrap_or_else(|| panic!("v1 theme token {token} missing"))
    }
}

/// Resolve a `ThemeV1Json` value for a mode (TS `resolveV1`).
pub fn resolve_v1(source: &Json, mode: Mode) -> Result<V1Theme, ThemeError> {
    let invalid = |detail: &str| ThemeError(format!("Invalid v1 theme: {detail}"));
    let root = source.as_object().ok_or_else(|| invalid("expected an object"))?;
    let defs = root.get("defs").and_then(Json::as_object).cloned().unwrap_or_default();
    let theme = root
        .get("theme")
        .and_then(Json::as_object)
        .ok_or_else(|| invalid("missing \"theme\" object"))?;

    let mode_key = mode.as_str();
    let mut colors = BTreeMap::new();
    for (key, value) in theme {
        if key == "selectedListItemText" || key == "backgroundMenu" || key == "thinkingOpacity" {
            continue;
        }
        if let Some(color) = resolve_color(value, &defs, theme, mode_key, &mut Vec::new())? {
            colors.insert(key.clone(), color);
        }
    }
    let has_selected_list_item_text =
        theme.get("selectedListItemText").is_some_and(|v| !v.is_null());
    let selected = match theme.get("selectedListItemText") {
        Some(value) => resolve_color(value, &defs, theme, mode_key, &mut Vec::new())?,
        None => None,
    };
    let selected = match selected {
        Some(color) => color,
        None => *colors.get("background").ok_or_else(|| invalid("missing \"background\" token"))?,
    };
    colors.insert("selectedListItemText".into(), selected);
    let menu = match theme.get("backgroundMenu").filter(|v| !v.is_null()) {
        Some(value) => resolve_color(value, &defs, theme, mode_key, &mut Vec::new())?,
        None => None,
    };
    let menu = match menu {
        Some(color) => color,
        None => *colors
            .get("backgroundElement")
            .ok_or_else(|| invalid("missing \"backgroundElement\" token"))?,
    };
    colors.insert("backgroundMenu".into(), menu);
    let thinking_opacity = theme.get("thinkingOpacity").and_then(Json::as_f64).unwrap_or(0.6);
    Ok(V1Theme { colors, thinking_opacity, has_selected_list_item_text })
}

fn resolve_color(
    value: &Json,
    defs: &Map<String, Json>,
    theme: &Map<String, Json>,
    mode: &str,
    chain: &mut Vec<String>,
) -> Result<Option<Rgba>, ThemeError> {
    match value {
        Json::Null => Ok(None),
        Json::String(s) => {
            if s == "transparent" || s == "none" {
                return Ok(Some(Rgba::TRANSPARENT));
            }
            if s.starts_with('#') {
                // `RGBA.fromHex` falls back to magenta on malformed input.
                return Ok(Some(Rgba::from_hex(s).unwrap_or(Rgba::from_ints(255, 0, 255, 255))));
            }
            if chain.iter().any(|entry| entry == s) {
                let mut trail = chain.clone();
                trail.push(s.clone());
                return Err(ThemeError(format!(
                    "Circular color reference: {}",
                    trail.join(" -> ")
                )));
            }
            let next = defs.get(s).or_else(|| theme.get(s));
            let Some(next) = next else {
                return Err(ThemeError(format!(
                    "Color reference \"{s}\" not found in defs or theme"
                )));
            };
            chain.push(s.clone());
            let resolved = resolve_color(next, defs, theme, mode, chain);
            chain.pop();
            resolved
        }
        Json::Number(n) => Ok(Some(ansi(n.as_f64().unwrap_or_default() as i64))),
        Json::Object(variant) => match variant.get(mode).filter(|v| !v.is_null()) {
            Some(value) => resolve_color(value, defs, theme, mode, chain),
            None => Err(ThemeError("Invalid V1 theme color".into())),
        },
        _ => Err(ThemeError("Invalid V1 theme color".into())),
    }
}

/// ANSI palette lookup (TS `ansi`): 16-color table, 6×6×6 cube, grayscale.
pub fn ansi(code: i64) -> Rgba {
    if !(0..256).contains(&code) {
        return Rgba::from_ints(0, 0, 0, 255);
    }
    if code < 16 {
        // ANSI-16 palette, ported verbatim from the TS lookup table.
        #[allow(clippy::unreadable_literal)]
        const TABLE: [u32; 16] = [
            0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xc0c0c0,
            0x808080, 0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
        ];
        let value = TABLE[code as usize];
        let channel = |shift: u32| ((value >> shift) & 0xff) as u8;
        return Rgba::from_ints(channel(16), channel(8), channel(0), 255);
    }
    if code < 232 {
        let index = code - 16;
        let value = |part: i64| if part == 0 { 0 } else { part * 40 + 55 } as u8;
        return Rgba::from_ints(value(index / 36), value((index / 6) % 6), value(index % 6), 255);
    }
    let gray = ((code - 232) * 10 + 8).clamp(0, 255) as u8;
    Rgba::from_ints(gray, gray, gray, 255)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_refs_variants_and_numbers() {
        let source = json!({
            "defs": { "bg": "#282a36", "fg": "bg" },
            "theme": {
                "background": { "dark": "bg", "light": "#ffffff" },
                "backgroundElement": "#333333",
                "text": { "dark": "fg", "light": "#000000" },
                "border": 244,
                "error": "#ff5555"
            }
        });
        let dark = resolve_v1(&source, Mode::Dark).unwrap();
        assert_eq!(dark.color("background"), Rgba::from_ints(0x28, 0x2a, 0x36, 255));
        assert_eq!(dark.color("text"), Rgba::from_ints(0x28, 0x2a, 0x36, 255));
        assert_eq!(dark.color("border"), Rgba::from_ints(0x80, 0x80, 0x80, 255));
        let light = resolve_v1(&source, Mode::Light).unwrap();
        assert_eq!(light.color("background"), Rgba::from_ints(255, 255, 255, 255));
        assert_eq!(light.thinking_opacity, 0.6);
        assert!(!light.has_selected_list_item_text);
    }

    #[test]
    fn synthesized_tokens_default() {
        let source = json!({
            "theme": {
                "background": "#101010",
                "backgroundElement": "#202020",
                "text": "#ffffff",
                "primary": "#ff0000"
            }
        });
        let theme = resolve_v1(&source, Mode::Dark).unwrap();
        assert_eq!(theme.color("selectedListItemText"), theme.color("background"));
        assert_eq!(theme.color("backgroundMenu"), theme.color("backgroundElement"));
    }

    #[test]
    fn circular_refs_error() {
        let source = json!({ "theme": { "background": "text", "text": "background" } });
        assert!(resolve_v1(&source, Mode::Dark).is_err());
    }

    #[test]
    fn ansi_matches_known_codes() {
        assert_eq!(ansi(1), Rgba::from_ints(0x80, 0, 0, 255));
        assert_eq!(ansi(16), Rgba::from_ints(0, 0, 0, 255));
        assert_eq!(ansi(21), Rgba::from_ints(0, 0, 255, 255));
        assert_eq!(ansi(232), Rgba::from_ints(8, 8, 8, 255));
        assert_eq!(ansi(255), Rgba::from_ints(238, 238, 238, 255));
    }
}
