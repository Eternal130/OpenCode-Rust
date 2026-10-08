//! The theme pipeline: `parseThemeDocument` → `selectThemeMode` →
//! `expandTheme` → resolve into typed [`ResolvedTheme`] tokens.
//! Ported from `packages/theme/src/tui/resolve.ts` and `select.ts`.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value as Json};

use crate::color::Rgba;
use crate::expand::{expand_theme, merge_theme};
use crate::schema::{self, ThemeError};
use crate::types::{
    ActionColors, BackgroundFeedback, BackgroundFeedbackColors, BorderColor, DiffBackground,
    DiffHighlight, DiffLineNumber, DiffLineNumberBackground, DiffText, HueMap, HueScale,
    HueStepIndex, MarkdownColors, Mode, RaisedSurfaces, ResolvedBackground, ResolvedDiff,
    ResolvedText, ResolvedTheme, ResolvedThemeTokens, ScrollbarColor, StatefulColor, SyntaxColors,
    TextFeedback, TextFeedbackColors,
};

/// Format a decode/validation failure like TS `themeDecodeError`.
fn theme_decode_error(name: &str, error: &ThemeError) -> ThemeError {
    ThemeError(format!("Invalid theme: {name} {}", error.0))
}

/// A theme document: validated (via [`parse_theme_document`]) or produced by
/// the v1 migration. Holds the cleaned JSON object.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeDocument {
    value: Map<String, Json>,
}

impl ThemeDocument {
    /// Wrap a document without re-validating (used by the v1 migration, whose
    /// output is valid by construction — mirroring TS `migrateV1`).
    pub fn new(value: Map<String, Json>) -> Self {
        Self { value }
    }

    pub fn as_value(&self) -> Json {
        Json::Object(self.value.clone())
    }

    /// Modes the document provides, in `light`, `dark` order.
    pub fn modes(&self) -> Vec<Mode> {
        [Mode::Light, Mode::Dark]
            .into_iter()
            .filter(|mode| self.value.get(mode.as_str()).is_some_and(|v| !v.is_null()))
            .collect()
    }

    pub fn supports_mode(&self, mode: Mode) -> bool {
        self.modes().contains(&mode)
    }

    fn base(&self) -> &Map<String, Json> {
        self.value.get("base").and_then(Json::as_object).expect("document has a base definition")
    }

    fn mode_definition(&self, mode: Mode) -> Option<&Map<String, Json>> {
        self.value.get(mode.as_str()).and_then(Json::as_object)
    }
}

/// Parse and validate a theme document (TS `parseThemeDocument`): decodes with
/// the v2 schema rules and resolves every provided mode as validation.
pub fn parse_theme_document(input: &Json, name: &str) -> Result<ThemeDocument, ThemeError> {
    let cleaned =
        schema::theme_document(input, "theme").map_err(|error| theme_decode_error(name, &error))?;
    let document = ThemeDocument::new(cleaned);
    for mode in document.modes() {
        resolve_theme_document(&document, Some(mode))
            .map_err(|error| theme_decode_error(name, &error))?;
    }
    Ok(document)
}

/// Select a mode and merge `base` with it (TS `selectThemeMode`).
/// Falls back to the first available mode when the requested one is absent.
pub fn select_theme_mode(
    document: &ThemeDocument,
    mode: Option<Mode>,
) -> Result<(Map<String, Json>, Mode), ThemeError> {
    let requested = mode.unwrap_or(Mode::Light);
    let modes = document.modes();
    let selected_mode = if modes.contains(&requested) {
        requested
    } else {
        *modes.first().ok_or_else(|| ThemeError("Theme must provide at least one mode".into()))?
    };
    let selected = document
        .mode_definition(selected_mode)
        .ok_or_else(|| ThemeError("Theme must provide at least one mode".into()))?;
    let merged = merge_theme([document.base(), selected]);
    Ok((merged, selected_mode))
}

/// Resolve a document into theme tokens for a mode (TS `resolveThemeDocument`).
pub fn resolve_theme_document(
    document: &ThemeDocument,
    mode: Option<Mode>,
) -> Result<ResolvedTheme, ThemeError> {
    let (selected, _mode) = select_theme_mode(document, mode)?;
    let definition = expand_theme(&selected);
    resolve_expanded_theme(&definition)
}

/// Validate and resolve a standalone theme definition (TS `resolveTheme`).
pub fn resolve_theme(definition: &Json) -> Result<ResolvedTheme, ThemeError> {
    let cleaned = schema::theme_definition(definition, "theme")?;
    let expanded = expand_theme(&cleaned);
    resolve_expanded_theme(&expanded)
}

/// The seven token sections of a definition (TS `tokens()`).
fn tokens_of(definition: &Map<String, Json>) -> Map<String, Json> {
    let mut out = Map::new();
    for key in ["text", "background", "border", "scrollbar", "diff", "syntax", "markdown"] {
        if let Some(value) = definition.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    out
}

fn resolve_expanded_theme(definition: &Map<String, Json>) -> Result<ResolvedTheme, ThemeError> {
    let hue_source = definition
        .get("hue")
        .and_then(Json::as_object)
        .ok_or_else(|| ThemeError("Invalid theme: missing hue definition".into()))?;
    let hue = resolve_hue(hue_source)?;
    let categorical = categorical(&hue, definition.get("categorical"))?;
    let base = tokens_of(definition);
    let base_view = view(&base, &hue, &categorical)?;
    let dialog_view = match definition.get("@dialog").and_then(Json::as_object) {
        Some(dialog) => view(&contextualize(&base, dialog), &hue, &categorical)?,
        None => base_view.clone(),
    };
    Ok(ResolvedTheme { tokens: base_view, dialog: Box::new(dialog_view) })
}

fn categorical(hue: &HueMap, definition: Option<&Json>) -> Result<Vec<HueScale>, ThemeError> {
    let names = definition
        .and_then(Json::as_array)
        .ok_or_else(|| ThemeError("Invalid theme: missing categorical definition".into()))?;
    let mut scales = Vec::with_capacity(names.len());
    for name in names {
        let name = name.as_str().unwrap_or_default();
        let scale = hue
            .get(name)
            .ok_or_else(|| ThemeError(format!("Categorical hue \"{name}\" was not found")))?;
        scales.push(*scale);
    }
    Ok(scales)
}

/// Merge base tokens with a surface override and rebuild action colors
/// per-state (TS `contextualize` + `contextualActions`).
fn contextualize(base: &Map<String, Json>, surface: &Map<String, Json>) -> Map<String, Json> {
    let mut result = merge_theme([base, surface]);
    for section in ["text", "background"] {
        let base_section = base.get(section).and_then(Json::as_object);
        let surface_section = surface.get(section).and_then(Json::as_object);
        let merged = result.get(section).and_then(Json::as_object).cloned().unwrap_or_default();
        let mut next = merged;
        next.insert(
            "action".into(),
            Json::Object(contextual_actions(
                base_section.and_then(|s| s.get("action")).and_then(Json::as_object),
                surface_section.and_then(|s| s.get("action")).and_then(Json::as_object),
            )),
        );
        result.insert(section.into(), Json::Object(next));
    }
    result
}

fn contextual_actions(
    base: Option<&Map<String, Json>>,
    surface: Option<&Map<String, Json>>,
) -> Map<String, Json> {
    let lookup = |map: Option<&Map<String, Json>>, key: &str| -> Option<Json> {
        map?.get(key).filter(|v| !v.is_null()).cloned()
    };
    let mut out = Map::new();
    for variant in ["primary", "secondary", "destructive"] {
        let base_variant = lookup(base, variant).and_then(|v| v.as_object().cloned());
        let surface_variant = lookup(surface, variant).and_then(|v| v.as_object().cloned());
        let mut entry = Map::new();
        entry.insert(
            "base".into(),
            lookup(surface_variant.as_ref(), "base")
                .or_else(|| lookup(base_variant.as_ref(), "base"))
                .unwrap_or(Json::Null),
        );
        for state in schema::ACTION_STATES {
            let value = lookup(surface_variant.as_ref(), &format!("${state}"))
                .or_else(|| lookup(surface_variant.as_ref(), "base"))
                .or_else(|| lookup(base_variant.as_ref(), &format!("${state}")))
                .or_else(|| lookup(base_variant.as_ref(), "base"));
            entry.insert(state.into(), value.unwrap_or(Json::Null));
        }
        out.insert(variant.into(), Json::Object(entry));
    }
    out
}

/// Intermediate value tree while resolving token references.
#[derive(Debug, Clone)]
enum Token {
    Color(Rgba),
    /// Numbers pass through untouched (kept for parity with the TS resolver).
    Num(#[allow(dead_code)] f64),
    Str(String),
    Map(BTreeMap<String, Token>),
    Null,
}

fn token_from_json(value: &Json) -> Token {
    match value {
        Json::String(s) => Token::Str(s.clone()),
        Json::Number(n) => Token::Num(n.as_f64().unwrap_or_default()),
        Json::Object(map) => {
            Token::Map(map.iter().map(|(k, v)| (k.clone(), token_from_json(v))).collect())
        }
        Json::Null => Token::Null,
        other => Token::Str(other.to_string()),
    }
}

fn resolved_key(key: &str) -> String {
    match key.strip_prefix('$') {
        Some(state) if schema::ACTION_STATES.contains(&state) => state.to_string(),
        _ => key.to_string(),
    }
}

struct Resolver<'a> {
    source: &'a BTreeMap<String, Token>,
    cache: HashMap<String, Rgba>,
}

impl Resolver<'_> {
    fn resolve(
        &mut self,
        value: &Token,
        path: &str,
        stack: &mut Vec<String>,
    ) -> Result<Token, ThemeError> {
        match value {
            Token::Color(_) | Token::Num(_) => Ok(value.clone()),
            Token::Null => Err(ThemeError(format!("Invalid theme value at \"{path}\""))),
            Token::Str(s) => self.resolve_color(s, path, stack).map(Token::Color),
            Token::Map(map) => {
                let mut out = BTreeMap::new();
                for (key, item) in map {
                    out.insert(
                        resolved_key(key),
                        self.resolve(item, &format!("{path}.{key}"), stack)?,
                    );
                }
                Ok(Token::Map(out))
            }
        }
    }

    fn resolve_color(
        &mut self,
        value: &str,
        path: &str,
        stack: &mut Vec<String>,
    ) -> Result<Rgba, ThemeError> {
        if value == "transparent" {
            return Ok(Rgba::TRANSPARENT);
        }
        if schema::is_hex_color(value) {
            return Rgba::from_hex(value)
                .ok_or_else(|| ThemeError(format!("Invalid color \"{value}\" at \"{path}\"")));
        }
        let Some(target) = value.strip_prefix('$') else {
            return Err(ThemeError(format!("Invalid color \"{value}\" at \"{path}\"")));
        };
        if let Some(hit) = self.cache.get(target) {
            return Ok(*hit);
        }
        if stack.iter().any(|entry| entry == target) {
            let mut trail = stack.clone();
            trail.push(target.to_string());
            return Err(ThemeError(format!("Circular theme reference: {}", trail.join(" -> "))));
        }
        let raw = self.read_color(target)?;
        stack.push(target.to_string());
        let result = self.resolve(&raw, target, stack);
        stack.pop();
        match result? {
            Token::Color(color) => {
                self.cache.insert(target.to_string(), color);
                Ok(color)
            }
            _ => {
                Err(ThemeError(format!("Theme reference \"{value}\" at \"{path}\" is not a color")))
            }
        }
    }

    fn read_color(&self, path: &str) -> Result<Token, ThemeError> {
        let not_found = || ThemeError(format!("Theme reference \"${path}\" was not found"));
        if let Some(rest) = path.strip_prefix("hue.") {
            if let Some((name, step)) = rest.rsplit_once('.') {
                if !name.is_empty() && schema::HUE_STEPS.contains(&step) {
                    let color = self
                        .source
                        .get("hue")
                        .and_then(|hue| match hue {
                            Token::Map(map) => map.get(name),
                            _ => None,
                        })
                        .and_then(|scale| match scale {
                            Token::Map(map) => map.get(step),
                            _ => None,
                        });
                    return match color {
                        Some(Token::Color(color)) => Ok(Token::Color(*color)),
                        _ => Err(not_found()),
                    };
                }
            }
        }
        let mut current = self.source;
        let mut value: Option<&Token> = None;
        for key in path.split('.') {
            value = current.get(key);
            match value {
                Some(Token::Map(map)) => current = map,
                Some(other) => return Ok(other.clone()),
                None => return Err(not_found()),
            }
        }
        value.cloned().ok_or_else(not_found)
    }
}

/// Resolve a hue definition into color scales, following `$hue.<name>`
/// aliases with cycle detection (TS `resolveHue`).
pub fn resolve_hue(source: &Map<String, Json>) -> Result<HueMap, ThemeError> {
    let mut cache: BTreeMap<String, HueScale> = BTreeMap::new();
    let mut names: Vec<&String> = source.keys().collect();
    names.sort();
    for name in names {
        resolve_hue_entry(name, source, &mut cache, &mut Vec::new())?;
    }
    Ok(cache)
}

fn resolve_hue_entry(
    name: &str,
    source: &Map<String, Json>,
    cache: &mut BTreeMap<String, HueScale>,
    stack: &mut Vec<String>,
) -> Result<HueScale, ThemeError> {
    if let Some(hit) = cache.get(name) {
        return Ok(*hit);
    }
    if stack.iter().any(|entry| entry == name) {
        let mut trail = stack.clone();
        trail.push(name.to_string());
        return Err(ThemeError(format!("Circular hue reference: {}", trail.join(" -> "))));
    }
    let value =
        source.get(name).ok_or_else(|| ThemeError(format!("Hue \"{name}\" was not found")))?;
    let scale = if let Some(alias) = value.as_str() {
        let Some(target) = alias.strip_prefix("$hue.") else {
            return Err(ThemeError(format!("Hue alias \"{alias}\" must reference a hue scale")));
        };
        if source.get(target).is_none() {
            return Err(ThemeError(format!("Hue alias \"{alias}\" references a missing hue")));
        }
        stack.push(name.to_string());
        let resolved = resolve_hue_entry(target, source, cache, stack);
        stack.pop();
        resolved?
    } else {
        let map =
            value.as_object().ok_or_else(|| ThemeError(format!("Hue \"{name}\" was not found")))?;
        let mut steps = [Rgba::TRANSPARENT; 9];
        for (position, step) in steps.iter_mut().zip(schema::HUE_STEPS) {
            let color = map
                .get(step)
                .and_then(Json::as_str)
                .ok_or_else(|| ThemeError(format!("Invalid hue color at \"hue.{name}.{step}\"")))?;
            let rgba = Rgba::from_hex(color)
                .ok_or_else(|| ThemeError(format!("Invalid hue color at \"hue.{name}.{step}\"")))?;
            *position = rgba;
        }
        HueScale::from_steps(&steps)
    };
    cache.insert(name.to_string(), scale);
    Ok(scale)
}

impl HueScale {
    fn from_steps(steps: &[Rgba; 9]) -> Self {
        let [s100, s200, s300, s400, s500, s600, s700, s800, s900] = *steps;
        Self { s100, s200, s300, s400, s500, s600, s700, s800, s900 }
    }

    fn from_tokens(map: &BTreeMap<String, Token>) -> Result<Self, ThemeError> {
        let mut steps = [Rgba::TRANSPARENT; 9];
        for (position, step) in schema::HUE_STEPS.iter().enumerate() {
            steps[position] = take_color(map, step, &format!("hue scale {step}"))?;
        }
        Ok(Self::from_steps(&steps))
    }
}

fn take_color(map: &BTreeMap<String, Token>, key: &str, path: &str) -> Result<Rgba, ThemeError> {
    match map.get(key) {
        Some(Token::Color(color)) => Ok(*color),
        _ => Err(ThemeError(format!("Invalid theme value at \"{path}\": expected a color"))),
    }
}

fn take_map<'a>(
    map: &'a BTreeMap<String, Token>,
    key: &str,
    path: &str,
) -> Result<&'a BTreeMap<String, Token>, ThemeError> {
    map.get(key)
        .and_then(|value| match value {
            Token::Map(map) => Some(map),
            _ => None,
        })
        .ok_or_else(|| ThemeError(format!("Invalid theme value at \"{path}\": expected an object")))
}

fn take_stateful(map: &BTreeMap<String, Token>, path: &str) -> Result<StatefulColor, ThemeError> {
    Ok(StatefulColor {
        base: take_color(map, "base", path)?,
        hovered: take_color(map, "hovered", path)?,
        focused: take_color(map, "focused", path)?,
        pressed: take_color(map, "pressed", path)?,
        selected: take_color(map, "selected", path)?,
        disabled: take_color(map, "disabled", path)?,
    })
}

fn take_actions(map: &BTreeMap<String, Token>, path: &str) -> Result<ActionColors, ThemeError> {
    Ok(ActionColors {
        primary: take_stateful(take_map(map, "primary", path)?, &format!("{path}.primary"))?,
        secondary: take_stateful(take_map(map, "secondary", path)?, &format!("{path}.secondary"))?,
        destructive: take_stateful(
            take_map(map, "destructive", path)?,
            &format!("{path}.destructive"),
        )?,
    })
}

/// Resolve one token view (TS `resolveView`).
fn view(
    tokens: &Map<String, Json>,
    hue: &HueMap,
    categorical: &[HueScale],
) -> Result<ResolvedThemeTokens, ThemeError> {
    let mut source: BTreeMap<String, Token> = BTreeMap::new();
    source.insert(
        "hue".into(),
        Token::Map(
            hue.iter()
                .map(|(name, scale)| (name.clone(), Token::Map(scale_steps(scale))))
                .collect(),
        ),
    );
    for (key, value) in tokens {
        source.insert(key.clone(), token_from_json(value));
    }

    let mut resolver = Resolver { source: &source, cache: HashMap::new() };
    let resolved = resolver.resolve(&Token::Map(source.clone()), "theme", &mut Vec::new())?;
    let Token::Map(resolved) = resolved else {
        return Err(ThemeError("Invalid theme value at \"theme\"".into()));
    };

    let text = take_map(&resolved, "text", "theme.text")?;
    let background = take_map(&resolved, "background", "theme.background")?;
    let diff = take_map(&resolved, "diff", "theme.diff")?;
    let syntax = take_map(&resolved, "syntax", "theme.syntax")?;
    let markdown = take_map(&resolved, "markdown", "theme.markdown")?;
    let hue_tokens = take_map(&resolved, "hue", "theme.hue")?;

    let feedback_text = take_map(text, "feedback", "theme.text.feedback")?;
    let feedback_background = take_map(background, "feedback", "theme.background.feedback")?;
    let raised = take_map(background, "raised", "theme.background.raised")?;
    let diff_text = take_map(diff, "text", "theme.diff.text")?;
    let diff_background = take_map(diff, "background", "theme.diff.background")?;
    let diff_highlight = take_map(diff, "highlight", "theme.diff.highlight")?;
    let diff_line = take_map(diff, "lineNumber", "theme.diff.lineNumber")?;
    let diff_line_background =
        take_map(diff_line, "background", "theme.diff.lineNumber.background")?;

    let resolved_hue: HueMap = hue_tokens
        .iter()
        .map(|(name, scale)| -> Result<(String, HueScale), ThemeError> {
            let Token::Map(scale) = scale else {
                return Err(ThemeError(format!("Invalid theme value at \"theme.hue.{name}\"")));
            };
            Ok((name.clone(), HueScale::from_tokens(scale)?))
        })
        .collect::<Result<_, _>>()?;

    let tokens = ResolvedThemeTokens {
        hue: resolved_hue,
        categorical: categorical.to_vec(),
        hue_steps: HueStepIndex::new(hue),
        text: ResolvedText {
            base: take_color(text, "base", "theme.text.base")?,
            muted: take_color(text, "muted", "theme.text.muted")?,
            action: take_actions(
                take_map(text, "action", "theme.text.action")?,
                "theme.text.action",
            )?,
            formfield: take_stateful(
                take_map(text, "formfield", "theme.text.formfield")?,
                "theme.text.formfield",
            )?,
            feedback: TextFeedbackColors {
                error: text_feedback(feedback_text, "error")?,
                warning: text_feedback(feedback_text, "warning")?,
                success: text_feedback(feedback_text, "success")?,
                info: text_feedback(feedback_text, "info")?,
            },
        },
        background: ResolvedBackground {
            base: take_color(background, "base", "theme.background.base")?,
            raised: RaisedSurfaces {
                base: take_color(raised, "base", "theme.background.raised.base")?,
                high: take_color(raised, "high", "theme.background.raised.high")?,
                max: take_color(raised, "max", "theme.background.raised.max")?,
            },
            action: take_actions(
                take_map(background, "action", "theme.background.action")?,
                "theme.background.action",
            )?,
            formfield: take_stateful(
                take_map(background, "formfield", "theme.background.formfield")?,
                "theme.background.formfield",
            )?,
            feedback: BackgroundFeedbackColors {
                error: background_feedback(feedback_background, "error")?,
                warning: background_feedback(feedback_background, "warning")?,
                success: background_feedback(feedback_background, "success")?,
                info: background_feedback(feedback_background, "info")?,
            },
        },
        border: BorderColor {
            base: take_color(
                take_map(&resolved, "border", "theme.border")?,
                "base",
                "theme.border.base",
            )?,
        },
        scrollbar: ScrollbarColor {
            base: take_color(
                take_map(&resolved, "scrollbar", "theme.scrollbar")?,
                "base",
                "theme.scrollbar.base",
            )?,
        },
        diff: ResolvedDiff {
            text: DiffText {
                added: take_color(diff_text, "added", "theme.diff.text.added")?,
                removed: take_color(diff_text, "removed", "theme.diff.text.removed")?,
                context: take_color(diff_text, "context", "theme.diff.text.context")?,
                hunk_header: take_color(diff_text, "hunkHeader", "theme.diff.text.hunkHeader")?,
            },
            background: DiffBackground {
                added: take_color(diff_background, "added", "theme.diff.background.added")?,
                removed: take_color(diff_background, "removed", "theme.diff.background.removed")?,
                context: take_color(diff_background, "context", "theme.diff.background.context")?,
            },
            highlight: DiffHighlight {
                added: take_color(diff_highlight, "added", "theme.diff.highlight.added")?,
                removed: take_color(diff_highlight, "removed", "theme.diff.highlight.removed")?,
            },
            line_number: DiffLineNumber {
                text: take_color(diff_line, "text", "theme.diff.lineNumber.text")?,
                background: DiffLineNumberBackground {
                    added: take_color(
                        diff_line_background,
                        "added",
                        "theme.diff.lineNumber.background.added",
                    )?,
                    removed: take_color(
                        diff_line_background,
                        "removed",
                        "theme.diff.lineNumber.background.removed",
                    )?,
                },
            },
        },
        syntax: SyntaxColors {
            comment: take_color(syntax, "comment", "theme.syntax.comment")?,
            keyword: take_color(syntax, "keyword", "theme.syntax.keyword")?,
            function: take_color(syntax, "function", "theme.syntax.function")?,
            variable: take_color(syntax, "variable", "theme.syntax.variable")?,
            string: take_color(syntax, "string", "theme.syntax.string")?,
            number: take_color(syntax, "number", "theme.syntax.number")?,
            r#type: take_color(syntax, "type", "theme.syntax.type")?,
            operator: take_color(syntax, "operator", "theme.syntax.operator")?,
            punctuation: take_color(syntax, "punctuation", "theme.syntax.punctuation")?,
        },
        markdown: MarkdownColors {
            text: take_color(markdown, "text", "theme.markdown.text")?,
            heading: take_color(markdown, "heading", "theme.markdown.heading")?,
            link: take_color(markdown, "link", "theme.markdown.link")?,
            link_text: take_color(markdown, "linkText", "theme.markdown.linkText")?,
            code: take_color(markdown, "code", "theme.markdown.code")?,
            block_quote: take_color(markdown, "blockQuote", "theme.markdown.blockQuote")?,
            emphasis: take_color(markdown, "emphasis", "theme.markdown.emphasis")?,
            strong: take_color(markdown, "strong", "theme.markdown.strong")?,
            horizontal_rule: take_color(
                markdown,
                "horizontalRule",
                "theme.markdown.horizontalRule",
            )?,
            list_item: take_color(markdown, "listItem", "theme.markdown.listItem")?,
            list_enumeration: take_color(
                markdown,
                "listEnumeration",
                "theme.markdown.listEnumeration",
            )?,
            image: take_color(markdown, "image", "theme.markdown.image")?,
            image_text: take_color(markdown, "imageText", "theme.markdown.imageText")?,
            code_block: take_color(markdown, "codeBlock", "theme.markdown.codeBlock")?,
        },
    };
    Ok(tokens)
}

fn scale_steps(scale: &HueScale) -> BTreeMap<String, Token> {
    scale
        .steps()
        .iter()
        .enumerate()
        .map(|(position, color)| (schema::HUE_STEPS[position].to_string(), Token::Color(*color)))
        .collect()
}

fn text_feedback(map: &BTreeMap<String, Token>, kind: &str) -> Result<TextFeedback, ThemeError> {
    let entry = take_map(map, kind, &format!("theme.text.feedback.{kind}"))?;
    Ok(TextFeedback {
        base: take_color(entry, "base", &format!("theme.text.feedback.{kind}.base"))?,
        muted: take_color(entry, "muted", &format!("theme.text.feedback.{kind}.muted"))?,
    })
}

fn background_feedback(
    map: &BTreeMap<String, Token>,
    kind: &str,
) -> Result<BackgroundFeedback, ThemeError> {
    let entry = take_map(map, kind, &format!("theme.background.feedback.{kind}"))?;
    Ok(BackgroundFeedback {
        base: take_color(entry, "base", &format!("theme.background.feedback.{kind}.base"))?,
    })
}
