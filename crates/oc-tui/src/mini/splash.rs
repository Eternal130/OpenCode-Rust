//! Entry and exit splash banners for the mini scrollback — port of
//! `packages/tui/src/mini/splash.ts`. Both become immutable terminal
//! history: the entry header is one label row (`entrySplash`,
//! `splash.ts:248-274`) and the exit banner keeps the block mark, the
//! session title and the resume command (`buildExit`,
//! `splash.ts:172-239`).

use super::mono::mono_truncate;
use crate::wrap::{display_width, take_width, wrap_label};

/// `SPLASH_TITLE_LIMIT` (`splash.ts:22`).
const SPLASH_TITLE_LIMIT: usize = 50;

/// `SPLASH_TITLE_FALLBACK` (`splash.ts:23`).
pub const SPLASH_TITLE_FALLBACK: &str = "Untitled session";

/// `title()` (`splash.ts:70-96`): collapse runs of whitespace to
/// single spaces (keeping leading newlines out), then truncate.
#[must_use]
pub fn splash_title(text: Option<&str>, mono: bool) -> String {
    let Some(text) = text else { return SPLASH_TITLE_FALLBACK.to_owned() };
    let mut value = String::new();
    let mut gap = false;
    for char in text.trim().chars() {
        if matches!(char, ' ' | '\n' | '\r' | '\t') {
            gap = true;
            continue;
        }
        if gap && !value.is_empty() {
            value.push(' ');
        }
        value.push(char);
        gap = false;
    }
    if value.is_empty() {
        return SPLASH_TITLE_FALLBACK.to_owned();
    }
    // `Locale.truncate` appends `…`; mono appends `...`
    // (`splash.ts:95`).
    if mono {
        return mono_truncate(&value, SPLASH_TITLE_LIMIT, true);
    }
    if display_width(&value) > SPLASH_TITLE_LIMIT {
        return format!("{}…", take_width(&value, SPLASH_TITLE_LIMIT - 1));
    }
    value
}

/// `directoryLabel` (`runtime.lifecycle.ts:120-125`): `~` for home,
/// `~/`-prefixed for under home, forward slashes.
#[must_use]
pub fn directory_label(directory: &str, home: &str) -> String {
    let resolved = normalize_path(directory);
    let display = if resolved == home {
        "~".to_owned()
    } else if !home.is_empty() && resolved.starts_with(&format!("{home}/")) {
        format!("~{}", &resolved[home.len()..])
    } else {
        resolved
    };
    display.replace('\\', "/")
}

fn normalize_path(path: &str) -> String {
    // A lexical absolute-path cleanup (the reference `path.resolve`s;
    // the writer only ever labels, so no symlink walk).
    if path.starts_with('/') {
        let mut parts: Vec<&str> = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        format!("/{}", parts.join("/"))
    } else {
        path.replace('\\', "/")
    }
}

/// `entrySplashLayout` (`splash.ts:276-303`): the staged
/// label/metadata fit. Stages add the path leaf, the version, then
/// progressively longer path forms; the first non-fitting stage stops
/// (never backfills).
#[must_use]
pub fn entry_splash_layout(input: &EntrySplashInput<'_>) -> EntrySplashLayout {
    let detail = input.detail.unwrap_or("");
    let separator = if input.mono { " - " } else { " · " };
    let ellipsis = if input.mono { "..." } else { "…" };
    let slash = if detail.contains('\\') { "\\" } else { "/" };
    let segments: Vec<&str> = detail.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let leaf = segments.last().copied().unwrap_or(detail);
    let mut paths: Vec<String> = segments[1.min(segments.len())..]
        .iter()
        .enumerate()
        .map(|(index, _)| format!("{ellipsis}{slash}{}", segments[index + 1..].join(slash)))
        .collect();
    paths.reverse();
    paths.retain(|path| display_width(path) < display_width(detail));
    let mut path_stages: Vec<String> = paths;
    if !detail.is_empty() {
        path_stages.push(detail.to_owned());
    }
    let mut layout = EntrySplashLayout {
        label: take_width("oc mini", input.width),
        version: String::new(),
        path: String::new(),
        metadata: String::new(),
    };
    // Stage 1: the mark + label (`splash.ts:290`).
    let mark = if input.mono { "[O]" } else { "▪" };
    let stage_label = format!("{mark} oc mini");
    let mut stages: Vec<EntryStage> = vec![EntryStage::Label(stage_label)];
    if !leaf.is_empty() {
        stages.push(EntryStage::Path(leaf.to_owned()));
    }
    if !input.version.is_empty() {
        stages.push(EntryStage::Version(input.version.to_owned()));
    }
    stages.extend(path_stages.into_iter().map(EntryStage::Path));
    for stage in stages {
        let next = match stage {
            EntryStage::Label(label) => EntrySplashLayout { label, ..layout.clone() },
            EntryStage::Path(path) => EntrySplashLayout { path, ..layout.clone() },
            EntryStage::Version(version) => EntrySplashLayout { version, ..layout.clone() },
        };
        let metadata = format!(
            "{}{}",
            if next.version.is_empty() { String::new() } else { format!(" v{}", next.version) },
            if next.path.is_empty() { String::new() } else { format!("{separator}{}", next.path) }
        );
        if display_width(&format!("{}{}", next.label, metadata)) > input.width {
            break;
        }
        layout = next;
        layout.metadata = metadata;
    }
    layout
}

#[derive(Debug, Clone, Default)]
pub struct EntrySplashInput<'a> {
    width: usize,
    version: &'a str,
    detail: Option<&'a str>,
    mono: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryStage {
    Label(String),
    Path(String),
    Version(String),
}

/// `entrySplashLayout`'s result (`splash.ts:288/303`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntrySplashLayout {
    pub label: String,
    pub version: String,
    pub path: String,
    pub metadata: String,
}

/// The rendered entry banner rows (`entrySplash`, `splash.ts:248-274`):
/// a `height: 2` block with `paddingTop: 1` — one blank row, then the
/// label + metadata line.
#[must_use]
pub fn entry_splash_rows(
    version: &str,
    detail: Option<&str>,
    mono: bool,
    width: usize,
) -> Vec<String> {
    let layout = entry_splash_layout(&EntrySplashInput { width, version, detail, mono });
    vec![String::new(), format!("{}{}", layout.label, layout.metadata)]
}

/// The exit banner rows (`buildExit`, `splash.ts:172-239`): the block
/// mark (`go.right.slice(1)`, `logo.ts:8` — `["█▀▀█", "█__█", "▀▀▀▀"]`;
/// mono `["[O]"]` per `splash.ts:179`), `Session  <title>` and
/// `Continue opencode mini -s <id>`.
#[must_use]
pub fn exit_splash_rows(
    title: Option<&str>,
    session_id: &str,
    mono: bool,
    width: usize,
) -> Vec<String> {
    exit_splash_styled_rows(title, session_id, mono, width, &SplashColors::mono())
        .into_iter()
        .map(|spans| spans.into_iter().map(|span| span.text).collect())
        .collect()
}

/// The splash color triple (`mini/theme.ts:173-175`): `left`
/// paints the mark glyphs and labels, `right` the title and
/// command, `shadow` the fill behind the mark (`background.raised`
/// at index 175; the plain theme's `splash` triple at :240 maps
/// shadow to the default background).
#[derive(Debug, Clone, Copy)]
pub struct SplashColors {
    pub left: ratatui::style::Color,
    pub right: ratatui::style::Color,
    pub shadow: ratatui::style::Color,
}

impl SplashColors {
    /// The mono theme (`theme.ts:204-240`): default foreground,
    /// default background — no color codes.
    #[must_use]
    pub fn mono() -> Self {
        Self {
            left: ratatui::style::Color::Reset,
            right: ratatui::style::Color::Reset,
            shadow: ratatui::style::Color::Reset,
        }
    }

    /// From the mini runtime's theme: left = `muted`, right = `text`,
    /// shadow = `shade` (`background.raised.base`) — v2 maps all
    /// three through the oklab `nearestIndexed` quantizer
    /// (`mini/theme.ts:172-175`), so the splash's scrollback cells
    /// emit xterm-256 indexes, never truecolor.
    #[must_use]
    pub fn from_footer_theme(theme: &crate::components::footer::FooterTheme) -> Self {
        Self {
            left: cell_color(super::theme::scrollback_color(theme.muted)),
            right: cell_color(super::theme::scrollback_color(theme.text)),
            shadow: cell_color(super::theme::scrollback_color(theme.shade)),
        }
    }
}

fn cell_color(color: crate::render::CellColor) -> ratatui::style::Color {
    use crate::render::CellColor;
    use ratatui::style::Color;
    match color {
        CellColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
        CellColor::Indexed(index) => Color::Indexed(index),
        CellColor::Default => Color::Reset,
    }
}

/// The styled exit banner (R-08): v2's `draw()`/`cells()`
/// half-block mark rendering (`splash.ts:46-68, 142-170`). The
/// `go` mark's cells map by glyph:
///
/// * `_` → a full shadow cell (space with bg=shadow, fg=left —
///   the `mark: "full"` push at splash.ts:155-158);
/// * `^` → `▀` with fg=left bg=shadow (the "mix" half-block:
///   the top half carries the mark color over the shadow);
/// * `~` → `▀` with fg=shadow only (the "top" push at :161-164);
/// * anything else (█ ▀ text glyphs) → fg=left.
///
/// The body keeps v2's colors: `Session`/`Continue` labels in
/// `left`, title and command in `right` + bold.
#[must_use]
pub fn exit_splash_styled_rows(
    title: Option<&str>,
    session_id: &str,
    mono: bool,
    width: usize,
    colors: &SplashColors,
) -> Vec<Vec<crate::layout::StyledSpan>> {
    use crate::layout::StyledSpan;
    use ratatui::style::{Modifier, Style};

    let session = "Session  ";
    let label = "Continue ";
    let command = format!("opencode mini -s {session_id}");
    let mark: Vec<&str> =
        if mono { vec!["[O]"] } else { vec!["█▀▀█", "█__█", "▀▀▀▀"] };
    let body_left = mark.first().map_or(0, |row| row.len()) + 2;
    let wide = body_left + display_width(&format!("{label}{command}")) <= width;
    let clean_title = splash_title(title, mono);

    let left_style = Style::new().fg(colors.left);
    let right_bold = Style::new().fg(colors.right).add_modifier(Modifier::BOLD);

    let mut rows: Vec<Vec<StyledSpan>> = Vec::new();
    // `top = 1` (`splash.ts:180`): one blank row above the banner.
    rows.push(Vec::new());
    if wide {
        // `height = top + Math.max(mark.length, 2)` (`splash.ts:205`).
        let body_rows = mark.len().max(2);
        let indent = " ".repeat(body_left);
        for index in 0..body_rows {
            let mut spans: Vec<StyledSpan> =
                mark.get(index).map_or_else(Vec::new, |row| mark_cells(row, colors));
            match index {
                0 => {
                    spans.push(StyledSpan { text: indent.clone(), style: left_style, link: None });
                    spans.push(StyledSpan {
                        text: session.to_owned(),
                        style: left_style,
                        link: None,
                    });
                    spans.push(StyledSpan {
                        text: clean_title.clone(),
                        style: right_bold,
                        link: None,
                    });
                }
                1 => {
                    spans.push(StyledSpan { text: indent.clone(), style: left_style, link: None });
                    spans.push(StyledSpan {
                        text: label.to_owned(),
                        style: left_style,
                        link: None,
                    });
                    spans.push(StyledSpan { text: command.clone(), style: right_bold, link: None });
                }
                _ => {}
            }
            rows.push(spans);
        }
    } else {
        // Narrow terminals: the command wraps at char width from
        // column 0 (`splash.ts:220-229`, `wrapMode: "char"`).
        for line in wrap_label(&command, width) {
            rows.push(vec![StyledSpan { text: line, style: right_bold, link: None }]);
        }
    }
    rows
}

/// v2's `cells()` + `draw()` per-mark-glyph mapping
/// (`splash.ts:46-68`, pushes at :153-168):
/// * `_` → a full shadow cell (space, fg=left bg=shadow);
/// * `^` → `▀` fg=left bg=shadow (the "mix" half-block);
/// * `~` → `▀` fg=shadow, no bg (the "top" half-block);
/// * anything else → the glyph itself in `left`.
#[must_use]
pub fn mark_cells(row: &str, colors: &SplashColors) -> Vec<crate::layout::StyledSpan> {
    use crate::layout::StyledSpan;
    use ratatui::style::Style;
    row.chars()
        .map(|char| {
            let style = match char {
                '_' | '^' => Style::new().fg(colors.left).bg(colors.shadow),
                '~' => Style::new().fg(colors.shadow),
                _ => Style::new().fg(colors.left),
            };
            let glyph = match char {
                '_' => ' ',
                '^' | '~' => '▀',
                other => other,
            };
            StyledSpan { text: glyph.to_string(), style, link: None }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_theme_splash_triple_quantizes_through_oklab() {
        // `mini/theme.ts:172-175` maps the triple through
        // `nearestIndexed` — the fallback-dark inputs (muted 808080,
        // text eeeeee, shade 141414) pin to the v2 corpus outputs.
        let colors = SplashColors::from_footer_theme(
            &crate::components::footer::FooterTheme::fallback_dark(),
        );
        assert_eq!(colors.left, ratatui::style::Color::Indexed(8), "muted #808080 → idx 8");
        assert_eq!(colors.right, ratatui::style::Color::Indexed(255), "text #eeeeee → idx 255");
        assert_eq!(colors.shadow, ratatui::style::Color::Indexed(233), "shade #141414 → idx 233");
    }

    #[test]
    fn titles_collapse_whitespace_and_fall_back() {
        assert_eq!(splash_title(None, false), SPLASH_TITLE_FALLBACK);
        assert_eq!(splash_title(Some("  \n\t  "), false), SPLASH_TITLE_FALLBACK);
        assert_eq!(splash_title(Some("fix the\n  parser bug"), false), "fix the parser bug");
        let long = "x".repeat(60);
        let title = splash_title(Some(&long), false);
        assert_eq!(display_width(&title), 50, "… counts one cell: 49 + ellipsis");
        assert!(title.ends_with('…'));
        assert_eq!(splash_title(Some(&long), true).chars().count(), 50);
        assert!(splash_title(Some(&long), true).ends_with("..."));
    }

    #[test]
    fn directory_label_uses_home_tilde() {
        assert_eq!(directory_label("/home/u", "/home/u"), "~");
        assert_eq!(directory_label("/home/u/work/api", "/home/u"), "~/work/api");
        assert_eq!(directory_label("/opt/other", "/home/u"), "/opt/other");
        assert_eq!(directory_label("/home/u/a/../b", "/home/u"), "~/b");
    }

    #[test]
    fn entry_layout_stages_fit_in_order() {
        // Wide terminal: every stage fits, so the LAST (longest) path
        // stage wins — the full detail (`splash.ts:293`).
        let layout = entry_splash_layout(&EntrySplashInput {
            width: 80,
            version: "2.0.0",
            detail: Some("/home/u/proj/api"),
            mono: false,
        });
        assert_eq!(layout.label, "▪ oc mini");
        assert_eq!(layout.metadata, " v2.0.0 · /home/u/proj/api");
        // Mono separator + mark (`splash.ts:281,290`).
        let mono_layout = entry_splash_layout(&EntrySplashInput {
            width: 80,
            version: "2.0.0",
            detail: Some("/home/u/proj/api"),
            mono: true,
        });
        assert_eq!(mono_layout.label, "[O] oc mini");
        assert_eq!(mono_layout.metadata, " v2.0.0 - /home/u/proj/api");
        // A leaf-only fit squeezes to the shortest path stage.
        let leaf_only = entry_splash_layout(&EntrySplashInput {
            width: display_width("▪ oc mini · api"),
            version: "",
            detail: Some("/home/u/proj/api"),
            mono: false,
        });
        assert_eq!(leaf_only.metadata, " · api");
    }

    #[test]
    fn entry_layout_stops_at_first_non_fitting_stage() {
        // Width fits "▪ oc mini · proj" but not "▪ oc mini v2.0.0 ·
        // proj": the leaf stage applied first, the version stage stops
        // the walk (leaf precedes version, `splash.ts:290-292`).
        let width = display_width("▪ oc mini v2.0.0 · proj") - 3;
        let layout = entry_splash_layout(&EntrySplashInput {
            width,
            version: "2.0.0",
            detail: Some("/home/u/proj"),
            mono: false,
        });
        assert_eq!(layout.metadata, " · proj");
        assert_eq!(layout.version, "");
        assert_eq!(layout.path, "proj");
    }

    #[test]
    fn entry_rows_blank_then_label() {
        let rows = entry_splash_rows("2.0.0", Some("/home/u/p"), false, 80);
        assert_eq!(rows.len(), 2, "paddingTop 1");
        assert_eq!(rows[0], "");
        // The longest fitting path stage wins: the full detail.
        assert_eq!(rows[1], "▪ oc mini v2.0.0 · /home/u/p");
    }

    #[test]
    fn exit_rows_carry_title_and_resume_command() {
        let rows = exit_splash_rows(Some("fix parser"), "ses_123", true, 80);
        let text = rows.join("\n");
        assert!(text.contains("[O]"), "mono mark: {text}");
        assert!(text.contains("Session  fix parser"), "session row: {text}");
        assert!(text.contains("Continue opencode mini -s ses_123"), "resume row: {text}");
        // Non-mono uses the go block mark (`logo.ts:8`).
        let styled = exit_splash_rows(Some("fix parser"), "ses_123", false, 80).join("\n");
        assert!(styled.contains("█▀▀█"), "block mark: {styled}");
    }

    #[test]
    fn exit_rows_wrap_command_on_narrow_terminals() {
        let rows = exit_splash_rows(Some("t"), "ses_1", true, 10);
        let text = rows.join("\n");
        assert!(text.contains("opencode"), "command present: {text}");
        assert!(!text.contains("Continue opencode"), "narrow drops the label+command row");
    }

    #[test]
    fn mark_cells_map_the_half_block_rules() {
        // v2 `cells()`/`draw()` (`splash.ts:46-68, 153-168`):
        // `_` → a full shadow cell (space, fg=left bg=shadow);
        // `^` → ▀ fg=left bg=shadow (mix); `~` → ▀ fg=shadow
        // (top); text glyphs → fg=left.
        let colors = SplashColors {
            left: ratatui::style::Color::Indexed(245),
            right: ratatui::style::Color::Indexed(231),
            shadow: ratatui::style::Color::Indexed(235),
        };
        let cells = mark_cells("█_^~▀", &colors);
        let text: String = cells.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "█ ▀▀▀");
        assert_eq!(cells[0].style.fg, Some(ratatui::style::Color::Indexed(245)));
        assert_eq!(cells[0].style.bg, None, "text glyphs carry no shadow");
        assert_eq!(cells[1].text, " ", "the `_` cell is a space");
        assert_eq!(cells[1].style.fg, Some(ratatui::style::Color::Indexed(245)));
        assert_eq!(cells[1].style.bg, Some(ratatui::style::Color::Indexed(235)));
        assert_eq!(cells[2].text, "▀");
        assert_eq!(cells[2].style.bg, Some(ratatui::style::Color::Indexed(235)), "^ mixes");
        assert_eq!(cells[3].style.fg, Some(ratatui::style::Color::Indexed(235)), "~ is shadow-fg");
        assert_eq!(cells[3].style.bg, None, "~ carries no bg");
    }

    #[test]
    fn styled_exit_banner_paints_the_go_mark_with_shadow_cells() {
        // The `go` mark rows (`logo.ts:8`): `█▀▀█` is plain text
        // in `left`, `█__█` renders its `_` as full shadow cells,
        // `▀▀▀▀` plain. The title carries `right` + bold.
        let colors = SplashColors {
            left: ratatui::style::Color::Indexed(245),
            right: ratatui::style::Color::Indexed(231),
            shadow: ratatui::style::Color::Indexed(235),
        };
        let rows = exit_splash_styled_rows(Some("fix parser"), "ses_123", false, 80, &colors);
        assert_eq!(rows.len(), 4, "blank + three mark rows");
        // Mark row 2 (`█__█`, logo.ts:8) carries the continue body
        // (v2 pushes mark cells at x=0 and the body at body_left on
        // the same row); its `_` cells render as full shadow cells.
        let mark_row: String = rows[2].iter().map(|span| span.text.as_str()).collect();
        assert!(mark_row.starts_with("█  █"), "`_` cells become spaces: {mark_row}");
        assert!(mark_row.starts_with("█  █"), "`_` cells become spaces: {mark_row}");
        assert!(rows[2]
            .iter()
            .skip(1)
            .take(2)
            .all(|span| span.style.bg == Some(ratatui::style::Color::Indexed(235))));
        // The session row (mark `█▀▀█`): label in left, title in
        // right + bold.
        let session: String = rows[1].iter().map(|span| span.text.as_str()).collect();
        assert!(session.contains("Session  fix parser"), "session row: {session}");
        let title_span = &rows[1].last().expect("title span");
        assert_eq!(title_span.style.fg, Some(ratatui::style::Color::Indexed(231)));
        assert!(title_span.style.add_modifier.contains(ratatui::style::Modifier::BOLD));
    }
}
