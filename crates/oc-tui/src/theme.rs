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

use crate::style::{Color, Style};
use oc_theme::builtins::BUILTIN_V2_OPENCODE;
use oc_theme::resolve::{parse_theme_document, resolve_theme_document, ThemeDocument};
use oc_theme::types::{Mode, ResolvedTheme};
use oc_theme::v1_migrate::migrate_v1;
use oc_theme::Rgba;

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
    pub syntax: crate::components::syntax::SyntaxStyles,
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
        let syntax =
            crate::components::syntax::SyntaxStyles::from_rules(&oc_theme::generate_syntax(tokens));
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

// -- system theme generation (theme/system.ts generateSystem) -----------------

/// The terminal palette pieces `generateSystem` consumes
/// (`TerminalColors` from `@opentui/core`): the OSC-queried defaults
/// plus the 16-color ANSI palette as hex strings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerminalColors {
    pub default_background: Option<String>,
    pub default_foreground: Option<String>,
    pub palette: Vec<Option<String>>,
}

/// The palette-provider seam (`context/theme.tsx refreshPalette`'s
/// `renderer.getPalette()`): production queries the terminal once at
/// boot ([`CachedPaletteProvider`]); tests inject a stub. `None` =
/// palette unavailable → the documented fallback keeps the builtin
/// `opencode` document in the `system` slot.
pub trait PaletteProvider {
    fn query(&self) -> Option<TerminalColors>;
}

/// The production provider: the boot palette snapshot. The OSC 4/10/11
/// probe runs before the input thread owns stdin
/// ([`crate::terminal_proto::query_terminal_palette`]) and every
/// regime reuses the snapshot (a transient timeout must not remap an
/// immutable resolved theme — the ref caches it the same way).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CachedPaletteProvider {
    colors: Option<TerminalColors>,
}

impl CachedPaletteProvider {
    #[must_use]
    pub fn new(colors: Option<TerminalColors>) -> Self {
        Self { colors }
    }
}

impl PaletteProvider for CachedPaletteProvider {
    fn query(&self) -> Option<TerminalColors> {
        self.colors.clone()
    }
}

/// `tint` (theme/color.ts): lerp `overlay` over `base` by `alpha` on
/// the 0-1 float channels, rounding like the ref.
fn tint(base: Rgba, overlay: Rgba, alpha: f64) -> Rgba {
    let [br, bg_, bb, _] = base.floats();
    let [or_, og, ob, _] = overlay.floats();
    let channel =
        |b: f64, o: f64| ((b + (o - b) * alpha) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba::from_ints(channel(br, or_), channel(bg_, og), channel(bb, ob), 255)
}

/// `generateGrayScale` (system.ts): 12 grays stepping 40% of the way
/// from the background toward its extreme, with flat ramps for
/// near-black/near-white backgrounds.
fn generate_gray_scale(bg: Rgba, is_dark: bool) -> Vec<Rgba> {
    let [bgr, bgg, bgb, _] = bg.to_ints();
    let bg_r = f64::from(bgr);
    let bg_g = f64::from(bgg);
    let bg_b = f64::from(bgb);
    let luminance = 0.299 * bg_r + 0.587 * bg_g + 0.114 * bg_b;
    let mut grays = vec![Rgba::TRANSPARENT; 13]; // index 0 unused, ref keys 1..=12
    for i in 1..=12usize {
        let factor = i as f64 / 12.0;
        let gray;
        if is_dark && luminance < 10.0 {
            gray = (factor * 0.4 * 255.0).floor();
            grays[i] = Rgba::from_ints(gray as u8, gray as u8, gray as u8, 255);
            continue;
        }
        if !is_dark && luminance > 245.0 {
            gray = (255.0 - factor * 0.4 * 255.0).floor();
            grays[i] = Rgba::from_ints(gray as u8, gray as u8, gray as u8, 255);
            continue;
        }
        let next = if is_dark {
            luminance + (255.0 - luminance) * factor * 0.4
        } else {
            luminance * (1.0 - factor * 0.4)
        };
        let ratio = if luminance == 0.0 { 0.0 } else { next / luminance };
        let channel = |value: f64| {
            ((value * ratio).min(255.0).max(0.0)).floor() as u8
        };
        grays[i] = Rgba::from_ints(channel(bg_r), channel(bg_g), channel(bg_b), 255);
    }
    grays
}

/// `generateMutedTextColor` (system.ts).
fn generate_muted_text_color(bg: Rgba, is_dark: bool) -> Rgba {
    let [r, g, b, _] = bg.to_ints();
    let luminance = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
    let gray = if is_dark {
        if luminance < 10.0 { 180.0 } else { (160.0 + luminance * 0.3).floor().min(200.0) }
    } else if luminance > 245.0 {
        75.0
    } else {
        (100.0 - (255.0 - luminance) * 0.2).floor().max(60.0)
    } as u8;
    Rgba::from_ints(gray, gray, gray, 255)
}

/// `generateSystem` (theme/system.ts): derive a v1-shaped theme from
/// the terminal palette — background/foreground fall back to palette
/// slots 0/7, missing palette entries fall back to the ANSI table
/// (`ansiToRgba`), and the migrated document is produced through the
/// same `migrateV1` pipeline every v1 theme takes
/// (`theme/index.ts parseTheme`).
#[must_use]
pub fn generate_system(colors: &TerminalColors, mode: Mode) -> Option<ThemeDocument> {
    let bg_hex = colors.default_background.clone().or_else(|| colors.palette.first().cloned().flatten())?;
    let fg_hex = colors
        .default_foreground
        .clone()
        .or_else(|| colors.palette.get(7).cloned().flatten())?;
    let bg = Rgba::from_hex(&bg_hex)?;
    let fg = Rgba::from_hex(&fg_hex)?;
    let is_dark = mode == Mode::Dark;

    // `col(index)`: palette entry when present, else the ANSI table.
    let col = |index: usize| -> Rgba {
        colors
            .palette
            .get(index)
            .and_then(|value| value.as_deref())
            .and_then(Rgba::from_hex)
            .unwrap_or_else(|| oc_theme::v1::ansi(index as i64))
    };

    let grays = generate_gray_scale(bg, is_dark);
    let text_muted = generate_muted_text_color(bg, is_dark);
    let red = col(1);
    let green = col(2);
    let yellow = col(3);
    let blue = col(4);
    let magenta = col(5);
    let cyan = col(6);
    let red_bright = col(9);
    let green_bright = col(10);

    let diff_alpha = if is_dark { 0.22 } else { 0.14 };
    let diff_added_bg = tint(bg, green, diff_alpha);
    let diff_removed_bg = tint(bg, red, diff_alpha);
    let diff_context_bg = grays[2];
    let diff_added_line_number_bg = tint(diff_context_bg, green, diff_alpha);
    let diff_removed_line_number_bg = tint(diff_context_bg, red, diff_alpha);

    let hex = |color: Rgba| serde_json::json!(color.hex());
    let mut theme = serde_json::Map::new();
    theme.insert("primary".into(), hex(cyan));
    theme.insert("secondary".into(), hex(magenta));
    theme.insert("accent".into(), hex(cyan));
    theme.insert("error".into(), hex(red));
    theme.insert("warning".into(), hex(yellow));
    theme.insert("success".into(), hex(green));
    theme.insert("info".into(), hex(cyan));
    theme.insert("text".into(), hex(fg));
    theme.insert("textMuted".into(), hex(text_muted));
    theme.insert("selectedListItemText".into(), hex(bg));
    // The ref keeps the terminal background: alpha 0.
    theme.insert("background".into(), serde_json::json!("transparent"));
    theme.insert("backgroundPanel".into(), hex(grays[2]));
    theme.insert("backgroundElement".into(), hex(grays[3]));
    theme.insert("backgroundMenu".into(), hex(grays[3]));
    theme.insert("borderSubtle".into(), hex(grays[6]));
    theme.insert("border".into(), hex(grays[7]));
    theme.insert("borderActive".into(), hex(grays[8]));
    theme.insert("diffAdded".into(), hex(green));
    theme.insert("diffRemoved".into(), hex(red));
    theme.insert("diffContext".into(), hex(grays[7]));
    theme.insert("diffHunkHeader".into(), hex(grays[7]));
    theme.insert("diffHighlightAdded".into(), hex(green_bright));
    theme.insert("diffHighlightRemoved".into(), hex(red_bright));
    theme.insert("diffAddedBg".into(), hex(diff_added_bg));
    theme.insert("diffRemovedBg".into(), hex(diff_removed_bg));
    theme.insert("diffContextBg".into(), hex(diff_context_bg));
    theme.insert("diffLineNumber".into(), hex(text_muted));
    theme.insert("diffAddedLineNumberBg".into(), hex(diff_added_line_number_bg));
    theme.insert("diffRemovedLineNumberBg".into(), hex(diff_removed_line_number_bg));
    theme.insert("markdownText".into(), hex(fg));
    theme.insert("markdownHeading".into(), hex(fg));
    theme.insert("markdownLink".into(), hex(blue));
    theme.insert("markdownLinkText".into(), hex(cyan));
    theme.insert("markdownCode".into(), hex(green));
    theme.insert("markdownBlockQuote".into(), hex(yellow));
    theme.insert("markdownEmph".into(), hex(yellow));
    theme.insert("markdownStrong".into(), hex(fg));
    theme.insert("markdownHorizontalRule".into(), hex(grays[7]));
    theme.insert("markdownListItem".into(), hex(blue));
    theme.insert("markdownListEnumeration".into(), hex(cyan));
    theme.insert("markdownImage".into(), hex(blue));
    theme.insert("markdownImageText".into(), hex(cyan));
    theme.insert("markdownCodeBlock".into(), hex(fg));
    theme.insert("syntaxComment".into(), hex(text_muted));
    theme.insert("syntaxKeyword".into(), hex(magenta));
    theme.insert("syntaxFunction".into(), hex(blue));
    theme.insert("syntaxVariable".into(), hex(fg));
    theme.insert("syntaxString".into(), hex(green));
    theme.insert("syntaxNumber".into(), hex(yellow));
    theme.insert("syntaxType".into(), hex(cyan));
    theme.insert("syntaxOperator".into(), hex(cyan));
    theme.insert("syntaxPunctuation".into(), hex(fg));

    let document = serde_json::json!({
        "$schema": "https://opencode.ai/theme.json",
        "theme": theme,
    });
    migrate_v1(&document).ok()
}

impl ThemeRegistry {
    /// `setSystemTheme(generateSystem(colors, mode))`: replace the
    /// `system` slot with the palette-derived theme (a failed
    /// derivation keeps the previous slot).
    pub fn set_system_theme(&mut self, colors: &TerminalColors, mode: Mode) -> bool {
        let Some(document) = generate_system(colors, mode) else { return false };
        self.insert(RegistryTheme {
            name: "system".to_string(),
            document,
            origin: ThemeOrigin::System,
        });
        true
    }
}

/// The resolution/boot seam (`context/theme.tsx refreshPalette` +
/// `applyPalette`): when the selected slot is `system`, query the
/// palette and install `generateSystem(colors, mode)` into the
/// registry BEFORE the name resolves. A non-`system` slot never probes
/// (the ref's `if (theme !== "system") return`), and an unavailable
/// palette returns `false`, leaving the documented builtin fallback in
/// the `system` slot.
pub fn install_system_theme(
    registry: &mut ThemeRegistry,
    name: &str,
    mode: Mode,
    provider: &dyn PaletteProvider,
) -> bool {
    if name != "system" {
        return false;
    }
    let Some(colors) = provider.query() else { return false };
    registry.set_system_theme(&colors, mode)
}

// -- system mode (theme/system.ts terminalMode) -------------------------------
//
// Lane B3a — the self-managed probe family RETIRED: the boot queries
// (OSC 11 background, kitty `CSI ? u`, DA1 attr-4 sixel) and the
// focus-time OSC 11 re-probe now ride opentui's capability probe
// (`Terminal::probe_capabilities`) and its theme watch, surfacing as
// `CliRenderEvent::Capabilities` / `ThemeMode` (see `crate::app`'s
// input thread and `crate::input_term`). The pure helpers stay: the
// luma classification is shared vocabulary.

/// System dark/light detection from a terminal background color: the
/// reference's `terminalMode` luma rule. Pure classification lives in
/// oc-theme (`oc_theme::mode_for_background`).
#[must_use]
pub fn mode_for_background(r: u8, g: u8, b: u8) -> Mode {
    oc_theme::mode_for_background(r, g, b)
}

/// Parse an OSC 11 answer (`\x1b]11;rgb:rrrr/gggg/bbbb` + ST/BEL).
/// 16-bit components scale to 8-bit (`/ 257`); shorter forms pass
/// through `u8::from_str_radix` directly. Kept pure (tests + the
/// vocabulary above); the wire probing is opentui's.
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
        let color = parse_osc_color("rgb:1717/1717/1717\x1b\\").expect("the tmux reply parses");
        assert_eq!(color, (0x17, 0x17, 0x17), "0x1717 = 23 decimal");
        assert_eq!(mode_for_background(color.0, color.1, color.2), Mode::Dark);
        // The BEL-terminated form parses identically.
        assert_eq!(parse_osc_color("rgb:1717/1717/1717\u{7}"), Some((0x17, 0x17, 0x17)));
        // Partial/garbled replies never resolve.
        assert_eq!(parse_osc_color("rgb:171"), None);
        assert_eq!(parse_osc_color("garbage with no rgb at all"), None);

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

// -- G-210: generateSystem (palette -> system theme derivation) ----------------

#[cfg(test)]
mod generate_system_tests {
    use super::*;

    fn sample_colors() -> TerminalColors {
        let mut palette = vec![None; 16];
        palette[0] = Some("#282828".into());
        palette[1] = Some("#cc241d".into());
        palette[2] = Some("#98971a".into());
        palette[3] = Some("#d79921".into());
        palette[4] = Some("#458588".into());
        palette[5] = Some("#b16286".into());
        palette[6] = Some("#689d6a".into());
        palette[7] = Some("#ebdbb2".into());
        palette[9] = Some("#fb4934".into());
        palette[10] = Some("#b8bb26".into());
        TerminalColors {
            default_background: Some("#282828".into()),
            default_foreground: Some("#ebdbb2".into()),
            palette,
        }
    }

    fn resolved(colors: &TerminalColors, mode: Mode) -> oc_theme::types::ResolvedTheme {
        let document = generate_system(colors, mode).expect("derives");
        resolve_theme_document(&document, Some(mode)).expect("resolves")
    }

    #[test]
    fn derives_the_ref_equivalent_token_values() {
        let theme = resolved(&sample_colors(), Mode::Dark);
        let hex = |color: Rgba| color.hex();
        // ANSI-slot semantics: primary/accent/info = palette cyan, ...
        assert_eq!(hex(theme.tokens.syntax.keyword), "#b16286"); // magenta (slot 5)
        assert_eq!(hex(theme.tokens.syntax.number), "#d79921"); // yellow (slot 3)
        assert_eq!(hex(theme.tokens.markdown.link), "#458588"); // blue (slot 4)
        assert_eq!(hex(theme.tokens.markdown.emphasis), "#d79921");
        assert_eq!(hex(theme.tokens.markdown.text), "#ebdbb2"); // foreground
        // Gray scale stepping 40% from the #282828 background.
        assert_eq!(hex(theme.tokens.border.base), "#5a5a5a"); // grays[7]
        assert_eq!(hex(theme.tokens.background.raised.base), "#363636"); // grays[2] panel
        assert_eq!(theme.tokens.background.base.a, 0); // ref keeps the terminal bg
        // Muted text: floor(160 + 40 * 0.3) = 172.
        assert_eq!(hex(theme.tokens.text.muted), "#acacac");
        // Diff tinting at the dark 0.22 alpha.
        assert_eq!(hex(theme.tokens.diff.background.added), "#414025");
        assert_eq!(hex(theme.tokens.diff.background.removed), "#4c2726");
        assert_eq!(hex(theme.tokens.diff.background.context), "#363636");
        assert_eq!(
            hex(theme.tokens.diff.line_number.background.added),
            "#4c4b30"
        );
        assert_eq!(hex(theme.tokens.diff.highlight.added), "#b8bb26"); // greenBright
    }

    #[test]
    fn missing_palette_entries_fall_back_to_the_ansi_table() {
        let mut colors = sample_colors();
        // Only the background/foreground survive; everything else drops
        // to `ansiToRgba(index)`.
        colors.palette = vec![Some("#282828".into()), None, None, None, None, None, None,
            Some("#ebdbb2".into())];
        let theme = resolved(&colors, Mode::Dark);
        let hex = |color: Rgba| color.hex();
        assert_eq!(hex(theme.tokens.syntax.keyword), "#800080"); // ansi(5)
        assert_eq!(hex(theme.tokens.diff.text.removed), "#800000"); // ansi(1)
        assert_eq!(hex(theme.tokens.syntax.string), "#008000"); // ansi(2)
    }

    #[test]
    fn a_missing_background_is_underivable() {
        let colors = TerminalColors::default();
        assert!(generate_system(&colors, Mode::Dark).is_none());
    }

    #[test]
    fn registry_system_slot_takes_the_generated_theme() {
        let mut registry = ThemeRegistry::builtins();
        assert!(registry.set_system_theme(&sample_colors(), Mode::Dark));
        let state = registry.resolve("system", Mode::Dark).expect("system resolves");
        let theme = resolve_theme_document(state.document(), Some(Mode::Dark)).expect("resolves");
        assert_eq!(theme.tokens.syntax.keyword.hex(), "#b16286");
        // Without palette information the slot keeps its fallback.
        let mut bare = ThemeRegistry::builtins();
        assert!(!bare.set_system_theme(&TerminalColors::default(), Mode::Dark));
        let fallback = bare.resolve("system", Mode::Dark).expect("fallback resolves");
        assert_ne!(
            resolve_theme_document(fallback.document(), Some(Mode::Dark)).unwrap().tokens.syntax.keyword,
            Rgba::from_hex("#b16286").unwrap()
        );
    }

    /// G-210 — the resolution seam: slot `system` probes the injected
    /// provider and installs its generated document (origin System).
    #[test]
    fn install_system_theme_derives_the_slot_through_the_provider() {
        struct Stub {
            colors: Option<TerminalColors>,
            calls: std::cell::Cell<usize>,
        }
        impl PaletteProvider for Stub {
            fn query(&self) -> Option<TerminalColors> {
                self.calls.set(self.calls.get() + 1);
                self.colors.clone()
            }
        }

        let provider = Stub { colors: Some(sample_colors()), calls: std::cell::Cell::new(0) };
        let mut registry = ThemeRegistry::builtins();
        assert!(install_system_theme(&mut registry, "system", Mode::Dark, &provider));
        assert_eq!(provider.calls.get(), 1, "the system slot probes exactly once");
        let entry = registry.get("system").expect("system slot");
        assert_eq!(entry.origin, ThemeOrigin::System, "origin changes to System");
        let theme = resolve_theme_document(&entry.document, Some(Mode::Dark)).expect("resolves");
        assert_eq!(theme.tokens.syntax.keyword.hex(), "#b16286");

        // A non-`system` name never probes (the ref's early return).
        let mut other = ThemeRegistry::builtins();
        assert!(!install_system_theme(&mut other, "opencode", Mode::Dark, &provider));
        assert_eq!(provider.calls.get(), 1, "non-system slots skip the probe");
        assert_eq!(other.get("system").expect("slot").origin, ThemeOrigin::Builtin);

        // An unavailable palette leaves the documented builtin fallback.
        let missing = Stub { colors: None, calls: std::cell::Cell::new(0) };
        let mut bare = ThemeRegistry::builtins();
        assert!(!install_system_theme(&mut bare, "system", Mode::Dark, &missing));
        assert_eq!(missing.calls.get(), 1, "the probe ran but returned nothing");
        assert_eq!(bare.get("system").expect("slot").origin, ThemeOrigin::Builtin);
    }
}

