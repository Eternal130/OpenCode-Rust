//! Theme wiring: consume an [`oc_theme::ResolvedTheme`] and map its
//! tokens to flat layout style defaults (the wave-7.0 spike's flat-style
//! mapping, now fed from the real resolver).
//!
//! Wave 7.2/3 scope: the full **theme registry** — builtin v2
//! `opencode` + the 32 v1 builtins (migrated + resolved through the
//! oc-theme pipeline), **custom theme discovery** from
//! `<config-root>/themes/*.json` + `.opencode/themes/*.json` config
//! dirs (invalid files logged + skipped, per `theme/discovery.ts`),
//! registration precedence builtin < custom < system
//! (`theme/index.ts listThemes`), and **system mode** detection from
//! the terminal background (OSC 11 query with timeout fallback to
//! dark; headless = dark — `theme/system.ts terminalMode`).

use std::path::{Path, PathBuf};

use oc_theme::builtins::BUILTIN_V2_OPENCODE;
use oc_theme::resolve::{parse_theme_document, resolve_theme_document, ThemeDocument};
use oc_theme::types::{Mode, ResolvedTheme};
use oc_theme::v1_migrate::migrate_v1;
use oc_theme::Rgba;
use ratatui::style::{Color, Style};

/// A loaded theme document plus the resolved view for the active mode.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeState {
    /// The registry name this state was loaded from (`opencode`, a v1
    /// builtin, a discovered custom name, or `system`).
    pub name: String,
    document: ThemeDocument,
    pub mode: Mode,
    pub resolved: ResolvedTheme,
}

impl Default for ThemeState {
    fn default() -> Self {
        Self::v2_dark()
    }
}

impl ThemeState {
    /// The default theme: builtin v2 `opencode` resolved in dark mode.
    #[must_use]
    pub fn v2_dark() -> Self {
        Self::from_builtin(Mode::Dark).expect("builtin v2 theme resolves")
    }

    /// Parse + resolve the builtin v2 document in `mode`.
    #[must_use]
    pub fn from_builtin(mode: Mode) -> Option<Self> {
        Self::from_json_str(BUILTIN_V2_OPENCODE, mode, "opencode")
    }

    /// Parse + resolve a theme document from raw JSON.
    #[must_use]
    pub fn from_json_str(source: &str, mode: Mode, name: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(source).ok()?;
        let document = parse_theme_document(&value, name).ok()?;
        let resolved = resolve_theme_document(&document, Some(mode)).ok()?;
        Some(Self { name: name.to_owned(), document, mode, resolved })
    }

    /// Resolve a v1 document (`"theme"` key) through the migration
    /// pipeline (`theme/index.ts parseTheme`: `"theme" in source →
    /// migrateV1`).
    #[must_use]
    pub fn from_v1_json_str(source: &str, mode: Mode, name: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(source).ok()?;
        let document = migrate_v1(&value).ok()?;
        let resolved = resolve_theme_document(&document, Some(mode)).ok()?;
        Some(Self { name: name.to_owned(), document, mode, resolved })
    }

    /// The theme document of a registry entry in `mode` (the registry
    /// keeps documents so mode switches re-resolve without re-parsing).
    #[must_use]
    pub fn from_document(document: &ThemeDocument, name: &str, mode: Mode) -> Option<Self> {
        let resolved = resolve_theme_document(document, Some(mode)).ok()?;
        Some(Self { name: name.to_owned(), document: document.clone(), mode, resolved })
    }

    /// The owning document (mode switches re-resolve through it).
    #[must_use]
    pub fn document(&self) -> &ThemeDocument {
        &self.document
    }

    /// Switch mode (re-resolves the same document); no-op when the
    /// document lacks the mode.
    pub fn switch_mode(&mut self, mode: Mode) {
        if !self.document.supports_mode(mode) {
            return;
        }
        if let Ok(resolved) = resolve_theme_document(&self.document, Some(mode)) {
            self.mode = mode;
            self.resolved = resolved;
        }
    }

    /// The flat style defaults components paint with.
    #[must_use]
    pub fn styles(&self) -> ThemeStyles {
        ThemeStyles::from_resolved(&self.resolved)
    }
}

/// Token → style mapping (the spike's flat-style mapping): every role a
/// 7.x component paints, resolved from the theme's RGBA tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeStyles {
    pub text: Style,
    pub text_muted: Style,
    pub background: Color,
    pub background_raised: Color,
    pub action_selected: Color,
    pub action_hovered: Color,
    pub accent: Color,
    pub error: Color,
    pub border: Color,
    /// Base text painted over the base background — the whole-frame fill
    /// style (`opentui` fills the frame with the theme style).
    pub base: Style,
    /// 7.2 lane C (message flow): the markdown token palette
    /// (`theme.markdown.*`) and the extra roles the transcript paints.
    pub markdown: MarkdownStyles,
    /// `text.feedback.warning.base` (the reasoning-group header tint
    /// derives from it at 0.6 alpha).
    pub warning: Color,
    /// `text.feedback.success.base` (SURF-1: the sidebar MCP
    /// connected dot — mcp.tsx:22).
    pub success: Color,
    /// `text.feedback.info.base` (SURF-1: the loading-history label,
    /// index.tsx:1426 — and the jump row's ref fg, auditB #50).
    pub info: Color,
    /// `text.action.secondary.base` (the diff-viewer source switch
    /// label; 7.2 wave 2 diff lane).
    pub action_secondary: Color,
    /// The `theme.diff.*` palette (7.2 wave 2 diff viewer).
    pub diff: DiffStyles,
    /// `text.formfield.base` (the rail add-slot's `idleNumber` tint
    /// derives from it at 0.55).
    pub formfield_base: Color,
    /// `text.formfield.selected` (auditB #8 — the select filter's
    /// un-focused-current fg; the dialog family's `current_fg`).
    pub formfield_selected: Color,
    /// `text.action.destructive.focused` (auditB #8 — the delete/drop
    /// confirm rows' selected fg).
    pub destructive_focused: Color,
    /// `background.action.destructive.focused` — the confirm rows'
    /// selected bg.
    pub destructive_focused_bg: Color,
    /// `text.action.secondary.hovered` (auditB #8 — the jump row's
    /// hover fg; `.base` already lands as `action_secondary`).
    pub action_secondary_hovered: Color,
    /// `theme.scrollbar.base` (auditB #8 — the scrollbox tracks'
    /// thumb color; the track itself paints `raised.base`).
    pub scrollbar: Color,
    /// `background.action.primary.focused` — the autocomplete
    /// selected-row background (7.2 wave 2 composer).
    pub action_focused_bg: Color,
    /// `text.action.primary.focused` — the selected row's text.
    pub action_focused_fg: Color,
    /// `theme.decrease(background.raised.base)` — the composer's prompt
    /// background (`promptBg`, also the selected tab pill's lift in the
    /// horizontal bar) AND the thought-expand gutter's border color
    /// (the `ThoughtEntry` borderColor binding,
    /// group-view.tsx:385-386).
    pub background_prompt: Color,
    /// `theme.hue.interactive[200]` — `activeNumber`
    /// (session-tabs.tsx:568/1331): the busy tab indicator's spinner
    /// color (`<spinner color={props.color}>` with the caller's
    /// `numberColor() = runs() ? activeNumber() : …`,
    /// session-tabs.tsx:1634-1641).
    pub interactive: Color,
    /// Agent identity colors (`local.agent.color`: categorical hues at
    /// step 200, deduped, index-modulo).
    pub agent_colors: [Color; 5],
    /// The flattened tree-sitter scope → style map
    /// (`generate_syntax` → `convert_theme_to_styles`), consumed by the
    /// fenced-code highlight path (R-01 spike).
    pub syntax: oc_syntax::SyntaxStyles,
}

/// The `theme.diff.*` token palette — the diff viewer's gutter, sign
/// and row-background colors (`feature-plugins/system/diff-viewer.tsx`
/// consumes exactly these).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffStyles {
    /// `diff.text.added` — the `+N` count glyphs.
    pub text_added: Color,
    /// `diff.text.removed` — the `-N` count glyphs.
    pub text_removed: Color,
    /// `diff.text.hunkHeader` — the `@@ -a,b +c,d @@` rows.
    pub hunk_header: Color,
    /// `diff.background.added` — added-row text background.
    pub background_added: Color,
    /// `diff.background.removed` — removed-row text background.
    pub background_removed: Color,
    /// `diff.background.context` — the patch card + context rows.
    pub background_context: Color,
    /// `diff.highlight.added` — the ` +` sign.
    pub highlight_added: Color,
    /// `diff.highlight.removed` — the ` -` sign.
    pub highlight_removed: Color,
    /// `diff.lineNumber.text` — gutter numbers.
    pub line_number: Color,
    /// `diff.lineNumber.background.added` — added-row gutter.
    pub line_number_added: Color,
    /// `diff.lineNumber.background.removed` — removed-row gutter.
    pub line_number_removed: Color,
}

/// The markdown token palette (`theme.markdown.*`) — 7.2 lane C's
/// markdown text renderer paints every block from these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkdownStyles {
    pub text: Color,
    pub heading: Color,
    pub strong: Color,
    pub emphasis: Color,
    pub code: Color,
    pub code_block: Color,
    pub link: Color,
    pub link_text: Color,
    pub block_quote: Color,
    pub horizontal_rule: Color,
}

impl ThemeStyles {
    #[must_use]
    pub fn from_resolved(theme: &ResolvedTheme) -> Self {
        let tokens = &theme.tokens;
        let fg = rgba(tokens.text.base);
        let bg = rgba(tokens.background.base);
        let md = &tokens.markdown;
        let mut agent_colors = [bg; 5];
        for (slot, scale) in tokens.categorical.iter().take(5).enumerate() {
            agent_colors[slot] = rgba(scale.s200);
        }
        // The syntax scope map is built once per resolved theme (the
        // message-build side of the R-01 spike: parsing happens per
        // block, style flattening once per theme).
        let syntax = oc_syntax::SyntaxStyles::from_rules(&oc_theme::generate_syntax(tokens));
        Self {
            text: Style::new().fg(fg).bg(bg),
            text_muted: Style::new().fg(rgba(tokens.text.muted)).bg(bg),
            background: bg,
            background_raised: rgba(tokens.background.raised.base),
            action_selected: rgba(tokens.background.action.primary.selected),
            action_hovered: rgba(tokens.background.raised.high),
            accent: rgba(tokens.semantic(oc_theme::types::SemanticHue::Accent).s200),
            error: rgba(tokens.text.feedback.error.base),
            border: rgba(tokens.border.base),
            base: Style::new().fg(fg).bg(bg),
            markdown: MarkdownStyles {
                text: rgba(md.text),
                heading: rgba(md.heading),
                strong: rgba(md.strong),
                emphasis: rgba(md.emphasis),
                code: rgba(md.code),
                code_block: rgba(md.code_block),
                link: rgba(md.link),
                link_text: rgba(md.link_text),
                block_quote: rgba(md.block_quote),
                horizontal_rule: rgba(md.horizontal_rule),
            },
            warning: rgba(tokens.text.feedback.warning.base),
            success: rgba(tokens.text.feedback.success.base),
            info: rgba(tokens.text.feedback.info.base),
            formfield_base: rgba(tokens.text.formfield.base),
            formfield_selected: rgba(tokens.text.formfield.selected),
            destructive_focused: rgba(tokens.text.action.destructive.focused),
            destructive_focused_bg: rgba(tokens.background.action.destructive.focused),
            action_secondary_hovered: rgba(tokens.text.action.secondary.hovered),
            scrollbar: rgba(tokens.scrollbar.base),
            action_focused_bg: rgba(tokens.background.action.primary.focused),
            action_focused_fg: rgba(tokens.text.action.primary.focused),
            background_prompt: {
                let decreased = tokens.decrease(tokens.background.raised.base, 1.0);
                rgba(decreased)
            },
            interactive: rgba(tokens.semantic(oc_theme::types::SemanticHue::Interactive).s200),
            action_secondary: rgba(tokens.text.action.secondary.base),
            diff: DiffStyles {
                text_added: rgba(tokens.diff.text.added),
                text_removed: rgba(tokens.diff.text.removed),
                hunk_header: rgba(tokens.diff.text.hunk_header),
                background_added: rgba(tokens.diff.background.added),
                background_removed: rgba(tokens.diff.background.removed),
                background_context: rgba(tokens.diff.background.context),
                highlight_added: rgba(tokens.diff.highlight.added),
                highlight_removed: rgba(tokens.diff.highlight.removed),
                line_number: rgba(tokens.diff.line_number.text),
                line_number_added: rgba(tokens.diff.line_number.background.added),
                line_number_removed: rgba(tokens.diff.line_number.background.removed),
            },
            agent_colors,
            syntax,
        }
    }

    /// The agent identity color (`local.agent.color`: the categorical
    /// palette indexed by the agent's position, modulo).
    #[must_use]
    pub fn agent_color(&self, index: usize) -> Color {
        self.agent_colors[index % self.agent_colors.len()]
    }
}

fn rgba(color: Rgba) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

// -- the theme registry (theme/index.ts listThemes + discovery.ts) -------------

/// How a registry theme came to exist. Precedence when names collide:
/// builtin < custom < system (`listThemes` spreads in that order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThemeOrigin {
    Builtin,
    Custom,
    System,
}

/// One registry entry: the theme's document (parsed once, resolved per
/// mode on selection) plus its origin tier.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryTheme {
    /// Registry key (`opencode`, a v1 builtin name, a discovered custom
    /// name, or `system`).
    pub name: String,
    pub document: ThemeDocument,
    pub origin: ThemeOrigin,
}

/// The theme registry (`listThemes`): the v2 `opencode` default (which
/// shadows the legacy v1 `opencode` asset), the 32 other v1 builtins,
/// discovered custom themes, and the `system` entry. Entries keep
/// their [`ThemeDocument`] so mode switches and palette previews
/// re-resolve without re-parsing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeRegistry {
    entries: Vec<RegistryTheme>,
}

impl ThemeRegistry {
    /// The full builtin registry (v2 `opencode` + the 32 non-shadowed
    /// v1 assets — `DEFAULT_THEMES` + the v2 default), plus the
    /// always-present `system` slot (`listThemes`'s final spread:
    /// `system: systemTheme ?? themes.system ?? themes.opencode`).
    /// Without a detected/generated system theme (headless: no cached
    /// palette, no OSC probe answer) the slot resolves to the v2
    /// `opencode` document — exactly the rig environment's ref
    /// behavior. A discovered custom `system` theme overrides it
    /// (`themes.system` in the chain sees customs; Builtin origin).
    #[must_use]
    pub fn builtins() -> Self {
        let mut registry = Self::default();
        registry.insert_parsed("opencode", BUILTIN_V2_OPENCODE, ThemeOrigin::Builtin);
        for (name, source) in oc_theme::builtins::BUILTIN_V1_THEMES {
            if *name == "opencode" {
                continue; // shadowed by the v2 default
            }
            registry.insert_parsed(name, source, ThemeOrigin::Builtin);
        }
        registry.insert_parsed("system", BUILTIN_V2_OPENCODE, ThemeOrigin::Builtin);
        registry
    }

    /// Parse + insert one theme source (`"theme"` key → v1 migration,
    /// else the v2 pipeline); invalid sources are logged + skipped.
    fn insert_parsed(&mut self, name: &str, source: &str, origin: ThemeOrigin) {
        match parse_theme_source(source, name) {
            Ok(document) => {
                self.insert(RegistryTheme { name: name.to_owned(), document, origin });
            }
            Err(reason) => crate::keymap::log_skip(&format!("theme {name}: {reason} (skipped)")),
        }
    }

    /// Register a custom theme parsed from raw JSON. Errors describe
    /// the file (the caller logs + skips).
    pub fn insert_custom(&mut self, name: &str, source: &str) -> Result<(), String> {
        let document = parse_theme_source(source, name)?;
        self.insert(RegistryTheme { name: name.to_owned(), document, origin: ThemeOrigin::Custom });
        Ok(())
    }

    /// Insert with the `listThemes` precedence: a higher-origin entry
    /// replaces a lower one of the same name; new entries keep the list
    /// sorted by name (the dialog's `localeCompare` base ordering).
    pub fn insert(&mut self, entry: RegistryTheme) {
        match self.entries.iter().position(|existing| existing.name == entry.name) {
            Some(index) if entry.origin >= self.entries[index].origin => {
                self.entries[index] = entry;
            }
            Some(_) => {}
            None => {
                let key = entry.name.to_lowercase();
                let at =
                    self.entries.partition_point(|existing| existing.name.to_lowercase() < key);
                self.entries.insert(at, entry);
            }
        }
    }

    /// Resolve one registry entry in `mode`.
    #[must_use]
    pub fn resolve(&self, name: &str, mode: Mode) -> Option<ThemeState> {
        let entry = self.entries.iter().find(|entry| entry.name == name)?;
        ThemeState::from_document(&entry.document, &entry.name, mode)
    }

    /// All names in palette order (sorted; the dialog's listing).
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.entries.iter().map(|entry| entry.name.clone()).collect()
    }

    /// The registry size.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// One entry by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RegistryTheme> {
        self.entries.iter().find(|entry| entry.name == name)
    }
}

/// Parse one theme source: `"theme" in source → migrateV1`, else the
/// v2 document pipeline (`theme/index.ts parseTheme`).
fn parse_theme_source(source: &str, name: &str) -> Result<ThemeDocument, String> {
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|error| error.to_string())?;
    if value.get("theme").is_some() {
        migrate_v1(&value).map_err(|error| format!("{error:?}"))
    } else {
        parse_theme_document(&value, name).map_err(|error| format!("{error:?}"))
    }
}

/// Discover custom themes (`theme/discovery.ts discoverThemes`): every
/// `<directory>/themes/*.json` (files + symlinks, sorted by path)
/// across the given directories; later directories override earlier
/// ones with the same name. Pure scan lives in oc-theme
/// (`oc_theme::discover_themes`).
#[must_use]
pub fn discover_custom_themes(directories: &[PathBuf]) -> Vec<(String, String)> {
    oc_theme::discover_themes(directories)
}

/// Register every discovered custom theme into the registry (invalid
/// entries logged + skipped, per the v2 discovery pipeline — the
/// reference's `setCustomThemes` filter drops invalid sources the same
/// way; a bad file never bricks the app).
pub fn register_custom_themes(registry: &mut ThemeRegistry, discovered: &[(String, String)]) {
    for (name, source) in discovered {
        if let Err(reason) = registry.insert_custom(name, source) {
            crate::keymap::log_skip(&format!("theme {name}: {reason} (skipped)"));
        }
    }
}

/// The TUI config directories for theme discovery
/// (`util/config-directories.ts configDirectories`): the config root,
/// then a `.opencode` dir in every cwd ancestor (root → leaf; later
/// directories win). Lives in oc-theme (`oc_theme::config_directories`).
#[must_use]
pub fn config_directories(config_root: &Path, cwd: &Path) -> Vec<PathBuf> {
    oc_theme::config_directories(config_root, cwd)
}

// -- system mode (theme/system.ts terminalMode) -------------------------------

/// System dark/light detection from a terminal background color: the
/// reference's `terminalMode` luma rule. Pure classification lives in
/// oc-theme (`oc_theme::mode_for_background`).
#[must_use]
pub fn mode_for_background(r: u8, g: u8, b: u8) -> Mode {
    oc_theme::mode_for_background(r, g, b)
}

/// Detect the system mode: query the terminal background (OSC 11) with
/// a short timeout and classify its luminance. Falls back to dark —
/// headless pipes (no TTY), timeouts, and unparseable answers all
/// resolve dark (`waitForThemeMode`'s default).
#[must_use]
pub fn detect_system_mode() -> Mode {
    query_terminal_background().map_or(Mode::Dark, |(r, g, b)| mode_for_background(r, g, b))
}

/// Query the terminal's default background color via OSC 11
/// (`\x1b]11;?\x07`) with a **raw, self-contained read** of the tty
/// (poll + read on fd 0). The probe runs BEFORE crossterm's event
/// reader starts (the run loop's input thread calls it first), so it
/// owns stdin exclusively for its short window; every byte that
/// arrives inside the window is consumed here — the reply never leaks
/// into the input stream (a responding terminal's `11;rgb:…` must
/// never surface as typed text).
///
/// `None` when stdin/stdout is not a TTY, the write fails, or no
/// parsable answer arrives within the timeout (headless pipes, dumb
/// terminals) — the caller's dark fallback.
#[must_use]
pub fn query_terminal_background() -> Option<(u8, u8, u8)> {
    use std::io::{IsTerminal, Write};
    use std::time::Duration;

    const PROBE_TIMEOUT: Duration = Duration::from_millis(150);

    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return None; // headless: dark fallback
    }
    if crossterm::terminal::enable_raw_mode().is_err() {
        return None;
    }
    let answer = (|| {
        {
            let mut stdout = std::io::stdout().lock();
            if stdout.write_all(b"\x1b]11;?\x07").is_err() || stdout.flush().is_err() {
                return None;
            }
        }
        read_osc11_reply(PROBE_TIMEOUT)
    })();
    let _ = crossterm::terminal::disable_raw_mode();
    answer
}

/// The live-loop variant of [`query_terminal_background`] (lane TAIL,
/// auditC P6): the caller ALREADY holds raw mode (the run loop), so
/// this probes WITHOUT toggling it — the `?997` stand-in's read arm.
#[must_use]
pub fn query_terminal_background_in_raw() -> Option<(u8, u8, u8)> {
    use std::io::{IsTerminal, Write};
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return None;
    }
    {
        let mut stdout = std::io::stdout().lock();
        if stdout.write_all(b"\x1b]11;?\x07").is_err() || stdout.flush().is_err() {
            return None;
        }
    }
    read_osc11_reply(std::time::Duration::from_millis(150))
}

/// The boot probe's findings: the OSC 11 background (the theme-mode
/// seed), the kitty keyboard advertisement (the `CSI ? u` query's
/// answer — crossterm 0.28 has no public event for it, so the answer
/// is parsed in this same boot window; lane TAIL, auditC P2/TM-1),
/// and the DA1 (Primary Device Attributes) answer's sixel verdict
/// (RB-07: `CSI c` → `CSI ? {attrs} c`; attribute `4` among the
/// reply's `;`-separated parameters means sixel graphics — the
/// VT240-class convention xterm `-ti vt340` and mlterm report as
/// e.g. `\x1b[?62;4;6;8;15;22c`; terminals without sixel (kitty:
/// `\x1b[?62;6;9;15;22c`) omit it). `None` = no DA1 answer inside
/// the window (the env fallback's case).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TerminalProbe {
    /// The terminal background color (`None` on timeout).
    pub background: Option<(u8, u8, u8)>,
    /// The terminal answered the kitty keyboard query (`CSI ? flags u`).
    pub kitty_keyboard: bool,
    /// The DA1 answer's sixel verdict (`None` = no DA1 answer).
    pub da1_sixel: Option<bool>,
}

/// The combined boot probe: write the OSC 11 background query, the
/// kitty keyboard capability query, AND the DA1 (`CSI c`) query
/// together, read ONE 300 ms window, parse all three answers (v2
/// writes its boot queries together; one window keeps the
/// keystroke-drop exposure to a single interval).
#[must_use]
pub fn probe_terminal() -> TerminalProbe {
    use std::io::{IsTerminal, Write};
    use std::time::Duration;

    const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return TerminalProbe::default(); // headless: dark fallback, no kitty
    }
    if crossterm::terminal::enable_raw_mode().is_err() {
        return TerminalProbe::default();
    }
    let answer = (|| {
        {
            let mut stdout = std::io::stdout().lock();
            let queries = b"\x1b]11;?\x07\x1b[?u\x1b[c";
            if stdout.write_all(queries).is_err() || stdout.flush().is_err() {
                return TerminalProbe::default();
            }
        }
        collect_probe_answers(PROBE_TIMEOUT)
    })();
    let _ = crossterm::terminal::disable_raw_mode();
    answer
}

/// Read the probe window until all answers land or the deadline: the
/// OSC 11 color (`parse_osc_reply`), the kitty advertisement
/// (`CSI ? flags u` — any digit run before a `u` after `[?`), and the
/// DA1 sixel verdict (`parse_da1_sixel` — `CSI ? attrs c`).
#[allow(unsafe_code)] // poll/read on fd 0 are only reachable through libc FFI
fn collect_probe_answers(timeout: std::time::Duration) -> TerminalProbe {
    let deadline = std::time::Instant::now() + timeout;
    let mut buffer: Vec<u8> = Vec::with_capacity(128);
    let mut probe = TerminalProbe::default();
    loop {
        if probe.background.is_some() && probe.kitty_keyboard && probe.da1_sixel.is_some() {
            return probe;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return probe;
        }
        let mut poll_fd = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
        #[allow(clippy::cast_possible_truncation)]
        let remaining_ms =
            i32::try_from((deadline - now).as_millis()).unwrap_or(0).min(i32::MAX - 1) + 1;
        let ready = unsafe { libc::poll(std::ptr::from_mut(&mut poll_fd), 1, remaining_ms) };
        if ready <= 0 {
            return probe; // timeout or poll failure: no answer
        }
        let mut chunk = [0u8; 128];
        let read = unsafe {
            libc::read(libc::STDIN_FILENO, chunk.as_mut_ptr().cast::<libc::c_void>(), chunk.len())
        };
        if read <= 0 {
            return probe;
        }
        #[allow(clippy::cast_sign_loss)]
        buffer.extend_from_slice(&chunk[..read as usize]);
        if probe.background.is_none() {
            probe.background = parse_osc_reply(&buffer);
        }
        if !probe.kitty_keyboard {
            probe.kitty_keyboard = parse_kitty_advertisement(&buffer);
        }
        if probe.da1_sixel.is_none() {
            probe.da1_sixel = parse_da1_sixel(&buffer);
        }
        // Overlong garbage never matches: give up rather than spin.
        if buffer.len() > 512 {
            return probe;
        }
    }
}

/// The kitty keyboard advertisement: a `CSI ? {flags} u` reply (any
/// flag bits — the run loop pushes its own flag set, not the reply's).
fn parse_kitty_advertisement(bytes: &[u8]) -> bool {
    let mut index = 0;
    while let Some(at) = bytes[index..].iter().position(|b| *b == b'\x1b') {
        let start = index + at;
        let rest = &bytes[start..];
        if rest.len() >= 3 && rest[1] == b'[' && rest[2] == b'?' {
            let mut cursor = 3;
            let mut digits = false;
            while cursor < rest.len() && rest[cursor].is_ascii_digit() {
                digits = true;
                cursor += 1;
            }
            if digits && cursor < rest.len() && rest[cursor] == b'u' {
                return true;
            }
        }
        index = start + 1;
    }
    false
}

/// The DA1 (Primary Device Attributes) sixel verdict: find a
/// `CSI ? {attrs} c` reply (`ESC [ ?` … `c`) and report whether
/// attribute `4` (sixel graphics, the VT240-class convention) is
/// among its `;`-separated top-level parameters. Sub-parameters
/// (`:`-groups, e.g. `4;` in `27;4:32`) only match on the leading
/// value. `None` when no complete DA1 reply is in the buffer.
fn parse_da1_sixel(bytes: &[u8]) -> Option<bool> {
    let mut index = 0;
    while let Some(at) = bytes[index..].iter().position(|b| *b == b'\x1b') {
        let start = index + at;
        let rest = &bytes[start..];
        if rest.len() >= 3 && rest[1] == b'[' && rest[2] == b'?' {
            let mut cursor = 3;
            let mut attrs: Vec<&[u8]> = Vec::new();
            let mut current = cursor;
            let mut complete = false;
            while cursor < rest.len() {
                match rest[cursor] {
                    digit if digit.is_ascii_digit() || digit == b':' => cursor += 1,
                    b';' => {
                        attrs.push(&rest[current..cursor]);
                        cursor += 1;
                        current = cursor;
                    }
                    b'c' => {
                        attrs.push(&rest[current..cursor]);
                        complete = true;
                        break;
                    }
                    _ => break, // not a DA1 reply after all
                }
            }
            if complete {
                let sixel = attrs
                    .iter()
                    .any(|attr| attr.split(|b| *b == b':').next().is_some_and(|head| head == b"4"));
                return Some(sixel);
            }
        }
        index = start + 1;
    }
    None
}

/// Read the terminal's OSC 11 answer off fd 0, discarding anything
/// else that arrives in the window (terminal responses only — user
/// keystrokes during the 150 ms boot probe are dropped, matching v2's
/// boot-time theme probe). Returns `None` on timeout or an unparsable
/// reply.
#[allow(unsafe_code)] // poll/read on fd 0 are only reachable through libc FFI
fn read_osc11_reply(timeout: std::time::Duration) -> Option<(u8, u8, u8)> {
    let deadline = std::time::Instant::now() + timeout;
    let mut buffer: Vec<u8> = Vec::with_capacity(64);
    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            return None;
        }
        let mut poll_fd = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
        #[allow(clippy::cast_possible_truncation)]
        let remaining_ms =
            i32::try_from((deadline - now).as_millis()).unwrap_or(0).min(i32::MAX - 1) + 1;
        let ready = unsafe { libc::poll(std::ptr::from_mut(&mut poll_fd), 1, remaining_ms) };
        if ready <= 0 {
            return None; // timeout or poll failure: no answer
        }
        let mut chunk = [0u8; 64];
        let read = unsafe {
            libc::read(libc::STDIN_FILENO, chunk.as_mut_ptr().cast::<libc::c_void>(), chunk.len())
        };
        if read <= 0 {
            return None;
        }
        #[allow(clippy::cast_sign_loss)]
        buffer.extend_from_slice(&chunk[..read as usize]);
        // A complete answer ends with BEL or ST after an OSC introducer.
        if let Some(color) = parse_osc_reply(&buffer) {
            return Some(color);
        }
        // Overlong garbage never matches: give up rather than spin.
        if buffer.len() > 256 {
            return None;
        }
    }
}

/// Extract an OSC 11 color reply from raw bytes: a `ESC ] 11 ;` prefix
/// (the query echo in some terminals is tolerated by scanning for the
/// `11;rgb:` body) terminated by BEL (`\x07`) or ST (`ESC \`).
fn parse_osc_reply(bytes: &[u8]) -> Option<(u8, u8, u8)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find("11;rgb:")?;
    let body = &text[start..];
    let end = body.find(['\x07', '\x1b']).unwrap_or(body.len());
    parse_osc_color(&body[..end])
}

/// Parse an OSC 11 answer (`\x1b]11;rgb:rrrr/gggg/bbbb` + ST/BEL).
/// 16-bit components scale to 8-bit (`/ 257`); shorter forms pass
/// through `u8::from_str_radix` directly.
#[must_use]
pub fn parse_osc_color(text: &str) -> Option<(u8, u8, u8)> {
    let start = text.find("rgb:")?;
    let rest = &text[start + 4..];
    let end = rest.find(['\x1b', '\u{7}']).unwrap_or(rest.len());
    let triple: Vec<&str> = rest[..end].split('/').collect();
    if triple.len() != 3 {
        return None;
    }
    let channel = |part: &str| -> Option<u8> {
        let hex = &part[..part.len().min(4)];
        if hex.len() == 4 {
            u16::from_str_radix(hex, 16).ok().map(|full| u8::try_from(full / 257).unwrap_or(255))
        } else {
            u8::from_str_radix(hex, 16).ok()
        }
    };
    Some((channel(triple[0])?, channel(triple[1])?, channel(triple[2])?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_dark_default_resolves() {
        let state = ThemeState::v2_dark();
        assert_eq!(state.mode, Mode::Dark);
        let styles = state.styles();
        assert_eq!(
            styles.background,
            Color::Rgb(
                state.resolved.tokens.background.base.r,
                state.resolved.tokens.background.base.g,
                state.resolved.tokens.background.base.b
            )
        );
    }

    #[test]
    fn mode_switch_reresolves() {
        let mut state = ThemeState::v2_dark();
        let dark_bg = state.resolved.tokens.background.base;
        state.switch_mode(Mode::Light);
        assert_eq!(state.mode, Mode::Light);
        assert_ne!(state.resolved.tokens.background.base, dark_bg, "light base differs");
        state.switch_mode(Mode::Dark);
        assert_eq!(state.resolved.tokens.background.base, dark_bg);
    }

    #[test]
    fn unknown_mode_is_ignored() {
        let mut state = ThemeState::v2_dark();
        state.switch_mode(Mode::Light);
        state.switch_mode(Mode::Dark);
        assert_eq!(state.mode, Mode::Dark);
    }

    #[test]
    fn system_detection_classifies_luminance() {
        // Dark backgrounds stay dark, bright ones go light (the
        // reference's 0.5 threshold on 0-1 components = 127.5 / 255).
        assert_eq!(mode_for_background(10, 10, 10), Mode::Dark);
        assert_eq!(mode_for_background(0, 0, 0), Mode::Dark);
        assert_eq!(mode_for_background(250, 250, 250), Mode::Light);
        assert_eq!(mode_for_background(255, 255, 0), Mode::Light);
        // OSC 11 answers parse (16-bit components scale; ST and BEL
        // terminators both accepted).
        assert_eq!(parse_osc_color("\x1b]11;rgb:2828/2828/2828\x1b\\"), Some((40, 40, 40)));
        assert_eq!(parse_osc_color("\x1b]11;rgb:ffff/ffff/ffff\u{7}"), Some((255, 255, 255)));
        assert_eq!(parse_osc_color("\x1b]11;rgb:00/00/00\u{7}"), Some((0, 0, 0)));
        assert_eq!(parse_osc_color("garbage"), None);
        // Headless (no TTY) detection falls back to dark.
        assert_eq!(detect_system_mode(), Mode::Dark);
    }

    /// The reported beta.2 regression set, pinned on the user's actual
    /// terminal: tmux answers OSC 11 with `rgb:1717/1717/1717`
    /// (#171717 background). The reply must parse, classify DARK (the
    /// luma rule), and the resolved default theme's composer accent —
    /// categorical[0] at step 200 — must be the v2 blue (#5c9cf5), not
    /// the gray border token the broken probe left behind.
    #[test]
    fn tmux_background_171717_resolves_dark_with_blue_accent() {
        // The raw reply bytes exactly as a responding terminal emits
        // them (and exactly the bytes that used to leak into the
        // composer as typed text).
        let reply = b"\x1b]11;rgb:1717/1717/1717\x1b\\";
        let color = parse_osc_reply(reply).expect("the tmux reply parses");
        assert_eq!(color, (0x17, 0x17, 0x17), "0x1717 = 23 decimal");
        assert_eq!(mode_for_background(color.0, color.1, color.2), Mode::Dark);
        // The BEL-terminated form parses identically.
        assert_eq!(parse_osc_reply(b"\x1b]11;rgb:1717/1717/1717\x07"), Some((0x17, 0x17, 0x17)));
        // Partial/garbled replies never resolve.
        assert_eq!(parse_osc_reply(b"\x1b]11;rgb:171"), None);
        assert_eq!(parse_osc_reply(b"garbage with no rgb at all"), None);

        // The accent chain under that mode: the v2 default theme
        // resolved in dark mode paints the composer rail in the FIRST
        // categorical hue's 200 step — blue #5c9cf5 (v1 migration maps
        // `secondary` → categorical[0]).
        let state = ThemeState::v2_dark();
        assert_eq!(state.mode, Mode::Dark);
        let styles = state.styles();
        assert_eq!(styles.agent_color(0), Color::Rgb(0x5C, 0x9C, 0xF5));
        // …and the gray border token is a DIFFERENT color (the rail
        // must never fall back to it when an agent is resolved).
        assert_ne!(styles.agent_color(0), styles.border);
    }

    /// RB-07 (lane R0): the DA1 sixel verdict. Attribute `4` among the
    /// `CSI ? {attrs} c` reply's `;`-separated parameters = sixel
    /// (xterm `-ti vt340` / mlterm style); an answered DA1 without it
    /// = no sixel (kitty's reply); no/other replies = `None`.
    #[test]
    fn da1_sixel_verdict_parses() {
        // xterm vt340-style: level 62 + sixel(4) + others.
        assert_eq!(parse_da1_sixel(b"\x1b[?62;4;6;8;15;22c"), Some(true));
        // The task-shape reply: `;4;` anywhere in the attribute list.
        assert_eq!(parse_da1_sixel(b"\x1b[?62;4;6;8;15;18;21;22c"), Some(true));
        // Sixel alone (minimal VT240-class reply).
        assert_eq!(parse_da1_sixel(b"\x1b[?4c"), Some(true));
        // kitty: answers DA1 but never with the sixel attribute.
        assert_eq!(parse_da1_sixel(b"\x1b[?62;6;9;15;22c"), Some(false));
        // The minimal `?6c` form still counts as answered-no-sixel.
        assert_eq!(parse_da1_sixel(b"\x1b[?6c"), Some(false));
        // Sub-parameter groups (`:`-separated) match on the leading
        // value only (DA1 itself has none; the tolerance is safe).
        assert_eq!(parse_da1_sixel(b"\x1b[?62;4:32c"), Some(true));
        assert_eq!(parse_da1_sixel(b"\x1b[?62;14:32c"), Some(false));
        // Mixed streams: the DA1 reply rides alongside other answers.
        assert_eq!(
            parse_da1_sixel(b"\x1b]11;rgb:1717/1717/1717\x1b\\\x1b[?1u\x1b[?62;4c"),
            Some(true)
        );
        // Not a DA1 final byte / no reply yet.
        assert_eq!(parse_da1_sixel(b"\x1b[?62;4"), None);
        assert_eq!(parse_da1_sixel(b"\x1b[?1;2c\x1b[?1u"), Some(false)); // classic xterm DA1
        assert_eq!(parse_da1_sixel(b""), None);
        assert_eq!(parse_da1_sixel(b"garbage"), None);
    }

    /// The builtin registry: 34 entries (v2 opencode + 32 v1 + the
    /// always-present `system` slot), every one resolvable in dark
    /// mode, names sorted.
    #[test]
    fn builtin_registry_resolves_every_theme() {
        let registry = ThemeRegistry::builtins();
        assert_eq!(
            registry.len(),
            34,
            "v2 opencode + 32 v1 (the v1 opencode is shadowed) + the system slot"
        );
        let names = registry.names();
        assert_eq!(names[0], "aura", "sorted, case-insensitive");
        assert!(names.contains(&"opencode".to_owned()));
        assert!(names.contains(&"system".to_owned()), "listThemes always exposes a system slot");
        // The headless system slot resolves to the opencode document
        // (`systemTheme ?? themes.system ?? themes.opencode` with no
        // detected/generated palette).
        let system = registry.resolve("system", Mode::Dark).expect("system resolves");
        assert_eq!(system.name, "system");
        assert_eq!(
            system.resolved.tokens.background.base,
            registry
                .resolve("opencode", Mode::Dark)
                .expect("opencode resolves")
                .resolved
                .tokens
                .background
                .base,
            "headless system == the opencode document"
        );
        for name in &names {
            let state =
                registry.resolve(name, Mode::Dark).unwrap_or_else(|| panic!("{name} resolves"));
            assert_eq!(state.name, *name);
        }
        // The v2 default is the `opencode` entry (not the v1 asset).
        let opencode = registry.resolve("opencode", Mode::Dark).expect("resolves");
        assert_eq!(
            opencode.resolved.tokens.background.base,
            ThemeState::v2_dark().resolved.tokens.background.base
        );
        // Mode switches re-resolve through the kept document.
        let light = registry.resolve("opencode", Mode::Light).expect("light mode");
        assert_eq!(light.mode, Mode::Light);
    }

    /// Custom discovery: themes/*.json across config dirs, later dirs
    /// overriding, invalid files skipped (logged), precedence
    /// custom > builtin.
    #[test]
    fn custom_theme_discovery_registers_with_precedence() {
        let dir = std::env::temp_dir().join(format!("oc-tui-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let themes = dir.join("themes");
        std::fs::create_dir_all(&themes).expect("mkdir");
        std::fs::write(themes.join("zeta.json"), r#"{ "$schema": "x" }"#).expect("write");
        // A valid v2 document: minimal but parseable through the
        // pipeline — reuse the builtin as a known-good source.
        std::fs::write(themes.join("mytheme.json"), BUILTIN_V2_OPENCODE).expect("write");
        std::fs::write(themes.join("broken.json"), "{ not json").expect("write");

        let discovered = discover_custom_themes(std::slice::from_ref(&dir));
        assert_eq!(discovered.len(), 3, "all files found, sorted");
        let mut registry = ThemeRegistry::builtins();
        register_custom_themes(&mut registry, &discovered);
        // The valid custom theme registered and RESOLVES; the broken
        // one AND the schema-only document (no `theme`/`base` key —
        // the reference's `isThemeSource` filter) were skipped.
        assert!(registry.get("mytheme").is_some(), "valid custom theme registered");
        assert!(registry.get("broken").is_none(), "broken theme skipped");
        assert!(registry.get("zeta").is_none(), "schema-only source filtered (isThemeSource)");
        let names = registry.names();
        assert_eq!(names.len(), 35, "34 builtins + the custom theme");
        assert!(registry.resolve("mytheme", Mode::Dark).is_some());
        // Precedence: a custom theme shadowing a builtin name replaces
        // it (custom > builtin).
        registry.insert_custom("opencode", BUILTIN_V2_OPENCODE).expect("insert over builtin");
        assert_eq!(registry.get("opencode").map(|entry| entry.origin), Some(ThemeOrigin::Custom));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Config directories: config root first, then `.opencode` in
    /// every cwd ancestor (root → leaf order).
    #[test]
    fn config_directories_walk_cwd_ancestors() {
        let directories = config_directories(
            Path::new("/root/.config/opencode"),
            Path::new("/opt/opencode-rust/crates/oc-tui"),
        );
        assert_eq!(directories[0], Path::new("/root/.config/opencode"));
        assert_eq!(directories[1], Path::new("/.opencode"));
        assert!(directories.contains(&PathBuf::from("/opt/opencode-rust/.opencode")));
        let last = directories.last().unwrap();
        assert_eq!(last, Path::new("/opt/opencode-rust/crates/oc-tui/.opencode"));
    }
}

// -- lane TAIL (auditB #8): the remaining token bindings --------

/// The nord-class tokens resolve away from the v2-dark defaults
/// (the audit's "correct on v2-dark, wrong on every other theme"
/// family).
#[test]
fn nord_resolves_the_new_token_family() {
    let registry = crate::theme::ThemeRegistry::builtins();
    let nord = registry.resolve("nord", oc_theme::types::Mode::Dark).expect("nord resolves");
    let dark = ThemeState::v2_dark();
    let a = nord.styles();
    let b = dark.styles();
    assert_ne!(a.formfield_selected, b.formfield_selected);
    assert_ne!(a.destructive_focused, b.destructive_focused);
    assert_ne!(a.destructive_focused_bg, b.destructive_focused_bg);
    assert_ne!(a.action_secondary_hovered, b.action_secondary_hovered);
    assert_ne!(a.scrollbar, b.scrollbar);
    // The jump row's info token already existed (SURF-1); it stays.
    assert_ne!(a.info, b.info);
}
