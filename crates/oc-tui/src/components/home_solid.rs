//! Lane M5: the home route on opentui-solid — the wordmark block,
//! the centered column chrome, and the `home.footer` slot re-homed
//! the M1..M4 way (see `docs/migration-m1-tabstrip.md`).
//!
//! **What this module is.** A structural port of the home route's
//! chrome (`routes/home.tsx` + `feature-plugins/home/footer.tsx`) as
//! an opentui-solid builder tree: the OPENCODE wordmark (two
//! styled-chunk text rows per art row — the halves' per-cell shadow
//! tints become explicit-bg chunk runs, the `gap={1}` cell stays the
//! page fill), the version row (`<64`-col gate + the content-row
//! collapse), and the wave-3 plugin-action label. The GEOMETRY is
//! NOT mirrored — it comes from the ONE shared
//! [`crate::components::home::plan_home`] walk (the M3 extract-the-
//! geometry rule): the vertical centering's half-up rule, the footer
//! ladder, and the paste origin are the same integers the shared
//! `plan_home` solver produces.
//!
//! **The prompt mount stays the composer's own seam.** The sessionless
//! composer is the SAME `Prompt` component the session route docks;
//! its frame comes through [`render_prompt_frame_dispatch`] (lane
//! both routes now share the one opentui-solid composer), built
//! over the same `oc_tui_core` text engine, and PASTES at the plan's
//! origin. The popup rows float above the mount through the same
//! `top = 1 + popup_rows` shape.
//!
//! **The caret (NEW-S1a) is untouched:** the terminal-cursor cell is
//! computed by the same `prompt_caret_screen_cell` walk and handed
//! to `Store::set_caret_cell` — the hardware cursor is the caret,
//! whichever painter drew the chrome.
//!
//! **Selection flag (retired).** The lane-M5 dual-path flag
//! (`OC_HOME_OPENTUI`) is gone: this module's opentui-solid tree is
//! the home route's ONLY renderer — the hand-rolled painter was
//! deleted after the one-release probation (the retirement lane).

// Terminal dimensions/colors are bounded usize→f32 casts (taffy
// points), the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]

use std::cell::RefCell;
use std::rc::Rc;

use opentui_core::renderer::CliRendererConfig;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::element::PropValue;
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, map, text, Element, Signal, SolidRenderer};

use crate::components::composer::render::{FooterHints, PromptView};
use crate::components::composer_solid::render_prompt_frame_dispatch;
use crate::components::home::{plan_home, HomeView, LOGO_LEFT, LOGO_RIGHT};
use crate::components::session_tabs_solid::frame_from_buffer;
use crate::render::Frame;
use crate::theme::ThemeState;
use oc_theme::Rgba as ThemeRgba;

/// An RGB triple (the port's cell currency).
type Rgb = (u8, u8, u8);

fn rgba(rgb: Rgb) -> Rgba {
    Rgba::rgb(rgb.0, rgb.1, rgb.2)
}

fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

fn theme_cell(color: ThemeRgba) -> Rgb {
    (color.r, color.g, color.b)
}

/// The wordmark's paint colors, resolved from the same token walk
/// `home.rs::paint_logo` reads (the halves' fg + the 0.25 shadow
/// tint over `background.base` — `theme/color.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LogoColors {
    base: Rgb,
    muted: Rgb,
    text_base: Rgb,
    shadow_muted: Rgb,
    shadow_base: Rgb,
}

/// `tint(base, overlay, 0.25)` per channel, rounded once (the
/// reference's float-space blend — linearly identical in 0..=255).
fn tint_channel(channel: u8, base: u8) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (f32::from(channel) * 0.25 + f32::from(base) * 0.75).round() as u8
    }
}

fn tint(base: Rgb, overlay: Rgb) -> Rgb {
    (
        tint_channel(overlay.0, base.0),
        tint_channel(overlay.1, base.1),
        tint_channel(overlay.2, base.2),
    )
}

impl LogoColors {
    fn of(theme: &ThemeState) -> Self {
        let tokens = &theme.resolved.tokens;
        let base = theme_cell(tokens.background.base);
        let muted = theme_cell(tokens.text.muted);
        let text_base = theme_cell(tokens.text.base);
        LogoColors {
            base,
            muted,
            text_base,
            shadow_muted: tint(base, muted),
            shadow_base: tint(base, text_base),
        }
    }
}

/// One wordmark art row → styled chunks (`Logo.renderLine`): `_`
/// paints a space OVER the shadow, `^` a `▀` over the shadow, `~` a
/// `▀` IN the shadow, `,` a `▄` in the shadow; every other glyph is
/// the half's color over the base. Adjacent cells with the same
/// (fg, bg) merge into one run — explicit chunk bg overwrites the
/// page fill exactly like the old painter's per-cell write.
fn logo_row_chunks(line: &str, fg: Rgb, shadow: Rgb, base: Rgb, bold: bool) -> StyledText {
    let attributes = u32::from(bold);
    let mut chunks: Vec<TextChunk> = Vec::new();
    let mut run: Option<(String, Rgb, Rgb)> = None;
    for ch in line.chars() {
        let (glyph, fore, back) = match ch {
            '_' => (' ', fg, shadow),
            '^' => ('▀', fg, shadow),
            '~' => ('▀', shadow, base),
            ',' => ('▄', shadow, base),
            other => (other, fg, base),
        };
        let continues = matches!(&run, Some((_, fore0, back0)) if *fore0 == fore && *back0 == back);
        if continues {
            if let Some((text, _, _)) = &mut run {
                text.push(glyph);
            }
        } else {
            if let Some((text, fore0, back0)) = run.take() {
                chunks.push(TextChunk {
                    text,
                    fg: Some(rgba(fore0)),
                    bg: Some(rgba(back0)),
                    attributes,
                    link: None,
                });
            }
            run = Some((glyph.to_string(), fore, back));
        }
    }
    if let Some((text, fore0, back0)) = run {
        chunks.push(TextChunk {
            text,
            fg: Some(rgba(fore0)),
            bg: Some(rgba(back0)),
            attributes,
            link: None,
        });
    }
    StyledText::new(chunks)
}

/// The wordmark's four rows × two halves as pre-shaped chunk texts —
/// row-major `[(left, right); 4]`.
fn logo_rows(colors: &LogoColors) -> [(StyledText, StyledText); 4] {
    std::array::from_fn(|row| {
        (
            logo_row_chunks(LOGO_LEFT[row], colors.muted, colors.shadow_muted, colors.base, false),
            logo_row_chunks(
                LOGO_RIGHT[row],
                colors.text_base,
                colors.shadow_base,
                colors.base,
                true,
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// inputs (the shared plan → view bridge)
// ---------------------------------------------------------------------------

/// One frame's home chrome snapshot — the ONLY input the tree reads.
/// The geometry (width, height, the plan's baked positions) is
/// MOUNT-STATIC; the version + action labels patch in place.
#[derive(Clone)]
struct HomeInputs {
    /// The version row's label (`StyledText` empty while gated off).
    version: StyledText,
    /// The plugin-action label (empty = none).
    actions: StyledText,
}

/// `dyn_(src, to PropValue::Styled)` — styled-chunk content.
fn dyn_styled<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<StyledText> + 'static,
{
    opentui_solid::dyn_(source, |value: &StyledText| PropValue::Styled(value.clone()))
}

/// The home chrome mount: a positioned root carrying the page
/// background + the wordmark's eight STATIC text rows (the art is
/// theme-static — a theme swap rebuilds through the mount key) + the
/// two footer dyn slots. The composer frame pastes over it at the
/// bridge (it owns its own mount, lane M3).
fn home_element(
    inputs: &Signal<HomeInputs>,
    plan: &crate::components::home::HomePlan,
    colors: &LogoColors,
    width: usize,
    height: usize,
) -> Element {
    let mut children: Vec<Element> = Vec::new();

    // The wordmark: two text elements per art row (the halves never
    // touch — the `gap={1}` cell keeps the page fill, pinned by the
    // goldens).
    if let Some((logo_x, logo_y)) = plan.logo {
        for (row, (left, right)) in logo_rows(colors).into_iter().enumerate() {
            children.push(
                text()
                    .position("absolute")
                    .left(logo_x as f32)
                    .top((logo_y + row) as f32)
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .content(PropValue::Styled(left))
                    .into_element(),
            );
            children.push(
                text()
                    .position("absolute")
                    .left((logo_x + 20) as f32)
                    .top((logo_y + row) as f32)
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .content(PropValue::Styled(right))
                    .into_element(),
            );
        }
    }

    // The footer content row: the version right-aligned (the <64
    // gate resolved by the plan — the label empties), the plugin
    // actions left-padded.
    if plan.show_footer {
        let version_inputs = inputs.clone();
        children.push(
            text()
                .position("absolute")
                .left(plan.version_x as f32)
                .top(plan.footer_row as f32)
                .wrap_mode("none")
                .flex_shrink(0.0)
                .dyn_prop(
                    "content",
                    dyn_styled(map(version_inputs, |i: &HomeInputs| i.version.clone())),
                )
                .into_element(),
        );
        let action_inputs = inputs.clone();
        children.push(
            text()
                .position("absolute")
                .left(plan.padding as f32)
                .top(plan.footer_row as f32)
                .wrap_mode("none")
                .flex_shrink(0.0)
                .dyn_prop(
                    "content",
                    dyn_styled(map(action_inputs, |i: &HomeInputs| i.actions.clone())),
                )
                .into_element(),
        );
    }

    box_()
        .width(width as f32)
        .height(height as f32)
        .position("relative")
        .background_color(hex(colors.base))
        .children(opentui_solid::element::Children::Static(children))
        .into_element()
}

// ---------------------------------------------------------------------------
// the mount (one per thread, keyed on geometry)
// ---------------------------------------------------------------------------

/// The mounted home chrome: the inputs signal + a real headless
/// [`SolidRenderer`]. Rebuilt when the geometry changes (resize, the
/// popup opening, a theme swap) — the M2 height-keying rule via the
/// plan's mount-relevant fields.
struct SolidHome {
    inputs: Signal<HomeInputs>,
    renderer: SolidRenderer,
    key: (usize, usize, Rgb, crate::components::home::HomePlan),
}

impl SolidHome {
    fn new(
        plan: &crate::components::home::HomePlan,
        colors: &LogoColors,
        width: usize,
        height: usize,
        inputs: &HomeInputs,
    ) -> Self {
        let signal = Signal::new(inputs.clone());
        let renderer = opentui_solid::render(
            CliRendererConfig {
                width: width.max(1) as u32,
                height: height.max(1) as u32,
                background_color: rgba(colors.base),
                ..CliRendererConfig::default()
            },
            home_element(&signal, plan, colors, width, height),
        );
        Self { inputs: signal, renderer, key: (width, height, colors.base, plan.clone()) }
    }

    /// Sync one frame's snapshot, flush the reconciler patches, and
    /// render — the per-frame bridge cost.
    fn paint(&mut self, inputs: HomeInputs) {
        self.inputs.set(inputs);
        self.renderer.flush();
    }
}

thread_local! {
    /// One home mount per thread-process — the app paints exactly one
    /// home frame per redraw; the harnesses reuse it across boots.
    static HOME: RefCell<Option<SolidHome>> = const { RefCell::new(None) };
}

fn styled_label(label: &str, fg: Rgb) -> StyledText {
    if label.is_empty() {
        StyledText::new(Vec::new())
    } else {
        StyledText::new(vec![TextChunk {
            text: label.to_owned(),
            fg: Some(rgba(fg)),
            bg: None,
            attributes: 0,
            link: None,
        }])
    }
}

/// The home footer's LEFT label (SURF-2, auditB #7): the MCP status
/// cell's colored `⊙ ` + count (success/error), the muted `/mcps`
/// hint, then the plugin actions — one multi-chunk `StyledText`
/// (footer.tsx:30-46 + the wave-3 actions).
fn footer_left_label(
    plan: &crate::components::home::HomePlan,
    theme: &ThemeState,
    muted: Rgb,
) -> StyledText {
    let mut chunks: Vec<TextChunk> = Vec::new();
    if let Some((failed, label, hint, _)) = &plan.mcp {
        let feedback = &theme.resolved.tokens.text.feedback;
        let (dot, base) = if *failed {
            (feedback.error.base, theme.resolved.tokens.text.base)
        } else {
            (feedback.success.base, theme.resolved.tokens.text.base)
        };
        chunks.push(TextChunk {
            text: "⊙ ".to_owned(),
            fg: Some(rgba(theme_cell(dot))),
            bg: None,
            attributes: 0,
            link: None,
        });
        chunks.push(TextChunk {
            text: label.clone(),
            fg: Some(rgba(theme_cell(base))),
            bg: None,
            attributes: 0,
            link: None,
        });
        if *hint {
            chunks.push(TextChunk {
                text: " /mcps".to_owned(),
                fg: Some(rgba(muted)),
                bg: None,
                attributes: 0,
                link: None,
            });
        }
        if plan.plugins_failed.is_some() || !plan.actions.is_empty() {
            chunks.push(TextChunk {
                text: "  ".to_owned(),
                fg: Some(rgba(muted)),
                bg: None,
                attributes: 0,
                link: None,
            });
        }
    }
    // Lane KM (auditA #10): the plugins-failed chip (footer.tsx:
    // 48-72) — the error dot + `{n} plugin(s) failed` + the muted
    // `/plugins` hint at width ≥ 80.
    if let Some((label, hint, _)) = &plan.plugins_failed {
        let feedback = &theme.resolved.tokens.text.feedback;
        chunks.push(TextChunk {
            text: "⊙ ".to_owned(),
            fg: Some(rgba(theme_cell(feedback.error.base))),
            bg: None,
            attributes: 0,
            link: None,
        });
        chunks.push(TextChunk {
            text: label.clone(),
            fg: Some(rgba(theme_cell(theme.resolved.tokens.text.base))),
            bg: None,
            attributes: 0,
            link: None,
        });
        if *hint {
            chunks.push(TextChunk {
                text: " /plugins".to_owned(),
                fg: Some(rgba(muted)),
                bg: None,
                attributes: 0,
                link: None,
            });
        }
        if !plan.actions.is_empty() {
            chunks.push(TextChunk {
                text: "  ".to_owned(),
                fg: Some(rgba(muted)),
                bg: None,
                attributes: 0,
                link: None,
            });
        }
    }
    if !plan.actions.is_empty() {
        chunks.push(TextChunk {
            text: plan.actions.clone(),
            fg: Some(rgba(muted)),
            bg: None,
            attributes: 0,
            link: None,
        });
    }
    StyledText::new(chunks)
}

/// Paint the home route through the opentui-solid path — the
/// new-path twin of [`crate::components::home::render_home_view`]'s
/// old painter: resolve the SHARED plan, render the chrome mount,
/// paste the composer dispatch frame at the plan's origin, and derive
/// the terminal-cursor cell from the same walk.
#[must_use]
#[allow(clippy::too_many_lines)] // the banner row is the lane-TAIL addition
pub fn paint_solid_home_view(
    view: &HomeView,
    theme: &ThemeState,
    size: (u16, u16),
) -> (Frame, Option<(usize, usize)>, Option<(usize, usize, usize)>) {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    if width == 0 || height == 0 {
        // The old painter's degenerate shape.
        return (Frame { cells: vec![Vec::new(); height], width, height }, None, None);
    }
    let plan = plan_home(view, width, height);
    let colors = LogoColors::of(theme);
    let muted = theme_cell(theme.resolved.tokens.text.muted);
    let inputs = HomeInputs {
        version: if plan.version_visible {
            styled_label(&view.version, muted)
        } else {
            StyledText::new(Vec::new())
        },
        actions: footer_left_label(&plan, theme, muted),
    };
    let mut frame = HOME.with_borrow_mut(|slot| {
        let key = (width, height, colors.base, plan.clone());
        let mount =
            slot.get_or_insert_with(|| SolidHome::new(&plan, &colors, width, height, &inputs));
        if mount.key != key {
            *mount = SolidHome::new(&plan, &colors, width, height, &inputs);
        }
        mount.paint(inputs);
        frame_from_buffer(&mount.renderer.renderer.current_render_buffer, &[])
    });

    // The prompt mount: the composer's own frame (its painter is the
    // M3 flag chain's), pasted at the plan's origin — the popup rows
    // float above the mount through the same `top = 1 + popup_rows`.
    let prompt_view = PromptView {
        draft: view.draft,
        placeholder: view.placeholder.clone(),
        autocomplete: view.autocomplete.clone(),
        footer: FooterHints {
            location: view.location.clone(),
            agent_shortcut: "shift+tab".into(),
            command_shortcut: "ctrl+p".into(),
            plugin_hints: view.plugin_hints.clone(),
            interrupt: String::new(),
            usage: String::new(),
            location_hovered: view.location_hovered,
            // TUI-12 — the live segment is session-scoped (the home
            // composer has no session family/shells to count).
            live: String::new(),
            live_shortcut: String::new(),
            live_hovered: false,
            // TUI-14 — the home composer's shell mode (the `!` prefix
            // works there too; home.tsx:20-23 carries the shell
            // placeholder pool).
            shell: view.shell_mode,
            // TUI-15 — the editor file label paints on home too
            // (the selection attach precedes the session create).
            editor_file: view.editor_file.clone(),
            editor_file_pending: view.editor_file_pending,
        },
        metadata: view.metadata.clone(),
        terminal_width: width,
        top: 1 + plan.popup_rows,
        rail: view.rail,
        caret: None,
    };
    let prompt_frame = render_prompt_frame_dispatch(
        &prompt_view,
        plan.prompt_width,
        plan.mount + plan.popup_rows,
        theme,
    );
    crate::components::home::paste(&mut frame, &prompt_frame, plan.paste.0, plan.paste.1);
    // The update-notification banner (auditB #10, home.tsx:125-175):
    // one centered row in the gap above the prompt mount — the
    // `/update`·`/exit` prefix in `action.primary.selected`, the body
    // in `text.muted` (the FadeIn text rides the same row; hover's
    // bg lift lands with the pointer-hit seam). `None` in every
    // golden state — the row paints ONLY when a live notice exists.
    let banner_hit = view.update.as_ref().and_then(|(prefix, body, remote)| {
        let row = plan.paste.1.saturating_sub(1);
        let (sr, sg, sb) = theme_cell(theme.resolved.tokens.text.action.primary.selected);
        let selected = crate::render::CellColor::Rgb(sr, sg, sb);
        let (mr, mg, mb) = muted;
        let muted_color = crate::render::CellColor::Rgb(mr, mg, mb);
        let width_of = crate::wrap::display_width;
        let prefix_width = if *remote { 0 } else { width_of(prefix) + 1 };
        let body_width = width_of(body);
        let total = prefix_width + body_width;
        let x0 = (width.saturating_sub(total)) / 2;
        let mut x = x0;
        if !*remote {
            for ch in prefix.chars() {
                if let Some(cell) = frame
                    .cells
                    .get_mut(row)
                    .and_then(|r| x.checked_sub(0).and_then(|_| r.get_mut(x)))
                {
                    cell.ch = ch;
                    cell.fg = selected;
                }
                x += 1;
            }
            x += 1; // the gap
        }
        for ch in body.chars() {
            if let Some(cell) = frame.cells.get_mut(row).and_then(|r| r.get_mut(x)) {
                cell.ch = ch;
                cell.fg = muted_color;
            }
            x += 1;
        }
        (total > 0 && row < height).then_some((x0, row, total))
    });
    // TUI-TD27 — the banner's rect rides out to the store (the
    // release seam: remote dismisses, installed exits, available
    // opens the update dialog, home.tsx:138-146).
    let banner_hit = banner_hit;
    // NEW-S1a: the caret's screen cell — the paste origin plus the
    // frame-relative cell (the same walk the old painter runs).
    let caret_screen = view
        .caret
        .and_then(|offset| {
            crate::components::composer::render::prompt_caret_screen_cell(
                view.draft,
                plan.prompt_width,
                1 + plan.popup_rows,
                height,
                offset,
            )
        })
        .map(|(x, row)| (plan.paste.0 + x, plan.paste.1 + row))
        .filter(|(_, row_y)| *row_y < height);
    (frame, caret_screen, banner_hit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::composer::render::AutocompleteView;
    use crate::components::composer::PromptDraft;
    use crate::components::home::LOGO_WIDTH;
    use crate::wrap::display_width;

    fn draft(text: &str) -> PromptDraft {
        PromptDraft { text: text.to_owned(), ..PromptDraft::default() }
    }

    /// One scenario: everything [`HomeView`] borrows, owned.
    struct Scenario {
        draft: PromptDraft,
        version: String,
        actions: Vec<String>,
        autocomplete: Option<AutocompleteView<'static>>,
        caret: Option<usize>,
    }

    impl Scenario {
        fn new(name: &str) -> Self {
            let _ = name;
            Scenario {
                draft: draft(""),
                version: "1.2.3-m5".into(),
                actions: Vec::new(),
                autocomplete: None,
                caret: None,
            }
        }
    }

    fn owned_view(scenario: &Scenario, placeholder: String) -> HomeView<'static> {
        HomeView {
            update: None,
            draft: Box::leak(Box::new(scenario.draft.clone())),
            placeholder,
            location: String::new(),
            directory: String::new(),
            location_hovered: false,
            shell_mode: false,
            editor_file: String::new(),
            editor_file_pending: false,
            version: scenario.version.clone(),
            metadata: crate::components::composer::render::MetadataLine::default(),
            plugin_hints: Vec::new(),
            plugin_actions: scenario.actions.clone(),
            mcp: None,
            plugins_failed: None,
            autocomplete: scenario.autocomplete.clone(),
            rail: None,
            caret: scenario.caret,
        }
    }

    /// The lane gate: the solid home frame must equal the old
    /// painter's frame CELL FOR CELL (char + attrs + fg + bg) over
    /// the home matrix — idle placeholder, typing, multiline, the
    /// popup open, plugin actions, the <64-col version gate + the
    /// content-row collapse, tiny frames (no logo/footer), the
    /// caret — × widths 40/60/80/120/160 × heights 20/24/30 (the
    /// The reactive path patches in place: a version-length change
    /// re-renders the SAME mount (same geometry).
    #[test]
    fn label_churn_patches_the_mounted_home() {
        let theme = ThemeState::v2_dark();
        let mut seen: Vec<(String, crate::render::Frame)> = Vec::new();
        for version in ["1.0.0", "1.0.0-dev.1+m5", "1.0.0"] {
            let scenario = Scenario { version: version.into(), ..Scenario::new("churn") };
            let view = owned_view(&scenario, "Ask".into());
            let (frame, _, _) = paint_solid_home_view(&view, &theme, (80, 24));
            if let Some((_, prior)) = seen.iter().find(|(v, _)| v == version) {
                assert_eq!(&frame, prior, "same inputs paint the same home: {version:?}");
            }
            seen.push((version.to_owned(), frame));
        }
        HOME.with_borrow(|slot| {
            let mount = slot.as_ref().expect("home mounted");
            assert!(!mount.renderer.ops_log().is_empty(), "the reconciler recorded its ops");
        });
    }

    /// A resize rebuilds cleanly — the mount-key rule (the plan's
    /// footer row keys with the geometry; absolute self-check since
    /// the retirement).
    #[test]
    fn reshape_rebuilds_cleanly() {
        let theme = ThemeState::v2_dark();
        let scenario = Scenario::new("resize");
        let view = owned_view(&scenario, "Ask anything… \"Fix broken tests\"".into());
        let mut first: Option<crate::render::Frame> = None;
        for size in [(80u16, 24u16), (60, 20), (120, 30), (80, 24)] {
            let (frame, _, _) = paint_solid_home_view(&view, &theme, size);
            assert_eq!(frame.height, usize::from(size.1), "full-height home at {size:?}");
            if size == (80, 24) {
                match &first {
                    None => first = Some(frame),
                    Some(p) => assert_eq!(&frame, p, "same inputs paint the same home"),
                }
            }
        }
    }

    /// The wordmark chunks carry the exact per-cell colors: the
    /// shadow tints round per channel, the bold bit rides the right
    /// half only, and the gap cell stays out (absolute, goldens-pinned
    /// shape).
    #[test]
    fn wordmark_chunks_match_the_old_paint_cells() {
        let colors = LogoColors::of(&ThemeState::v2_dark());
        let left =
            logo_row_chunks(LOGO_LEFT[1], colors.muted, colors.shadow_muted, colors.base, false);
        let right =
            logo_row_chunks(LOGO_RIGHT[3], colors.text_base, colors.shadow_base, colors.base, true);
        // Left row 1 opens with █ (muted over base) and carries the
        // `_` spaces as muted-over-shadow runs.
        let first = left.chunks.first().expect("non-empty");
        assert!(first.text.starts_with("█▀▀█"), "left row 1: {:?}", first.text);
        assert_eq!(first.fg, Some(rgba(colors.muted)));
        assert_eq!(first.bg, Some(rgba(colors.base)));
        assert_eq!(first.attributes, 0, "the left half is not bold");
        // Right row 3 is all plain ▀ glyphs: text.base over base,
        // BOLD (the right half's bold bit).
        let last = right.chunks.last().expect("non-empty");
        assert!(last.text.ends_with("▀▀▀▀"), "right row 3: {:?}", last.text);
        assert_eq!(last.fg, Some(rgba(colors.text_base)));
        assert_eq!(last.bg, Some(rgba(colors.base)));
        assert_eq!(last.attributes, 1, "BOLD = bit 0");
        // Left row 3 carries the `~` glyphs: ▀ IN the shadow (the
        // tinted fg over the base bg).
        let left_shadow =
            logo_row_chunks(LOGO_LEFT[3], colors.muted, colors.shadow_muted, colors.base, false);
        let shadow_run = left_shadow
            .chunks
            .iter()
            .find(|chunk| chunk.text.contains('▀') && chunk.fg == Some(rgba(colors.shadow_muted)))
            .expect("the ~~ run paints in the shadow tint");
        assert!(shadow_run.text.starts_with("▀▀"), "the ~~ pair: {:?}", shadow_run.text);
        assert_eq!(shadow_run.bg, Some(rgba(colors.base)));
        // The halves are 19 cells each; the gap cell is outside both.
        assert_eq!(display_width(LOGO_LEFT[1]), 19);
        assert_eq!(display_width(LOGO_RIGHT[3]), 19);
        assert_eq!(LOGO_WIDTH, 39);
    }
}
