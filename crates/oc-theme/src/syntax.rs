//! Syntax highlighting rules generated from a resolved theme
//! (`generateSyntax` in `packages/theme/src/tui/syntax.ts`), plus the
//! scope-flattening used to register them with the renderer
//! (`convertThemeToStyles` in `@opentui/core`).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::color::Rgba;
use crate::types::ResolvedThemeTokens;

/// The highlight step used for prompt/extmark accents.
const STEP: u16 = 200;

/// One scope-group → style rule (`ThemeTokenStyle`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyntaxRule {
    pub scope: &'static [&'static str],
    pub foreground: Rgba,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Rgba>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
}

fn rule(scope: &'static [&'static str], foreground: Rgba) -> SyntaxRule {
    SyntaxRule { scope, foreground, background: None, bold: None, italic: None, underline: None }
}

fn styled(
    scope: &'static [&'static str],
    foreground: Rgba,
    background: Option<Rgba>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
) -> SyntaxRule {
    SyntaxRule { scope, foreground, background, bold, italic, underline }
}

/// Generate the tree-sitter/markup highlighting rules for a theme.
pub fn generate_syntax(theme: &ResolvedThemeTokens) -> Vec<SyntaxRule> {
    let syntax = &theme.syntax;
    let markdown = &theme.markdown;
    let feedback = &theme.text.feedback;
    let accent = theme.semantic(crate::types::SemanticHue::Accent).step(STEP).expect("accent hue");
    let agent = theme.categorical[0].step(STEP).expect("categorical step");
    let skill = theme
        .categorical
        .get(1)
        .unwrap_or(&theme.categorical[0])
        .step(STEP)
        .expect("categorical step");

    vec![
        rule(&["default"], theme.text.base),
        rule(&["prompt"], accent),
        styled(&["extmark.file"], feedback.warning.base, None, Some(true), None, None),
        styled(&["extmark.agent"], agent, None, Some(true), None, None),
        styled(&["extmark.skill"], skill, None, Some(true), None, None),
        // V1 migration preserves its selected/inverse foreground in this action state.
        styled(
            &["extmark.paste"],
            theme.text.action.primary.focused,
            Some(feedback.warning.base),
            Some(true),
            None,
            None,
        ),
        styled(&["comment", "comment.documentation"], syntax.comment, None, None, Some(true), None),
        rule(&["string", "symbol", "character.special", "character"], syntax.string),
        rule(&["number", "boolean", "constant", "float"], syntax.number),
        styled(
            &["keyword.return", "keyword.conditional", "keyword.repeat", "keyword.coroutine"],
            syntax.keyword,
            None,
            None,
            Some(true),
            None,
        ),
        styled(&["keyword.type"], syntax.r#type, None, Some(true), Some(true), None),
        rule(&["keyword.function", "function.method"], syntax.function),
        styled(&["keyword"], syntax.keyword, None, None, Some(true), None),
        rule(
            &[
                "keyword.import",
                "string.escape",
                "string.regexp",
                "tag.attribute",
                "keyword.export",
            ],
            syntax.keyword,
        ),
        rule(
            &[
                "operator",
                "keyword.operator",
                "punctuation.delimiter",
                "keyword.conditional.ternary",
            ],
            syntax.operator,
        ),
        rule(
            &[
                "variable",
                "variable.parameter",
                "function.method.call",
                "function.call",
                "property",
                "parameter",
                "field",
            ],
            syntax.variable,
        ),
        rule(&["variable.member", "function", "constructor"], syntax.function),
        rule(&["type", "module", "class", "namespace"], syntax.r#type),
        styled(&["type.definition"], syntax.r#type, None, Some(true), None, None),
        rule(&["punctuation", "punctuation.bracket"], syntax.punctuation),
        rule(
            &[
                "variable.builtin",
                "type.builtin",
                "function.builtin",
                "module.builtin",
                "constant.builtin",
                "variable.super",
            ],
            feedback.error.base,
        ),
        styled(
            &["keyword.directive", "keyword.modifier", "keyword.exception"],
            syntax.keyword,
            None,
            None,
            Some(true),
            None,
        ),
        rule(&["punctuation.special", "tag.delimiter"], syntax.operator),
        styled(
            &[
                "markup.heading",
                "markup.heading.2",
                "markup.heading.3",
                "markup.heading.4",
                "markup.heading.5",
                "markup.heading.6",
            ],
            markdown.heading,
            None,
            Some(true),
            None,
            None,
        ),
        styled(&["markup.heading.1"], markdown.heading, None, Some(true), None, Some(true)),
        styled(&["markup.bold", "markup.strong"], markdown.strong, None, Some(true), None, None),
        styled(&["markup.italic"], markdown.emphasis, None, None, Some(true), None),
        rule(&["markup.list"], markdown.list_item),
        styled(&["markup.quote"], markdown.block_quote, None, None, Some(true), None),
        rule(&["markup.raw", "markup.raw.block"], markdown.code),
        styled(
            &["markup.raw.inline"],
            markdown.code,
            Some(theme.background.base),
            None,
            None,
            None,
        ),
        styled(
            &["markup.link", "markup.link.url", "string.special", "string.special.url"],
            markdown.link,
            None,
            None,
            None,
            Some(true),
        ),
        styled(&["markup.link.label"], markdown.link_text, None, None, None, Some(true)),
        rule(&["label"], markdown.link_text),
        rule(&["spell", "nospell"], theme.text.base),
        styled(&["markup.underline"], theme.text.base, None, None, None, Some(true)),
        styled(&["comment.error"], feedback.error.base, None, Some(true), Some(true), None),
        styled(&["comment.warning"], feedback.warning.base, None, Some(true), Some(true), None),
        styled(
            &["comment.todo", "comment.note"],
            feedback.info.base,
            None,
            Some(true),
            Some(true),
            None,
        ),
        rule(&["attribute", "annotation"], feedback.warning.base),
        rule(&["tag"], feedback.error.base),
        rule(&["markup.strikethrough", "markup.list.unchecked", "debug"], theme.text.muted),
        rule(&["markup.list.checked"], feedback.success.base),
        styled(
            &["diff.plus"],
            theme.diff.text.added,
            Some(theme.diff.background.added),
            None,
            None,
            None,
        ),
        styled(
            &["diff.minus"],
            theme.diff.text.removed,
            Some(theme.diff.background.removed),
            None,
            None,
            None,
        ),
        styled(
            &["diff.delta"],
            theme.diff.text.context,
            Some(theme.diff.background.context),
            None,
            None,
            None,
        ),
        styled(&["error"], feedback.error.base, None, Some(true), None, None),
        styled(&["warning"], feedback.warning.base, None, Some(true), None, None),
        rule(&["info"], feedback.info.base),
    ]
}

/// A flattened scope → style map (`StyleDefinition`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SyntaxStyleDef {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<Rgba>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<Rgba>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dim: Option<bool>,
}

/// Flatten rules into one style per scope (`convertThemeToStyles`).
/// Later rules overwrite earlier ones for the same scope.
pub fn convert_theme_to_styles(rules: &[SyntaxRule]) -> BTreeMap<String, SyntaxStyleDef> {
    let mut styles = BTreeMap::new();
    for rule in rules {
        let def = SyntaxStyleDef {
            fg: Some(rule.foreground),
            bg: rule.background,
            bold: rule.bold,
            italic: rule.italic,
            underline: rule.underline,
            dim: None,
        };
        for scope in rule.scope {
            styles.insert((*scope).to_string(), def);
        }
    }
    styles
}
