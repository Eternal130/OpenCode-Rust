//! Lane M1 (换血 pilot): the session tab strip re-homed onto
//! opentui-solid — the first migration surface, the template for
//! M2..Mn.
//!
//! **What this module is.** A 1:1 structural port of v2's
//! `HorizontalSessionTabs` (`component/session-tabs.tsx:1288-1803`)
//! as an opentui-solid builder tree (`box_`/`text`/`show`/`for_list`
//! — see `/opt/opentui-rust/crates/opentui-solid`), driven by the
//! SAME oc-tui store data the hand-rolled renderer
//! ([`crate::components::session_tabs`]) consumes. The store, the
//! mouse routing (`app.rs` → `strip_hit` → `Store::select_tab` /
//! `close_tab` / `Route::Home`), and the PTY layers are NOT migrated:
//! only the view layer. The bridging pattern (and its gotchas) is
//! documented in `docs/migration-m1-tabstrip.md`.
//!
//! **Selection flag (retired).** The lane-M1 dual-path flag
//! (`OC_TABS_OPENTUI`) is gone: this module's opentui-solid tree is
//! the strip's ONLY renderer — the hand-rolled bar/rail walk was
//! deleted after the one-release probation (the retirement lane).
//!
//! **The bridge shape** (what M2..Mn will copy):
//!
//! * *inputs*: one [`Signal<StripInputs>`] snapshot — tabs/views,
//!   active, hover, add-hover, width, theme colors — synced from the
//!   store at the app loop's existing redraw cadence (exactly the
//!   moments the old path re-rendered; the store's own signal graph
//!   is untouched). Setting it fires every derived `map` chain.
//! * *reactive shape*: mirrors the tsx — the window/layout derives
//!   once per input change (`For` keyed by sessionID over
//!   `adaptive_session_tab_layout`, the same solver the old path
//!   uses), per-tab props (width/background/title/indicator/close)
//!   are dynamic props over per-key mapped views, the overflow
//!   markers and the ` + ` affordance are `Show`-gated elements with
//!   dynamic styled content.
//! * *interaction seams*: the tree carries no event wiring — the
//!   store's handlers stay driven by `app.rs`'s existing
//!   `strip_hit`-based routing (the ref's onMouseUp/onMouseOver
//!   bodies all land in the same three store methods). When a later
//!   lane migrates the PTY mouse stream onto the renderer's hit
//!   grid, this is the seam to revisit.
//! * *frame out*: the strip paints through a real
//!   [`opentui_core::renderer::CliRenderer`] (taffy layout + cell
//!   raster) into a 1-row `OptimizedBuffer`; the buffer cells are
//!   copied into the oc-tui [`Frame`] the route compositor expects.
//!   The glow gradient (the ref's `TabPulse` resting state) renders
//!   as per-cell 1-wide boxes — the same `resting_glow_bg` math the
//!   old path bakes into `Fill`s — so text over it keeps its
//!   transparent-bg blend (destination bg preserved), exactly the
//!   upstream `TabPulse`-under-text stacking.

// Tab widths/gaps are terminal dimensions (far below 2^23), so the
// pedantic usize→f32 cast lints carry no real risk here — the same
// allowance the hand-rolled renderer carries.
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use std::cell::RefCell;
use std::rc::Rc;

use opentui_core::buffer::{
    grapheme_id_from_char, is_continuation_char, is_grapheme_char, OptimizedBuffer,
};
use opentui_core::renderer::CliRendererConfig;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::{Children, ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{
    box_, children_expr, for_list, map, show, text, Element, Signal, SolidRenderer,
};

use crate::components::session_tabs::{
    adaptive_session_tab_layout, session_tab_overflow_width, AdaptiveLayout, TabStatus, TabView,
    ADD_TAB_WIDTH,
};
use crate::components::tab_pulse::{glow_text_color, resting_glow_bg, tint, Rgb, GLOW_TAIL};
#[cfg(test)]
use crate::render::paint_placed_with_fills;
use crate::render::{Cell, CellColor, Frame, CONTINUATION};
use crate::store::Unread;
use crate::theme::ThemeStyles;
use crate::wrap::take_width;

// ---------------------------------------------------------------------------
// inputs (the store → view bridge)
// ---------------------------------------------------------------------------

/// Theme-derived colors the strip needs (the old path's `ThemeStyles`
/// reads, pre-resolved once per sync so the element builders only
/// format hex strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StripColors {
    /// `theme.background` — the strip's base/clear color.
    background: Rgb,
    /// `theme.background_prompt` — the selected tab pill (the ref's
    /// `tint(base, decrease(raised.base), selection)`).
    selected: Rgb,
    /// `theme.action_hovered` — `background.action.primary.hovered`.
    hovered: Rgb,
    /// `theme.text.fg` — `text.base`.
    text: Rgb,
    /// `theme.text_muted.fg` — `text.muted`.
    muted: Rgb,
    /// `theme.error` — the error unread glow hue.
    error: Rgb,
    /// `theme.accent` — `theme.hue.accent[200]`, the activity glow hue.
    accent: Rgb,
}

impl StripColors {
    fn of(theme: &ThemeStyles) -> Self {
        fn rgb_of(color: ratatui::style::Color) -> Rgb {
            match color {
                ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
                _ => (128, 128, 128),
            }
        }
        StripColors {
            background: rgb_of(theme.background),
            selected: rgb_of(theme.background_prompt),
            hovered: rgb_of(theme.action_hovered),
            text: rgb_of(theme.text.fg.unwrap_or(ratatui::style::Color::Rgb(255, 255, 255))),
            muted: rgb_of(theme.text_muted.fg.unwrap_or(ratatui::style::Color::Rgb(128, 128, 128))),
            error: rgb_of(theme.error),
            accent: rgb_of(theme.accent),
        }
    }
}

/// One frame's store snapshot — the ONLY input the tree reads. Synced
/// at the app loop's redraw cadence by [`paint_solid_bar`]; every
/// derived `map` (layout window, per-tab paint state, affordances)
/// recomputes from it, and the reconciler's per-prop equality skip
/// turns unchanged values into no-ops.
#[derive(Debug, Clone)]
struct StripInputs {
    tabs: Vec<TabView>,
    active: Option<String>,
    show_plus: bool,
    hover: Option<String>,
    add_hovered: bool,
    width: usize,
    colors: StripColors,
}

impl StripInputs {
    /// The strip layout — the same adaptive solver the old path uses
    /// (`render_bar` → `adaptive_session_tab_layout`), so the visible
    /// window and per-tab widths are bit-identical between paths.
    fn layout(&self) -> AdaptiveLayout {
        let owned: Vec<crate::store::SessionTab> =
            self.tabs.iter().map(|view| view.tab.clone()).collect();
        adaptive_session_tab_layout(
            &owned,
            self.active.as_deref(),
            self.width.saturating_sub(if self.show_plus { ADD_TAB_WIDTH } else { 0 }),
            0,
        )
    }

    /// `numberWidth` — `max(2, digits(tabs.length))` over ALL tabs
    /// (the strip's ordinal slot, not the visible window).
    fn number_width(&self) -> usize {
        self.tabs.len().to_string().len().max(2)
    }

    fn find(&self, key: &str) -> Option<&TabView> {
        self.tabs.iter().find(|view| view.tab.session_id == key)
    }
}

/// `tabFeedbackColor ?? accent` — the hue a glow renders in (the old
/// path's `glow_hue`).
fn glow_hue(status: TabStatus, colors: &StripColors) -> Rgb {
    if status.attention.is_none() && status.unread == Some(Unread::Error) {
        colors.error
    } else {
        colors.accent
    }
}

/// Everything one tab's dynamic props derive from — the per-key
/// mapped view (the tsx's per-tab `createMemo` cluster:
/// `selected`/`hovered`/`width`/`background`, `characterColor`,
/// `numberColor`, the ✕ state).
struct TabPaint {
    width: usize,
    selected: bool,
    hovered: bool,
    /// The flat tab background (selected pill / hover lift / base).
    bg: Rgb,
    /// The full glow color while glowing (`tint(bg, hue, 1)`).
    glow: Option<Rgb>,
}

fn tab_paint(inputs: &StripInputs, layout: &AdaptiveLayout, key: &str) -> Option<TabPaint> {
    let index = layout.tabs.iter().position(|tab| tab.session_id == key)?;
    let view = inputs.find(key)?;
    let selected = inputs.active.as_deref() == Some(key);
    let hovered = inputs.hover.as_deref() == Some(key);
    let bg = if selected {
        inputs.colors.selected
    } else if hovered {
        inputs.colors.hovered
    } else {
        inputs.colors.background
    };
    let glow =
        (view.status.glows(selected)).then(|| tint(bg, glow_hue(view.status, &inputs.colors), 1.0));
    Some(TabPaint { width: layout.widths[index], selected, hovered, bg, glow })
}

// ---------------------------------------------------------------------------
// styled-chunk builders (byte-parity with the old path's runs)
// ---------------------------------------------------------------------------

fn rgba(rgb: Rgb) -> Rgba {
    Rgba::rgb(rgb.0, rgb.1, rgb.2)
}

fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

/// The title chunks: one run at the tab foreground, or
/// per-character glow tints while glowing (`characterColor`, one
/// chunk per glyph — the tsx's `<Index each={visibleTitleParts}>`
/// spans). Chunks carry NO bg: the cell keeps the pulse/background
/// beneath (the upstream transparent-bg blend), which is
/// byte-identical to the old path's per-run `bg(...)` because the
/// gradient at that column IS the destination cell.
fn title_chunks(inputs: &StripInputs, paint: &TabPaint, key: &str) -> StyledText {
    // A closing tab's mapped views can evaluate one last time against
    // inputs that already dropped it (its For unmount lands at the
    // next flush) — degrade to empty content; the element unmounts
    // moments later.
    let Some(view) = inputs.find(key) else {
        return StyledText::new(Vec::new());
    };
    let number_width = inputs.number_width();
    // `restingTitleWidth` = width − 1 − numberWidth; hovering shifts
    // the right bound left of the revealed ✕ (− 2).
    let title_width =
        paint.width.saturating_sub(1 + number_width + usize::from(paint.hovered) * 2).max(1);
    let title = view.tab.title.clone().unwrap_or_else(|| "Untitled session".into());
    let title_text = take_width(&title, title_width);
    let foreground =
        if paint.selected || paint.hovered { inputs.colors.text } else { inputs.colors.muted };
    let bold = u32::from(paint.selected); // attributes bit 1 = BOLD
    match paint.glow {
        None => StyledText::new(vec![TextChunk {
            text: title_text,
            fg: Some(rgba(foreground)),
            bg: None,
            attributes: bold,
            link: None,
        }]),
        Some(glow) => StyledText::new(
            title_text
                .chars()
                .enumerate()
                .map(|(cell, ch)| {
                    let column = 1 + number_width + cell;
                    TextChunk {
                        text: ch.to_string(),
                        fg: Some(rgba(glow_text_color(foreground, glow, column, paint.width, 1.0))),
                        bg: None,
                        attributes: bold,
                        link: None,
                    }
                })
                .collect(),
        ),
    }
}

/// The indicator chunks (`TabIndicator.label`): `!`/`?`/spinner/`•`
/// right-aligned in its (numberWidth+1)-box, unread/attention in the
/// glow hue, else `text.base`. An empty label leaves the reserved
/// slot blank (no chunks).
fn indicator_chunks(inputs: &StripInputs, key: &str) -> StyledText {
    // See title_chunks: tolerate the closing-tab window.
    let Some(view) = inputs.find(key) else {
        return StyledText::new(Vec::new());
    };
    let glyph = view.status.indicator_label();
    if glyph.is_empty() {
        return StyledText::new(Vec::new());
    }
    let fg = if view.status.unread.is_some() || view.status.attention.is_some() {
        glow_hue(view.status, &inputs.colors)
    } else {
        inputs.colors.text
    };
    StyledText::new(vec![TextChunk {
        text: glyph.to_string(),
        fg: Some(rgba(fg)),
        bg: None,
        attributes: 0,
        link: None,
    }])
}

/// The ✕ close mark: one cell, `closeColor = tint(muted, text, 0.6)`
/// over the FLAT tab background — explicit chunk bg, because the old
/// path paints the mark's bg flat over a possibly-gradient tab
/// (`close_mark`, `session_tabs.rs:406-428`). Empty while unhovered.
fn close_chunks(inputs: &StripInputs, paint: &TabPaint) -> StyledText {
    if !paint.hovered || paint.width < 2 {
        return StyledText::new(Vec::new());
    }
    let fg = tint(inputs.colors.muted, inputs.colors.text, 0.6);
    let bg = if paint.selected { inputs.colors.selected } else { inputs.colors.hovered };
    StyledText::new(vec![TextChunk {
        text: "✕".to_string(),
        fg: Some(rgba(fg)),
        bg: Some(rgba(bg)),
        attributes: 0,
        link: None,
    }])
}

/// The ` + ` affordance: all three cells (pads included) carry the
/// muted/text fg over base/hovered bg — the golden bytes pin the
/// whole run's colors (`session-tabs.tsx:1768-1796`).
fn plus_chunks(add_hovered: bool, colors: &StripColors) -> StyledText {
    let (fg, bg) =
        if add_hovered { (colors.text, colors.hovered) } else { (colors.muted, colors.background) };
    StyledText::new(vec![TextChunk {
        text: " + ".to_string(),
        fg: Some(rgba(fg)),
        bg: Some(rgba(bg)),
        attributes: 0,
        link: None,
    }])
}

/// The overflow markers: `‹{before}` / ` {after}›` in `text.muted`.
fn overflow_chunks(text: String, colors: &StripColors) -> StyledText {
    StyledText::new(vec![TextChunk {
        text,
        fg: Some(rgba(colors.muted)),
        bg: None,
        attributes: 0,
        link: None,
    }])
}

// ---------------------------------------------------------------------------
// dynamic-prop adapters (typed mapped views → PropValue sources)
// ---------------------------------------------------------------------------

/// `dyn_(src, to PropValue::Num)` — a usize-valued dynamic prop.
fn dyn_num<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<usize> + 'static,
{
    opentui_solid::dyn_(source, |value: &usize| PropValue::Num(*value as f32))
}

/// `dyn_(src, to PropValue::Color)` — an Rgb-valued dynamic prop.
fn dyn_color<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<Rgb> + 'static,
{
    opentui_solid::dyn_(source, |value: &Rgb| PropValue::Color(hex(*value)))
}

/// `dyn_(src, to PropValue::Styled)` — styled-chunk content.
fn dyn_styled<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<StyledText> + 'static,
{
    opentui_solid::dyn_(source, |value: &StyledText| PropValue::Styled(value.clone()))
}

// ---------------------------------------------------------------------------
// the element tree (the tsx structure, builder-for-JSX)
// ---------------------------------------------------------------------------

/// One tab's element — the tsx `<For each={layout().tabs}>` body
/// (session-tabs.tsx:1646-1760) with the animations-off subset:
///
/// ```text
/// <box width position="relative" flexDirection="row" backgroundColor>
///   <box position="absolute" left=0 top=0 width="100%" height=1>   ← TabPulse
///     …one 1-wide box per glow cell…
///   </box>
///   <box width="100%" flexDirection="row">                          ← zIndex 1
///     <box width={numberWidth+1} paddingRight=1 justify-end flexShrink=0>
///       <text>{indicator}</text>                                     ← TabIndicator
///     </box>
///     <text width={titleWidth} wrapMode="none">{title}</text>       ← title_shimmer
///     <text position="absolute" right=1 width=1>{✕}</text>
///   </box>
/// </box>
/// ```
///
/// Every hover/selection-dependent value is a DYNAMIC prop over the
/// per-key mapped view — the tsx's per-tab reactive reads — so a
/// hover change patches `backgroundColor`/`content`/`width` in place
/// without remounting the tab. Each mapped view is its own
/// subscription chain over the shared inputs signal (the port's
/// `Mapped` is not `Clone` — one chain per prop, like the tsx's one
/// render effect per expression).
#[allow(clippy::too_many_lines)] // one JSX element per section, mirroring the tsx body
fn tab_element(inputs: &Signal<StripInputs>, key: &str) -> Element {
    // Own the key: every mapped view below captures it ('static).
    let key = key.to_owned();
    // TabPulse resting state: one 1-wide opaque box per glow cell
    // (the old path's per-cell `Fill`s, `bar_tab_fills`). Recomputed
    // as expression children; unkeyed cells replace wholesale on
    // status change (rare), which matches the tsx's TabPulse level
    // re-derivation.
    let pulse_key = key.clone();
    let pulse_cells = children_expr(map(inputs.clone(), move |i: &StripInputs| {
        let layout = i.layout();
        let Some(paint) = tab_paint(i, &layout, &pulse_key) else {
            return Vec::new();
        };
        match paint.glow {
            None => Vec::new(),
            Some(glow) => (0..paint.width)
                .map(|cell| {
                    let color = resting_glow_bg(paint.bg, glow, cell, paint.width, GLOW_TAIL, 1.0);
                    box_()
                        .width(1.0)
                        .height(1.0)
                        .flex_shrink(0.0)
                        .background_color(hex(color))
                        .into_element()
                })
                .collect(),
        }
    }));

    // TabIndicator box: `width={numberWidth + 1}`, paddingRight 1,
    // justifyContent flex-end — the glyph right-aligns at column
    // numberWidth − 1 exactly like the tsx box.
    let indicator_key = key.clone();
    let indicator_box = box_()
        .flex_shrink(0.0)
        .flex_direction("row")
        .justify_content("flex-end")
        .padding_right(1.0)
        .dyn_prop(
            "width",
            dyn_num(map(inputs.clone(), move |i: &StripInputs| i.number_width() + 1)),
        )
        .child(text().dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), move |i: &StripInputs| {
                indicator_chunks(i, &indicator_key)
            })),
        ));

    // The title: explicit width + wrapMode none (title_shimmer's
    // width prop); per-char glow chunks while glowing.
    let title_width_key = key.clone();
    let title_key = key.clone();
    let title_text = text()
        .wrap_mode("none")
        .dyn_prop(
            "width",
            dyn_num(map(inputs.clone(), move |i: &StripInputs| {
                let layout = i.layout();
                let number_width = i.number_width();
                tab_paint(i, &layout, &title_width_key).map_or(1, |paint| {
                    (paint.width.saturating_sub(1 + number_width + usize::from(paint.hovered) * 2))
                        .max(1)
                })
            })),
        )
        .dyn_prop(
            "content",
            dyn_styled(map(inputs.clone(), move |i: &StripInputs| {
                let layout = i.layout();
                match tab_paint(i, &layout, &title_key) {
                    Some(paint) => title_chunks(i, &paint, &title_key),
                    None => StyledText::new(Vec::new()),
                }
            })),
        );

    // The ✕: absolute right=1 (one cell in from the tab's right
    // edge), kept mounted with empty content while unhovered — the
    // tsx keeps the renderable for its hover region.
    let close_key = key.clone();
    let close_text = text().position("absolute").right(1.0).width(1.0).dyn_prop(
        "content",
        dyn_styled(map(inputs.clone(), move |i: &StripInputs| {
            let layout = i.layout();
            match tab_paint(i, &layout, &close_key) {
                Some(paint) => close_chunks(i, &paint),
                None => StyledText::new(Vec::new()),
            }
        })),
    );

    let content_box = box_().width("100%").flex_direction("row").position("relative").children(
        Children::Static(vec![
            indicator_box.into_element(),
            title_text.into_element(),
            close_text.into_element(),
        ]),
    );

    // The TabPulse carrier: absolute, full-width, bg-transparent —
    // paints nothing itself, only hosts the glow cells (the ref's
    // TabPulse sits under the zIndex-1 content box; paint order is
    // DOM order).
    let pulse_box = box_()
        .position("absolute")
        .left(0.0)
        .top(0.0)
        .width("100%")
        .height(1.0)
        .flex_direction("row")
        .children_dyn(pulse_cells);

    let width_key = key.clone();
    let tab_width = map(inputs.clone(), move |i: &StripInputs| {
        let layout = i.layout();
        tab_paint(i, &layout, &width_key).map_or(1, |paint| paint.width)
    });
    let bg_key = key;
    let tab_bg = map(inputs.clone(), move |i: &StripInputs| {
        let layout = i.layout();
        tab_paint(i, &layout, &bg_key).map_or(i.colors.background, |paint| paint.bg)
    });

    box_()
        .position("relative")
        .flex_direction("row")
        .dyn_prop("width", dyn_num(tab_width))
        .dyn_prop("backgroundColor", dyn_color(tab_bg))
        .children(Children::Static(vec![pulse_box.into_element(), content_box.into_element()]))
        .into_element()
}

/// The strip element — `HorizontalSessionTabs`'s root box
/// (session-tabs.tsx:1528-1556): height 1, flexShrink 0, position
/// relative, flexDirection row. Each dynamic construct (`Show`/`For`)
/// owns a bg-transparent carrier box — one element carries ONE
/// dynamic-children source, so multi-construct children use carriers
/// (the port's answer to solid's mixed expression children arrays).
fn strip_element(inputs: &Signal<StripInputs>) -> Element {
    // `‹{before}` — mounted while before > 0 (`<Show>`).
    let before = show(
        Rc::new(map(inputs.clone(), |i: &StripInputs| i.layout().before > 0))
            as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs.clone();
            move || {
                vec![text()
                    .dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), |i: &StripInputs| {
                            overflow_chunks(format!("‹{}", i.layout().before), &i.colors)
                        })),
                    )
                    .dyn_prop(
                        "width",
                        dyn_num(map(inputs.clone(), |i: &StripInputs| {
                            session_tab_overflow_width(i.layout().before)
                        })),
                    )
                    .into_element()]
            }
        },
    );

    // ` {after}›` — mounted while after > 0 (`<Show>`).
    let after = show(
        Rc::new(map(inputs.clone(), |i: &StripInputs| i.layout().after > 0))
            as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs.clone();
            move || {
                vec![text()
                    .dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), |i: &StripInputs| {
                            overflow_chunks(format!(" {}›", i.layout().after), &i.colors)
                        })),
                    )
                    .dyn_prop(
                        "width",
                        dyn_num(map(inputs.clone(), |i: &StripInputs| {
                            session_tab_overflow_width(i.layout().after)
                        })),
                    )
                    .into_element()]
            }
        },
    );

    // The tabs — `<For each={layout().tabs}>` keyed by sessionID over
    // the same adaptive window (identity-stable across window shifts;
    // the render closure captures the shared inputs signal).
    let window = ForItemSource {
        source: map(inputs.clone(), |i: &StripInputs| {
            i.layout()
                .tabs
                .iter()
                .map(|tab| ForItem {
                    key: tab.session_id.clone(),
                    value: PropValue::Str(tab.session_id.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let tabs_for = {
        let inputs = inputs.clone();
        for_list(Rc::new(window), move |item: &ForItem, _index: usize| {
            tab_element(&inputs, &item.key)
        })
    };

    // ` + ` — mounted while the add affordance shows (`showPlus`).
    let plus = show(
        Rc::new(map(inputs.clone(), |i: &StripInputs| i.show_plus)) as Rc<dyn Reactive<bool>>,
        {
            let inputs = inputs.clone();
            move || {
                vec![text()
                    .width(ADD_TAB_WIDTH)
                    .dyn_prop(
                        "content",
                        dyn_styled(map(inputs.clone(), |i: &StripInputs| {
                            plus_chunks(i.add_hovered, &i.colors)
                        })),
                    )
                    .into_element()]
            }
        },
    );

    box_()
        .height(1.0)
        .flex_shrink(0.0)
        .position("relative")
        .flex_direction("row")
        .children(Children::Static(vec![
            // The carriers: one per dynamic construct, bg-transparent,
            // ROW-direction — the strip's own flex direction (the port
            // pins yoga's column default to Column, so a bare carrier
            // would stack its children vertically out of the 1-row
            // strip — lane M1 gotcha, see the pattern doc).
            box_().flex_direction("row").children_dyn(Rc::new(before)).into_element(),
            box_().flex_direction("row").children_dyn(Rc::new(tabs_for)).into_element(),
            box_().flex_direction("row").children_dyn(Rc::new(after)).into_element(),
            box_().flex_direction("row").children_dyn(Rc::new(plus)).into_element(),
        ]))
        .into_element()
}

// ---------------------------------------------------------------------------
// the mounted strip (one per process/thread, resized/re-themed in place)
// ---------------------------------------------------------------------------

/// The mounted strip: the inputs signal + a real headless
/// [`SolidRenderer`] (taffy layout + cell raster + hit grid). Built
/// once per (width, theme) generation — a terminal resize or theme
/// swap rebuilds the renderer (rare; the reconciler keeps per-tab
/// identity across ordinary input changes).
struct SolidTabsStrip {
    inputs: Signal<StripInputs>,
    renderer: SolidRenderer,
    width: usize,
    colors: StripColors,
}

impl SolidTabsStrip {
    fn new(width: usize, colors: StripColors) -> Self {
        let inputs = Signal::new(StripInputs {
            tabs: Vec::new(),
            active: None,
            show_plus: false,
            hover: None,
            add_hovered: false,
            width,
            colors,
        });
        let mut renderer = SolidRenderer::new(CliRendererConfig {
            width: width as u32,
            height: 1,
            background_color: rgba(colors.background),
            ..CliRendererConfig::default()
        });
        renderer.render_root(strip_element(&inputs));
        Self { inputs, renderer, width, colors }
    }

    /// Sync one frame's store snapshot, flush the reconciler patches,
    /// and render — the per-frame bridge cost. A width/theme change
    /// rebuilds the whole mount (rare, and the reconciler has no
    /// in-place renderer resize seam for background colors).
    fn paint(&mut self, inputs: StripInputs) {
        if self.width != inputs.width || self.colors != inputs.colors {
            *self = Self::new(inputs.width, inputs.colors);
        }
        self.inputs.set(inputs);
        self.renderer.flush();
    }
}

/// opentui attribute bits → the frame's ECMA-48 bits (1=bold ..
/// 9=crossed; opentui packs strikethrough at bit 8 = SGR 9).
fn frame_attrs(attributes: u32) -> u16 {
    let mut bits = 0u32;
    if attributes & 0x1 != 0 {
        bits |= 1 << 0; // bold
    }
    if attributes & (1 << 1) != 0 {
        bits |= 1 << 1; // dim
    }
    if attributes & (1 << 2) != 0 {
        bits |= 1 << 2; // italic
    }
    if attributes & (1 << 3) != 0 {
        bits |= 1 << 3; // underline
    }
    if attributes & (1 << 4) != 0 {
        bits |= 1 << 4; // blink
    }
    if attributes & (1 << 5) != 0 {
        bits |= 1 << 5; // inverse
    }
    if attributes & (1 << 6) != 0 {
        bits |= 1 << 6; // hidden
    }
    if attributes & (1 << 7) != 0 {
        bits |= 1 << 8; // crossed out (SGR 9)
    }
    bits as u16
}

fn frame_color(color: Rgba) -> CellColor {
    if color.alpha() == 0 {
        CellColor::Default
    } else {
        CellColor::Rgb(color.red(), color.green(), color.blue())
    }
}

/// The glow cell ranges of this frame's layout — the strip-local
/// `[x, x + width)` spans of visible glowing tabs (leading overflow
/// marker included). The old path paints these as `Fill`s with
/// `fg(Color::Reset)`; opentui's opaque fills hard-write fg=white, so
/// the frame copy resets the fg on their non-glyph cells (a painted
/// glyph always carries its own fg — the two never collide).
fn glow_ranges(inputs: &StripInputs) -> Vec<(usize, usize)> {
    let layout = inputs.layout();
    let mut ranges = Vec::new();
    let mut x = if layout.before > 0 { session_tab_overflow_width(layout.before) } else { 0 };
    for (index, tab) in layout.tabs.iter().enumerate() {
        let width = layout.widths[index];
        if let Some(view) = inputs.find(&tab.session_id) {
            let selected = inputs.active.as_deref() == Some(tab.session_id.as_str());
            if view.status.glows(selected) {
                ranges.push((x, width));
            }
        }
        x += width;
    }
    ranges
}

/// The painted 1-row buffer → the oc-tui [`Frame`] (the route
/// compositor's currency). Opaque fills in opentui write
/// fg=white/bg=color space cells — byte-identical to the old path's
/// base-fill + `Fill` layer, so the copy is direct; the one divergence
/// is the glow fills' `fg(Color::Reset)` semantics, restored via
/// [`glow_ranges`] (space cells inside a glow range read fg Default).
/// Shared with the later solid surfaces (lane M2's dialogs pass an
/// empty glow set).
pub(crate) fn frame_from_buffer(buffer: &OptimizedBuffer, glows: &[(usize, usize)]) -> Frame {
    let width = buffer.width as usize;
    let mut cells = Vec::with_capacity(buffer.height as usize);
    for y in 0..buffer.height {
        let mut row = Vec::with_capacity(width);
        for x in 0..buffer.width {
            let Some(cell) = buffer.get(x, y) else {
                row.push(Cell::default());
                continue;
            };
            let ch = if is_continuation_char(cell.char) {
                CONTINUATION
            } else if is_grapheme_char(cell.char) {
                buffer
                    .grapheme_bytes(grapheme_id_from_char(cell.char))
                    .chars()
                    .next()
                    .unwrap_or(' ')
            } else {
                char::from_u32(cell.char).unwrap_or(' ')
            };
            let attrs = frame_attrs(cell.attributes);
            let fg = if ch == ' '
                && attrs == 0
                && cell.fg == Rgba::rgb(255, 255, 255)
                && glows.iter().any(|(start, len)| x >= *start as u32 && x < (*start + *len) as u32)
            {
                // A pulse fill cell no glyph covers: the old path's
                // Fill carries fg Reset — the terminal default. The
                // fill's signature is opentui's hard-written white; a
                // glyph-painted space (a title's own spaces carry the
                // per-character glow tint) keeps its fg.
                CellColor::Default
            } else {
                frame_color(cell.fg)
            };
            row.push(Cell { ch, attrs, fg, bg: frame_color(cell.bg) });
        }
        cells.push(row);
    }
    Frame { cells, width, height: buffer.height as usize }
}

thread_local! {
    /// One strip per thread-process — the app mounts exactly one; the
    /// PTY/golden harnesses reuse it across boots in the same process.
    static STRIP: RefCell<Option<SolidTabsStrip>> = const { RefCell::new(None) };
}

/// Paint the strip through the opentui-solid path — the new-path
/// twin of the old `render_bar` + `paint_placed_with_fills` pair,
/// fused: sync inputs → flush patches → render → copy the cells.
///
/// Paint the strip through the opentui-solid path (see the module
/// doc for the bridge shape and the flag).
#[must_use]
pub fn paint_solid_bar(
    tabs: &[TabView],
    active: Option<&str>,
    show_plus: bool,
    width: usize,
    theme: &ThemeStyles,
    hover: Option<&str>,
    add_hovered: bool,
) -> Frame {
    let inputs = StripInputs {
        tabs: tabs.to_vec(),
        active: active.map(str::to_owned),
        show_plus,
        hover: hover.map(str::to_owned),
        add_hovered,
        width,
        colors: StripColors::of(theme),
    };
    let glows = glow_ranges(&inputs);
    STRIP.with_borrow_mut(|slot| {
        let strip = slot.get_or_insert_with(|| SolidTabsStrip::new(inputs.width, inputs.colors));
        strip.paint(inputs);
        frame_from_buffer(&strip.renderer.renderer.current_render_buffer, &glows)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::session_tabs::{Attention, TabStatus};
    use crate::store::SessionTab;

    fn view(id: &str, title: &str, status: TabStatus) -> TabView {
        TabView {
            tab: SessionTab { session_id: id.to_string(), title: Some(title.to_string()) },
            status,
            detail: Some("project".to_string()),
        }
    }

    fn theme() -> ThemeStyles {
        crate::theme::ThemeState::v2_dark().styles()
    }

    /// The reactive path actually patches in place: a hover change on
    /// a mounted strip re-renders with the lift + ✕ WITHOUT a
    /// rebuild (same mount, per-prop patches — the ops log shows
    /// reconciler activity from the first paint onward).
    #[test]
    fn hover_change_patches_the_mounted_strip() {
        let styles = theme();
        let tabs = vec![
            view("first", "First", TabStatus::EMPTY),
            view("second", "Second tab title", TabStatus::EMPTY),
        ];
        let resting = paint_solid_bar(&tabs, Some("first"), true, 80, &styles, None, false);
        let hovered =
            paint_solid_bar(&tabs, Some("first"), true, 80, &styles, Some("second"), false);
        // Two 32-wide tabs at 80: the second spans x32..64, so its
        // title starts at 32 + 1 + numberWidth(2) = 35.
        assert_eq!(
            resting.cell(0, 35).ch,
            'S',
            "the second tab's title starts past the indicator: {:?}",
            resting.text_grid()[0]
        );
        // The ✕: one cell in from the hovered tab's right edge (62).
        assert_eq!(hovered.cell(0, 62).ch, '✕', "hover reveals the close mark");
        assert_eq!(resting.cell(0, 62).ch, ' ', "no close mark at rest");
        STRIP.with_borrow(|slot| {
            let strip = slot.as_ref().expect("strip mounted");
            assert!(!strip.renderer.ops_log().is_empty(), "the reconciler recorded its mount ops");
        });
    }

    /// opentui attribute bits map onto the frame's ECMA bits (the
    /// selected tab's title is bold — bit 1).
    #[test]
    fn selected_title_carries_bold() {
        let styles = theme();
        let tabs = vec![view("only", "Title", TabStatus::EMPTY)];
        let frame = paint_solid_bar(&tabs, Some("only"), true, 80, &styles, None, false);
        let bold_cells = frame.cells[0].iter().filter(|cell| cell.attrs & 0x1 != 0).count();
        assert_eq!(bold_cells, 5, "exactly the 5 title glyphs are bold");
        let unselected = paint_solid_bar(&tabs, None, true, 80, &styles, None, false);
        assert!(
            unselected.cells[0].iter().all(|cell| cell.attrs & 0x1 == 0),
            "no bold without selection"
        );
    }

    /// A width change rebuilds cleanly (resize seam) and re-renders
    /// the same scenario the old path renders at that width.
    /// Resize rebuilds the mount (the M1 gotcha: a width change is a
    /// fresh taffy tree) and the rebuilt strip still paints a full
    /// row — the retirement-era residue of the old parity gate, now
    /// an absolute self-check.
    #[test]
    fn resize_rebuilds_and_paints_full_rows() {
        let styles = theme();
        let tabs = vec![
            view("first", "First", TabStatus::EMPTY),
            view(
                "second",
                "Second",
                TabStatus { unread: Some(Unread::Activity), ..TabStatus::EMPTY },
            ),
        ];
        // Mount at 80, then squeeze to 40, then widen to 160.
        let mut first_at_80: Option<String> = None;
        for width in [80usize, 40, 160, 80] {
            let frame = paint_solid_bar(&tabs, Some("second"), true, width, &styles, None, false);
            assert_eq!(frame.text_grid().len(), 1, "one strip row at width {width}");
            let row = frame.text_grid()[0].clone();
            assert_eq!(row.chars().count(), width, "full row at width {width}");
            if width == 80 {
                match &first_at_80 {
                    None => first_at_80 = Some(row),
                    Some(prev) => assert_eq!(&row, prev, "same inputs paint the same row"),
                }
            }
        }
    }
}
