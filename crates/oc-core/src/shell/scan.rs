//! Port of `packages/core/src/shell/scan.ts` — the hand-written portable
//! command scanner (bash/zsh/posix dialects + PowerShell).
//!
//! This scanner is **permission-security-critical**: every command executed
//! through the shell tool is parsed here to derive per-command resource spans
//! (`resource`), legacy prefix grants (`save`), and opaque refusals
//! (`dynamic-execution` taint analysis, zsh glob qualifiers, heredocs…).
//! The port is a faithful, statement-by-statement translation of the
//! TypeScript original — indexes are char-based over the decoded input.
//!
//! Structural notes:
//!
//! - JS object identity for [`Command`] matters: `readBashHeredocs` rewrites
//!   `heredoc.command.resource` on the *same* object that was pushed into the
//!   command list, so commands are shared via [`CmdRef`]
//!   (`Rc<RefCell<Command>>`).
//! - The TypeScript sticky regexes (`TOKEN_RE`, `ZSH_*`, PowerShell `*_RE`)
//!   become hand-rolled position-anchored matchers over `&[char]`.
//! - The TS for-loops mutate `index` then `continue` (implicit `index++`);
//!   every assignment site here accounts for that final increment.

// Fidelity-first port: the lint surface mirrors hand-rolled pointer math and
// ported control flow; style lints are silenced deliberately.
#![allow(
    clippy::must_use_candidate,
    clippy::needless_return,
    clippy::match_single_binding,
    clippy::if_same_then_else,
    clippy::match_same_arms,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::option_map_unit_fn,
    clippy::result_map_unit_fn,
    clippy::single_match,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::manual_map,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::needless_bool,
    clippy::vec_init_then_push,
    clippy::while_let_loop,
    clippy::redundant_field_names,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::unnested_or_patterns,
    clippy::unreadable_literal,
    clippy::redundant_else,
    clippy::manual_range_contains,
    clippy::nonminimal_bool,
    clippy::too_many_lines,
    clippy::struct_excessive_bools,
    clippy::map_unwrap_or,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::needless_pass_by_value,
    clippy::unnecessary_wraps,
    clippy::question_mark,
    clippy::format_push_string,
    clippy::needless_option_as_deref,
    clippy::manual_let_else,
    clippy::if_not_else,
    clippy::assigning_clones,
    unused_parens
)]

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// `ShellScan.OpaqueReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpaqueReason {
    CommandSubstitution,
    CompoundCommand,
    DynamicCommandName,
    DynamicExecution,
    Heredoc,
    InvalidRedirect,
    InvalidStructure,
    UnterminatedEscape,
    UnterminatedQuote,
}

impl OpaqueReason {
    /// Wire name (kebab-case), used in the parse error message.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CommandSubstitution => "command-substitution",
            Self::CompoundCommand => "compound-command",
            Self::DynamicCommandName => "dynamic-command-name",
            Self::DynamicExecution => "dynamic-execution",
            Self::Heredoc => "heredoc",
            Self::InvalidRedirect => "invalid-redirect",
            Self::InvalidStructure => "invalid-structure",
            Self::UnterminatedEscape => "unterminated-escape",
            Self::UnterminatedQuote => "unterminated-quote",
        }
    }
}

impl std::fmt::Display for OpaqueReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One scanned command: its permission resource span plus token data.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Command {
    /// Trimmed source span used as the permission resource.
    pub resource: String,
    /// Cooked words (quotes resolved).
    pub words: Vec<String>,
    /// Raw token texts (source-shaped).
    pub raw_words: Vec<String>,
    /// Exclusive raw-token ends relative to `resource` (PowerShell only).
    pub word_ends: Option<Vec<usize>>,
    /// Statement-head commands (PowerShell only).
    pub statement_head: bool,
    /// Declarations (`export …`, `local …`) are skipped by the legacy walk.
    pub declaration: bool,
    /// Words after this many raw words are redirect destinations.
    pub redirect_word_count: Option<usize>,
}

/// Shared command handle preserving JS object identity across heredocs.
pub type CmdRef = Rc<RefCell<Command>>;

/// `ShellScan.Result`.
#[derive(Debug, Clone, PartialEq)]
pub enum ScanResult {
    Scanned { commands: Vec<Command> },
    Opaque { reason: OpaqueReason },
}

/// `ShellScan.Dialect` — Posix stays sound for bash/zsh/dash alike and is the
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Bash,
    Zsh,
    Posix,
}

// ---------------------------------------------------------------------------
// Character helpers
// ---------------------------------------------------------------------------

fn at(chars: &[char], index: usize) -> char {
    chars.get(index).copied().unwrap_or('\0')
}

fn slice(chars: &[char], start: usize, end: usize) -> String {
    if start >= end || start >= chars.len() {
        return String::new();
    }
    chars[start..end.min(chars.len())].iter().collect()
}

fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

// ---------------------------------------------------------------------------
// Constant tables
// ---------------------------------------------------------------------------

const MAX_INPUT_LENGTH: usize = 64 * 1024;
const MAX_SUBSTITUTION_DEPTH: usize = 32;

const BASH_REDIRECTS: &[&str] =
    &["&>>", "&>", "<<<", "<<-", "<<", "<>", "<&", ">&", ">|", ">>", ">", "<"];
const BASH_LIST_OPERATORS: &[&str] = &["&&", "||", "|&"];

fn bash_case_operators() -> Vec<&'static str> {
    let mut operators = vec![";;&", ";;", ";&", ";|"];
    operators.extend_from_slice(BASH_LIST_OPERATORS);
    operators
}

macro_rules! word_set {
    ($name:ident, $($word:literal),* $(,)?) => {
        fn $name(word: &str) -> bool {
            static SET: std::sync::OnceLock<HashSet<&'static str>> = std::sync::OnceLock::new();
            SET.get_or_init(|| [$($word),*].into_iter().collect()).contains(word)
        }
    };
}

// Builtins that bind their operands to variables or evaluate them as names.
word_set!(
    is_binding_builtin,
    "declare",
    "typeset",
    "local",
    "export",
    "readonly",
    "private",
    "unset",
    "read",
    "getln",
    "vared",
    "getopts",
    "mapfile",
    "readarray",
    "compgen",
    "zparseopts",
    "zregexparse",
    "zformat",
    "zstyle",
    "zstat",
    "stat",
);
word_set!(is_arithmetic_comparison, "-eq", "-ne", "-lt", "-le", "-gt", "-ge");
word_set!(is_assignment_declaration, "declare", "typeset", "export", "readonly", "local");
word_set!(is_declaration, "declare", "typeset", "export", "readonly", "local", "unset", "unsetenv");
word_set!(is_bash_precommand, "builtin", "command", "time", "coproc", "!");
word_set!(is_zsh_precommand, "noglob", "nocorrect", "exec", "-");
word_set!(
    is_zsh_arithmetic_builtin,
    "integer",
    "float",
    "exit",
    "return",
    "shift",
    "break",
    "continue",
    "bye",
    "logout",
);
word_set!(
    is_non_function_keyword,
    "if",
    "for",
    "select",
    "case",
    "then",
    "elif",
    "else",
    "fi",
    "do",
    "done",
    "in",
    "esac",
);

const BASH_ANSI_ESCAPES: &[(char, char)] = &[
    ('a', '\u{7}'),
    ('b', '\u{8}'),
    ('e', '\u{1b}'),
    ('E', '\u{1b}'),
    ('f', '\u{c}'),
    ('n', '\n'),
    ('r', '\r'),
    ('t', '\t'),
    ('v', '\u{b}'),
    ('\\', '\\'),
    ('\'', '\''),
    ('"', '"'),
    ('?', '?'),
];

// ---------------------------------------------------------------------------
// Shared context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Close {
    Paren,
    Brace,
    NoFork,
}

struct BashContext {
    remaining: i64,
    dialect: Dialect,
    bound_dollar: bool,
    bound_open: bool,
    variable_sink: bool,
}

impl BashContext {
    fn new(dialect: Dialect) -> Self {
        Self {
            remaining: (MAX_INPUT_LENGTH * MAX_SUBSTITUTION_DEPTH) as i64,
            dialect,
            bound_dollar: false,
            bound_open: false,
            variable_sink: false,
        }
    }
}

type Ctx<'a> = &'a RefCell<BashContext>;

fn spend(context: Ctx<'_>) -> Result<(), OpaqueReason> {
    let mut ctx = context.borrow_mut();
    ctx.remaining -= 1;
    if ctx.remaining < 0 {
        return Err(OpaqueReason::InvalidStructure);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructureKind {
    If,
    While,
    Until,
    For,
    Case,
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructurePhase {
    Header,
    Condition,
    Pattern,
    Body,
    Do,
}

#[derive(Debug, Clone, Copy)]
struct Structure {
    kind: StructureKind,
    phase: StructurePhase,
    count: usize,
    saw_else: bool,
    saw_in: bool,
    pattern_started: bool,
    parenthesized: bool,
}

impl Structure {
    fn new(kind: StructureKind, phase: StructurePhase) -> Self {
        Self {
            kind,
            phase,
            count: 0,
            saw_else: false,
            saw_in: false,
            pattern_started: false,
            parenthesized: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct BashHeredoc {
    delimiter: String,
    dash_delimiter: Option<String>,
    quoted: bool,
    tabs: bool,
    binding: bool,
    command: Option<CmdRef>,
    start: Option<usize>,
}

/// Cooked word text, its literal decodes, and its substitutions' output.
#[derive(Debug, Clone, Default)]
struct BashText {
    word: String,
    literal: String,
    output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BashEvaluation {
    Arithmetic,
    Binding,
    Deferred,
}

/// Word text is unquoted; quoted text follows double-quote rules; arithmetic
/// text expands like double-quoted text while still allowing `\` escapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BashTextMode {
    Word,
    Quoted,
    Arithmetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpanMode {
    Quoted,
    Arithmetic,
}

#[derive(Debug, Clone, Copy)]
struct BashSpan {
    open: Option<char>,
    close: Option<char>,
    reject: &'static str,
    mode: SpanMode,
}

const fn span(close: Option<char>, reject: &'static str, mode: SpanMode) -> BashSpan {
    BashSpan { open: None, close, reject, mode }
}

const fn span_nested(open: char, close: char, reject: &'static str, mode: SpanMode) -> BashSpan {
    BashSpan { open: Some(open), close: Some(close), reject, mode }
}

struct Spans;

impl Spans {
    const DOUBLE: BashSpan = span(Some('"'), "", SpanMode::Quoted);
    const HEREDOC: BashSpan = span(None, "", SpanMode::Quoted);
    /// Bash rereads unparseable `((...))` as nested subshells.
    const ARITHMETIC: BashSpan = span_nested('(', ')', ";#}", SpanMode::Arithmetic);
    const FOR_ARITHMETIC: BashSpan = span_nested('(', ')', "#}", SpanMode::Arithmetic);
    const BRACKET_ARITHMETIC: BashSpan = span(Some(']'), ";|&<>()[\n'\"\\#", SpanMode::Arithmetic);
    const SUBSCRIPT: BashSpan = span_nested('[', ']', "\"}", SpanMode::Arithmetic);
    const QUOTED_SUBSCRIPT: BashSpan = span_nested('[', ']', "\"", SpanMode::Quoted);
    /// Dash splits an assignment subscript at blanks and operators.
    const ASSIGNMENT_SUBSCRIPT: BashSpan =
        span_nested('[', ']', " \t\n\r\u{b}\u{c};&|<>()", SpanMode::Arithmetic);
    const ARITHMETIC_QUOTE: BashSpan = span(Some('\''), "()[];\"}", SpanMode::Quoted);
    const ARITHMETIC_DOUBLE: BashSpan = span(Some('"'), "()[];}", SpanMode::Quoted);
    const PARAMETER_QUOTE: BashSpan = span(Some('\''), "\"}[]", SpanMode::Quoted);
}

// ---------------------------------------------------------------------------
// Bash scanner state
// ---------------------------------------------------------------------------

struct BashState<'a> {
    chars: &'a [char],
    context: Ctx<'a>,
    commands: Vec<CmdRef>,
    nested_commands: Vec<CmdRef>,
    words: Vec<String>,
    raw_words: Vec<String>,
    /// Pending heredocs (own buffer; a group shares its parent's buffer).
    own_heredocs: RefCell<Vec<BashHeredoc>>,
    shared_heredocs: Option<&'a RefCell<Vec<BashHeredoc>>>,
    structures: Vec<Structure>,
    word: String,
    literal: String,
    output: String,
    word_started: bool,
    word_start: usize,
    word_end: usize,
    command_start: Option<usize>,
    command_end: usize,
    resource_end: Option<usize>,
    redirect_word_count: Option<usize>,
    /// Posix reads `&>` as `&` + `>` so later words start another command.
    ampersand_redirect_words: Option<usize>,
    command_word_index: isize,
    assignment_word: bool,
    assignment_head_unsafe: bool,
    invalid: Option<OpaqueReason>,
    redirect_target: bool,
    here_string: bool,
    has_redirect: bool,
    compound_end: bool,
    /// A list operator still awaits its right-hand command.
    dangling: bool,
    /// Collects each word's literal text when the list is a substitution.
    outer: Option<BashText>,
    close: Option<Close>,
}

impl<'a> BashState<'a> {
    fn new(chars: &'a [char], context: Ctx<'a>, start: usize, close: Option<Close>) -> Self {
        Self {
            chars,
            context,
            commands: Vec::new(),
            nested_commands: Vec::new(),
            words: Vec::new(),
            raw_words: Vec::new(),
            own_heredocs: RefCell::new(Vec::new()),
            shared_heredocs: None,
            structures: Vec::new(),
            word: String::new(),
            literal: String::new(),
            output: String::new(),
            word_started: false,
            word_start: start,
            word_end: start,
            command_start: None,
            command_end: start,
            resource_end: None,
            redirect_word_count: None,
            ampersand_redirect_words: None,
            command_word_index: -1,
            assignment_word: false,
            assignment_head_unsafe: false,
            invalid: None,
            redirect_target: false,
            here_string: false,
            has_redirect: false,
            compound_end: false,
            dangling: false,
            outer: None,
            close,
        }
    }

    fn with_heredocs(
        chars: &'a [char],
        context: Ctx<'a>,
        start: usize,
        close: Option<Close>,
        heredocs: &'a RefCell<Vec<BashHeredoc>>,
        outer: Option<BashText>,
    ) -> Self {
        let mut state = Self::new(chars, context, start, close);
        state.shared_heredocs = Some(heredocs);
        state.outer = outer;
        state
    }

    fn heredocs_cell(&self) -> &RefCell<Vec<BashHeredoc>> {
        self.shared_heredocs.unwrap_or(&self.own_heredocs)
    }

    fn set_invalid(&mut self, reason: OpaqueReason) {
        if self.invalid.is_none() {
            self.invalid = Some(reason);
        }
    }

    fn in_header(&self) -> bool {
        self.structures.last().is_some_and(|structure| {
            matches!(
                structure.phase,
                StructurePhase::Header | StructurePhase::Pattern | StructurePhase::Do
            )
        })
    }

    fn at_command_start(&self) -> bool {
        self.words.is_empty() && !self.has_redirect && !self.compound_end && !self.in_header()
    }

    fn pending(&self) -> bool {
        self.word_started || !self.words.is_empty() || self.has_redirect || self.compound_end
    }

    fn statement(&mut self) {
        if let Some(structure) = self.structures.last_mut() {
            structure.count += 1;
        }
    }

    fn dialect(&self) -> Dialect {
        self.context.borrow().dialect
    }
}

fn commands_of(list: &[CmdRef]) -> Vec<Command> {
    list.iter().map(|cmd| cmd.borrow().clone()).collect()
}

// ---------------------------------------------------------------------------
// Lazy-prefix regex helpers (ports of the anchored patterns)
// ---------------------------------------------------------------------------

/// `LINE_CONTINUATION_RE` — index after `(\\\n)*`.
fn line_continuation_end(chars: &[char], at_index: usize) -> usize {
    let mut index = at_index;
    while index + 1 < chars.len() && at(chars, index) == '\\' && at(chars, index + 1) == '\n' {
        index += 2;
    }
    index
}

/// `TOKEN_RE` — `[A-Za-z_][A-Za-z0-9_]*` + `(\\\n)*` + delimiter/EOF.
fn token_re(chars: &[char], at_index: usize) -> Option<String> {
    let first = at(chars, at_index);
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut end = at_index + 1;
    loop {
        let c = at(chars, end);
        if c.is_ascii_alphanumeric() || c == '_' {
            end += 1;
        } else {
            break;
        }
    }
    let after = line_continuation_end(chars, end);
    let next = at(chars, after);
    if next == '\0' || " \t\n;|&()<>".contains(next) {
        Some(chars[at_index..end].iter().collect())
    } else {
        None
    }
}

/// `BRACE_CLOSE_AHEAD_RE` — `(\\\n)*` then delimiter or EOF.
fn brace_close_ahead(chars: &[char], at_index: usize) -> bool {
    let after = line_continuation_end(chars, at_index);
    let next = at(chars, after);
    next == '\0' || " \t\n;&|()<>".contains(next)
}

/// `SPACE_CONTINUATION_AHEAD_RE`.
fn space_continuation_ahead(chars: &[char], at_index: usize) -> bool {
    let after = line_continuation_end(chars, at_index);
    " \t\n".contains(at(chars, after))
}

/// `NEGATION_AHEAD_RE`.
fn negation_ahead(chars: &[char], at_index: usize) -> bool {
    let after = line_continuation_end(chars, at_index);
    " \t\n(".contains(at(chars, after))
}

/// Skip `(?:[ \t]|\\\n)+`.
fn blanks_with_continuation(chars: &[char], at_index: usize) -> Option<usize> {
    let mut index = at_index;
    let mut any = false;
    loop {
        let c = at(chars, index);
        if is_blank(c) {
            index += 1;
            any = true;
        } else if c == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
            any = true;
        } else {
            break;
        }
    }
    if any {
        Some(index)
    } else {
        None
    }
}

/// Shared tail of `COPROC_AHEAD_RE`/`TIME_AHEAD_RE`: `(!blanks)*` then a
/// compound open or keyword.
fn command_ahead_of_block(chars: &[char], at_index: usize) -> bool {
    let mut index = at_index;
    loop {
        if at(chars, index) != '!' {
            break;
        }
        let mut inner = index + 1;
        if let Some(after) = blanks_with_continuation(chars, inner) {
            inner = after;
        }
        index = inner;
    }
    let c = at(chars, index);
    if c == '{' || c == '(' {
        return true;
    }
    if c == '[' && at(chars, index + 1) == '[' {
        let after = line_continuation_end(chars, index + 2);
        if " \t\n".contains(at(chars, after)) {
            return true;
        }
    }
    ["if", "while", "until", "for", "select", "case", "repeat"].into_iter().any(|keyword| {
        slice(chars, index, index + keyword.len()) == keyword && {
            let after = at(chars, index + keyword.len());
            after == '\0' || (!after.is_ascii_alphanumeric() && after != '_')
        }
    })
}

/// `COPROC_AHEAD_RE` — match length when `coproc …` heads a compound command.
fn coproc_ahead(chars: &[char], at_index: usize) -> Option<usize> {
    if slice(chars, at_index, at_index + 6) != "coproc" {
        return None;
    }
    let mut index = blanks_with_continuation(chars, at_index + 6)?;
    if slice(chars, index, index + 2) == "--" {
        index = blanks_with_continuation(chars, index + 2)?;
    }
    let first = at(chars, index);
    if first.is_ascii_alphabetic() || first == '_' {
        let mut end = index + 1;
        loop {
            let c = at(chars, end);
            if c.is_ascii_alphanumeric() || c == '_' {
                end += 1;
            } else {
                break;
            }
        }
        if let Some(after) = blanks_with_continuation(chars, end) {
            index = after;
        }
    }
    if command_ahead_of_block(chars, index) {
        Some(index - at_index)
    } else {
        None
    }
}

/// `TIME_AHEAD_RE`.
fn time_ahead(chars: &[char], at_index: usize) -> Option<usize> {
    if slice(chars, at_index, at_index + 4) != "time" {
        return None;
    }
    let mut index = blanks_with_continuation(chars, at_index + 4)?;
    if slice(chars, index, index + 2) == "-p" {
        index = blanks_with_continuation(chars, index + 2)?;
    }
    if slice(chars, index, index + 2) == "--" {
        index = blanks_with_continuation(chars, index + 2)?;
    }
    if command_ahead_of_block(chars, index) {
        Some(index - at_index)
    } else {
        None
    }
}

/// `FUNCTION_HEAD_RE` + `FUNCTION_BODY_RE` — head length or 0.
fn bash_function_head_length(chars: &[char], start: usize) -> usize {
    let mut index = start;
    let mut has_function = false;
    if slice(chars, index, index + 8) == "function" && is_blank(at(chars, index + 8)) {
        let Some(after) = blanks_with_continuation(chars, index + 8) else { return 0 };
        index = after;
        has_function = true;
    }
    let mut name: Option<String> = None;
    let first = at(chars, index);
    if first.is_ascii_alphabetic() || first == '_' {
        let mut end = index + 1;
        loop {
            let c = at(chars, end);
            if c.is_ascii_alphanumeric() || "_ .:+@%-".contains(c) {
                end += 1;
            } else {
                break;
            }
        }
        name = Some(slice(chars, index, end));
        index = end;
    }
    loop {
        if is_blank(at(chars, index)) {
            index += 1;
        } else if at(chars, index) == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
        } else {
            break;
        }
    }
    let mut has_paren = false;
    if at(chars, index) == '(' {
        let mut inner = index + 1;
        while is_blank(at(chars, inner)) {
            inner += 1;
        }
        if at(chars, inner) == ')' {
            index = inner + 1;
            has_paren = true;
        }
    }
    let tail_start = index;
    let mut tail_end = index;
    loop {
        let c = at(chars, tail_end);
        if c == ' ' || c == '\t' || c == '\n' {
            tail_end += 1;
        } else if c == '\\' && at(chars, tail_end + 1) == '\n' {
            tail_end += 2;
        } else if c == '#' {
            let mut newline = tail_end;
            while newline < chars.len() && at(chars, newline) != '\n' {
                newline += 1;
            }
            if newline < chars.len() {
                tail_end = newline + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    let name_valid = if has_function {
        name.is_some()
    } else {
        has_paren && name.as_deref().is_some_and(|n| !is_non_function_keyword(n))
    };
    if !name_valid {
        return 0;
    }
    let body_at = tail_end;
    let body_ok = {
        let c = at(chars, body_at);
        let direct = c == '{' || c == '(';
        let bracket = c == '['
            && at(chars, body_at + 1) == '['
            && " \t\n".contains(at(chars, line_continuation_end(chars, body_at + 2)));
        let keyword =
            ["if", "while", "until", "for", "select", "case", "repeat"].into_iter().any(|kw| {
                slice(chars, body_at, body_at + kw.len()) == kw && {
                    let after = line_continuation_end(chars, body_at + kw.len());
                    let next = at(chars, after);
                    next == '\0' || " \t\n(".contains(next)
                }
            });
        direct || bracket || keyword
    };
    if body_ok {
        tail_start - start
    } else {
        0
    }
}

/// `DO_AHEAD_RE`.
fn do_ahead(chars: &[char], at_index: usize) -> bool {
    let mut index = at_index;
    loop {
        let c = at(chars, index);
        if c == ' ' || c == '\t' || c == '\n' || c == ';' {
            index += 1;
        } else if c == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
        } else if c == '#' {
            let mut cursor = index;
            while cursor < chars.len() && at(chars, cursor) != '\n' {
                cursor += 1;
            }
            if cursor >= chars.len() {
                break;
            }
            index = cursor + 1;
        } else {
            break;
        }
    }
    let c = at(chars, index);
    if c == 'd' && slice(chars, index, index + 2) == "do" && {
        let after = line_continuation_end(chars, index + 2);
        let next = at(chars, after);
        next == '\0' || " \t\n;{(".contains(next)
    } {
        return true;
    }
    if c == '{' {
        let after = line_continuation_end(chars, index + 1);
        if " \t\n".contains(at(chars, after)) {
            return true;
        }
    }
    false
}

/// `NOFORK_OPEN_RE` — `${` + continuations + blank/newline/`|`; match end.
fn nofork_open_end(chars: &[char], at_index: usize) -> Option<usize> {
    if at(chars, at_index) != '$' || at(chars, at_index + 1) != '{' {
        return None;
    }
    let after = line_continuation_end(chars, at_index + 2);
    if " \t\n|".contains(at(chars, after)) {
        Some(after)
    } else {
        None
    }
}

/// `ZSH_SAFE_FLAGS_RE` — safe `${(flags)` group; index after `)`.
fn zsh_safe_flags_end(chars: &[char], at_index: usize) -> Option<usize> {
    if at(chars, at_index) != '$'
        || at(chars, at_index + 1) != '{'
        || at(chars, at_index + 2) != '('
    {
        return None;
    }
    let mut index = at_index + 3;
    loop {
        let c = at(chars, index);
        if "@AaBbCcDEfFikLMmNnOopqStUuVvWwXZz".contains(c) || c.is_ascii_digit() {
            index += 1;
        } else if c == 'q' {
            index += 1;
            if at(chars, index) == '-' || at(chars, index) == '+' {
                index += 1;
            }
        } else if c == 'j' || c == 's' {
            let delim = at(chars, index + 1);
            if "({[<\\\0".contains(delim) {
                return None;
            }
            let mut inner = index + 2;
            while at(chars, inner) != ')' && at(chars, inner) != delim && at(chars, inner) != '\0' {
                inner += 1;
            }
            if at(chars, inner) != delim {
                return None;
            }
            index = inner + 1;
        } else if c == ')' {
            return Some(index + 1);
        } else {
            return None;
        }
    }
}

/// `ZSH_GLOB_MODIFIER_RE` — `([\^=~#+]|\\\n)*~`.
fn zsh_glob_modifier(chars: &[char], at_index: usize) -> bool {
    // `(class)*~` with backtracking: a '~' anywhere at a class-run boundary
    // matches — so test '~' before consuming it as part of the class.
    let mut index = at_index;
    loop {
        let c = at(chars, index);
        if c == '~' {
            return true;
        }
        if "^=~#+".contains(c) {
            index += 1;
        } else if c == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
        } else {
            return false;
        }
    }
}

/// `ZSH_SUBSCRIPT_RE` — `$` modifiers name `[`; index of `[`.
fn zsh_subscript_bracket(chars: &[char], at_index: usize) -> Option<usize> {
    if at(chars, at_index) != '$' {
        return None;
    }
    let mut index = at_index + 1;
    loop {
        let c = at(chars, index);
        if "^=+~#".contains(c) {
            index += 1;
        } else if c == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
        } else {
            break;
        }
    }
    let c = at(chars, index);
    let name_end = if c.is_ascii_alphabetic() || c == '_' {
        let mut end = index + 1;
        loop {
            let n = at(chars, end);
            if n.is_ascii_alphanumeric() || n == '_' {
                end += 1;
            } else if n == '\\' && at(chars, end + 1) == '\n' {
                end += 2;
            } else {
                break;
            }
        }
        Some(end)
    } else if c.is_ascii_digit() || "@*#?$!-".contains(c) {
        Some(index + 1)
    } else {
        None
    }?;
    let after = line_continuation_end(chars, name_end);
    if at(chars, after) == '[' {
        Some(after)
    } else {
        None
    }
}

/// `PARAMETER_PREFIX_RE` — end index.
fn parameter_prefix_end(chars: &[char], at_index: usize) -> usize {
    let mut index = line_continuation_end(chars, at_index);
    if at(chars, index) == '(' {
        let mut inner = index + 1;
        while at(chars, inner) != ')' && at(chars, inner) != '\0' {
            inner += 1;
        }
        if at(chars, inner) == ')' {
            index = inner + 1;
        }
    }
    loop {
        let c = at(chars, index);
        if "!#^=+~".contains(c) {
            index += 1;
        } else if c == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
        } else {
            break;
        }
    }
    index
}

/// `PARAMETER_NAME_RE` — end index.
fn parameter_name_end(chars: &[char], at_index: usize) -> Option<usize> {
    let first = at(chars, at_index);
    if first.is_ascii_alphabetic() || first == '_' {
        let mut index = at_index + 1;
        loop {
            let c = at(chars, index);
            if c.is_ascii_alphanumeric() || c == '_' {
                index += 1;
            } else if c == '\\' && at(chars, index + 1) == '\n' {
                index += 2;
            } else {
                break;
            }
        }
        Some(index)
    } else if first.is_ascii_digit() {
        let mut index = at_index;
        while at(chars, index).is_ascii_digit()
            || (at(chars, index) == '\\' && at(chars, index + 1) == '\n')
        {
            index += if at(chars, index) == '\\' { 2 } else { 1 };
        }
        Some(index)
    } else if "@*#?$!-".contains(first) {
        let mut index = at_index + 1;
        while at(chars, index) == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
        }
        Some(index)
    } else {
        None
    }
}

/// `PARAMETER_PROMPT_RE` — `@(\\\n)*[EP](\\\n)*}`.
fn parameter_prompt(chars: &[char], at_index: usize) -> bool {
    if at(chars, at_index) != '@' {
        return false;
    }
    let mut index = line_continuation_end(chars, at_index + 1);
    let c = at(chars, index);
    if c != 'E' && c != 'P' {
        return false;
    }
    index = line_continuation_end(chars, index + 1);
    at(chars, index) == '}'
}

/// `BASH_EXPANSION_RE` — `\$(escape)*[({]` or backtick anywhere.
fn bash_expansion_test(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == '`' {
            return true;
        }
        if c == '$' {
            let mut inner = index + 1;
            while at(&chars, inner) == '\\'
                && at(&chars, inner + 1) != '\0'
                && at(&chars, inner + 1) != '\n'
            {
                inner += 2;
            }
            let c2 = at(&chars, inner);
            if c2 == '(' || c2 == '{' {
                return true;
            }
        }
        index += 1;
    }
    false
}

/// `BASH_ESCAPE_RE` — `\x##`, octal, `\u####`, `\U########`.
fn bash_escape_test(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let hex = |c: char| c.is_ascii_hexdigit();
    for (index, &c) in chars.iter().enumerate() {
        if c != '\\' {
            continue;
        }
        let escaped = at(&chars, index + 1);
        if escaped == 'x' {
            if hex(at(&chars, index + 2)) {
                return true;
            }
        } else if escaped.is_ascii_digit() && escaped < '8' {
            return true;
        } else if (escaped == 'u' || escaped == 'U') && hex(at(&chars, index + 2)) {
            return true;
        }
    }
    false
}

/// `/^(?:READ)?NULLCMD$/`.
fn is_nullcmd(word: &str) -> bool {
    word == "NULLCMD" || word == "READNULLCMD"
}

/// `/^(?:-[A-Za-z]*v)?(name)(?:\+?=|\[|$)/` for the zsh binding names.
fn is_zsh_binding_name(word: &str) -> bool {
    let mut rest = word;
    if let Some(stripped) = rest.strip_prefix('-') {
        let letters: String = stripped.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        if letters.len() < stripped.len() && stripped.as_bytes()[letters.len()] == b'v' {
            rest = &stripped[letters.len() + 1..];
        } else if letters == stripped && stripped.ends_with('v') {
            rest = &stripped[..stripped.len() - 1];
        }
    }
    [
        "functions",
        "galiases",
        "salises",
        "aliases",
        "dis_functions",
        "dis_galiases",
        "dis_saliases",
        "dis_aliases",
        "options",
        "commands",
    ]
    .iter()
    .any(|candidate| {
        rest.starts_with(candidate) && {
            let tail = &rest[candidate.len()..];
            tail.is_empty()
                || tail.starts_with("+=")
                || tail.starts_with('=')
                || tail.starts_with('[')
        }
    })
}

// Lazy scans: `^[-+]?[class]*<marker>` where the class run precedes the
// marker (backtracking-equivalent).
fn lazy_prefix_marker(word: &str, sign: bool, class: fn(char) -> bool, marker: char) -> bool {
    let bytes: Vec<char> = word.chars().collect();
    let mut index = 0;
    if sign {
        if bytes.is_empty() || (bytes[0] != '-' && bytes[0] != '+') {
            return false;
        }
        index = 1;
    }
    while index < bytes.len() {
        let c = bytes[index];
        if c == marker {
            return true;
        }
        if !class(c) {
            return false;
        }
        index += 1;
    }
    false
}

fn is_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// `/^-[A-Za-z0-9]*[Akm]o/`-style lazy checks used by the zsh option rules.
fn regex_set_arithmetic(word: &str) -> bool {
    lazy_prefix_marker(word, true, is_alnum, 'A')
}

fn regex_option_o(word: &str) -> bool {
    lazy_prefix_marker(word, true, is_alnum, 'o')
}

fn regex_set_keyword_flag(word: &str) -> bool {
    word == "keyword" || lazy_prefix_marker(word, true, is_alnum, 'k')
}

fn regex_wait_p(word: &str) -> bool {
    if let Some(stripped) = word.strip_prefix('-') {
        let prefix: String = stripped.chars().take_while(|c| *c == 'n' || *c == 'f').collect();
        return word[1 + prefix.len()..].starts_with('p');
    }
    false
}

/// `/^-[cl]*a$/`.
fn regex_exec_a(word: &str) -> bool {
    if let Some(stripped) = word.strip_prefix('-') {
        let prefix: String = stripped.chars().take_while(|c| *c == 'c' || *c == 'l').collect();
        return &word[1 + prefix.len()..] == "a";
    }
    false
}

/// `/^-[A-Za-z]*v/` (print) or `/^-v/` (printf).
fn regex_print_v(word: &str, print: bool) -> bool {
    if print {
        lazy_prefix_marker(word, false, is_letter, 'v') && word.starts_with('-')
    } else {
        word.starts_with("-v")
    }
}

// /\+?=(?:\(|[$`])/ — assignment-array detection.
fn regex_array_assign(word: &str) -> bool {
    let rest = word.strip_prefix('+').unwrap_or(word);
    if let Some(stripped) = rest.strip_prefix('=') {
        matches!(stripped.chars().next(), Some('(') | Some('$') | Some('`'))
    } else {
        false
    }
}

/// zsh option that enables evaluation: `^-[A-Za-z0-9]*m` or
/// `^(no)?(globsubst|kshglob|promptsubst|promptvars|aliases)$` after
/// stripping a `[-+]…o` prefix, lowercasing and removing `_-`.
fn zsh_risky_option(word: &str) -> bool {
    if lazy_prefix_marker(word, true, is_alnum, 'm') {
        return true;
    }
    let mut rest = word;
    // strip /^[-+][A-Za-z0-9]*?o/i — first `o` after the sign with only
    // alphanumerics before it.
    let chars: Vec<char> = word.chars().collect();
    if !chars.is_empty() && (chars[0] == '-' || chars[0] == '+') {
        let mut index = 1;
        while index < chars.len() {
            let c = chars[index];
            if c == 'o' || c == 'O' {
                rest = &word[chars[..=index].iter().map(|c| c.len_utf8()).sum::<usize>()..];
                break;
            }
            if !c.is_ascii_alphanumeric() {
                break;
            }
            index += 1;
        }
    }
    let normalized: String =
        rest.to_lowercase().chars().filter(|c| *c != '_' && *c != '-').collect();
    let stripped = normalized.strip_prefix("no").unwrap_or(&normalized);
    matches!(stripped, "globsubst" | "kshglob" | "promptsubst" | "promptvars" | "aliases")
}

fn is_fd_prefix(word: &str) -> bool {
    if !word.is_empty() && word.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if word.starts_with('{') && word.ends_with('}') {
        let inner = &word[1..word.len() - 1];
        let mut chars = inner.chars();
        let first = chars.next();
        if first == Some('_') || first.is_some_and(|c| c.is_ascii_alphabetic()) {
            return chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        }
    }
    false
}

fn is_assignable_name(word: &str) -> bool {
    let mut chars = word.chars();
    let first = chars.next();
    if first != Some('_') && !first.is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let body = word.strip_suffix('+').unwrap_or(word);
    body.chars().skip(1).all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    let first = chars.next();
    (first == Some('_') || first.is_some_and(|c| c.is_ascii_alphabetic()))
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !word.is_empty()
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// `ShellScan.scan` — bash/zsh/posix dialects.
pub fn scan(input: &str, dialect: Dialect) -> ScanResult {
    let chars: Vec<char> = input.chars().collect();
    if chars.len() > MAX_INPUT_LENGTH {
        return ScanResult::Opaque { reason: OpaqueReason::InvalidStructure };
    }
    let context = RefCell::new(BashContext::new(dialect));
    let mut state = BashState::new(&chars, &context, 0, None);
    let result = scan_bash(&mut state, 0, 0);
    let result = match result {
        Ok(list) => list,
        Err(reason) => return ScanResult::Opaque { reason },
    };
    let ctx = context.borrow();
    if ctx.bound_dollar && ctx.bound_open && ctx.variable_sink {
        return ScanResult::Opaque { reason: OpaqueReason::DynamicExecution };
    }
    ScanResult::Scanned { commands: commands_of(&result.commands) }
}

/// `ShellScan.scanPowerShell`.
pub fn scan_powershell(input: &str) -> ScanResult {
    let chars: Vec<char> = input.chars().collect();
    if chars.len() > MAX_INPUT_LENGTH {
        return ScanResult::Opaque { reason: OpaqueReason::InvalidStructure };
    }
    for &c in &chars {
        if matches!(c, '\0' | '\u{85}' | '\u{2013}'..='\u{2015}' | '\u{2018}'..='\u{201e}' | '\u{feff}')
        {
            return ScanResult::Opaque { reason: OpaqueReason::InvalidStructure };
        }
    }
    let budget = RefCell::new((MAX_INPUT_LENGTH * MAX_SUBSTITUTION_DEPTH) as i64);
    let skipped = HashSet::new();
    let result = scan_powershell_list(&chars, 0, 0, &budget, None, PowershellHash::None, &skipped);
    match result {
        Ok(list) => ScanResult::Scanned { commands: commands_of(&list.commands) },
        Err(reason) => ScanResult::Opaque { reason },
    }
}

// ---------------------------------------------------------------------------
// Bash scan core
// ---------------------------------------------------------------------------

struct BashList {
    commands: Vec<CmdRef>,
    end: usize,
}

fn end_bash_list(state: &mut BashState) -> Result<bool, OpaqueReason> {
    if state.pending() {
        finish_bash_command(state, false)?;
    }
    Ok(!state.dangling)
}

fn close_bash_list(state: &mut BashState, end: usize) -> Result<BashList, OpaqueReason> {
    if !end_bash_list(state)? {
        return Err(OpaqueReason::InvalidStructure);
    }
    if let Some(reason) = state.invalid {
        return Err(reason);
    }
    if !state.structures.is_empty() {
        return Err(OpaqueReason::CompoundCommand);
    }
    Ok(BashList { commands: std::mem::take(&mut state.commands), end })
}

fn push_heredoc_command(state: &mut BashState, command: CmdRef) {
    let start = state.command_start;
    let mut heredocs = state.heredocs_cell().borrow_mut();
    for heredoc in heredocs.iter_mut() {
        if heredoc.command.is_some() {
            continue;
        }
        heredoc.command = Some(Rc::clone(&command));
        heredoc.start = start;
    }
}

fn finish_bash_word(state: &mut BashState) -> Result<(), OpaqueReason> {
    if !state.word_started {
        return Ok(());
    }
    if state.word.contains("PS4") {
        state.context.borrow_mut().variable_sink = true;
    }
    let evaluation = bash_word_evaluation(state);
    if state.dialect() != Dialect::Bash
        && (is_nullcmd(&state.word)
            || ((state.assignment_word || evaluation == Some(BashEvaluation::Binding))
                && is_zsh_binding_name(&state.word)))
    {
        state.set_invalid(OpaqueReason::DynamicExecution);
    }
    if let Some(evaluation) = evaluation {
        let mut text = BashText {
            word: state.word.clone(),
            literal: state.literal.clone(),
            output: state.output.clone(),
        };
        if bash_evaluates_expansion(&mut text, evaluation, state.context) {
            state.set_invalid(OpaqueReason::DynamicExecution);
        }
    }
    if let Some(outer) = state.outer.as_mut() {
        outer.output.push(' ');
        outer.output.push_str(&state.literal);
        outer.output.push(' ');
        outer.output.push_str(&state.output);
    }
    let repeat_header = state.structures.last().is_some_and(|structure| {
        structure.kind == StructureKind::Repeat && structure.phase == StructurePhase::Header
    });
    if !state.redirect_target && repeat_header {
        if let Some(structure) = state.structures.last_mut() {
            structure.phase = StructurePhase::Do;
        }
    }
    if !state.redirect_target && !repeat_header {
        if !state.assignment_word && state.command_word_index < 0 {
            state.command_word_index = state.words.len() as isize;
        }
        if state.command_start.is_none() {
            state.command_start = Some(state.word_start);
        }
        state.words.push(std::mem::take(&mut state.word));
        // Unquoted trailing continuations are ignored syntax, not part of the
        // raw token.
        state.raw_words.push(slice(state.chars, state.word_start, state.word_end));
        state.command_end = state.word_end;
    }
    state.redirect_target = false;
    state.here_string = false;
    state.word.clear();
    state.literal.clear();
    state.output.clear();
    state.word_started = false;
    state.assignment_word = false;
    state.assignment_head_unsafe = false;
    Ok(())
}

/// How the shell evaluates the current word's literal text.
fn bash_word_evaluation(state: &BashState) -> Option<BashEvaluation> {
    if state.redirect_target {
        return if state.here_string { Some(BashEvaluation::Deferred) } else { None };
    }
    let structure = state.structures.last().copied();
    if structure
        .is_some_and(|s| s.kind == StructureKind::Repeat && s.phase == StructurePhase::Header)
    {
        return Some(BashEvaluation::Arithmetic);
    }
    if structure.is_some_and(|s| {
        s.kind == StructureKind::For && s.phase == StructurePhase::Header && s.saw_in
    }) {
        return Some(BashEvaluation::Deferred);
    }
    if state.command_word_index < 0 {
        return if state.assignment_word { Some(BashEvaluation::Deferred) } else { None };
    }
    let name = bash_builtin_index(state);
    let Some(command) = state.words.get(name).cloned() else {
        return None;
    };
    let dialect = state.dialect();
    if command == "let" || (dialect != Dialect::Bash && is_zsh_arithmetic_builtin(&command)) {
        return Some(BashEvaluation::Arithmetic);
    }
    if command == "set" {
        let arithmetic = state.words[name + 1..].iter().any(|word| regex_set_arithmetic(word));
        return if arithmetic {
            Some(BashEvaluation::Binding)
        } else {
            Some(BashEvaluation::Deferred)
        };
    }
    if is_binding_builtin(&command) {
        let options = &state.words[name + 1..];
        if options
            .iter()
            .any(|word| word.starts_with('-') && word.chars().any(|c| "EFin".contains(c)))
        {
            return Some(BashEvaluation::Arithmetic);
        }
        if state.assignment_word
            && is_assignment_declaration(&command)
            && options.iter().all(|word| !word.starts_with('-'))
            && (command == "export"
                || command == "readonly"
                || dialect == Dialect::Zsh
                || !regex_array_assign(&state.word))
        {
            return Some(BashEvaluation::Deferred);
        }
        return Some(BashEvaluation::Binding);
    }
    if command == "printf" || command == "print" {
        let print = command == "print";
        let mut options: Vec<&str> = state.words[name + 1..].iter().map(String::as_str).collect();
        options.push(state.word.as_str());
        return if options.iter().any(|word| regex_print_v(word, print)) {
            Some(BashEvaluation::Binding)
        } else {
            None
        };
    }
    if command == "wait" {
        let last = state.words.last().cloned().unwrap_or_default();
        return if regex_wait_p(&state.word) || regex_wait_p(&last) {
            Some(BashEvaluation::Binding)
        } else {
            None
        };
    }
    if command == "test" || command == "[" {
        if state.words.last().map(String::as_str) == Some("-v") {
            return Some(BashEvaluation::Binding);
        }
        if dialect != Dialect::Bash && state.words.last().map(String::as_str) == Some("-t") {
            return Some(BashEvaluation::Arithmetic);
        }
    }
    None
}

/// Index of the word naming the builtin, skipping precommand wrappers.
fn bash_builtin_index(state: &BashState) -> usize {
    let mut name = state.command_word_index.max(0) as usize;
    let dialect = state.dialect();
    loop {
        let current = state.words.get(name).cloned().unwrap_or_default();
        if !(is_bash_precommand(&current)
            || (dialect != Dialect::Bash && is_zsh_precommand(&current)))
        {
            return name;
        }
        let precommand = current;
        name += 1;
        while precommand != "-" && state.words.get(name).is_some_and(|word| word.starts_with('-')) {
            if precommand == "exec" && state.words.get(name).is_some_and(|word| regex_exec_a(word))
            {
                name += 1;
            }
            name += 1;
        }
    }
}

fn finish_bash_command(state: &mut BashState, boundary: bool) -> Result<(), OpaqueReason> {
    finish_bash_word(state)?;
    if state.redirect_target {
        state.set_invalid(OpaqueReason::InvalidRedirect);
    }
    state.redirect_target = false;
    if state.compound_end && !state.words.is_empty() {
        state.set_invalid(OpaqueReason::InvalidStructure);
    }
    let name = state.command_word_index;
    let in_header = state.in_header();
    if name >= 0 && state.words.get(name as usize).map(String::as_str).unwrap_or("").is_empty() {
        state.set_invalid(OpaqueReason::InvalidStructure);
    }
    if name >= 0 && !in_header {
        let command_start = state.command_start.unwrap_or(0);
        let resource =
            slice(state.chars, command_start, state.resource_end.unwrap_or(state.word_end))
                .trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n')
                .to_string();
        let raw_head = state.raw_words.first().cloned().unwrap_or_default();
        let declaration = name == 0 && is_declaration(&raw_head) && resource.starts_with(&raw_head);
        let redirect_word_count = state
            .redirect_word_count
            .filter(|count| *count < state.words.len())
            .map(|count| count - name as usize);
        let command = CmdRef::new(RefCell::new(Command {
            resource,
            words: state.words[name as usize..].to_vec(),
            raw_words: state.raw_words[name as usize..].to_vec(),
            word_ends: None,
            statement_head: false,
            declaration,
            redirect_word_count,
        }));
        state.commands.push(Rc::clone(&command));
        let builtin = bash_builtin_index(state);
        let builtin_name = state.words.get(builtin).cloned().unwrap_or_default();
        let builtin_args: Vec<String> = state.words[builtin + 1..].to_vec();
        if builtin_name == "eval" {
            state.context.borrow_mut().variable_sink = true;
        }
        if builtin_name == "set" && builtin_args.iter().any(|word| regex_set_keyword_flag(word)) {
            state.set_invalid(OpaqueReason::DynamicExecution);
        }
        // Dash expands an alias defined earlier in the same script.
        if builtin_name == "alias" && builtin_args.iter().any(|word| word.contains('=')) {
            state.set_invalid(OpaqueReason::DynamicExecution);
        }
        // Zsh options such as globsubst/kshglob/promptsubst enable evaluation.
        if state.dialect() != Dialect::Bash
            && (builtin_name == "emulate"
                || ((builtin_name == "setopt"
                    || builtin_name == "unsetopt"
                    || (builtin_name == "set"
                        && builtin_args.iter().any(|word| regex_option_o(word))))
                    && builtin_args.iter().any(|word| zsh_risky_option(word))))
        {
            state.set_invalid(OpaqueReason::DynamicExecution);
        }
        // Bash compgen -C and -F execute their argument immediately.
        if state.dialect() != Dialect::Zsh
            && builtin_name == "compgen"
            && builtin_args
                .iter()
                .any(|word| word.starts_with('-') && (word.contains('C') || word.contains('F')))
        {
            state.set_invalid(OpaqueReason::DynamicExecution);
        }
        if is_binding_builtin(&builtin_name) {
            let mut heredocs = state.heredocs_cell().borrow_mut();
            for heredoc in heredocs.iter_mut() {
                heredoc.binding = true;
            }
        }
        if state.resource_end.is_none() {
            push_heredoc_command(state, command);
        }
    }
    if boundary
        && state.words.is_empty()
        && !state.has_redirect
        && !state.compound_end
        && !in_header
    {
        state.set_invalid(OpaqueReason::InvalidStructure);
    }
    if let Some(limit) = state.ampersand_redirect_words {
        if state.words.len() > limit {
            state.set_invalid(OpaqueReason::InvalidRedirect);
        }
    }
    let nested = std::mem::take(&mut state.nested_commands);
    state.commands.extend(nested);
    if !in_header && (!state.words.is_empty() || state.has_redirect) {
        state.statement();
    }
    state.words.clear();
    state.raw_words.clear();
    state.command_word_index = -1;
    state.command_start = None;
    state.has_redirect = false;
    state.resource_end = None;
    state.redirect_word_count = None;
    state.ampersand_redirect_words = None;
    state.compound_end = false;
    state.dangling = false;
    Ok(())
}

fn scan_bash(state: &mut BashState, start: usize, depth: usize) -> Result<BashList, OpaqueReason> {
    let chars = state.chars;
    if depth > MAX_SUBSTITUTION_DEPTH || state.context.borrow().remaining < 0 {
        return Err(OpaqueReason::InvalidStructure);
    }
    let mut index = start;
    while index < chars.len() {
        spend(state.context)?;
        let char = at(chars, index);
        // Line continuations are removed before tokens are read.
        if char == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
            continue;
        }
        if !state.word_started {
            state.word_start = index;
            if is_blank(char) {
                index += 1;
                continue;
            }
            if char == '}' {
                let heredocs_empty = state.heredocs_cell().borrow().is_empty();
                if state.words.is_empty()
                    && !state.redirect_target
                    && (state.close == Some(Close::Brace) || state.close == Some(Close::NoFork))
                    && state.structures.is_empty()
                    && (!state.shared_heredocs.is_some() || heredocs_empty)
                    && (state.close == Some(Close::NoFork) || brace_close_ahead(chars, index + 1))
                {
                    return close_bash_list(state, index);
                }
                return Err(if state.structures.is_empty() {
                    OpaqueReason::InvalidStructure
                } else {
                    OpaqueReason::CompoundCommand
                });
            }
            if let Some(step) = scan_bash_command_start(state, index, depth)? {
                index = step + 1;
                continue;
            }
        }
        if char == '\\' && index + 1 >= chars.len() {
            return Err(OpaqueReason::UnterminatedEscape);
        }
        if !state.assignment_word
            && ("'\"\\".contains(char) || (char == '$' && "'\"".contains(at(chars, index + 1))))
        {
            state.assignment_head_unsafe = true;
        }
        let in_case_pattern = state
            .structures
            .last()
            .is_some_and(|structure| structure.phase == StructurePhase::Pattern);
        // Elsewhere a parenthesized group inside a word is a Zsh glob
        // qualifier, which can run code.
        let array = (char == '=' || char == '(')
            && state.assignment_word
            && state.word.chars().count() > 1
            && state.word.ends_with('=')
            && !state.word.ends_with("==")
            && (state.command_word_index < 0
                || is_assignment_declaration(
                    state
                        .raw_words
                        .get(state.command_word_index.max(0) as usize)
                        .map(String::as_str)
                        .unwrap_or(""),
                ));
        if char == '=' && (!state.word_started || array) && at(chars, index + 1) == '(' {
            let end = scan_bash_nested(
                chars,
                index + 2,
                depth,
                state.context,
                &mut state.nested_commands,
                Close::Paren,
                None,
            )?;
            state.word_started = true;
            state.word.push_str(&slice(chars, index, end + 1));
            state.word_end = end + 1;
            index = end + 1;
            continue;
        }
        if "\\$'\"`<>".contains(char) {
            let allow_bracket = !state.assignment_word
                && state.command_word_index >= 0
                && !state.raw_words.first().map(String::as_str).is_some_and(is_declaration);
            // v2 passes the whole state as the unit's text sink (its
            // word/literal/output fields are duck-typed). Scan into a
            // scratch text and merge back — the unit functions never touch
            // other state fields, so this is exact.
            let mut scratch = BashText {
                word: std::mem::take(&mut state.word),
                literal: std::mem::take(&mut state.literal),
                output: std::mem::take(&mut state.output),
            };
            let unit = scan_bash_unit(
                chars,
                index,
                depth,
                state.context,
                &mut state.nested_commands,
                BashTextMode::Word,
                allow_bracket,
                Some(&mut scratch),
            );
            let unit = match unit {
                Ok(unit) => unit,
                Err(reason) => {
                    state.word = scratch.word;
                    state.literal = scratch.literal;
                    state.output = scratch.output;
                    return Err(reason);
                }
            };
            state.word = scratch.word;
            state.literal = scratch.literal;
            state.output = scratch.output;
            if let Some(unit) = unit {
                state.word_started = true;
                state.word_end = unit + 1;
                index = unit + 1;
                continue;
            }
        }
        if char == '['
            && !in_case_pattern
            && !state.assignment_word
            && !state.assignment_head_unsafe
            && state.command_word_index < 0
            && is_identifier(&state.word)
        {
            let mut subscript: Vec<CmdRef> = Vec::new();
            let end = scan_bash_subscript(
                chars,
                index + 1,
                depth,
                state.context,
                &mut subscript,
                &Spans::ASSIGNMENT_SUBSCRIPT,
            )?;
            let assign = bash_operator(chars, end + 1, &["+=", "="])?;
            if assign.is_some() {
                // Dash word-splits unquoted expansions inside name[sub]=value.
                if state.dialect() == Dialect::Posix
                    && slice(chars, index + 1, end).chars().any(|c| c == '$' || c == '`')
                {
                    return Err(OpaqueReason::InvalidStructure);
                }
                state.nested_commands.extend(subscript);
                state.assignment_word = true;
                state.word.push_str(&slice(chars, index, end + 1));
                state.word_end = end + 1;
                index = end + 1;
                continue;
            }
        }
        if char == '('
            && (array
                || ((in_case_pattern || state.dialect() == Dialect::Bash)
                    && state.word.ends_with(['?', '*', '+', '@', '!'])))
        {
            let mode = if array { ArrayMode::Array } else { ArrayMode::Pattern };
            let end = scan_bash_array_or_pattern(
                chars,
                index,
                depth + 1,
                state.context,
                &mut state.nested_commands,
                mode,
            )?;
            if bash_crosses_heredoc(state, index, end) {
                return Err(OpaqueReason::Heredoc);
            }
            // Zsh ends an array assignment at its closing parenthesis.
            if array && end + 1 < chars.len() && !" \t\n;&|<>)".contains(at(chars, end + 1)) {
                return Err(OpaqueReason::InvalidStructure);
            }
            state.word_started = true;
            state.word.push_str(&slice(chars, index, end + 1));
            state.word_end = end + 1;
            index = end + 1;
            continue;
        }
        if char == '#' && !state.word_started {
            let Some(newline) = chars[index..].iter().position(|&c| c == '\n').map(|p| p + index)
            else {
                break;
            };
            index = newline + 1;
            continue;
        }
        if "<>&|;\n".contains(char) {
            let step = scan_bash_operator_or_separator(state, index, depth)?;
            index = step + 1;
            continue;
        }
        if in_case_pattern && char == ')' {
            finish_bash_word(state)?;
            continue;
        }
        if char == ')' {
            if state.close != Some(Close::Paren)
                || !state.structures.is_empty()
                || (state.shared_heredocs.is_some() && !state.heredocs_cell().borrow().is_empty())
            {
                return Err(OpaqueReason::CompoundCommand);
            }
            return close_bash_list(state, index);
        }
        if char == '(' {
            return Err(if state.word_started {
                OpaqueReason::DynamicExecution
            } else {
                OpaqueReason::CompoundCommand
            });
        }
        if char.is_whitespace() && !" \t\n".contains(char) {
            return Err(OpaqueReason::InvalidStructure);
        }
        if is_blank(char) {
            finish_bash_word(state)?;
            index += 1;
            continue;
        }
        state.word_started = true;
        if char == '='
            && !state.assignment_word
            && !state.assignment_head_unsafe
            && is_assignable_name(&state.word)
        {
            state.assignment_word = true;
        }
        state.word.push(char);
        state.literal.push(char);
        state.word_end = index + 1;
        index += 1;
    }

    if state.close.is_some() {
        return Err(if state.close == Some(Close::Paren) {
            OpaqueReason::CommandSubstitution
        } else {
            OpaqueReason::InvalidStructure
        });
    }
    if !state.heredocs_cell().borrow().is_empty() {
        return Err(OpaqueReason::Heredoc);
    }
    let end = chars.len();
    close_bash_list(state, end)
}

/// `scanBashOperatorOrSeparator` — returns the last consumed index.
fn scan_bash_operator_or_separator(
    state: &mut BashState,
    index: usize,
    depth: usize,
) -> Result<usize, OpaqueReason> {
    let chars = state.chars;
    let char = at(chars, index);
    let redirect =
        if "<>&".contains(char) { bash_operator(chars, index, BASH_REDIRECTS)? } else { None };
    let structure = state.structures.last().copied();
    if let Some(redirect) = redirect {
        if structure
            .is_some_and(|s| s.kind == StructureKind::Case && s.phase != StructurePhase::Body)
        {
            return Err(OpaqueReason::InvalidRedirect);
        }
        state.has_redirect = true;
        if state.command_start.is_none() {
            state.command_start = Some(state.word_start);
        }
        if state.redirect_target {
            state.set_invalid(OpaqueReason::InvalidRedirect);
        }
        let fd_prefix =
            state.word_started && !state.assignment_head_unsafe && is_fd_prefix(&state.word);
        if fd_prefix {
            // A continuation separates the legacy number token from the
            // redirect descriptor.
            if state.word_end < index {
                state.command_end = state.word_end;
            }
            state.word.clear();
            state.literal.clear();
            state.output.clear();
            state.word_started = false;
        }
        if !fd_prefix {
            finish_bash_word(state)?;
        }
        if redirect.starts_with('&') && state.dialect() == Dialect::Posix {
            if state.ampersand_redirect_words.is_none() {
                state.ampersand_redirect_words = Some(state.words.len());
            }
        }
        // Trailing redirects wrap a whole list/pipeline in the legacy
        // grammar; prefix redirects remain part of the command.
        if state.redirect_word_count.is_none() && state.command_word_index >= 0 {
            state.redirect_word_count = Some(state.words.len());
            if state.dangling {
                state.resource_end = Some(state.command_end);
            }
        }
        if redirect == "<<" || redirect == "<<-" {
            let Some(delimiter) =
                bash_heredoc_delimiter(chars, index + redirect.chars().count(), redirect == "<<-")
            else {
                return Err(OpaqueReason::InvalidRedirect);
            };
            let end = delimiter.end;
            state.heredocs_cell().borrow_mut().push(delimiter.heredoc);
            state.word_end = end + 1;
            state.redirect_target = false;
            return Ok(end);
        }
        state.redirect_target = true;
        state.here_string = redirect == "<<<";
        return Ok(index + redirect.chars().count() - 1);
    }
    if structure.is_some_and(|s| s.phase == StructurePhase::Pattern) && char == '|' {
        finish_bash_word(state)?;
        return Ok(index - 1);
    }
    let case_body =
        structure.is_some_and(|s| s.kind == StructureKind::Case && s.phase == StructurePhase::Body);
    let operator = if case_body {
        bash_operator(chars, index, &bash_case_operators())?
    } else {
        bash_operator(chars, index, BASH_LIST_OPERATORS)?
    };
    let separator = operator.unwrap_or_else(|| char.to_string());
    if case_body && separator.starts_with(';') && separator.chars().count() > 1 {
        if !end_bash_list(state)? {
            return Err(OpaqueReason::InvalidStructure);
        }
        if let Some(structure) = state.structures.last_mut() {
            structure.phase = StructurePhase::Pattern;
            structure.pattern_started = false;
        }
        return Ok(index + separator.chars().count() - 1);
    }
    if structure.is_some_and(|s| {
        s.kind == StructureKind::Case
            && (s.phase == StructurePhase::Header || s.phase == StructurePhase::Pattern)
    }) {
        if separator != "\n" {
            return Err(OpaqueReason::CompoundCommand);
        }
        if structure.is_some_and(|s| s.phase == StructurePhase::Pattern)
            && (state.word_started || !state.words.is_empty())
        {
            return Err(OpaqueReason::CompoundCommand);
        }
        finish_bash_word(state)?;
        return read_bash_heredocs(state, index, depth);
    }
    if structure.is_some_and(|s| {
        (s.kind == StructureKind::For || s.kind == StructureKind::Repeat)
            && (s.phase == StructurePhase::Header || s.phase == StructurePhase::Do)
    }) {
        if separator != ";" && separator != "\n" {
            return Err(OpaqueReason::CompoundCommand);
        }
        if structure.is_some_and(|s| {
            s.phase == StructurePhase::Header
                && !s.saw_in
                && !state.word_started
                && state.words.is_empty()
        }) {
            return Err(OpaqueReason::CompoundCommand);
        }
    }
    if separator == "\n" && !state.pending() {
        return read_bash_heredocs(state, index, depth);
    }
    finish_bash_command(state, true)?;
    if state
        .structures
        .last()
        .is_some_and(|s| s.kind == StructureKind::For && s.phase == StructurePhase::Header)
    {
        if let Some(structure) = state.structures.last_mut() {
            structure.phase = StructurePhase::Do;
        }
    }
    state.dangling = separator != "&" && separator != ";" && separator != "\n";
    // Reprocess the newline to read pending heredoc bodies.
    if separator == "\n" && !state.heredocs_cell().borrow().is_empty() {
        return Ok(index - 1);
    }
    Ok(index + separator.chars().count() - 1)
}

/// `bashOperator` — the first listed operator at `index`.
fn bash_operator(
    chars: &[char],
    index: usize,
    operators: &[&str],
) -> Result<Option<String>, OpaqueReason> {
    let mut text = String::new();
    let mut cursor = index;
    while text.chars().count() < 3 && cursor < chars.len() {
        if at(chars, cursor) == '\\' && at(chars, cursor + 1) == '\n' {
            cursor += 1;
        } else {
            text.push(at(chars, cursor));
        }
        cursor += 1;
    }
    let operator = operators.iter().find(|candidate| text.starts_with(**candidate));
    if let Some(operator) = operator {
        let length = operator.chars().count();
        if slice(chars, index, index + length) != **operator {
            return Err(OpaqueReason::InvalidStructure);
        }
        Ok(Some((*operator).to_string()))
    } else {
        Ok(None)
    }
}

/// Reads pending heredoc bodies after the newline at `index`.
fn read_bash_heredocs(
    state: &mut BashState,
    index: usize,
    depth: usize,
) -> Result<usize, OpaqueReason> {
    let mut end = index;
    let heredocs: Vec<BashHeredoc> = std::mem::take(&mut *state.heredocs_cell().borrow_mut());
    for heredoc in heredocs {
        let Some(body) = bash_heredoc(state.chars, end + 1, &heredoc) else {
            return Err(OpaqueReason::Heredoc);
        };
        if let Some(command) = &heredoc.command {
            command.borrow_mut().resource =
                slice(state.chars, heredoc.start.unwrap_or(0), body.end).trim().to_string();
        }
        let mut text = if heredoc.quoted {
            BashText {
                word: body.source.clone(),
                literal: body.source.clone(),
                output: String::new(),
            }
        } else {
            BashText::default()
        };
        if !heredoc.quoted {
            scan_bash_span(
                &body.source_chars,
                0,
                depth,
                state.context,
                &mut state.commands,
                &Spans::HEREDOC,
                true,
                Some(&mut text),
            )?;
        }
        if heredoc.binding {
            bash_evaluates_expansion(&mut text, BashEvaluation::Deferred, state.context);
        }
        end = body.end;
    }
    Ok(end)
}

/// Whether a construct read as one unit holds a newline that would start
/// pending heredoc bodies.
fn bash_crosses_heredoc(state: &BashState, start: usize, end: usize) -> bool {
    !state.heredocs_cell().borrow().is_empty() && slice(state.chars, start, end).contains('\n')
}

fn scan_bash_command_start(
    state: &mut BashState,
    index: usize,
    depth: usize,
) -> Result<Option<usize>, OpaqueReason> {
    let chars = state.chars;
    let char = at(chars, index);
    let structure = state.structures.last().copied();
    let Some(structure) = structure else {
        // No open structure: the keyword scan still runs (if/for/… heads).
        if let Some(keyword_step) = scan_bash_keyword(state, index)? {
            return Ok(Some(keyword_step));
        }
        return scan_bash_tail(state, index, depth);
    };
    if structure.phase == StructurePhase::Pattern
        && !structure.pattern_started
        && state.words.is_empty()
        && char == '('
    {
        if let Some(last) = state.structures.last_mut() {
            last.pattern_started = true;
        }
        return Ok(Some(index));
    }
    if structure.phase == StructurePhase::Pattern && char == '|' {
        if state.words.is_empty() {
            return Err(OpaqueReason::CompoundCommand);
        }
        finish_bash_word(state)?;
        if let Some(last) = state.structures.last_mut() {
            last.pattern_started = true;
        }
        return Ok(Some(index));
    }
    if structure.phase == StructurePhase::Pattern && char == ')' {
        if state.words.is_empty() {
            return Err(OpaqueReason::CompoundCommand);
        }
        finish_bash_command(state, false)?;
        if let Some(last) = state.structures.last_mut() {
            last.phase = StructurePhase::Body;
            last.count = 0;
        }
        return Ok(Some(index));
    }
    if structure.kind == StructureKind::For
        && structure.phase == StructurePhase::Header
        && char == '('
        && at(chars, index + 1) != '('
    {
        let end = scan_bash_array_or_pattern(
            chars,
            index,
            depth + 1,
            state.context,
            &mut state.nested_commands,
            ArrayMode::Array,
        )?;
        if bash_crosses_heredoc(state, index, end) {
            return Err(OpaqueReason::Heredoc);
        }
        finish_bash_command(state, false)?;
        // Zsh permits a sublist or brace group directly after the value list.
        if let Some(last) = state.structures.last_mut() {
            last.phase = StructurePhase::Do;
            last.parenthesized = true;
        }
        if !do_ahead(chars, end + 1) {
            state.structures.pop();
        }
        return Ok(Some(end));
    }
    if structure.kind == StructureKind::Repeat
        && structure.phase == StructurePhase::Do
        && char != '\n'
        && char != '#'
        && char != ';'
        && token_re(chars, index).as_deref() != Some("do")
    {
        state.structures.pop();
    }
    if let Some(keyword_step) = scan_bash_keyword(state, index)? {
        return Ok(Some(keyword_step));
    }
    if state.structures.last().is_some_and(|structure| {
        structure.kind == StructureKind::For
            && structure.phase == StructurePhase::Do
            && char != '\n'
            && char != '#'
            && char != ';'
    }) {
        if char != '{' || !space_continuation_ahead(chars, index + 1) {
            return Err(OpaqueReason::CompoundCommand);
        }
        state.structures.pop();
    }
    scan_bash_tail(state, index, depth)
}

/// The keyword-independent tail of `scanBashCommandStart`.
fn scan_bash_tail(
    state: &mut BashState,
    index: usize,
    depth: usize,
) -> Result<Option<usize>, OpaqueReason> {
    let chars = state.chars;
    let char = at(chars, index);
    let at_start = state.at_command_start();
    let function_head =
        if at_start && char == '(' { bash_function_head_length(chars, index) } else { 0 };
    if function_head > 0 {
        return Ok(Some(index + function_head - 1));
    }
    if at_start && char == '!' && negation_ahead(chars, index + 1) {
        return Ok(Some(index));
    }
    let for_header = state.structures.last().is_some_and(|structure| {
        structure.kind == StructureKind::For && structure.phase == StructurePhase::Header
    });
    let compound_open = if (at_start || for_header) && (char == '(' || char == '[') {
        bash_operator(chars, index, &["((", "[["])?
    } else {
        None
    };
    if compound_open.as_deref() == Some("((") {
        if for_header && !state.words.is_empty() {
            return Err(OpaqueReason::CompoundCommand);
        }
        let span = if for_header { &Spans::FOR_ARITHMETIC } else { &Spans::ARITHMETIC };
        match scan_bash_arithmetic(
            chars,
            index + 2,
            depth,
            state.context,
            &mut state.commands,
            span,
        )? {
            ArithmeticOutcome::End(end) => {
                if bash_crosses_heredoc(state, index, end) {
                    return Err(OpaqueReason::Heredoc);
                }
                if !for_header && state.dialect() == Dialect::Posix {
                    // Dash reads `((` as two nested subshells; report both
                    // readings.
                    let subshells = scan_bash_subshell(
                        chars,
                        index + 1,
                        depth + 1,
                        state.context,
                        state.heredocs_cell(),
                    )?;
                    if subshells.end != end {
                        return Err(OpaqueReason::CompoundCommand);
                    }
                    let mut seen: HashSet<String> = state
                        .commands
                        .iter()
                        .map(|cmd| {
                            let cmd = cmd.borrow();
                            format!("{}\u{2}{}", cmd.resource, cmd.words.join("\u{1}"))
                        })
                        .collect();
                    for command in subshells.commands {
                        let key = {
                            let cmd = command.borrow();
                            format!("{}\u{2}{}", cmd.resource, cmd.words.join("\u{1}"))
                        };
                        if seen.insert(key) {
                            state.commands.push(command);
                        }
                    }
                }
                if for_header {
                    if let Some(structure) = state.structures.last_mut() {
                        structure.phase = StructurePhase::Do;
                        structure.saw_in = true;
                    }
                } else {
                    state.statement();
                    state.compound_end = true;
                }
                return Ok(Some(end));
            }
            ArithmeticOutcome::Subshell => {
                if for_header {
                    return Err(OpaqueReason::InvalidStructure);
                }
            }
        }
    }
    if at_start && (char == '(' || (char == '{' && space_continuation_ahead(chars, index + 1))) {
        // A group shares its parent's heredocs.
        let group = scan_bash_group(
            chars,
            index + 1,
            depth + 1,
            state.context,
            state.heredocs_cell(),
            char,
        )?;
        if slice(chars, index + 1, group.end).trim().is_empty() {
            return Err(OpaqueReason::InvalidStructure);
        }
        state.commands.extend(group.commands);
        state.statement();
        state.compound_end = true;
        return Ok(Some(group.end));
    }
    if at_start
        && compound_open.as_deref() == Some("[[")
        && space_continuation_ahead(chars, index + 2)
    {
        let end =
            scan_bash_conditional(chars, index + 2, depth, state.context, &mut state.commands)?;
        if bash_crosses_heredoc(state, index, end) {
            return Err(OpaqueReason::Heredoc);
        }
        state.statement();
        state.compound_end = true;
        return Ok(Some(end));
    }
    Ok(None)
}

fn scan_bash_keyword(state: &mut BashState, index: usize) -> Result<Option<usize>, OpaqueReason> {
    let chars = state.chars;
    let char = at(chars, index);
    if !(char.is_ascii_uppercase() || char.is_ascii_lowercase() || char == '_') {
        return Ok(None);
    }
    let token = token_re(chars, index).unwrap_or_default();
    let end = index + token.chars().count();
    let structure = state.structures.last().copied();
    if let Some(structure) = structure {
        if structure.kind == StructureKind::Case
            && structure.phase == StructurePhase::Header
            && token == "in"
        {
            if state.words.len() != 1 || state.has_redirect {
                return Err(OpaqueReason::CompoundCommand);
            }
            finish_bash_command(state, false)?;
            if let Some(last) = state.structures.last_mut() {
                last.phase = StructurePhase::Pattern;
                last.pattern_started = false;
            }
            return Ok(Some(end - 1));
        }
        if structure.kind == StructureKind::For
            && (structure.phase == StructurePhase::Header || structure.phase == StructurePhase::Do)
            && !structure.saw_in
            && !structure.parenthesized
            && token == "in"
            && (structure.phase == StructurePhase::Do || !state.words.is_empty())
        {
            if let Some(last) = state.structures.last_mut() {
                last.saw_in = true;
                last.phase = StructurePhase::Header;
            }
            return Ok(Some(end - 1));
        }
        if structure.phase == StructurePhase::Pattern
            && !structure.pattern_started
            && state.words.is_empty()
            && token == "esac"
        {
            state.structures.pop();
            state.statement();
            state.compound_end = true;
            return Ok(Some(end - 1));
        }
        // POSIX `for name do` omits the in list.
        if structure.kind == StructureKind::For
            && structure.phase == StructurePhase::Header
            && !structure.saw_in
            && state.words.len() == 1
            && !state.has_redirect
            && token == "do"
        {
            finish_bash_command(state, false)?;
            if let Some(last) = state.structures.last_mut() {
                last.phase = StructurePhase::Do;
            }
        }
    }
    let in_header = state.in_header();
    if !token.is_empty()
        && ["then", "elif", "else", "fi", "do", "done", "esac"].contains(&token.as_str())
        && state.words.is_empty()
        && !state.redirect_target
        && (!in_header
            || (token == "do"
                && state
                    .structures
                    .last()
                    .is_some_and(|structure| structure.phase == StructurePhase::Do)))
    {
        // Bash/Dash read a reserved word after a redirect as a command word.
        let list_ok = if state.has_redirect && state.dialect() != Dialect::Zsh {
            false
        } else {
            end_bash_list(state)?
        };
        let Some(structure) = state.structures.last().copied() else {
            return Err(OpaqueReason::CompoundCommand);
        };
        if !list_ok {
            return Err(OpaqueReason::CompoundCommand);
        }
        match token.as_str() {
            "then" => {
                if structure.kind != StructureKind::If
                    || structure.phase != StructurePhase::Condition
                    || structure.count == 0
                {
                    return Err(OpaqueReason::CompoundCommand);
                }
                if let Some(last) = state.structures.last_mut() {
                    last.phase = StructurePhase::Body;
                    last.count = 0;
                }
                return Ok(Some(end - 1));
            }
            "elif" | "else" => {
                if structure.kind != StructureKind::If
                    || structure.phase != StructurePhase::Body
                    || structure.count == 0
                    || structure.saw_else
                {
                    return Err(OpaqueReason::CompoundCommand);
                }
                if let Some(last) = state.structures.last_mut() {
                    last.phase = if token == "elif" {
                        StructurePhase::Condition
                    } else {
                        StructurePhase::Body
                    };
                    last.saw_else = token == "else";
                    last.count = 0;
                }
                return Ok(Some(end - 1));
            }
            "do" => {
                let kind_ok = matches!(
                    structure.kind,
                    StructureKind::For
                        | StructureKind::While
                        | StructureKind::Until
                        | StructureKind::Repeat
                );
                let phase_ok = structure.phase == StructurePhase::Condition
                    || structure.phase == StructurePhase::Do;
                let count_ok = structure.kind == StructureKind::For
                    || structure.kind == StructureKind::Repeat
                    || structure.count > 0;
                if !kind_ok || !phase_ok || !count_ok {
                    return Err(OpaqueReason::CompoundCommand);
                }
                if let Some(last) = state.structures.last_mut() {
                    last.phase = StructurePhase::Body;
                    last.count = 0;
                }
                return Ok(Some(end - 1));
            }
            _ => {}
        }
        if (token == "fi"
            && (structure.kind != StructureKind::If
                || structure.phase != StructurePhase::Body
                || structure.count == 0))
            || (token == "done"
                && (!matches!(
                    structure.kind,
                    StructureKind::For
                        | StructureKind::While
                        | StructureKind::Until
                        | StructureKind::Repeat
                ) || structure.phase != StructurePhase::Body
                    || structure.count == 0))
            || (token == "esac"
                && (structure.kind != StructureKind::Case
                    || structure.phase == StructurePhase::Header))
        {
            return Err(OpaqueReason::CompoundCommand);
        }
        state.structures.pop();
        state.statement();
        state.compound_end = true;
        return Ok(Some(end - 1));
    }
    if !state.at_command_start() {
        return Ok(None);
    }
    let definition_length = bash_function_head_length(chars, index);
    if definition_length > 0 {
        return Ok(Some(index + definition_length - 1));
    }
    let dialect = state.dialect();
    if ["if", "while", "until", "for", "select", "case"].contains(&token.as_str())
        || (token == "repeat" && dialect != Dialect::Bash)
    {
        if index + state.structures.len() >= MAX_SUBSTITUTION_DEPTH {
            return Err(OpaqueReason::CompoundCommand);
        }
        let kind = match token.as_str() {
            "if" => StructureKind::If,
            "while" => StructureKind::While,
            "until" => StructureKind::Until,
            "for" | "select" => StructureKind::For,
            "case" => StructureKind::Case,
            _ => StructureKind::Repeat,
        };
        let phase = if ["for", "select", "case", "repeat"].contains(&token.as_str()) {
            StructurePhase::Header
        } else {
            StructurePhase::Condition
        };
        state.structures.push(Structure::new(kind, phase));
        // The compound command itself satisfies a preceding list operator.
        state.dangling = false;
        return Ok(Some(end - 1));
    }
    if token == "coproc" {
        if let Some(length) = coproc_ahead(chars, index) {
            return Ok(Some(index + length - 1));
        }
    }
    if token == "time" {
        if let Some(length) = time_ahead(chars, index) {
            return Ok(Some(index + length - 1));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Unit scanning (quoting/expansion)
// ---------------------------------------------------------------------------

/// `scanBashUnit` — scans one quoting or expansion unit at `index`.
fn scan_bash_unit(
    chars: &[char],
    index: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    mode: BashTextMode,
    allow_bracket: bool,
    mut text: Option<&mut BashText>,
) -> Result<Option<usize>, OpaqueReason> {
    let char = at(chars, index);
    let next = at(chars, index + 1);
    if char == '\\' {
        if mode == BashTextMode::Quoted && !"$`\"\\\n".contains(next) {
            return Ok(None);
        }
        if let Some(text) = text.as_deref_mut() {
            if next != '\n' {
                text.word.push(next);
                text.literal.push(next);
            }
        }
        return Ok(Some(index + 1));
    }
    if char == '$' && context.borrow().dialect != Dialect::Bash && zsh_evaluates(chars, index) {
        return Err(OpaqueReason::DynamicExecution);
    }
    if char == '$'
        && context.borrow().dialect != Dialect::Bash
        && (mode == BashTextMode::Word || mode == BashTextMode::Quoted)
    {
        if let Some(bracket) = zsh_subscript_bracket(chars, index) {
            let mut subscript = BashText::default();
            let span = if mode == BashTextMode::Quoted {
                &Spans::QUOTED_SUBSCRIPT
            } else {
                &Spans::SUBSCRIPT
            };
            // The span starts AFTER the `[` (the regex match consumes it).
            let end = scan_bash_span(
                chars,
                bracket + 1,
                depth,
                context,
                commands,
                span,
                false,
                Some(&mut subscript),
            )?;
            if bash_evaluates_expansion(&mut subscript, BashEvaluation::Arithmetic, context) {
                return Err(OpaqueReason::DynamicExecution);
            }
            if let Some(text) = text.as_deref_mut() {
                text.word.push_str(&format!(
                    "{}{}]",
                    slice(chars, index, bracket + 1),
                    subscript.word
                ));
            }
            return Ok(Some(end));
        }
    }
    let opener = if char == '$' {
        bash_operator(chars, index, &["$$", "$((", "$(", "${", "$[", "$'", "$\""])?
    } else if mode == BashTextMode::Word && (char == '<' || char == '>') {
        let op = format!("{char}(");
        bash_operator(chars, index, &[op.as_str()])?
    } else {
        None
    };
    if opener.as_deref() == Some("$$") {
        let quote_start = line_continuation_end(chars, index + 2);
        if at(chars, quote_start) == '\'' {
            let Some(next_quote) = chars[quote_start + 1..]
                .iter()
                .position(|&c| c == '\'')
                .map(|p| p + quote_start + 1)
            else {
                return Err(OpaqueReason::UnterminatedQuote);
            };
            // Zsh/Bash 3.2 inside ${...} parse $$'...' with ANSI-C escapes
            // while Bash 5/Dash treat $$ as PID.
            if slice(chars, quote_start + 1, next_quote).contains('\\')
                && chars[next_quote + 1..].contains(&'\'')
            {
                return Err(OpaqueReason::UnterminatedQuote);
            }
        }
        if let Some(text) = text.as_deref_mut() {
            text.word.push_str("$$");
        }
        return Ok(Some(index + 1));
    }
    if mode == BashTextMode::Word && opener.as_deref() == Some("$'") {
        let Some(quote) = bash_ansi_quote(chars, index + 1) else {
            return Err(OpaqueReason::UnterminatedQuote);
        };
        let first_quote = chars[index + 2..].iter().position(|&c| c == '\'').map(|p| p + index + 2);
        // Dash closes the single-quoted span at the first `'`, even after `\`.
        let dialect = context.borrow().dialect;
        if quote.end != first_quote.unwrap_or(usize::MAX)
            && ((dialect != Dialect::Bash && index >= 2 && at(chars, index - 2) == '$')
                || (dialect == Dialect::Posix && {
                    let later = match first_quote {
                        Some(first_quote) => chars[first_quote + 1..]
                            .iter()
                            .position(|&c| c == '\'')
                            .map(|p| p + first_quote + 1),
                        None => None,
                    };
                    later.is_some_and(|later| later < quote.end) || {
                        let tail = match first_quote {
                            Some(first_quote) => slice(chars, first_quote + 1, usize::MAX),
                            None => chars.iter().collect(),
                        };
                        let escaped_quote = tail.contains("\\'");
                        tail.contains('#')
                            || tail.contains('"')
                            || tail.contains('\n')
                            || escaped_quote
                    }
                }))
        {
            return Err(OpaqueReason::UnterminatedQuote);
        }
        if let Some(text) = text.as_deref_mut() {
            text.word.push_str(&quote.value);
            text.literal.push_str(&quote.value);
        }
        return Ok(Some(quote.end));
    }
    if mode == BashTextMode::Arithmetic && opener.as_deref() == Some("$'") {
        let Some(quote) = bash_ansi_quote(chars, index + 1) else {
            return Err(OpaqueReason::InvalidStructure);
        };
        let first_quote = chars[index + 2..].iter().position(|&c| c == '\'').map(|p| p + index + 2);
        if first_quote != Some(quote.end) || quote.value.chars().any(|c| "()[];\"}".contains(c)) {
            return Err(OpaqueReason::InvalidStructure);
        }
        if let Some(text) = text.as_deref_mut() {
            text.word.push_str(&quote.value);
            text.literal.push_str(&quote.value);
        }
        let end = scan_bash_span(
            chars,
            index + 2,
            depth,
            context,
            commands,
            &Spans::ARITHMETIC_QUOTE,
            allow_bracket,
            text.as_deref_mut(),
        )?;
        return Ok(Some(end));
    }
    if mode == BashTextMode::Word && char == '\'' {
        let Some(end) = chars[index + 1..].iter().position(|&c| c == '\'').map(|p| p + index + 1)
        else {
            return Err(OpaqueReason::UnterminatedQuote);
        };
        if let Some(text) = text.as_deref_mut() {
            let inner = slice(chars, index + 1, end);
            text.word.push_str(&inner);
            text.literal.push_str(&inner);
        }
        return Ok(Some(end));
    }
    if mode == BashTextMode::Arithmetic && char == '\'' {
        let end = scan_bash_span(
            chars,
            index + 1,
            depth,
            context,
            commands,
            &Spans::ARITHMETIC_QUOTE,
            allow_bracket,
            text.as_deref_mut(),
        )?;
        return Ok(Some(end));
    }
    if mode == BashTextMode::Word && opener.as_deref() == Some("$\"") {
        let end = scan_bash_span(
            chars,
            index + 2,
            depth,
            context,
            commands,
            &Spans::DOUBLE,
            allow_bracket,
            text.as_deref_mut(),
        )?;
        return Ok(Some(end));
    }
    if mode != BashTextMode::Quoted && char == '"' {
        let span = if mode == BashTextMode::Arithmetic {
            &Spans::ARITHMETIC_DOUBLE
        } else {
            &Spans::DOUBLE
        };
        let end = scan_bash_span(
            chars,
            index + 1,
            depth,
            context,
            commands,
            span,
            allow_bracket,
            text.as_deref_mut(),
        )?;
        return Ok(Some(end));
    }
    let opener_is_unit = opener.as_deref().is_some_and(|opener| opener != "$'" && opener != "$\"");
    let end = if opener.as_deref() == Some("<(") || opener.as_deref() == Some(">(") {
        Some(scan_bash_nested(
            chars,
            index + 2,
            depth,
            context,
            commands,
            Close::Paren,
            text.as_deref_mut(),
        )?)
    } else if opener_is_unit || char == '`' {
        Some(scan_bash_dollar_or_backtick(
            chars,
            index,
            depth,
            context,
            commands,
            mode != BashTextMode::Word,
            allow_bracket,
            text.as_deref_mut(),
        )?)
    } else {
        None
    };
    if end.is_none() && char == '$' && "_0123456789@*#?$!-".contains(next_alnum(chars, index)) {
        if let Some(text) = text.as_deref_mut() {
            text.word.push('$');
        }
        return Ok(Some(index));
    }
    if let (Some(text), Some(end)) = (text, end) {
        text.word.push_str(&slice(chars, index, end + 1));
    }
    Ok(end)
}

fn next_alnum(chars: &[char], index: usize) -> char {
    at(chars, index + 1)
}

/// Zsh flag groups such as `${(e)name}` evaluate a value; `~` modifiers glob.
fn zsh_evaluates(chars: &[char], index: usize) -> bool {
    let mut at_index = index + 1;
    if at(chars, at_index) == '{'
        && at(chars, at_index + 1) == '\\'
        && at(chars, at_index + 2) == '\n'
    {
        return true;
    }
    if at(chars, at_index) == '{' && at(chars, at_index + 1) == '(' {
        let Some(end) = zsh_safe_flags_end(chars, index) else {
            return true;
        };
        at_index = end;
    }
    if at(chars, at_index) == '{' {
        at_index += 1;
    }
    zsh_glob_modifier(chars, at_index)
}

/// Taint analysis: literal expansions reaching arithmetic or sinks.
fn bash_evaluates_expansion(
    text: &mut BashText,
    evaluation: BashEvaluation,
    context: Ctx<'_>,
) -> bool {
    let combined = format!("{} {}", text.literal, text.output);
    let has_expansion = bash_expansion_test(&combined);
    let has_escape = bash_escape_test(&text.literal);
    {
        let mut ctx = context.borrow_mut();
        if has_expansion || has_escape || text.literal.contains('$') || text.literal.contains('`') {
            ctx.bound_dollar = true;
        }
        if has_expansion
            || has_escape
            || text.literal.contains('(')
            || text.literal.contains('{')
            || text.literal.contains('`')
        {
            ctx.bound_open = true;
        }
    }
    if evaluation == BashEvaluation::Deferred {
        return false;
    }
    if evaluation == BashEvaluation::Arithmetic {
        // /[A-Za-z_]|(?:\$(?![?#!-]|\$(?!\[)))/
        let word = &text.word;
        let mut hit = false;
        for (position, c) in word.char_indices() {
            if c.is_ascii_alphabetic() || c == '_' {
                hit = true;
                break;
            }
            if c == '$' {
                match word[position + 1..].chars().next() {
                    Some('?') | Some('#') | Some('!') | Some('-') => {}
                    Some('$') => {
                        if !word[position + 2..].starts_with('[') {
                            hit = true;
                            break;
                        }
                    }
                    _ => {
                        hit = true;
                        break;
                    }
                }
            }
        }
        if hit {
            context.borrow_mut().variable_sink = true;
        }
    } else if text.word.contains('$') || combined.contains('[') {
        context.borrow_mut().variable_sink = true;
    }
    if evaluation == BashEvaluation::Arithmetic {
        return has_expansion;
    }
    combined.contains('[') && has_expansion
}

/// `scanBashSubscript` — subscript after `[`.
fn scan_bash_subscript(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    span: &BashSpan,
) -> Result<usize, OpaqueReason> {
    let mut text = BashText::default();
    let end = scan_bash_span(chars, start, depth, context, commands, span, false, Some(&mut text))?;
    if bash_evaluates_expansion(&mut text, BashEvaluation::Arithmetic, context) {
        return Err(OpaqueReason::DynamicExecution);
    }
    Ok(end)
}

/// `scanBashSpan` — scans to the span's unnested close character.
fn scan_bash_span(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    span: &BashSpan,
    allow_bracket: bool,
    mut text: Option<&mut BashText>,
) -> Result<usize, OpaqueReason> {
    let mut nesting: i64 = 0;
    let mut index = start;
    while index < chars.len() {
        spend(context)?;
        let char = at(chars, index);
        if Some(char) == span.close && nesting == 0 {
            return Ok(index);
        }
        let prev = if index == 0 { '\0' } else { at(chars, index - 1) };
        if span.reject.contains(char) && !(char == '#' && (prev.is_ascii_digit() || prev == '$')) {
            return Err(OpaqueReason::InvalidStructure);
        }
        if Some(char) == span.open {
            nesting += 1;
            if nesting as usize + depth > MAX_SUBSTITUTION_DEPTH {
                return Err(OpaqueReason::InvalidStructure);
            }
        }
        if Some(char) == span.close {
            nesting -= 1;
        }
        let unit = if Some(char) == span.open || Some(char) == span.close {
            None
        } else {
            let span_mode = match span.mode {
                SpanMode::Quoted => BashTextMode::Quoted,
                SpanMode::Arithmetic => BashTextMode::Arithmetic,
            };
            scan_bash_unit(
                chars,
                index,
                depth,
                context,
                commands,
                span_mode,
                allow_bracket,
                text.as_deref_mut(),
            )?
        };
        if let Some(unit) = unit {
            index = unit + 1;
            continue;
        }
        if char == '\\'
            && (at(chars, index + 1) == span.open.unwrap_or('\0')
                || at(chars, index + 1) == span.close.unwrap_or('\0'))
        {
            if let Some(text) = text.as_deref_mut() {
                let pair = slice(chars, index, index + 2);
                text.word.push_str(&pair);
                text.literal.push_str(&pair);
            }
            index += 2;
            continue;
        }
        if let Some(text) = text.as_deref_mut() {
            text.word.push(char);
            text.literal.push(char);
        }
        index += 1;
    }
    if span.close.is_some() {
        Err(OpaqueReason::UnterminatedQuote)
    } else {
        Ok(chars.len())
    }
}

/// `scanBashNested` — a nested list such as a command/process substitution.
fn scan_bash_nested(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    close: Close,
    outer: Option<&mut BashText>,
) -> Result<usize, OpaqueReason> {
    let fresh = RefCell::new(Vec::new());
    let mut state = BashState::with_heredocs(
        chars,
        context,
        start,
        Some(close),
        &fresh,
        outer.map(|text| (*text).clone()),
    );
    let result = scan_bash(&mut state, start, depth + 1)?;
    commands.extend(result.commands);
    Ok(result.end)
}

/// A `(` subshell group sharing the parent's heredocs.
fn scan_bash_group(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    heredocs: &RefCell<Vec<BashHeredoc>>,
    opener: char,
) -> Result<BashList, OpaqueReason> {
    let mut state = BashState::with_heredocs(
        chars,
        context,
        start,
        Some(if opener == '{' { Close::Brace } else { Close::Paren }),
        heredocs,
        None,
    );
    scan_bash(&mut state, start, depth)
}

fn scan_bash_subshell(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    heredocs: &RefCell<Vec<BashHeredoc>>,
) -> Result<BashList, OpaqueReason> {
    let mut state =
        BashState::with_heredocs(chars, context, start, Some(Close::Paren), heredocs, None);
    scan_bash(&mut state, start, depth)
}

enum ArithmeticOutcome {
    End(usize),
    Subshell,
}

/// `scanBashDollarOrBacktick`.
fn scan_bash_dollar_or_backtick(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    quoted: bool,
    allow_bracket: bool,
    mut text: Option<&mut BashText>,
) -> Result<usize, OpaqueReason> {
    if depth >= MAX_SUBSTITUTION_DEPTH {
        return Err(OpaqueReason::CommandSubstitution);
    }
    if at(chars, start) == '`' {
        return scan_bash_backtick(chars, start, depth + 1, context, commands, quoted, text);
    }
    if slice(chars, start, start + 3) == "$((" {
        if let ArithmeticOutcome::End(end) = scan_bash_arithmetic(
            chars,
            start + 3,
            depth + 1,
            context,
            commands,
            &Spans::ARITHMETIC,
        )? {
            return Ok(end);
        }
    }
    if slice(chars, start, start + 2) == "$(" {
        return scan_bash_nested(
            chars,
            start + 2,
            depth,
            context,
            commands,
            Close::Paren,
            text.as_deref_mut(),
        );
    }
    if let Some(nofork_end) = nofork_open_end(chars, start) {
        return scan_bash_nested(
            chars,
            nofork_end,
            depth,
            context,
            commands,
            Close::NoFork,
            text.as_deref_mut(),
        );
    }
    if slice(chars, start, start + 2) == "${" {
        // Literal text in parameter words reaches the enclosing word's value.
        let mut parameter = BashText::default();
        let end = scan_bash_parameter(
            chars,
            start + 2,
            depth + 1,
            context,
            commands,
            quoted,
            &mut parameter,
        )?;
        // ${!name} evaluates the subscript in its name.
        if at(chars, line_continuation_end(chars, start + 2)) == '!' {
            context.borrow_mut().variable_sink = true;
            if bash_evaluates_expansion(&mut parameter, BashEvaluation::Binding, context) {
                return Err(OpaqueReason::DynamicExecution);
            }
        }
        if let Some(text) = text.as_deref_mut() {
            text.literal.push_str(&parameter.literal);
            text.output.push_str(&parameter.output);
        }
        return Ok(end);
    }
    // Dash leaves `$[` literal; Bash and Zsh always read arithmetic.
    if !allow_bracket && context.borrow().dialect == Dialect::Posix {
        return Err(OpaqueReason::CommandSubstitution);
    }
    let mut expression = BashText::default();
    let end = scan_bash_span(
        chars,
        start + 2,
        depth + 1,
        context,
        commands,
        &Spans::BRACKET_ARITHMETIC,
        true,
        Some(&mut expression),
    )?;
    if bash_evaluates_expansion(&mut expression, BashEvaluation::Arithmetic, context) {
        return Err(OpaqueReason::DynamicExecution);
    }
    Ok(end)
}

/// `scanBashArithmetic` — the text of `((...))` after its opening parens.
fn scan_bash_arithmetic(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    span: &BashSpan,
) -> Result<ArithmeticOutcome, OpaqueReason> {
    let mut arithmetic: Vec<CmdRef> = Vec::new();
    let mut text = BashText::default();
    let end = match scan_bash_span(
        chars,
        start,
        depth,
        context,
        &mut arithmetic,
        span,
        true,
        Some(&mut text),
    ) {
        Ok(end) => end,
        // Any span failure inside `((...))` collapses to invalid-structure.
        Err(_) => return Err(OpaqueReason::InvalidStructure),
    };
    let close = bash_operator(chars, end, &["))"])?;
    if close.is_none() {
        return Ok(ArithmeticOutcome::Subshell);
    }
    if bash_evaluates_expansion(&mut text, BashEvaluation::Arithmetic, context) {
        return Err(OpaqueReason::DynamicExecution);
    }
    commands.extend(arithmetic);
    Ok(ArithmeticOutcome::End(end + 1))
}

fn scan_bash_backtick(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    quoted: bool,
    text: Option<&mut BashText>,
) -> Result<usize, OpaqueReason> {
    let mut source = String::new();
    let mut index = start + 1;
    while index < chars.len() {
        spend(context)?;
        if at(chars, index) == '`' {
            let source_chars: Vec<char> = source.chars().collect();
            let fresh = RefCell::new(Vec::new());
            let mut inner = BashState::with_heredocs(
                &source_chars,
                context,
                0,
                None,
                &fresh,
                text.map(|text| (*text).clone()),
            );
            let inner_result =
                scan_bash(&mut inner, 0, depth).map_err(|_| OpaqueReason::CommandSubstitution)?;
            commands.extend(inner_result.commands);
            return Ok(index);
        }
        let next = at(chars, index + 1);
        if at(chars, index) == '\\' && ("$`\\\n".contains(next) || (quoted && next == '"')) {
            if next != '\n' {
                source.push(next);
            }
            index += 2;
            continue;
        }
        source.push(at(chars, index));
        index += 1;
    }
    Err(OpaqueReason::CommandSubstitution)
}

fn scan_bash_parameter(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    quoted: bool,
    text: &mut BashText,
) -> Result<usize, OpaqueReason> {
    let name_start = parameter_prefix_end(chars, start);
    // The operands of ${name:offset:length} are arithmetic; ${name:=word}
    // binds a variable.
    let mut colon: isize = parameter_name_end(chars, name_start).map_or(-1, |end| end as isize);
    let name = if colon > name_start as isize {
        slice(chars, name_start, colon as usize).replace("\\\n", "")
    } else {
        String::new()
    };
    let dialect = context.borrow().dialect;
    let char_at_colon = |colon: isize| {
        if colon < 0 {
            '\0'
        } else {
            at(chars, colon as usize)
        }
    };
    let binding_name = [
        "functions",
        "galiases",
        "salises",
        "aliases",
        "dis_functions",
        "dis_galiases",
        "dis_saliases",
        "dis_aliases",
        "options",
        "commands",
    ]
    .iter()
    .any(|candidate| name == *candidate)
        && " [=:".contains(char_at_colon(colon));
    if dialect != Dialect::Bash && (is_nullcmd(&name) || binding_name) {
        return Err(OpaqueReason::DynamicExecution);
    }
    if name == "PS4" {
        context.borrow_mut().variable_sink = true;
    }
    let mut evaluation: Option<BashEvaluation> = None;
    let mut word_offset = 0usize;
    let mut literal_offset = 0usize;
    let mut index = start;
    while index < chars.len() {
        spend(context)?;
        let char = at(chars, index);
        if char == '}' {
            if let Some(evaluation) = evaluation {
                let mut slice_text = BashText {
                    word: text.word[word_offset.min(text.word.len())..].to_string(),
                    literal: text.literal[literal_offset.min(text.literal.len())..].to_string(),
                    output: text.output.clone(),
                };
                if bash_evaluates_expansion(&mut slice_text, evaluation, context) {
                    return Err(OpaqueReason::DynamicExecution);
                }
            }
            return Ok(index);
        }
        if index as isize == colon && evaluation.is_none() {
            if char == ':' && !"-=?+".contains(at(chars, index + 1)) {
                evaluation = Some(BashEvaluation::Arithmetic);
            }
            if char == '=' || (char == ':' && at(chars, index + 1) == '=') {
                evaluation = Some(BashEvaluation::Deferred);
            }
            if evaluation.is_some() {
                word_offset = text.word.len();
                literal_offset = text.literal.len();
            }
        }
        // Bash ${name@P} evaluates the value as prompt text.
        if char == '@' && parameter_prompt(chars, index) {
            return Err(OpaqueReason::DynamicExecution);
        }
        // Zsh globs an unquoted parameter's words (glob qualifiers).
        if !quoted && dialect != Dialect::Bash && char == '(' && index > start {
            return Err(OpaqueReason::DynamicExecution);
        }
        if quoted && char == '\'' && index > 0 && at(chars, index - 1) == '$' {
            let Some(ansi) = bash_ansi_quote(chars, index) else {
                return Err(OpaqueReason::UnterminatedQuote);
            };
            text.literal.push_str(&ansi.value);
        }
        let subscript = index as isize == colon && char == '[';
        let unit: Option<usize> = if subscript {
            Some(scan_bash_subscript(
                chars,
                index + 1,
                depth,
                context,
                commands,
                &Spans::SUBSCRIPT,
            )?)
        } else if quoted && char == '\'' {
            Some(scan_bash_span(
                chars,
                index + 1,
                depth,
                context,
                commands,
                &Spans::PARAMETER_QUOTE,
                false,
                Some(text),
            )?)
        } else if quoted && char == '"' {
            Some(scan_bash_span(
                chars,
                index + 1,
                depth,
                context,
                commands,
                &Spans::DOUBLE,
                false,
                Some(text),
            )?)
        } else {
            scan_bash_unit(
                chars,
                index,
                depth,
                context,
                commands,
                if quoted { BashTextMode::Quoted } else { BashTextMode::Word },
                false,
                Some(text),
            )?
        };
        // Bash 3.2 ends a double-quoted parameter at an unquoted `}` inside
        // `$(...)`.
        if let Some(unit_end) = unit {
            if quoted
                && slice(chars, index, index + 2) == "$("
                && slice(chars, index, unit_end + 1).contains('}')
            {
                return Err(OpaqueReason::CommandSubstitution);
            }
        }
        if subscript
            || (index == name_start && slice(chars, index, index + 2) == "${" && unit.is_some())
        {
            let unit_end = unit.unwrap_or(index);
            colon = line_continuation_end(chars, unit_end + 1) as isize;
        }
        if let Some(unit_end) = unit {
            index = unit_end + 1;
        } else {
            text.word.push(char);
            text.literal.push(char);
            index += 1;
        }
    }
    Err(OpaqueReason::CommandSubstitution)
}

fn scan_bash_conditional(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
) -> Result<usize, OpaqueReason> {
    let mut words: Vec<BashText> = Vec::new();
    let mut text = BashText::default();
    let mut word_started = false;
    let mut has_word = false;
    let mut paren_depth: i64 = 0;
    let mut pattern_paren_depth: i64 = 0;
    let mut index = start;
    while index < chars.len() {
        spend(context)?;
        let char = at(chars, index);
        if char == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
            continue;
        }
        if !word_started && has_word {
            words.push(std::mem::take(&mut text));
            has_word = false;
        }
        if !word_started && char == ']' {
            if bash_operator(chars, index, &["]]"])?.is_some()
                && brace_close_ahead(chars, index + 2)
            {
                if paren_depth != 0 {
                    return Err(OpaqueReason::InvalidStructure);
                }
                // -v names a variable; -t (zsh) and arithmetic comparisons
                // evaluate both operands.
                let dialect = context.borrow().dialect;
                let mut evaluated = false;
                for at_index in 0..words.len() {
                    let previous_word =
                        if at_index > 0 { words[at_index - 1].word.as_str() } else { "" }
                            .to_string();
                    let next_word =
                        words.get(at_index + 1).map(|w| w.word.clone()).unwrap_or_default();
                    let mut word_text = words[at_index].clone();
                    let is_v = previous_word == "-v"
                        && bash_evaluates_expansion(
                            &mut word_text,
                            BashEvaluation::Binding,
                            context,
                        );
                    let arithmetic = (dialect != Dialect::Bash && previous_word == "-t")
                        || is_arithmetic_comparison(&previous_word)
                        || is_arithmetic_comparison(&next_word);
                    let is_arith = arithmetic
                        && bash_evaluates_expansion(
                            &mut word_text,
                            BashEvaluation::Arithmetic,
                            context,
                        );
                    words[at_index] = word_text;
                    if is_v || is_arith {
                        evaluated = true;
                        break;
                    }
                }
                if evaluated {
                    return Err(OpaqueReason::DynamicExecution);
                }
                return Ok(index + 1);
            }
        }
        if !word_started && char == '#' {
            let Some(newline) = chars[index..].iter().position(|&c| c == '\n').map(|p| p + index)
            else {
                return Err(OpaqueReason::InvalidStructure);
            };
            index = newline + 1;
            continue;
        }
        if char == ' ' || char == '\t' || char == '\n' {
            if pattern_paren_depth == 0 {
                word_started = false;
            }
            index += 1;
            continue;
        }
        if char == ';' {
            return Err(OpaqueReason::InvalidStructure);
        }
        if char == '(' || char == ')' {
            if char == ')' && paren_depth == 0 {
                return Err(OpaqueReason::InvalidStructure);
            }
            let in_pattern = pattern_paren_depth > 0
                || (char == '('
                    && words
                        .last()
                        .map(|w| w.word.as_str())
                        .is_some_and(|word| ["=", "==", "!=", "=~"].contains(&word)));
            paren_depth += if char == '(' { 1 } else { -1 };
            if in_pattern {
                pattern_paren_depth += if char == '(' { 1 } else { -1 };
                word_started = true;
                has_word = true;
                text.word.push(char);
                text.literal.push(char);
                index += 1;
                continue;
            }
            if has_word {
                words.push(std::mem::take(&mut text));
                has_word = false;
            }
            words.push(BashText {
                word: char.to_string(),
                literal: String::new(),
                output: String::new(),
            });
            word_started = false;
            index += 1;
            continue;
        }
        let unit = scan_bash_unit(
            chars,
            index,
            depth,
            context,
            commands,
            BashTextMode::Word,
            true,
            Some(&mut text),
        )?;
        if let Some(unit) = unit {
            index = unit + 1;
            continue;
        }
        if pattern_paren_depth == 0 && "&|<>".contains(char) {
            if has_word {
                words.push(std::mem::take(&mut text));
                has_word = false;
            }
            words.push(BashText {
                word: char.to_string(),
                literal: String::new(),
                output: String::new(),
            });
            word_started = false;
            index += 1;
            continue;
        }
        word_started = true;
        has_word = true;
        text.word.push(char);
        text.literal.push(char);
        index += 1;
    }
    Err(OpaqueReason::InvalidStructure)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrayMode {
    Array,
    Pattern,
}

fn scan_bash_array_or_pattern(
    chars: &[char],
    start: usize,
    depth: usize,
    context: Ctx<'_>,
    commands: &mut Vec<CmdRef>,
    mode: ArrayMode,
) -> Result<usize, OpaqueReason> {
    if depth > MAX_SUBSTITUTION_DEPTH {
        return Err(OpaqueReason::CommandSubstitution);
    }
    // Array elements bind variables, so their decoded text is a subscript
    // sink.
    let mut text = if mode == ArrayMode::Array { Some(BashText::default()) } else { None };
    let mut word_started = false;
    let mut index = start + 1;
    while index < chars.len() {
        spend(context)?;
        let char = at(chars, index);
        if char == '\\' && at(chars, index + 1) == '\n' {
            index += 2;
            continue;
        }
        if let Some(text) = text.as_mut() {
            if char == ')' || is_blank(char) || char == '\n' {
                bash_evaluates_expansion(text, BashEvaluation::Deferred, context);
                text.literal.clear();
                text.output.clear();
            }
        }
        if char == ')' {
            return Ok(index);
        }
        if mode == ArrayMode::Array && !word_started && char == '#' {
            let Some(newline) = chars[index..].iter().position(|&c| c == '\n').map(|p| p + index)
            else {
                return Err(OpaqueReason::CommandSubstitution);
            };
            index = newline + 1;
            continue;
        }
        if is_blank(char) || (mode == ArrayMode::Array && char == '\n') {
            word_started = false;
            index += 1;
            continue;
        }
        // Array elements are globbed (zsh glob qualifiers).
        if char == '\n'
            || char == ';'
            || char == '&'
            || (mode == ArrayMode::Array && "|(".contains(char))
        {
            return Err(OpaqueReason::CommandSubstitution);
        }
        word_started = true;
        let unit: Option<usize> =
            if mode == ArrayMode::Array && char == '=' && at(chars, index + 1) == '(' {
                scan_bash_nested(chars, index + 2, depth, context, commands, Close::Paren, None)
                    .map(Some)?
            } else if char == '(' {
                scan_bash_array_or_pattern(chars, index, depth + 1, context, commands, mode)
                    .map(Some)?
            } else if char == '[' {
                scan_bash_subscript(chars, index + 1, depth, context, commands, &Spans::SUBSCRIPT)
                    .map(Some)?
            } else {
                scan_bash_unit(
                    chars,
                    index,
                    depth,
                    context,
                    commands,
                    BashTextMode::Word,
                    false,
                    text.as_mut(),
                )?
            };
        if let Some(unit) = unit {
            index = unit + 1;
        } else {
            if let Some(text) = text.as_mut() {
                text.literal.push(char);
            }
            index += 1;
        }
    }
    Err(OpaqueReason::CommandSubstitution)
}

// ---------------------------------------------------------------------------
// ANSI quotes and heredocs
// ---------------------------------------------------------------------------

struct AnsiQuote {
    value: String,
    end: usize,
}

fn bash_ansi_quote(chars: &[char], start: usize) -> Option<AnsiQuote> {
    let mut value = String::new();
    let mut index = start + 1;
    while index < chars.len() {
        if at(chars, index) == '\'' {
            return Some(AnsiQuote { value, end: index });
        }
        if at(chars, index) != '\\' {
            value.push(at(chars, index));
            index += 1;
            continue;
        }
        index += 1;
        let escaped = at(chars, index);
        if index >= chars.len() {
            return None;
        }
        if let Some((_, replacement)) = BASH_ANSI_ESCAPES.iter().find(|(key, _)| *key == escaped) {
            value.push(*replacement);
            index += 1;
            continue;
        }
        let octal = escaped.is_ascii_digit() && escaped < '8';
        let digits: Option<String> = if escaped == 'x' {
            let mut digits = String::new();
            for offset in 1..=2 {
                let c = at(chars, index + offset);
                if c.is_ascii_hexdigit() {
                    digits.push(c);
                } else {
                    break;
                }
            }
            if digits.is_empty() {
                None
            } else {
                Some(digits)
            }
        } else if octal {
            let mut digits = String::new();
            digits.push(escaped);
            for offset in 1..=2 {
                let c = at(chars, index + offset);
                if c.is_ascii_digit() && c < '8' {
                    digits.push(c);
                } else {
                    break;
                }
            }
            Some(digits)
        } else {
            None
        };
        let Some(digits) = digits else {
            return None;
        };
        let point = u32::from_str_radix(&digits, if octal { 8 } else { 16 }).ok()?;
        if point == 0 {
            return None;
        }
        value.push(char::from_u32(point)?);
        index += if octal { digits.len() } else { digits.len() + 1 };
    }
    None
}

struct HeredocDelimiter {
    heredoc: BashHeredoc,
    end: usize,
}

fn bash_heredoc_delimiter(chars: &[char], start: usize, tabs: bool) -> Option<HeredocDelimiter> {
    let mut delimiter = String::new();
    let mut dash_delimiter: Option<String> = None;
    let mut quoted = false;
    let mut quote: Option<char> = None;
    let mut end = start;
    let mut started = false;
    let mut index = start;
    while index < chars.len() {
        let char = at(chars, index);
        if !started && is_blank(char) {
            index += 1;
            continue;
        }
        if !started && char == '#' {
            return None;
        }
        if quote.is_none() && " \t\n;&|()<>".contains(char) {
            if !started {
                return None;
            }
            return Some(HeredocDelimiter {
                heredoc: BashHeredoc {
                    delimiter,
                    dash_delimiter,
                    quoted,
                    tabs,
                    binding: false,
                    command: None,
                    start: None,
                },
                end,
            });
        }
        if quote.is_none()
            && char == '$'
            && at(chars, index + 1) == '\\'
            && at(chars, index + 2) == '\n'
        {
            return None;
        }
        // Dash retains `$` in `$'...'` delimiters; Bash and Zsh strip it.
        if quote.is_none() && char == '$' && at(chars, index + 1) == '\'' {
            let Some(literal) = bash_ansi_quote(chars, index + 1) else {
                return None;
            };
            let first_quote =
                chars[index + 2..].iter().position(|&c| c == '\'').map(|p| p + index + 2);
            if first_quote != Some(literal.end) {
                return None;
            }
            let dash = dash_delimiter.get_or_insert_with(|| delimiter.clone());
            dash.push('$');
            dash.push_str(&slice(chars, index + 2, literal.end));
            delimiter.push_str(&literal.value);
            quoted = true;
            started = true;
            index = literal.end;
            end = index;
            continue;
        }
        // Bash strips `$` from `$"..."` delimiters; Zsh/Dash retain it.
        if quote.is_none() && char == '$' && at(chars, index + 1) == '"' {
            return None;
        }
        if Some(char) == quote {
            quote = None;
            end = index;
            index += 1;
            continue;
        }
        if char == '\\' && quote != Some('\'') {
            let next = at(chars, index + 1);
            if index + 1 >= chars.len() {
                return None;
            }
            if next == '\n' {
                index += 2;
                continue;
            }
            // Double quotes only remove escapes for shell-special characters.
            if quote.is_none() || "$`\"\\".contains(next) {
                quoted = true;
                started = true;
                index += 1;
                let escaped = at(chars, index);
                if let Some(dash) = dash_delimiter.as_mut() {
                    dash.push(escaped);
                }
                delimiter.push(escaped);
                end = index;
                index += 1;
                continue;
            }
        }
        if quote.is_none() && (char == '\'' || char == '"') {
            quote = Some(char);
            quoted = true;
            started = true;
            end = index;
            index += 1;
            continue;
        }
        if let Some(dash) = dash_delimiter.as_mut() {
            dash.push(char);
        }
        delimiter.push(char);
        started = true;
        end = index;
        index += 1;
    }
    if started && quote.is_none() {
        Some(HeredocDelimiter {
            heredoc: BashHeredoc {
                delimiter,
                dash_delimiter,
                quoted,
                tabs,
                binding: false,
                command: None,
                start: None,
            },
            end,
        })
    } else {
        None
    }
}

struct HeredocBody {
    source: String,
    source_chars: Vec<char>,
    end: usize,
}

fn bash_heredoc(chars: &[char], start: usize, delimiter: &BashHeredoc) -> Option<HeredocBody> {
    let body_start = start;
    let mut line_start = start;
    let mut start = start;
    let mut line = String::new();
    let mut backslashes: usize = 0;
    let mut index = start;
    loop {
        if index > chars.len() {
            break;
        }
        if index < chars.len() && at(chars, index) != '\n' {
            index += 1;
            continue;
        }
        let text = if delimiter.tabs && start == line_start {
            slice(chars, start, index).trim_start_matches('\t').to_string()
        } else {
            slice(chars, start, index)
        };
        let mut run = 0;
        let characters: Vec<char> = text.chars().collect();
        while run < characters.len() && characters[characters.len() - 1 - run] == '\\' {
            run += 1;
        }
        backslashes = if run == characters.len() { backslashes + run } else { run };
        let continued = !delimiter.quoted && backslashes % 2 == 1 && index < chars.len();
        let max_length = delimiter
            .dash_delimiter
            .as_ref()
            .map(|dash| dash.chars().count().max(delimiter.delimiter.chars().count()))
            .unwrap_or_else(|| delimiter.delimiter.chars().count());
        // Joined lines only grow.
        if line.chars().count() <= max_length {
            if continued {
                line.push_str(&text[..text.len() - 1]);
            } else {
                line.push_str(&text);
            }
        }
        if continued {
            backslashes -= 1;
            start = index + 1;
            index += 1;
            continue;
        }
        if let Some(dash) = delimiter.dash_delimiter.as_ref() {
            if line == *dash {
                return None;
            }
        }
        if line == delimiter.delimiter {
            // Dash does not join backslash-continued delimiter lines.
            if start > line_start
                && slice(chars, index + 1, chars.len()).contains(&delimiter.delimiter)
            {
                return None;
            }
            return Some(HeredocBody {
                source: slice(chars, body_start, line_start),
                source_chars: chars[body_start..line_start].to_vec(),
                end: index,
            });
        }
        line.clear();
        backslashes = 0;
        start = index + 1;
        line_start = start;
        if index >= chars.len() {
            break;
        }
        index += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// PowerShell scanner
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowershellHash {
    None,
    True,
    Clause,
    Switch,
}

type PsBudget<'a> = &'a RefCell<i64>;

fn ps_spend(budget: PsBudget<'_>) -> Result<(), OpaqueReason> {
    let mut remaining = budget.borrow_mut();
    *remaining -= 1;
    if *remaining < 0 {
        return Err(OpaqueReason::InvalidStructure);
    }
    Ok(())
}

fn ps_is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '?'
}

fn ps_is_space(c: char) -> bool {
    c.is_whitespace()
}

/// `POWERSHELL_STOP_PARSING_RE` — `--%` at a boundary.
fn ps_stop_parsing_at(chars: &[char], index: usize) -> bool {
    if slice(chars, index, index + 3) != "--%" {
        return false;
    }
    let next = at(chars, index + 3);
    next == '\0' || " \t\r\n;|&(){}".contains(next) || (ps_is_space(next) && next != '\u{a0}')
}

/// `POWERSHELL_PARAMETER_COLON_RE` — `^-[\w?][\w?-]*:$`.
fn ps_parameter_colon(word: &str) -> bool {
    let Some(stripped) = word.strip_prefix('-') else { return false };
    let Some(body) = stripped.strip_suffix(':') else { return false };
    if body.is_empty() {
        return false;
    }
    let mut characters = body.chars();
    let first = characters.next().unwrap_or('\0');
    if !(first.is_alphanumeric() || first == '_' || first == '?') {
        return false;
    }
    characters.all(|c| ps_is_word_char(c) || c == '-')
}

/// `POWERSHELL_MERGE_REDIRECT_RE` — `\d>&` at index.
fn ps_merge_redirect_at(chars: &[char], index: usize) -> bool {
    at(chars, index).is_ascii_digit() && at(chars, index + 1) == '>' && at(chars, index + 2) == '&'
}

/// `POWERSHELL_LABEL_RE` — `:name` followed by a loop keyword.
fn ps_label_at(chars: &[char], index: usize) -> Option<usize> {
    if at(chars, index) != ':' {
        return None;
    }
    let first = at(chars, index + 1);
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut end = index + 2;
    while at(chars, end).is_ascii_alphanumeric() || at(chars, end) == '_' {
        end += 1;
    }
    let mut cursor = end;
    while cursor < chars.len() && ps_is_space(at(chars, cursor)) {
        cursor += 1;
    }
    for keyword in ["while", "for", "foreach", "do", "switch"] {
        if slice(chars, cursor, cursor + keyword.len()).eq_ignore_ascii_case(keyword) {
            let after = at(chars, cursor + keyword.len());
            if after == '\0'
                || " \t\r\n&(),;{|}".contains(after)
                || (ps_is_space(after) && after != '\u{a0}')
            {
                return Some(end - index);
            }
        }
    }
    None
}

/// `POWERSHELL_KEYWORD_RE` — `[A-Za-z]+(?=$|[\s&(),;{|}])`.
fn ps_keyword_at(chars: &[char], index: usize) -> Option<String> {
    let first = at(chars, index);
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut end = index + 1;
    while at(chars, end).is_ascii_alphabetic() {
        end += 1;
    }
    let next = at(chars, end);
    if next == '\0' || " \t\r\n&(),;{|}".contains(next) || (ps_is_space(next) && next != '\u{a0}') {
        Some(chars[index..end].iter().collect())
    } else {
        None
    }
}

/// `POWERSHELL_FOREACH_RE` / `POWERSHELL_PARAM_RE` — `foreach\s*\(`.
fn ps_paren_keyword(chars: &[char], index: usize, keyword: &str) -> bool {
    if !slice(chars, index, index + keyword.len()).eq_ignore_ascii_case(keyword) {
        return false;
    }
    let mut cursor = index + keyword.len();
    while cursor < chars.len() && ps_is_space(at(chars, cursor)) {
        cursor += 1;
    }
    at(chars, cursor) == '('
}

/// `POWERSHELL_IN_RE` — `in` at a boundary.
fn ps_in_at(chars: &[char], index: usize) -> bool {
    if !slice(chars, index, index + 2).eq_ignore_ascii_case("in") {
        return false;
    }
    let after = at(chars, index + 2);
    after == '\0' || " \t\r\n&(),;{|}".contains(after) || (ps_is_space(after) && after != '\u{a0}')
}

/// `POWERSHELL_AT_OPENER_RE` — one of `({'"\w?`.
fn ps_at_opener(c: char) -> bool {
    "({'\"".contains(c) || ps_is_word_char(c)
}

/// `POWERSHELL_INDEXABLE_VAR_RE`.
fn ps_indexable_var(chars: &[char], index: usize) -> bool {
    if at(chars, index) == ':' {
        let mut cursor = index + 1;
        let mut any = false;
        while ps_is_word_char(at(chars, cursor)) {
            cursor += 1;
            any = true;
        }
        return any;
    }
    ps_is_word_char(at(chars, index))
}

/// `POWERSHELL_HERE_HEADER_RE` — `@['"] blanks (newline)`.
fn ps_here_header(chars: &[char], start: usize) -> Option<usize> {
    if at(chars, start) != '@' {
        return None;
    }
    let quote = at(chars, start + 1);
    if quote != '\'' && quote != '"' {
        return None;
    }
    let mut index = start + 2;
    while index < chars.len()
        && ps_is_space(at(chars, index))
        && at(chars, index) != '\r'
        && at(chars, index) != '\n'
    {
        index += 1;
    }
    if at(chars, index) == '\r' && at(chars, index + 1) == '\n' {
        Some(index + 2)
    } else if at(chars, index) == '\r' || at(chars, index) == '\n' {
        Some(index + 1)
    } else {
        None
    }
}

/// `POWERSHELL_UNICODE_ESCAPE_RE` — `u{hex}` at index; returns (point, end
/// exclusive).
fn ps_unicode_escape(chars: &[char], index: usize) -> Option<(u32, usize)> {
    if !(at(chars, index) == 'u' || at(chars, index) == 'U') || at(chars, index + 1) != '{' {
        return None;
    }
    let mut end = index + 2;
    let mut digits = String::new();
    while digits.len() < 6 && at(chars, end).is_ascii_hexdigit() {
        digits.push(at(chars, end));
        end += 1;
    }
    if digits.is_empty() || at(chars, end) != '}' {
        return None;
    }
    u32::from_str_radix(&digits, 16).ok().map(|point| (point, end + 1))
}

const POWERSHELL_ESCAPES: &[(char, char)] = &[
    ('0', '\0'),
    ('a', '\u{7}'),
    ('b', '\u{8}'),
    ('e', '\u{1b}'),
    ('f', '\u{c}'),
    ('n', '\n'),
    ('r', '\r'),
    ('t', '\t'),
    ('v', '\u{b}'),
];

struct PsEscape {
    value: String,
    end: usize,
}

fn ps_skip(chars: &[char], skipped: &HashSet<usize>, mut index: usize) -> usize {
    while skipped.contains(&index) && index <= chars.len() {
        index += 1;
    }
    index
}

fn ps_escape(
    chars: &[char],
    start: usize,
    line_continuation: bool,
    skipped: &HashSet<usize>,
) -> Option<PsEscape> {
    let next = ps_skip(chars, skipped, start + 1);
    if next >= chars.len() {
        return None;
    }
    let char = at(chars, next);
    if char == '\r' || char == '\n' {
        if line_continuation && char == '\r' && at(chars, next + 1) == '\n' {
            return Some(PsEscape { value: "\r\n".into(), end: next + 1 });
        }
        return Some(PsEscape { value: char.to_string(), end: next });
    }
    if (char == 'u' || char == 'U') && at(chars, next + 1) == '{' {
        let (point, end) = ps_unicode_escape(chars, next)?;
        if point > 0x10ffff {
            return None;
        }
        return Some(PsEscape { value: char::from_u32(point)?.to_string(), end: end - 1 });
    }
    let value = POWERSHELL_ESCAPES
        .iter()
        .find(|(key, _)| *key == char)
        .map(|(_, value)| value.to_string())
        .unwrap_or_else(|| char.to_string());
    Some(PsEscape { value, end: next })
}

fn ps_braced_variable(chars: &[char], start: usize, skipped: &HashSet<usize>) -> Option<usize> {
    let mut index = start + 2;
    while index < chars.len() {
        if skipped.contains(&index) {
            index += 1;
            continue;
        }
        if at(chars, index) == '`' {
            let escape = ps_escape(chars, index, false, skipped)?;
            index = escape.end + 1;
            continue;
        }
        if at(chars, index) == '}' {
            return if index > start + 2 { Some(index) } else { None };
        }
        index += 1;
    }
    None
}

struct PsSubExpression {
    skipped: HashSet<usize>,
    raw_end: Option<usize>,
}

fn ps_sub_expression(
    chars: &[char],
    start: usize,
    here_string: bool,
    budget: PsBudget<'_>,
    skipped: &HashSet<usize>,
) -> Option<PsSubExpression> {
    let mut next_skipped = skipped.clone();
    let mut depth: i64 = 1;
    let mut raw_end: Option<usize> = None;
    let mut index = start;
    while index < chars.len() {
        {
            let mut remaining = budget.borrow_mut();
            *remaining -= 1;
            if *remaining < 0 {
                return None;
            }
        }
        if next_skipped.contains(&index) {
            index += 1;
            continue;
        }
        let char = at(chars, index);
        if char == '(' {
            depth += 1;
        } else if char == ')' {
            depth -= 1;
            if depth == 0 && raw_end.is_none() {
                raw_end = Some(index);
            }
        } else if !here_string && (char == '"' || char == '`') {
            let mut next = index + 1;
            while next_skipped.contains(&next) {
                next += 1;
            }
            if at(chars, next) == '"' {
                next_skipped.insert(index);
                index = next;
            }
        }
        index += 1;
    }
    Some(PsSubExpression { skipped: next_skipped, raw_end })
}

fn ps_stop_parsing_end(chars: &[char], start: usize, skipped: &HashSet<usize>) -> usize {
    let mut quoted = false;
    let mut index = start;
    while index < chars.len() {
        if skipped.contains(&index) {
            index += 1;
            continue;
        }
        if at(chars, index) == '"' {
            quoted = !quoted;
        }
        let char = at(chars, index);
        if char == '\r'
            || char == '\n'
            || ((char == '|' || (char == '&' && at(chars, index + 1) == '&')) && !quoted)
        {
            return index;
        }
        index += 1;
    }
    chars.len()
}

struct PsHereString {
    commands: Vec<CmdRef>,
    source: String,
    end: usize,
}

fn ps_here_string(
    chars: &[char],
    start: usize,
    depth: usize,
    budget: PsBudget<'_>,
    skipped: &HashSet<usize>,
) -> Result<PsHereString, OpaqueReason> {
    let Some(body) = ps_here_header(chars, start) else {
        return Err(OpaqueReason::UnterminatedQuote);
    };
    let quote = at(chars, start + 1);
    let mut commands: Vec<CmdRef> = Vec::new();
    let mut line_start = true;
    let mut index = body;
    while index < chars.len() {
        ps_spend(budget)?;
        if skipped.contains(&index) {
            index += 1;
            continue;
        }
        if line_start && at(chars, index) == quote && at(chars, index + 1) == '@' {
            let end = if index > body
                && index >= 2
                && at(chars, index - 1) == '\n'
                && at(chars, index - 2) == '\r'
            {
                index - 2
            } else if index > body {
                index - 1
            } else {
                index
            };
            return Ok(PsHereString { commands, source: slice(chars, body, end), end: index + 1 });
        }
        if quote == '"' {
            if at(chars, index) == '`' {
                let escape = ps_escape(chars, index, false, skipped)
                    .ok_or(OpaqueReason::UnterminatedEscape)?;
                index = escape.end + 1;
                line_start = false;
                continue;
            }
            if at(chars, index) == '$' && at(chars, index + 1) == '{' {
                let end = ps_braced_variable(chars, index, skipped)
                    .ok_or(OpaqueReason::InvalidStructure)?;
                index = end + 1;
                line_start = false;
                continue;
            }
            if at(chars, index) == '$' && at(chars, index + 1) == '(' {
                let Some(sub) = ps_sub_expression(chars, index + 2, true, budget, skipped) else {
                    return Err(OpaqueReason::InvalidStructure);
                };
                let result = scan_powershell_list(
                    chars,
                    index + 2,
                    depth + 1,
                    budget,
                    Some(PsClose::Paren),
                    PowershellHash::None,
                    &sub.skipped,
                )?;
                commands.extend(result.commands);
                index = if sub.raw_end.is_some_and(|raw| raw > result.end) {
                    sub.raw_end.unwrap_or(result.end)
                } else {
                    result.end
                };
                index += 1;
                line_start = false;
                continue;
            }
        }
        line_start = at(chars, index) == '\r' || at(chars, index) == '\n';
        index += 1;
    }
    Err(OpaqueReason::UnterminatedQuote)
}

/// `powerShellRedirect` — `Ok(None)` = no redirect shape, `Err` = malformed.
fn ps_redirect(chars: &[char], index: usize) -> Result<Option<String>, OpaqueReason> {
    let mut cursor = index;
    if at(chars, cursor) == '*' {
        cursor += 1;
    } else {
        while at(chars, cursor).is_ascii_digit() {
            cursor += 1;
        }
    }
    if at(chars, cursor) != '>' {
        return Ok(None);
    }
    cursor += 1;
    if at(chars, cursor) == '>' {
        cursor += 1;
    }
    if at(chars, cursor) == '&' {
        cursor += 1;
        while at(chars, cursor).is_ascii_digit() {
            cursor += 1;
        }
    }
    let redirect = slice(chars, index, cursor);
    // /^(?:(?:[1-6]|\*)?>>?|[2-6*]>&1)$/
    let valid = if let Some(body) = redirect.strip_prefix(['1', '2', '3', '4', '5', '6', '*']) {
        match body {
            ">" | ">>" => true,
            ">&1" => redirect.starts_with(['2', '3', '4', '5', '6', '*']),
            _ => false,
        }
    } else {
        false
    };
    if valid {
        Ok(Some(redirect))
    } else {
        Err(OpaqueReason::InvalidRedirect)
    }
}

/// `POWERSHELL_EXPRESSION_RE` — expression starters.
fn ps_expression_at(chars: &[char], index: usize) -> bool {
    let c = at(chars, index);
    let next = at(chars, index + 1);
    let boundary = |at_index: usize| {
        let n = at(chars, at_index);
        n == '\0' || " \t\r\n!#%&()*+,-./;<=>]|{}".contains(n) || ps_is_space(n)
    };
    if c == '!' && !ps_is_word_char(next) && next != '.' {
        return true;
    }
    if c == '+' && next != '=' && !ps_is_word_char(next) && next != '.' {
        return true;
    }
    if (c == '+' && next == '+') || (c == '-' && next == '-') {
        return true;
    }
    if c == '$' {
        if next == '{' {
            return true;
        }
        if next == '?' || next == '$' || next == '^' {
            return boundary(index + 2);
        }
        if next == ':' {
            let mut cursor = index + 2;
            while ps_is_word_char(at(chars, cursor)) {
                cursor += 1;
            }
            return cursor > index + 2;
        }
        if ps_is_word_char(next) && next.is_alphanumeric() {
            return true;
        }
        return false;
    }
    if c == ',' {
        let mut cursor = index + 1;
        while cursor < chars.len() && ps_is_space(at(chars, cursor)) {
            cursor += 1;
        }
        return cursor < chars.len();
    }
    if c.is_ascii_digit() || (c == '.' && next.is_ascii_digit()) {
        return true;
    }
    if c == '-' {
        for word in ["not", "bnot", "join", "split"] {
            for prefix in ["", "i", "c"] {
                let candidate = format!("{prefix}{word}");
                let end = index + 1 + candidate.len();
                if slice(chars, index + 1, end).eq_ignore_ascii_case(&candidate)
                    && !ps_is_word_char(at(chars, end))
                {
                    return true;
                }
            }
        }
        if next.is_ascii_digit() || (next == '.' && at(chars, index + 2).is_ascii_digit()) {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PsClose {
    Paren,
    Brace,
    Bracket,
}

impl PsClose {
    fn char(self) -> char {
        match self {
            Self::Paren => ')',
            Self::Brace => '}',
            Self::Bracket => ']',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PsClause {
    Member,
    Block,
    If,
    Try,
    Clause,
    Switch,
    Paren,
    Do,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PsIndexable {
    #[default]
    None,
    Variable,
    Closed,
    Member,
}

struct PsList {
    commands: Vec<CmdRef>,
    end: usize,
}

struct PsState<'a> {
    chars: &'a [char],
    hash: PowershellHash,
    commands: Vec<CmdRef>,
    nested_commands: Vec<CmdRef>,
    words: Vec<String>,
    raw_words: Vec<String>,
    word_ends: Vec<usize>,
    initial_expression: bool,
    initial_clause: Option<PsClause>,
    segment: usize,
    word: String,
    started: bool,
    word_start: usize,
    quote: Option<char>,
    standalone: bool,
    expression: bool,
    clause: Option<PsClause>,
    after: Option<PsClause>,
    indexable: PsIndexable,
    stop_parsing: bool,
    command_end: usize,
    statement_head: bool,
    invalid: bool,
    redirect_target: bool,
    dangling: bool,
    invocation: bool,
}

impl PsState<'_> {
    fn finish_word(&mut self, end: usize) {
        if !self.started {
            return;
        }
        // Generic tokens can spell stop-parsing with escapes or embedded
        // quotes; literal strings cannot.
        if self.word == "--%"
            && !self.words.is_empty()
            && !self.expression
            && self.clause.is_none()
            && !self.redirect_target
            && !"'\"".contains(at(self.chars, self.word_start))
            && !self.raw_words.last().is_some_and(|last| last.ends_with(','))
            && !self.raw_words.last().is_some_and(|last| ps_parameter_colon(last))
        {
            self.stop_parsing = true;
        }
        if !self.redirect_target {
            if self.words.is_empty() && !self.invocation && !self.expression {
                self.segment = self.word_start;
            }
            self.words.push(std::mem::take(&mut self.word));
            self.raw_words.push(slice(self.chars, self.word_start, end));
            self.word_ends.push(end);
            self.dangling = false;
        } else {
            self.word.clear();
        }
        self.command_end = end;
        self.redirect_target = false;
        self.started = false;
    }

    fn finish_command(&mut self, end: usize, required: bool) {
        self.finish_word(end);
        if !self.words.is_empty() && !self.expression && self.clause.is_none() {
            let command = Command {
                resource: slice(self.chars, self.segment, self.command_end).trim_end().to_string(),
                words: self.words.clone(),
                raw_words: self.raw_words.clone(),
                word_ends: Some(
                    self.word_ends.iter().map(|value| value.saturating_sub(self.segment)).collect(),
                ),
                statement_head: self.statement_head && !self.invocation,
                declaration: false,
                redirect_word_count: None,
            };
            self.commands.push(Rc::new(RefCell::new(command)));
        } else if ((!self.expression && self.clause.is_none() && self.invocation)
            || (required && self.words.is_empty() && self.nested_commands.is_empty())
            || self.clause.is_some_and(|clause| clause != PsClause::Member))
        {
            self.invalid = true;
        }
        if self.redirect_target {
            self.invalid = true;
        }
        let nested = std::mem::take(&mut self.nested_commands);
        self.commands.extend(nested);
        self.words.clear();
        self.raw_words.clear();
        self.word_ends.clear();
        self.redirect_target = false;
        self.invocation = false;
        self.expression = self.initial_expression;
        self.clause = self.initial_clause;
        self.after = None;
        self.indexable = PsIndexable::None;
        self.stop_parsing = false;
    }

    fn prev_word(&self) -> String {
        if self.started {
            self.word.clone()
        } else {
            self.words.last().cloned().unwrap_or_default()
        }
    }

    fn member_operator(&self) -> bool {
        self.indexable == PsIndexable::Member && {
            let prev = self.prev_word();
            prev.ends_with('.') || prev.ends_with(':')
        }
    }
}

fn scan_powershell_list(
    chars: &[char],
    start: usize,
    depth: usize,
    budget: PsBudget<'_>,
    close: Option<PsClose>,
    hash: PowershellHash,
    skipped: &HashSet<usize>,
) -> Result<PsList, OpaqueReason> {
    if depth >= MAX_SUBSTITUTION_DEPTH || *budget.borrow() < 0 {
        return Err(OpaqueReason::InvalidStructure);
    }
    let initial_expression = matches!(hash, PowershellHash::True | PowershellHash::Clause);
    let initial_clause = match hash {
        PowershellHash::Clause | PowershellHash::Switch => Some(PsClause::Member),
        _ => None,
    };
    let mut state = PsState {
        chars,
        hash,
        commands: Vec::new(),
        nested_commands: Vec::new(),
        words: Vec::new(),
        raw_words: Vec::new(),
        word_ends: Vec::new(),
        initial_expression,
        initial_clause,
        segment: start,
        word: String::new(),
        started: false,
        word_start: start,
        quote: None,
        standalone: false,
        expression: initial_expression,
        clause: initial_clause,
        after: None,
        indexable: PsIndexable::None,
        stop_parsing: false,
        command_end: start,
        statement_head: true,
        invalid: false,
        redirect_target: false,
        dangling: false,
        invocation: false,
    };
    let mut index = start;
    let mut broke_at_close = false;
    'outer: while index < chars.len() {
        ps_spend(budget)?;
        if skipped.contains(&index) {
            index += 1;
            continue;
        }
        let char = at(chars, index);
        let in_expression = state.expression && !state.redirect_target;
        let at_boundary = !state.started || (!in_expression && state.word.ends_with(','));
        if !state.started {
            state.word_start = index;
        }
        if state.stop_parsing {
            let stop = ps_stop_parsing_end(chars, index, skipped);
            let raw = slice(chars, index, stop);
            let text = raw.trim().to_string();
            if !text.is_empty() {
                let lead = raw.chars().take_while(|c| c.is_whitespace()).count();
                state.word_start = index + lead;
                state.word = text.clone();
                state.started = true;
                let end = state.word_start + text.chars().count();
                state.finish_word(end);
            }
            state.stop_parsing = false;
            index = stop;
            continue;
        }
        if let Some(quote_char) = state.quote {
            let next_quote = ps_skip(chars, skipped, index + 1);
            if char == quote_char && at(chars, next_quote) == quote_char {
                state.word.push(quote_char);
                index = next_quote + 1;
                continue;
            }
            if char == quote_char {
                state.quote = None;
                if state.standalone {
                    state.finish_word(index + 1);
                    state.indexable = PsIndexable::Closed;
                }
                index += 1;
                continue;
            }
            if quote_char == '"' && char == '`' {
                let Some(escape) = ps_escape(chars, index, false, skipped) else {
                    return Err(OpaqueReason::UnterminatedEscape);
                };
                state.word.push_str(&escape.value);
                index = escape.end + 1;
                continue;
            }
            if quote_char == '"' && char == '$' && at(chars, index + 1) == '{' {
                let end = ps_braced_variable(chars, index, skipped)
                    .ok_or(OpaqueReason::InvalidStructure)?;
                state.word.push_str(&slice(chars, index, end + 1));
                index = end + 1;
                continue;
            }
            if quote_char == '"' && char == '$' && at(chars, index + 1) == '(' {
                let Some(sub) = ps_sub_expression(chars, index + 2, false, budget, skipped) else {
                    return Err(OpaqueReason::InvalidStructure);
                };
                let result = scan_powershell_list(
                    chars,
                    index + 2,
                    depth + 1,
                    budget,
                    Some(PsClose::Paren),
                    PowershellHash::None,
                    &sub.skipped,
                )?;
                state.nested_commands.extend(result.commands);
                let end = if sub.raw_end.is_some_and(|raw| raw > result.end) {
                    sub.raw_end.unwrap_or(result.end)
                } else {
                    result.end
                };
                state.word.push_str(&slice(chars, index, end + 1));
                index = end + 1;
                continue;
            }
            state.word.push(char);
            index += 1;
            continue;
        }
        if char == '\'' || char == '"' {
            if !state.started && state.words.is_empty() && !state.invocation {
                state.expression = true;
            }
            state.quote = Some(char);
            state.standalone = at_boundary
                || (state.expression && !state.redirect_target)
                || (state.indexable == PsIndexable::Member
                    && (state.word.ends_with('.') || state.word.ends_with(':')));
            if !state.standalone {
                state.indexable = PsIndexable::None;
            }
            state.started = true;
            index += 1;
            continue;
        }
        if char == '`' {
            let Some(escape) = ps_escape(chars, index, at_boundary || in_expression, skipped)
            else {
                return Err(OpaqueReason::UnterminatedEscape);
            };
            let raw_space = index + 1 < chars.len() && ps_is_space(at(chars, index + 1));
            if (in_expression || at_boundary) && state.started && raw_space {
                state.finish_word(index);
            } else if !at_boundary || !raw_space {
                state.started = true;
                state.indexable = PsIndexable::None;
                state.word.push_str(&escape.value);
            }
            index = escape.end + 1;
            continue;
        }
        if !state.started
            && !state.words.is_empty()
            && !state.expression
            && state.clause.is_none()
            && !state.redirect_target
            && ps_stop_parsing_at(chars, index)
        {
            state.word = "--%".into();
            state.started = true;
            state.finish_word(index + 3);
            index += 3;
            continue;
        }
        if char == '<'
            && at(chars, index + 1) == '#'
            && (at_boundary || in_expression || state.indexable == PsIndexable::Member)
        {
            let member_operator = state.member_operator();
            if state.started {
                state.finish_word(index);
            }
            let Some(end) = (|| {
                let mut cursor = index + 2;
                while cursor + 1 < chars.len() {
                    if at(chars, cursor) == '#' && at(chars, cursor + 1) == '>' {
                        return Some(cursor);
                    }
                    cursor += 1;
                }
                None
            })() else {
                return Err(OpaqueReason::InvalidStructure);
            };
            if !member_operator {
                state.indexable = PsIndexable::None;
            }
            index = end + 2;
            continue;
        }
        let bracket_word = |word: &str| -> bool {
            let mut characters = word.chars();
            let first = characters.next();
            first.is_some_and(|c| c.is_alphanumeric() || c == '_')
                && characters.all(|c| {
                    ps_is_word_char(c)
                        || c == '.'
                        || c == '#'
                        || c == '+'
                        || c == '-'
                        || c == '`'
                        || c == '\\'
                })
        };
        if char == '#'
            && (at_boundary
                || state.indexable == PsIndexable::Member
                || (in_expression
                    && !(close == Some(PsClose::Bracket) && bracket_word(&state.word))))
        {
            let prev = state.prev_word();
            let member_operator = state.indexable == PsIndexable::Member
                && (prev.ends_with('.') || prev.ends_with(':'));
            if state.clause.is_some_and(|clause| clause != PsClause::Member)
                || prev.ends_with(',')
                || member_operator
            {
                state.finish_word(index);
                if !member_operator {
                    state.indexable = PsIndexable::None;
                }
            } else if !state.words.is_empty() || state.started || state.invocation {
                state.finish_command(index, false);
            }
            state.statement_head = !state.dangling;
            let newline = [
                chars[index..].iter().position(|&c| c == '\n').map(|p| p + index),
                chars[index..].iter().position(|&c| c == '\r').map(|p| p + index),
            ]
            .into_iter()
            .flatten()
            .min();
            let Some(newline) = newline else {
                index = chars.len();
                break 'outer;
            };
            index = if at(chars, newline) == '\r' && at(chars, newline + 1) == '\n' {
                newline + 2
            } else {
                newline + 1
            };
            continue;
        }
        if in_expression
            && state.started
            && (char == '>' || (char == '*' && at(chars, index + 1) == '>'))
        {
            state.finish_word(index);
        }
        let redirect_probe = if !state.started
            && (char == '>'
                || char == '*'
                || (char.is_ascii_digit()
                    && (!state.words.is_empty() || ps_merge_redirect_at(chars, index))))
        {
            ps_redirect(chars, index)?
        } else {
            None
        };
        if let Some(redirect) = redirect_probe {
            if state.redirect_target
                || state.words.is_empty()
                || state.clause.is_some_and(|clause| clause != PsClause::Member)
            {
                return Err(OpaqueReason::InvalidRedirect);
            }
            state.redirect_target = !redirect.contains('&');
            state.indexable = PsIndexable::None;
            index += redirect.chars().count();
            state.command_end = index;
            continue;
        }
        if !state.started
            && state.words.is_empty()
            && !state.invocation
            && !state.expression
            && state.clause.is_none()
            && char == ':'
        {
            if let Some(length) = ps_label_at(chars, index) {
                index += length;
                continue;
            }
        }
        if !state.started
            && !state.invocation
            && state.clause.is_none()
            && (if state.words.is_empty() {
                !state.expression
            } else {
                state.words.iter().all(|item| item.starts_with('['))
            })
            && char.is_ascii_alphabetic()
        {
            let keyword = ps_keyword_at(chars, index).map(|k| k.to_lowercase());
            if let Some(keyword) = keyword {
                let matched = if !state.words.is_empty() {
                    (keyword == "param" && ps_paren_keyword(chars, index, "param"))
                        || keyword == "class"
                        || keyword == "enum"
                } else {
                    (keyword == "param" && ps_paren_keyword(chars, index, "param"))
                        || (keyword == "foreach" && ps_paren_keyword(chars, index, "foreach"))
                        || (keyword == "until" && state.after == Some(PsClause::Do))
                        || ((keyword == "else" || keyword == "elseif")
                            && state.after == Some(PsClause::If))
                        || ((keyword == "catch" || keyword == "finally")
                            && state.after == Some(PsClause::Try))
                        || matches!(
                            keyword.as_str(),
                            "if" | "for"
                                | "while"
                                | "do"
                                | "switch"
                                | "function"
                                | "filter"
                                | "workflow"
                                | "parallel"
                                | "sequence"
                                | "configuration"
                                | "try"
                                | "begin"
                                | "process"
                                | "end"
                                | "clean"
                                | "trap"
                                | "class"
                                | "enum"
                                | "data"
                                | "dynamicparam"
                                | "using"
                                | "break"
                                | "continue"
                        )
                };
                if matched {
                    state.expression =
                        matches!(keyword.as_str(), "class" | "enum" | "break" | "continue");
                    state.clause = Some(match keyword.as_str() {
                        "using" | "break" | "continue" => PsClause::Member,
                        "param" | "until" => PsClause::Paren,
                        "while" if state.after == Some(PsClause::Do) => PsClause::Paren,
                        "do" => PsClause::Do,
                        "if" | "elseif" => PsClause::If,
                        "try" | "catch" => PsClause::Try,
                        "switch" => PsClause::Switch,
                        "class" | "enum" => PsClause::Clause,
                        _ => PsClause::Block,
                    });
                    state.after = None;
                    index += keyword.chars().count();
                    continue;
                }
                if state.words.is_empty() && matches!(keyword.as_str(), "return" | "throw" | "exit")
                {
                    state.after = None;
                    index += keyword.chars().count();
                    continue;
                }
            }
        }
        let assign = in_expression
            && state.clause.is_none()
            && (char == '=' || ("+-*/%".contains(char) && at(chars, index + 1) == '='));
        if assign
            || (in_expression
                && close == Some(PsClose::Bracket)
                && !state.started
                && ps_in_at(chars, index))
        {
            if assign {
                state.finish_word(index);
            }
            state.words.clear();
            state.raw_words.clear();
            state.word_ends.clear();
            state.expression = false;
            state.indexable = PsIndexable::None;
            if !assign || char != '=' {
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if at_boundary && char == '@' && !ps_at_opener(at(chars, index + 1)) {
            return Err(OpaqueReason::InvalidStructure);
        }
        if at_boundary
            && char == '@'
            && (at(chars, index + 1) == '\'' || at(chars, index + 1) == '"')
        {
            if state.started {
                state.finish_word(index);
            }
            state.word_start = index;
            let literal = ps_here_string(chars, index, depth, budget, skipped)?;
            if state.words.is_empty() && !state.invocation {
                state.expression = true;
            }
            state.nested_commands.extend(literal.commands);
            state.word = literal.source;
            state.started = true;
            index = literal.end;
            state.finish_word(index + 1);
            state.indexable = PsIndexable::Closed;
            index += 1;
            continue;
        }
        if char == '$' && at(chars, index + 1) == '{' {
            let end =
                ps_braced_variable(chars, index, skipped).ok_or(OpaqueReason::InvalidStructure)?;
            if !state.started
                && state.words.is_empty()
                && !state.invocation
                && state.hash != PowershellHash::Switch
            {
                state.expression = true;
            }
            state.indexable = if at_boundary
                || (state.indexable == PsIndexable::Member
                    && (state.word.ends_with('.') || state.word.ends_with(':')))
            {
                PsIndexable::Closed
            } else {
                PsIndexable::None
            };
            state.word.push_str(&slice(chars, index, end + 1));
            state.started = true;
            index = end;
            if state.expression && !state.redirect_target && at(chars, index + 1) != '[' {
                state.finish_word(index + 1);
            }
            index += 1;
            continue;
        }
        let opener: Option<usize> =
            if (char == '$' || (at_boundary && char == '@')) && at(chars, index + 1) == '(' {
                Some(index + 1)
            } else if at_boundary && char == '@' && at(chars, index + 1) == '{' {
                Some(index + 1)
            } else if char == '('
                || char == '{'
                || (char == '['
                    && (in_expression
                        || state.indexable != PsIndexable::None
                        || (!state.started
                            && !state.invocation
                            && (state.words.is_empty()
                                || state.clause == Some(PsClause::Block)
                                || state.clause == Some(PsClause::Try)))))
            {
                Some(index)
            } else {
                None
            };
        if let Some(opener_index) = opener {
            let open_char = at(chars, opener_index);
            let standalone_block = at_boundary || in_expression || char == '(' || open_char == '{';
            let block_close = if open_char == '(' {
                PsClose::Paren
            } else if open_char == '[' {
                PsClose::Bracket
            } else {
                PsClose::Brace
            };
            let block_hash = if open_char == '[' {
                PowershellHash::True
            } else if char == '@' && open_char == '{' {
                PowershellHash::True
            } else if state.clause == Some(PsClause::Switch) && open_char == '{' {
                PowershellHash::Switch
            } else if state.clause == Some(PsClause::Clause) && open_char == '{' {
                PowershellHash::Clause
            } else if close == Some(PsClose::Bracket) && char == '(' && state.started {
                PowershellHash::Clause
            } else {
                PowershellHash::None
            };
            if state.started && (char == '{' || char == '(' || (char == '@' && at_boundary)) {
                state.finish_word(index);
                if state.stop_parsing {
                    continue;
                }
                state.word_start = index;
            }
            if !state.started && state.words.is_empty() && !state.invocation {
                state.expression = true;
            }
            let result = scan_powershell_list(
                chars,
                opener_index + 1,
                depth + 1,
                budget,
                Some(block_close),
                block_hash,
                skipped,
            )?;
            state.nested_commands.extend(result.commands);
            state.started = true;
            state.word.push_str(&slice(chars, index, result.end + 1));
            index = result.end;
            if open_char == '[' {
                state.indexable = PsIndexable::Closed;
                if at(chars, index + 1) != '[' {
                    state.finish_word(index + 1);
                }
            } else if standalone_block {
                state.finish_word(index + 1);
                state.indexable = PsIndexable::Closed;
            }
            if (state.clause == Some(PsClause::Paren) && char == '(')
                || (state.clause.is_some() && state.clause != Some(PsClause::Paren) && char == '{')
            {
                let completed = match state.clause {
                    Some(PsClause::Do) | Some(PsClause::If) | Some(PsClause::Try) => state.clause,
                    _ => None,
                };
                state.clause = Some(PsClause::Member);
                state.finish_command(index + 1, false);
                state.after = completed;
            }
            index += 1;
            continue;
        }
        if Some(char) == close.map(PsClose::char) {
            broke_at_close = true;
            break 'outer;
        }
        if char == '}' || char == ')' || (char == ']' && in_expression) {
            return Err(OpaqueReason::InvalidStructure);
        }
        if !state.started
            && state.words.is_empty()
            && ((char == '&' && at(chars, index + 1) != '&')
                || (char == '.'
                    && (index + 1 >= chars.len()
                        || " \t\r\n\"'$/,;|{}&".contains(at(chars, index + 1))
                        || ps_is_space(at(chars, index + 1)))))
        {
            if state.invocation {
                return Err(OpaqueReason::InvalidStructure);
            }
            state.segment = index;
            state.invocation = true;
            state.after = None;
            index += 1;
            continue;
        }
        if ps_is_space(char) && char != '\n' && char != '\r' {
            state.finish_word(index);
            state.indexable = PsIndexable::None;
            index += 1;
            continue;
        }
        let next = at(chars, index + 1);
        let separator: Option<String> = if char == '\r' && next == '\n' {
            Some("\r\n".to_string())
        } else if (char == '&' && next == '&') || (char == '|' && next == '|') {
            Some(if char == '&' { "&&".to_string() } else { "||".to_string() })
        } else if ";|&\n\r".contains(char) {
            Some(char.to_string())
        } else {
            None
        };
        if let Some(separator) = separator {
            if close == Some(PsClose::Bracket)
                && (separator == ";"
                    || separator == "|"
                    || separator == "||"
                    || separator == "&&"
                    || separator == "&")
            {
                return Err(OpaqueReason::InvalidStructure);
            }
            if separator == "\n" || separator == "\r" || separator == "\r\n" {
                let prev = state.prev_word();
                let member_operator = state.indexable == PsIndexable::Member
                    && (prev.ends_with('.') || prev.ends_with(':'));
                if state.clause.is_some_and(|clause| clause != PsClause::Member)
                    || prev.ends_with(',')
                    || member_operator
                {
                    state.finish_word(index);
                    index += separator.chars().count();
                    if !member_operator {
                        state.indexable = PsIndexable::None;
                    }
                    continue;
                }
                if !state.started && state.words.is_empty() && !state.invocation {
                    index += separator.chars().count();
                    state.indexable = PsIndexable::None;
                    continue;
                }
            }
            let required =
                state.dangling || !matches!(separator.as_str(), ";" | "\n" | "\r" | "\r\n");
            state.finish_command(index, required);
            state.dangling = !matches!(separator.as_str(), ";" | "&" | "\n" | "\r" | "\r\n");
            state.statement_head = !state.dangling;
            index += separator.chars().count();
            continue;
        }
        if !state.started
            && state.words.is_empty()
            && !state.invocation
            && state.hash != PowershellHash::Switch
            && ps_expression_at(chars, index)
        {
            state.expression = true;
        }
        if (at_boundary
            || (state.indexable == PsIndexable::Member
                && (state.word.ends_with('.') || state.word.ends_with(':'))))
            && char == '$'
        {
            state.indexable = if ps_indexable_var(chars, index + 1) {
                PsIndexable::Variable
            } else {
                PsIndexable::None
            };
        } else if state.indexable == PsIndexable::Closed {
            state.indexable = if (char == '.'
                && at(chars, index + 1) != '.'
                && !ps_is_space(at(chars, index + 1)))
                || (!state.started
                    && char == ':'
                    && at(chars, index + 1) == ':'
                    && !ps_is_space(at(chars, index + 2)))
                || (!state.started
                    && char == '?'
                    && at(chars, index + 1) == '.'
                    && at(chars, index + 2) != '.'
                    && !ps_is_space(at(chars, index + 2)))
            {
                PsIndexable::Member
            } else if !state.started && char == '?' && at(chars, index + 1) == '[' {
                PsIndexable::Closed
            } else {
                PsIndexable::None
            };
        } else if state.indexable == PsIndexable::Variable {
            state.indexable = if (char == '.'
                && at(chars, index + 1) != '.'
                && !ps_is_space(at(chars, index + 1)))
                || (char == ':'
                    && at(chars, index + 1) == ':'
                    && !ps_is_space(at(chars, index + 2)))
            {
                PsIndexable::Member
            } else if (char == '.' && at(chars, index + 1) == '.') || !ps_is_word_char(char) {
                PsIndexable::None
            } else {
                PsIndexable::Variable
            };
        } else if state.indexable == PsIndexable::Member {
            state.indexable = if char == '.' && at(chars, index + 1) == '.' {
                PsIndexable::None
            } else if (char == '.' && !ps_is_space(at(chars, index + 1)))
                || (char == ':'
                    && (at(chars, index + 1) == ':' || (index > 0 && at(chars, index - 1) == ':')))
                || (char == '?' && at(chars, index + 1) == '.' && at(chars, index + 2) != '.')
                || char.is_alphanumeric()
                || char == '_'
            {
                PsIndexable::Member
            } else {
                PsIndexable::None
            };
        }
        state.after = None;
        state.started = true;
        state.dangling = false;
        state.word.push(char);
        index += 1;
    }

    let end = if broke_at_close { index } else { chars.len().min(index) };
    if close.is_some() && !broke_at_close {
        return Err(OpaqueReason::InvalidStructure);
    }
    if state.quote.is_some() {
        return Err(OpaqueReason::UnterminatedQuote);
    }
    state.finish_command(end, false);
    if state.redirect_target || state.invalid || state.dangling {
        return Err(OpaqueReason::InvalidStructure);
    }
    if state
        .commands
        .iter()
        .any(|command| command.borrow().words.first().map(String::is_empty).unwrap_or(true))
    {
        return Err(OpaqueReason::DynamicCommandName);
    }
    Ok(PsList { commands: state.commands, end })
}
