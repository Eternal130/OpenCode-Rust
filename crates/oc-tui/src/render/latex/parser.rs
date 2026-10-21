//! The TeX-subset parser (upstream `packages/latex/src/parser.ts`).
//!
//! Recursive descent over `Vec<char>`: rows, groups, commands,
//! environments (matrix family), `\left…\right`, scripts, macro
//! expansion, and depth/length guards.

use std::collections::HashMap;

use crate::render::latex::symbols::{
    lookup_delimiter, lookup_large_operator, lookup_named_operator, lookup_spacing, lookup_symbol,
};
use crate::render::latex::types::{
    assert_nesting_depth, assert_source_length, latex_error, resolve_positive_integer, AccentKind,
    BracePosition, LatexError, MathNode, MathVariant, MatrixEnvironment, NumeratorAlign,
    ParseOptions, DEFAULT_MAX_NESTING_DEPTH, DEFAULT_MAX_SOURCE_LENGTH,
};

/// `parseLatex`.
pub fn parse_latex(source: &str, options: &ParseOptions) -> Result<MathNode, LatexError> {
    let expanded = expand_latex_macros(source, options)?;
    let max_depth =
        resolve_positive_integer(options.max_depth, DEFAULT_MAX_NESTING_DEPTH, "maxDepth")?;
    Parser::new(expanded, options.strict, max_depth).parse()
}

/// `expandLatexMacros`.
pub fn expand_latex_macros(source: &str, options: &ParseOptions) -> Result<Vec<char>, LatexError> {
    let max_source_length = resolve_positive_integer(
        options.max_source_length,
        DEFAULT_MAX_SOURCE_LENGTH,
        "maxSourceLength",
    )?;
    let max_expanded_length = resolve_positive_integer(
        options.max_expanded_length,
        max_source_length,
        "maxExpandedLength",
    )?;
    let max_expand = resolve_positive_integer(options.max_expand, 100, "maxExpand")?;
    let max_depth =
        resolve_positive_integer(options.max_depth, DEFAULT_MAX_NESTING_DEPTH, "maxDepth")?;
    let chars: Vec<char> = source.chars().collect();
    assert_source_length(source, max_source_length, "LaTeX source")?;
    assert_nesting_depth(&chars, max_depth)?;
    let expanded = expand_macros(&chars, options.macros.as_ref(), max_expand, max_expanded_length)?;
    if expanded != chars {
        assert_nesting_depth(&expanded, max_depth)?;
    }
    Ok(expanded)
}

/// `expandMacros` — pass-limited `\command` substitution. Commands are
/// `\[A-Za-z@]+` or `\<any single char>`; macro keys match with or
/// without the leading backslash.
fn expand_macros(
    source: &[char],
    macros: Option<&HashMap<String, String>>,
    max_expand: u32,
    max_expanded_length: u32,
) -> Result<Vec<char>, LatexError> {
    assert_source_length_chars(source, max_expanded_length, "Expanded LaTeX source")?;
    let macros = match macros {
        Some(macros) if !macros.is_empty() => macros,
        _ => return Ok(source.to_vec()),
    };

    let mut result = source.to_vec();
    for _ in 0..max_expand {
        let mut changed = false;
        let mut cursor = 0usize;
        let mut output_length = 0usize;
        let mut output: Vec<char> = Vec::new();

        let mut index = 0usize;
        while index < result.len() {
            if result[index] != '\\' {
                index += 1;
                continue;
            }
            // \[A-Za-z@]+ | \.
            let mut end = index + 1;
            if end < result.len() && (result[end].is_ascii_alphabetic() || result[end] == '@') {
                while end < result.len()
                    && (result[end].is_ascii_alphabetic() || result[end] == '@')
                {
                    end += 1;
                }
            } else if end < result.len() {
                end += 1;
            } else {
                // trailing backslash: "\." with no char — upstream `\\.` needs a char
                index += 1;
                continue;
            }
            let command: String = result[index..end].iter().collect();
            let replacement = macros.get(&command).or_else(|| macros.get(&command[1..]));
            let Some(replacement) = replacement else {
                index = end;
                continue;
            };

            let skipped = &result[cursor..index];
            append_within_limit(&mut output, skipped, output_length, max_expanded_length)?;
            output_length += index - cursor;
            let replacement: Vec<char> = replacement.chars().collect();
            append_within_limit(&mut output, &replacement, output_length, max_expanded_length)?;
            output_length += replacement.len();
            cursor = end;
            index = end;
            changed = true;
        }

        if !changed {
            return Ok(result);
        }
        append_within_limit(&mut output, &result[cursor..], output_length, max_expanded_length)?;
        result = output;
    }

    Err(latex_error(format!("Macro expansion exceeded {max_expand} passes"), 0))
}

fn assert_source_length_chars(
    source: &[char],
    maximum: u32,
    label: &str,
) -> Result<(), LatexError> {
    if source.len() > maximum as usize {
        return Err(latex_error(
            format!("{label} exceeds the {maximum}-character limit"),
            maximum as usize,
        ));
    }
    Ok(())
}

fn append_within_limit(
    output: &mut Vec<char>,
    value: &[char],
    current_length: usize,
    maximum: u32,
) -> Result<(), LatexError> {
    if current_length + value.len() > maximum as usize {
        return Err(latex_error(
            format!("Expanded LaTeX source exceeds the {maximum}-character limit"),
            maximum as usize,
        ));
    }
    output.extend_from_slice(value);
    Ok(())
}

/// The accents table.
fn accent_kind(command: &str) -> Option<AccentKind> {
    Some(match command {
        "hat" => AccentKind::Hat,
        "widehat" => AccentKind::WideHat,
        "bar" => AccentKind::Bar,
        "overline" => AccentKind::Overline,
        "underline" => AccentKind::Underline,
        "vec" => AccentKind::Vec,
        "tilde" | "widetilde" => AccentKind::Tilde,
        "dot" => AccentKind::Dot,
        "ddot" => AccentKind::DoubleDot,
        _ => return None,
    })
}

/// The variant commands.
fn variant_kind(command: &str) -> Option<MathVariant> {
    Some(match command {
        "mathrm" | "textrm" | "mathnormal" => MathVariant::Normal,
        "mathbf" | "boldsymbol" | "bm" => MathVariant::Bold,
        "mathit" => MathVariant::Italic,
        "mathsf" => MathVariant::Sans,
        "mathtt" => MathVariant::Monospace,
        "mathbb" => MathVariant::DoubleStruck,
        "mathcal" | "mathscr" => MathVariant::Script,
        "mathfrak" => MathVariant::Fraktur,
        _ => return None,
    })
}

struct Parser {
    source: Vec<char>,
    position: usize,
    depth: u32,
    strict: bool,
    max_depth: u32,
}

impl Parser {
    fn new(source: Vec<char>, strict: bool, max_depth: u32) -> Self {
        Self { source, position: 0, depth: 0, strict, max_depth }
    }

    fn parse(mut self) -> Result<MathNode, LatexError> {
        let body = self.parse_row(None)?;
        self.skip_math_whitespace();
        if !self.done() {
            return Err(Self::fail(format!("Unexpected \"{}\"", self.peek()), self.position));
        }
        Ok(row(body))
    }

    /// `parseRow(stop?)` — `stop` is checked after whitespace.
    fn parse_row(
        &mut self,
        stop: Option<&dyn Fn(&Parser) -> bool>,
    ) -> Result<Vec<MathNode>, LatexError> {
        let mut body = Vec::new();
        while !self.done() {
            self.skip_math_whitespace();
            if self.done() {
                break;
            }
            if let Some(stop) = stop {
                if stop(self) {
                    break;
                }
            }
            let current = self.peek();
            if current == '}' {
                break;
            }
            if current == '^' || current == '_' {
                self.position += 1;
                let script = self.parse_argument()?;
                let previous = body.pop().unwrap_or_else(|| MathNode::Row(Vec::new()));
                let mut existing = match previous {
                    MathNode::Scripts { base, superscript, subscript } => {
                        MathNode::Scripts { base, superscript, subscript }
                    }
                    base => MathNode::Scripts {
                        base: Box::new(base),
                        superscript: None,
                        subscript: None,
                    },
                };
                if let MathNode::Scripts { superscript, subscript, .. } = &mut existing {
                    if current == '^' {
                        *superscript = Some(Box::new(script));
                    } else {
                        *subscript = Some(Box::new(script));
                    }
                }
                body.push(existing);
                continue;
            }
            if current == '\\' && self.apply_limits_modifier(&mut body) {
                continue;
            }
            body.push(self.parse_atom()?);
        }
        Ok(body)
    }

    fn parse_atom(&mut self) -> Result<MathNode, LatexError> {
        self.depth += 1;
        if self.depth > self.max_depth {
            self.depth -= 1;
            return Err(Self::fail(
                format!("LaTeX nesting exceeds the {}-level limit", self.max_depth),
                self.position,
            ));
        }
        let result = self.parse_atom_inner();
        self.depth -= 1;
        result
    }

    fn parse_atom_inner(&mut self) -> Result<MathNode, LatexError> {
        let current = self.peek();
        if current == '{' {
            return self.parse_group();
        }
        if current == '\\' {
            return self.parse_command();
        }
        if current == '~' {
            self.position += 1;
            return Ok(MathNode::Space(1));
        }
        self.position += 1;
        Ok(MathNode::Symbol { value: current.to_string(), role: Some(infer_role(current)) })
    }

    #[allow(clippy::too_many_lines)]
    fn parse_command(&mut self) -> Result<MathNode, LatexError> {
        let start = self.position;
        let command = self.read_command();

        if command == "\\" {
            return Ok(MathNode::Row(Vec::new()));
        }
        if command == "begin" {
            return self.parse_environment();
        }
        if matches!(command.as_str(), "frac" | "dfrac" | "tfrac" | "cfrac") {
            self.skip_math_whitespace();
            let mut numerator_align = None;
            if command == "cfrac" && self.peek() == '[' {
                // /^\[([lr]?)\]/
                let mut end = self.position + 1;
                let align = if end < self.source.len()
                    && (self.source[end] == 'l' || self.source[end] == 'r')
                {
                    let align = self.source[end];
                    end += 1;
                    Some(align)
                } else {
                    None
                };
                if end < self.source.len() && self.source[end] == ']' {
                    self.position = end + 1;
                    numerator_align = align.map(|align| {
                        if align == 'l' {
                            NumeratorAlign::Left
                        } else {
                            NumeratorAlign::Right
                        }
                    });
                } else {
                    return Err(Self::fail(
                        "Unsupported \\cfrac alignment; expected [l], [r], or []",
                        self.position,
                    ));
                }
            }
            let numerator = self.parse_argument()?;
            let denominator = self.parse_argument()?;
            return Ok(MathNode::Fraction {
                numerator: Box::new(numerator),
                denominator: Box::new(denominator),
                bar: true,
                numerator_align,
            });
        }
        if matches!(command.as_str(), "binom" | "dbinom" | "tbinom") {
            let numerator = self.parse_argument()?;
            let denominator = self.parse_argument()?;
            let fraction = MathNode::Fraction {
                numerator: Box::new(numerator),
                denominator: Box::new(denominator),
                bar: false,
                numerator_align: None,
            };
            return Ok(MathNode::Delimited {
                left: "(".to_string(),
                body: Box::new(fraction),
                right: ")".to_string(),
            });
        }
        if command == "sqrt" {
            let index = self.parse_optional_argument()?;
            let body = self.parse_argument()?;
            return Ok(MathNode::Root { body: Box::new(body), index: index.map(Box::new) });
        }
        if command == "left" {
            return self.parse_left_right();
        }
        if command == "middle" {
            return Ok(MathNode::Symbol { value: self.read_delimiter()?, role: None });
        }
        if command == "right" {
            self.position = start;
            return Err(Self::fail("Unexpected \\right", self.position));
        }
        if let Some(accent) = accent_kind(&command) {
            return Ok(MathNode::Accent { accent, body: Box::new(self.parse_argument()?) });
        }
        if let Some(variant) = variant_kind(&command) {
            let body = if command == "textrm" {
                MathNode::Text(self.read_text_group()?)
            } else {
                self.parse_argument()?
            };
            return Ok(MathNode::Variant { variant, body: Box::new(body) });
        }
        if command == "text" || command == "mbox" {
            return Ok(MathNode::Text(self.read_text_group()?));
        }
        if command == "operatorname" {
            let limits = self.peek() == '*';
            if limits {
                self.position += 1;
            }
            return Ok(MathNode::Operator { value: self.read_text_group()?, limits });
        }
        if command == "overset" || command == "stackrel" {
            let over = self.parse_argument()?;
            let base = self.parse_argument()?;
            return Ok(MathNode::OverUnder {
                base: Box::new(base),
                over: Some(Box::new(over)),
                under: None,
            });
        }
        if command == "underset" {
            let under = self.parse_argument()?;
            let base = self.parse_argument()?;
            return Ok(MathNode::OverUnder {
                base: Box::new(base),
                over: None,
                under: Some(Box::new(under)),
            });
        }
        if command == "overbrace" || command == "underbrace" {
            return Ok(MathNode::Brace {
                body: Box::new(self.parse_argument()?),
                position: if command == "overbrace" {
                    BracePosition::Over
                } else {
                    BracePosition::Under
                },
            });
        }
        if command == "textcolor" {
            let color = self.read_raw_group()?;
            return Ok(MathNode::Color { color, body: Box::new(self.parse_argument()?) });
        }
        if command == "color" {
            let color = self.read_raw_group()?;
            return Ok(MathNode::Color { color, body: Box::new(row(self.parse_row(None)?)) });
        }
        if command == "not" {
            let target = self.parse_atom()?;
            if let MathNode::Symbol { value, role } = target {
                return Ok(MathNode::Symbol { value: negate_symbol(&value), role });
            }
            return Ok(MathNode::Row(vec![
                MathNode::Symbol { value: "¬".to_string(), role: None },
                target,
            ]));
        }
        if command == "pmod" {
            return Ok(MathNode::Row(vec![
                MathNode::Space(1),
                MathNode::Text("(mod ".to_string()),
                self.parse_argument()?,
                MathNode::Text(")".to_string()),
            ]));
        }
        if command == "mod" || command == "bmod" {
            return Ok(MathNode::Operator { value: "mod".to_string(), limits: false });
        }
        if command == "displaylines" {
            self.skip_math_whitespace();
            self.expect("{")?;
            return self.parse_matrix(MatrixEnvironment::Gathered, "}", None);
        }
        if matches!(
            command.as_str(),
            "limits"
                | "nolimits"
                | "displaystyle"
                | "textstyle"
                | "scriptstyle"
                | "scriptscriptstyle"
        ) {
            return Ok(MathNode::Row(Vec::new()));
        }
        // /^(?:big|Big|bigg|Bigg)[lrm]?$/
        if is_big_delimiter_command(&command) {
            return Ok(MathNode::Symbol { value: self.read_delimiter()?, role: None });
        }
        if let Some(width) = lookup_spacing(&command) {
            return Ok(MathNode::Space(width));
        }
        if let Some(symbol) = lookup_symbol(&command) {
            return Ok(MathNode::Symbol {
                value: symbol.value.to_string(),
                role: Some(symbol.role),
            });
        }
        if let Some((value, limits)) = lookup_large_operator(&command) {
            return Ok(MathNode::Operator { value: value.to_string(), limits });
        }
        if let Some((value, limits)) = lookup_named_operator(&command) {
            return Ok(MathNode::Operator { value, limits });
        }
        if command == "backslash" {
            return Ok(MathNode::Symbol { value: "\\".to_string(), role: None });
        }
        let backslash_key = format!("\\{command}");
        if let Some(delimiter) =
            lookup_delimiter(&backslash_key).or_else(|| lookup_delimiter(&command))
        {
            return Ok(MathNode::Symbol { value: delimiter.to_string(), role: None });
        }
        if command == "{" || command == "}" {
            return Ok(MathNode::Symbol { value: command.clone(), role: None });
        }
        if matches!(command.as_str(), "%" | "#" | "$" | "&" | "_") {
            return Ok(MathNode::Symbol { value: command.clone(), role: None });
        }
        if self.strict {
            return Err(Self::fail(format!("Unsupported command \\{command}"), start));
        }
        Ok(MathNode::Text(format!("\\{command}")))
    }

    fn parse_environment(&mut self) -> Result<MathNode, LatexError> {
        let raw_environment = self.read_raw_group()?;
        let unstarred: String =
            raw_environment.strip_suffix('*').unwrap_or(&raw_environment).to_string();
        let Some(environment) = MatrixEnvironment::from_name(&unstarred) else {
            if self.strict {
                return Err(
                    Self::fail(format!("Unsupported environment {unstarred}"), self.position)
                );
            }
            let content = self.read_until_end(&raw_environment)?;
            return Ok(MathNode::Text(content));
        };
        let columns = if environment == MatrixEnvironment::Array {
            let raw = self.read_raw_group()?;
            Some(raw.chars().filter(|char| !char.is_whitespace()).collect::<String>())
        } else {
            None
        };
        if let Some(columns) = &columns {
            let has_alignment = columns.chars().any(|char| matches!(char, 'l' | 'c' | 'r'));
            let valid = columns.chars().all(|char| matches!(char, 'l' | 'c' | 'r' | '|'));
            if !valid || !has_alignment {
                return Err(
                    Self::fail("Unsupported array columns; expected l, c, r, and |", self.position)
                );
            }
        }
        let end = format!("\\end{{{raw_environment}}}");
        self.parse_matrix(environment, &end, columns)
    }

    fn parse_matrix(
        &mut self,
        environment: MatrixEnvironment,
        end: &str,
        columns: Option<String>,
    ) -> Result<MathNode, LatexError> {
        let end: Vec<char> = end.chars().collect();
        let mut rows: Vec<Vec<MathNode>> = Vec::new();
        let mut cells: Vec<MathNode> = Vec::new();

        let at_end = |parser: &Self| starts_with_at(&parser.source, &end, parser.position);
        let cell_stop = |parser: &Self| {
            parser.peek() == '&'
                || starts_with_at(&parser.source, &['\\', '\\'], parser.position)
                || starts_with_at(&parser.source, &end, parser.position)
        };

        while !self.done() {
            self.skip_math_whitespace();
            if at_end(self) && cells.is_empty() {
                break;
            }
            let cell_start = self.position;
            let cell = row(self.parse_row(Some(&cell_stop))?);
            cells.push(cell);
            self.skip_math_whitespace();

            if self.peek() == '&' {
                self.position += 1;
                continue;
            }
            if starts_with_at(&self.source, &['\\', '\\'], self.position) {
                self.position += 2;
                self.consume_optional_bracket();
                rows.push(std::mem::take(&mut cells));
                continue;
            }
            if at_end(self) {
                break;
            }
            // Empty cells are valid only when a cell, row, or closing delimiter advances the parser.
            if self.position == cell_start {
                return Err(Self::fail(
                    format!("Unexpected \"{}\" in {}", self.peek(), environment_name(environment)),
                    self.position,
                ));
            }
        }

        if !at_end(self) {
            let end_text: String = end.iter().collect();
            return Err(Self::fail(format!("Missing {end_text}"), self.position));
        }
        self.expect_chars(&end)?;
        if !cells.is_empty() || rows.is_empty() {
            rows.push(cells);
        }
        Ok(MathNode::Matrix { rows, environment, columns })
    }

    fn parse_left_right(&mut self) -> Result<MathNode, LatexError> {
        let left = self.read_delimiter()?;
        let at_right = |parser: &Self| {
            starts_with_at(&parser.source, &['\\', 'r', 'i', 'g', 'h', 't'], parser.position)
                && !parser
                    .source
                    .get(parser.position + 6)
                    .is_some_and(|char| char.is_ascii_alphabetic() || *char == '@')
        };
        let body = row(self.parse_row(Some(&at_right))?);
        if !at_right(self) {
            return Err(Self::fail("Missing \\right", self.position));
        }
        self.read_command();
        let right = self.read_delimiter()?;
        Ok(MathNode::Delimited { left, body: Box::new(body), right })
    }

    fn parse_argument(&mut self) -> Result<MathNode, LatexError> {
        self.skip_math_whitespace();
        if self.peek() == '{' {
            return self.parse_group();
        }
        if self.done() {
            return Err(Self::fail("Expected an argument", self.position));
        }
        self.parse_atom()
    }

    fn parse_group(&mut self) -> Result<MathNode, LatexError> {
        self.expect("{")?;
        let body = row(self.parse_row(None)?);
        self.expect("}")?;
        Ok(body)
    }

    fn parse_optional_argument(&mut self) -> Result<Option<MathNode>, LatexError> {
        self.skip_math_whitespace();
        if self.peek() != '[' {
            return Ok(None);
        }
        self.position += 1;
        let stop = |parser: &Self| parser.peek() == ']';
        let body = row(self.parse_row(Some(&stop))?);
        self.expect("]")?;
        Ok(Some(body))
    }

    fn consume_optional_bracket(&mut self) {
        self.skip_math_whitespace();
        if self.peek() != '[' {
            return;
        }
        let mut depth = 0i32;
        while !self.done() {
            let char = self.source[self.position];
            self.position += 1;
            if char == '[' {
                depth += 1;
            }
            if char == ']' {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
        }
    }

    fn read_delimiter(&mut self) -> Result<String, LatexError> {
        self.skip_math_whitespace();
        if self.done() {
            return Err(Self::fail("Expected a delimiter", self.position));
        }
        let start = self.position;
        if self.peek() == '\\' {
            let command = self.read_command();
            let backslash_key = format!("\\{command}");
            if let Some(delimiter) =
                lookup_delimiter(&backslash_key).or_else(|| lookup_delimiter(&command))
            {
                return Ok(delimiter.to_string());
            }
            if self.strict {
                return Err(Self::fail(format!("Unsupported delimiter \\{command}"), start));
            }
            return Ok(backslash_key);
        }
        let token = self.source[self.position];
        self.position += 1;
        Ok(lookup_delimiter(&token.to_string()).map_or_else(
            || token.to_string(),
            ToString::to_string,
        ))
    }

    fn read_command(&mut self) -> String {
        // The callers only invoke this with `self.peek() == '\\'`.
        let _ = self.expect("\\");
        if self.done() {
            return "\\".to_string();
        }
        let next = self.peek();
        if !(next.is_ascii_alphabetic() || next == '@') {
            self.position += 1;
            return next.to_string();
        }
        let start = self.position;
        while !self.done() && (self.peek().is_ascii_alphabetic() || self.peek() == '@') {
            self.position += 1;
        }
        let command: String = self.source[start..self.position].iter().collect();
        if self.peek() == ' ' {
            self.position += 1;
        }
        command
    }

    fn read_raw_group(&mut self) -> Result<String, LatexError> {
        self.skip_math_whitespace();
        self.expect("{")?;
        let start = self.position;
        let mut depth = 1i32;
        while !self.done() {
            let char = self.source[self.position];
            self.position += 1;
            let escaped = (char == '{' || char == '}') && self.is_escaped(self.position - 1);
            if char == '{' && !escaped {
                depth += 1;
            }
            if char == '}' && !escaped {
                depth -= 1;
                if depth == 0 {
                    return Ok(self.source[start..self.position - 1].iter().collect());
                }
            }
        }
        Err(Self::fail("Unterminated group", start))
    }

    /// `applyLimitsModifier` — `\limits`/`\nolimits` after an operator.
    fn apply_limits_modifier(&mut self, body: &mut [MathNode]) -> bool {
        let word =
            if starts_with_at(&self.source, &['\\', 'l', 'i', 'm', 'i', 't', 's'], self.position)
                && !self
                    .source
                    .get(self.position + 7)
                    .is_some_and(|char| char.is_ascii_alphabetic() || *char == '@')
            {
                "limits"
            } else if starts_with_at(
                &self.source,
                &['\\', 'n', 'o', 'l', 'i', 'm', 'i', 't', 's'],
                self.position,
            ) && !self
                .source
                .get(self.position + 9)
                .is_some_and(|char| char.is_ascii_alphabetic() || *char == '@')
            {
                "nolimits"
            } else {
                return false;
            };
        self.position += if word == "limits" { 7 } else { 9 };

        if let Some(target) = body.last_mut() {
            let operator = match target {
                MathNode::Operator { limits, .. } => Some(limits),
                MathNode::Scripts { base, .. } => match base.as_mut() {
                    MathNode::Operator { limits, .. } => Some(limits),
                    _ => None,
                },
                _ => None,
            };
            if let Some(limits) = operator {
                *limits = word == "limits";
            }
        }
        true
    }

    fn is_escaped(&self, index: usize) -> bool {
        let mut slash_count = 0u32;
        let mut cursor = index;
        while cursor > 0 && self.source.get(cursor - 1) == Some(&'\\') {
            slash_count += 1;
            cursor -= 1;
        }
        slash_count % 2 == 1
    }

    /// `readTextGroup` — the raw group with escape/spacing cleanup.
    fn read_text_group(&mut self) -> Result<String, LatexError> {
        let raw = self.read_raw_group()?;
        // .replace(/\\([A-Za-z@]+|.)/g, …)
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::with_capacity(raw.len());
        let mut index = 0;
        while index < chars.len() {
            if chars[index] != '\\' || index + 1 >= chars.len() {
                out.push(chars[index]);
                index += 1;
                continue;
            }
            let mut end = index + 1;
            if chars[end].is_ascii_alphabetic() || chars[end] == '@' {
                while end < chars.len() && (chars[end].is_ascii_alphabetic() || chars[end] == '@') {
                    end += 1;
                }
            } else {
                end += 1;
            }
            let command: String = chars[index + 1..end].iter().collect();
            if "{}%#$&_ ".contains(&command) {
                out.push_str(&command);
            } else if command == "textbackslash" {
                out.push('\\');
            } else if command == "!" {
                // dropped
            } else if let Some(width) = lookup_spacing(&command) {
                out.push_str(&" ".repeat(width.max(1) as usize));
            } else {
                // keep the match verbatim
                out.extend(&chars[index..end]);
            }
            index = end;
        }
        Ok(out.replace('~', " "))
    }

    fn read_until_end(&mut self, environment: &str) -> Result<String, LatexError> {
        let marker = format!("\\end{{{environment}}}");
        let marker_chars: Vec<char> = marker.chars().collect();
        let mut end = None;
        for index in self.position..self.source.len() {
            if starts_with_at(&self.source, &marker_chars, index) {
                end = Some(index);
                break;
            }
        }
        let Some(end) = end else {
            return Err(Self::fail(format!("Missing {marker}"), self.position));
        };
        let content: String = self.source[self.position..end].iter().collect();
        self.position = end + marker_chars.len();
        Ok(content)
    }

    fn skip_math_whitespace(&mut self) {
        while !self.done() {
            if self.peek().is_whitespace() {
                self.position += 1;
                continue;
            }
            if self.peek() == '%' {
                while !self.done() && self.peek() != '\n' {
                    self.position += 1;
                }
                continue;
            }
            break;
        }
    }

    fn expect(&mut self, value: &str) -> Result<(), LatexError> {
        self.expect_chars(&value.chars().collect::<Vec<_>>())
    }

    fn expect_chars(&mut self, value: &[char]) -> Result<(), LatexError> {
        if !starts_with_at(&self.source, value, self.position) {
            let text: String = value.iter().collect();
            return Err(Self::fail(format!("Expected \"{text}\""), self.position));
        }
        self.position += value.len();
        Ok(())
    }

    fn peek(&self) -> char {
        self.source.get(self.position).copied().unwrap_or('\0')
    }

    fn done(&self) -> bool {
        self.position >= self.source.len()
    }

    fn fail(message: impl Into<String>, position: usize) -> LatexError {
        latex_error(message, position)
    }
}

fn environment_name(environment: MatrixEnvironment) -> &'static str {
    match environment {
        MatrixEnvironment::Matrix => "matrix",
        MatrixEnvironment::Pmatrix => "pmatrix",
        MatrixEnvironment::Bmatrix => "bmatrix",
        MatrixEnvironment::BigBmatrix => "Bmatrix",
        MatrixEnvironment::Vmatrix => "vmatrix",
        MatrixEnvironment::DoubleVmatrix => "Vmatrix",
        MatrixEnvironment::Cases => "cases",
        MatrixEnvironment::Aligned => "aligned",
        MatrixEnvironment::Align => "align",
        MatrixEnvironment::Gathered => "gathered",
        MatrixEnvironment::Gather => "gather",
        MatrixEnvironment::SmallMatrix => "smallmatrix",
        MatrixEnvironment::Array => "array",
    }
}

fn starts_with_at(source: &[char], needle: &[char], at: usize) -> bool {
    source.len() >= at + needle.len()
        && source[at..at + needle.len()].iter().zip(needle).all(|(left, right)| left == right)
}

fn is_big_delimiter_command(command: &str) -> bool {
    let core = command.strip_suffix(['l', 'r', 'm']).unwrap_or(command);
    matches!(core, "big" | "Big" | "bigg" | "Bigg")
}

fn row(body: Vec<MathNode>) -> MathNode {
    if body.len() == 1 {
        body.into_iter().next().unwrap_or(MathNode::Row(Vec::new()))
    } else {
        MathNode::Row(body)
    }
}

fn infer_role(value: char) -> super::types::SymbolRole {
    use super::types::SymbolRole;
    if "+-*/×÷±∓".contains(value) {
        return SymbolRole::Binary;
    }
    if "=<>≤≥≠≈∈∉⊂⊃".contains(value) {
        return SymbolRole::Relation;
    }
    if ",;:".contains(value) {
        return SymbolRole::Punctuation;
    }
    if "([{".contains(value) {
        return SymbolRole::Opening;
    }
    if ")]}".contains(value) {
        return SymbolRole::Closing;
    }
    SymbolRole::Ordinary
}

fn negate_symbol(value: &str) -> String {
    let negated = match value {
        "=" => Some("≠"),
        "∈" => Some("∉"),
        "∋" => Some("∌"),
        "≡" => Some("≢"),
        "≈" => Some("≉"),
        "∼" => Some("≁"),
        "<" => Some("≮"),
        ">" => Some("≯"),
        "≤" => Some("≰"),
        "≥" => Some("≱"),
        "⊂" => Some("⊄"),
        "⊃" => Some("⊅"),
        "⊆" => Some("⊈"),
        "⊇" => Some("⊉"),
        "∣" => Some("∤"),
        "∥" => Some("∦"),
        _ => None,
    };
    match negated {
        Some(negated) => negated.to_string(),
        None => format!("{value}̸"),
    }
}
