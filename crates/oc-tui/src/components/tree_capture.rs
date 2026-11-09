//! Lane TB — the oc-tui production-tree capturer.
//!
//! Walks the SETTLED mounted shared tree (the `chrome_solid` hub's
//! `SolidRenderer`, i.e. the real production renderable tree every
//! migrated family mounts into) after a headless frame build
//! (`reset_hub` → drive the store to a target state → `render_app`)
//! and serializes it as an **opentui-diff tree scenario** (the v1
//! contract in opentui-rust `tools/diff/README.md` "Tree scenarios"):
//!
//! * every ink-carrying node becomes a TOP-LEVEL absolutely-positioned
//!   node at its settled screen rect — the tree contract's nesting is
//!   flow-only, so the capturer FLATTENS the document (z-order =
//!   document order = DFS preorder, later on top — exactly the
//!   raster's paint order);
//! * `Content::Box`-family nodes → `kind:"box"` (bg/border/title
//!   style mapped); `Content::Text` → `kind:"text"` with the styled
//!   document rebuilt as a chunk array (per-span fg/bg/attrs, line
//!   breaks as `\n` chunks, unstyled runs as plain strings);
//!   `Content::Group` boxes paint nothing and are skipped (children
//!   still walk); kinds outside the v1 contract (textarea, input,
//!   code, markdown, diff, embedded-terminal, …) are skipped and
//!   COUNTED in the scenario's `_meta.skipped_kinds` (the bounded
//!   deviation list — both engines still render the captured subset
//!   identically, which is what the dual-engine gate asserts).
//!
//! Determinism: the walk runs after the compositor seam's flush (the
//! layout is settled), reads only the frozen renderable tree (no
//! Signals, no mount order dependence — rect/style/content are plain
//! values by then), and emits in DFS preorder — re-running the same
//! state produces byte-identical JSON (pinned by the companion test).

use opentui_core::renderable::{Content, Renderable};
use opentui_core::Rgba;

use crate::components::chrome_solid;

/// Drive the headless twin to a target application state and capture
/// its settled shared tree as a tree-scenario JSON value — the
/// capturer's ONLY entry point (Lane TB): fresh hub → the caller's
/// state mutations → `render_app` (every family mounts + the
/// compositor seam flushes) → the walk. This reuses the exact
/// headless assembly path the golden tests use, so the captured tree
/// IS the production mount for that state.
pub fn capture_app_state(
    name: &str,
    source: &str,
    width: u16,
    height: u16,
    drive: impl FnOnce(&crate::store::Store),
) -> Option<serde_json::Value> {
    let store = crate::store::Store::new(crate::reactive::Runtime::new());
    // The same boot `App::with_store` gives the live twin (app.rs):
    // the sidebar toggle's visibility input + the composer's wrap
    // viewport — without these the state renders a different frame
    // than the app would.
    store.set_terminal_size((width, height));
    store.set_prompt_viewport_width(crate::components::composer::prompt_text_width(usize::from(
        width,
    )));
    chrome_solid::reset_hub();
    drive(&store);
    let theme = store.theme();
    let _ = crate::components::dialog::render_app(&store, &theme, (width, height));
    capture_tree_scenario(name, source, width, height)
}

/// Serialize the thread hub's SETTLED shared tree as a tree-scenario
/// JSON value. Call AFTER `render_app` (or any path that ends in
/// `composite_chrome_tree`) for the state being captured — the walk
/// reads the hub's post-flush renderable tree. Returns `None` when no
/// hub is installed (nothing mounted).
#[must_use]
pub fn capture_tree_scenario(
    name: &str,
    source: &str,
    width: u16,
    height: u16,
) -> Option<serde_json::Value> {
    chrome_solid::with_hub_tree(|tree| {
        let mut nodes = Vec::new();
        let mut skipped: Vec<(String, usize)> = Vec::new();
        let root = &tree.solid_renderer_root();
        for child in &root.children {
            visit(child, width, height, &mut nodes, &mut skipped);
        }
        let skipped_kinds: serde_json::Map<String, serde_json::Value> =
            skipped.into_iter().map(|(kind, n)| (kind, serde_json::json!(n))).collect();
        serde_json::json!({
            "$schema": "opentui-diff tree scenario v1",
            "name": name,
            "width": width,
            "height": height,
            "tree": nodes,
            "_meta": {
                "source": source,
                "captured_by": "oc-tui tree_capture (chrome_solid hub walk)",
                "skipped_kinds": skipped_kinds,
            },
        })
    })
}

/// One DFS-preorder visit — `out` grows in paint order (parent ink
/// first, children over it, later siblings over earlier ones).
fn visit(
    r: &Renderable,
    width: u16,
    height: u16,
    out: &mut Vec<serde_json::Value>,
    skipped: &mut Vec<(String, usize)>,
) {
    // A node the layout hid paints nothing — its subtree usually
    // hides with it, but walk the children anyway (absolute children
    // of a hidden box would still have settled rects if the raster
    // kept them; skipping here would drop ink the frame carries).
    if let Some(node) = node_of(r, width, height) {
        out.push(node);
    } else if !matches!(r.content, Content::Group) {
        let kind = kind_name(&r.content);
        match skipped.iter_mut().find(|(k, _)| *k == kind) {
            Some(entry) => entry.1 += 1,
            None => skipped.push((kind.to_owned(), 1)),
        }
    }
    // The diff kind's children are its INTERNAL gutter/code sides —
    // they paint as part of the diff renderable in the rebuilt tree,
    // so walking them would double-paint (text nodes have no children
    // by construction).
    if matches!(r.content, Content::Diff(_)) {
        return;
    }
    for child in &r.children {
        visit(child, width, height, out, skipped);
    }
}

fn kind_name(content: &Content) -> &'static str {
    match content {
        Content::Group => "group",
        Content::Box(_) => "box",
        Content::Text(_) => "text",
        Content::Textarea(_) => "textarea",
        Content::Slider(_) => "slider",
        Content::Arrow(_) => "arrow",
        Content::ScrollBar(_) => "scrollbar",
        Content::ScrollContent(..) => "scroll-content",
        Content::ScrollBox(..) => "scrollbox",
        Content::Input(_) => "input",
        Content::TimeToFirstDraw(_) => "ttfd",
        _ => "other",
    }
}

/// One renderable → one tree node (or `None` when it paints nothing
/// the contract can express: Groups, empty texts, zero-size mounts,
/// out-of-viewport rects).
fn node_of(r: &Renderable, width: u16, height: u16) -> Option<serde_json::Value> {
    let x = r.screen_x.max(0) as u32;
    let y = r.screen_y.max(0) as u32;
    let w = r.w.round() as i64;
    let h = r.h.round() as i64;
    if w <= 0 || h <= 0 {
        return None;
    }
    // keep the declared rect inside the canvas (README contract rule)
    let w = (w as u32).min(u32::from(width).saturating_sub(x));
    let h = (h as u32).min(u32::from(height).saturating_sub(y));
    if w == 0 || h == 0 {
        return None;
    }
    let rect = serde_json::json!({ "x": x, "y": y, "w": w, "h": h });
    let id = r.id.0.clone();
    match &r.content {
        Content::Box(state) => {
            let mut style = serde_json::Map::new();
            if state.background_color.alpha() > 0 {
                style.insert("bg".into(), hex(state.background_color).into());
            }
            if !matches!(state.border, opentui_core::renderables::box_::BorderSpec::Off) {
                style.insert("border".into(), true.into());
                if state.border_color.alpha() > 0 {
                    style.insert("borderColor".into(), hex(state.border_color).into());
                }
                style.insert("borderStyle".into(), state.border_style.clone().into());
            }
            if !state.should_fill {
                style.insert("shouldFill".into(), false.into());
            }
            if let Some(title) = &state.title {
                style.insert("title".into(), title.clone().into());
                style.insert("titleAlignment".into(), state.title_alignment.clone().into());
            }
            Some(node_json(&id, "box", rect, style, None, None))
        }
        Content::ScrollContent(state, _) | Content::ScrollBox(state, _) => {
            // paints like a Box (it IS one); scroll state is not part
            // of the v1 tree contract — the settled rect/style is
            let mut style = serde_json::Map::new();
            if state.background_color.alpha() > 0 {
                style.insert("bg".into(), hex(state.background_color).into());
            }
            if !state.should_fill {
                style.insert("shouldFill".into(), false.into());
            }
            Some(node_json(&id, "box", rect, style, None, None))
        }
        Content::Diff(state) => {
            // The v1 contract's diff kind: the renderable carries its
            // OWN source (`DiffOptions.diff`) + the full color/geometry
            // option matrix, so the capture is lossless — content is
            // the node's `diff` field (the desugarer maps `content` to
            // a key the diff step ignores), the options passthrough
            // overrides the wrapper's `100%` height with the settled
            // content height (the element mounts `height: rows`).
            let o = &state.options;
            let mut style = serde_json::Map::new();
            if let Some(fg) = o.fg.filter(|c| c.alpha() > 0) {
                style.insert("fg".into(), hex(fg).into());
            }
            let mut options = serde_json::Map::new();
            match state.view {
                opentui_core::renderables::diff::DiffView::Split => {
                    options.insert("view".into(), "split".into());
                }
                opentui_core::renderables::diff::DiffView::Unified => {
                    options.insert("view".into(), "unified".into());
                }
            }
            if let Some(ft) = &o.filetype {
                options.insert("filetype".into(), ft.clone().into());
            }
            if matches!(o.wrap_mode, Some(opentui_core::renderables::text::WrapMode::None)) {
                options.insert("wrapMode".into(), "none".into());
            }
            options.insert("showLineNumbers".into(), o.show_line_numbers.into());
            let color = |m: &mut serde_json::Map<String, serde_json::Value>,
                         k: &str,
                         c: opentui_core::Rgba| {
                if c.alpha() > 0 {
                    m.insert(k.into(), hex(c).into());
                }
            };
            color(&mut options, "lineNumberFg", o.line_number_fg);
            color(&mut options, "lineNumberBg", o.line_number_bg);
            color(&mut options, "addedBg", o.added_bg);
            color(&mut options, "removedBg", o.removed_bg);
            color(&mut options, "contextBg", o.context_bg);
            if let Some(c) = o.added_content_bg.filter(|c| c.alpha() > 0) {
                options.insert("addedContentBg".into(), hex(c).into());
            }
            if let Some(c) = o.removed_content_bg.filter(|c| c.alpha() > 0) {
                options.insert("removedContentBg".into(), hex(c).into());
            }
            if let Some(c) = o.context_content_bg.filter(|c| c.alpha() > 0) {
                options.insert("contextContentBg".into(), hex(c).into());
            }
            color(&mut options, "addedSignColor", o.added_sign_color);
            color(&mut options, "removedSignColor", o.removed_sign_color);
            color(&mut options, "addedLineNumberBg", o.added_line_number_bg);
            color(&mut options, "removedLineNumberBg", o.removed_line_number_bg);
            // the element mounted `height: <rows>` — pass the settled
            // height through (the passthrough overrides the wrapper's
            // 100%×100% mount, preserving the internal layout).
            options.insert("height".into(), f64::from(h).into());
            Some(node_json(&id, "diff", rect, style, None, Some(options))).map(|mut node| {
                node.as_object_mut()
                    .expect("node is an object")
                    .insert("diff".into(), o.diff.clone().into());
                node
            })
        }
        Content::Text(state) => {
            let content = text_content(state)?;
            let mut style = serde_json::Map::new();
            // defaults: only deviate from the engine defaults (white
            // fg / transparent bg / no attrs) when the node says so —
            // both drivers default identically.
            let white = Rgba::rgb(255, 255, 255);
            if state.default_fg != white && state.default_fg.alpha() > 0 {
                style.insert("fg".into(), hex(state.default_fg).into());
            }
            if state.default_bg.alpha() > 0 {
                style.insert("bg".into(), hex(state.default_bg).into());
            }
            if state.attributes != 0 {
                style.insert("attributes".into(), u32::from(state.attributes as u8).into());
            }
            let mut options = serde_json::Map::new();
            if matches!(state.wrap_mode, opentui_core::renderables::text::WrapMode::None) {
                options.insert("wrapMode".into(), "none".into());
            }
            let align = match state.text_align {
                opentui_core::text_buffer::TextAlign::Center => Some("center"),
                opentui_core::text_buffer::TextAlign::Right => Some("right"),
                opentui_core::text_buffer::TextAlign::Left => None,
            };
            if let Some(a) = align {
                options.insert("textAlign".into(), a.into());
            }
            Some(node_json(&id, "text", rect, style, Some(content), Some(options)))
        }
        _ => None,
    }
}

fn node_json(
    id: &str,
    kind: &str,
    rect: serde_json::Value,
    style: serde_json::Map<String, serde_json::Value>,
    content: Option<serde_json::Value>,
    options: Option<serde_json::Map<String, serde_json::Value>>,
) -> serde_json::Value {
    let mut node = serde_json::Map::new();
    node.insert("id".into(), sanitize_id(id).into());
    node.insert("kind".into(), kind.into());
    node.insert("rect".into(), rect);
    if !style.is_empty() {
        node.insert("style".into(), style.into());
    }
    if let Some(content) = content {
        node.insert("content".into(), content);
    }
    if let Some(options) = options.filter(|o| !o.is_empty()) {
        node.insert("options".into(), options.into());
    }
    serde_json::Value::Object(node)
}

/// Mount ids must not collide with the desugarer's generated ones or
/// repeat across scenarios' rebuilds — the renderer's auto ids
/// (`r_N`) and keyed mounts both pass through unchanged.
fn sanitize_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "n".to_owned()
    } else {
        cleaned
    }
}

/// The styled document → chunk-array content (contract: `content` is
/// a string or a `{text, fg?, bg?, attributes?}` chunk array). Spans
/// slice the line text by CLUSTER COLUMNS; unstyled gaps between
/// spans carry the node defaults (chunk without fg/bg/attrs). `\n`
/// between lines is a zero-width chunk (the doc builder drops its
/// span — harmless, the line split is what matters).
fn text_content(
    state: &opentui_core::renderables::text::TextRenderableState,
) -> Option<serde_json::Value> {
    use opentui_core::renderables::text::TextRenderableState;
    let state: &TextRenderableState = state;
    if state.doc.lines.is_empty() {
        return None;
    }
    let mut chunks: Vec<serde_json::Value> = Vec::new();
    for (index, line) in state.doc.lines.iter().enumerate() {
        let mut col = 0u32;
        for span in &line.spans {
            if span.col_start > col {
                if let Some(text) = slice_by_columns(line, col, span.col_start) {
                    chunks.push(serde_json::json!({ "text": text }));
                }
            }
            let text = slice_by_columns(line, span.col_start, span.col_end)?;
            let mut chunk = serde_json::Map::new();
            chunk.insert("text".into(), text.into());
            if let Some(fg) = span.fg.filter(|c| c.alpha() > 0) {
                chunk.insert("fg".into(), hex(fg).into());
            }
            if let Some(bg) = span.bg.filter(|c| c.alpha() > 0) {
                chunk.insert("bg".into(), hex(bg).into());
            }
            if span.attributes != 0 {
                chunk.insert("attributes".into(), u32::from(span.attributes as u8).into());
            }
            chunks.push(chunk.into());
            col = span.col_end;
        }
        if col < line.width_cols {
            if let Some(text) = slice_by_columns(line, col, line.width_cols) {
                chunks.push(serde_json::json!({ "text": text }));
            }
        }
        if index + 1 < state.doc.lines.len() {
            chunks.push(serde_json::json!({ "text": "\n" }));
        }
    }
    // a single unstyled chunk round-trips as a plain string
    if chunks.len() == 1
        && chunks[0].as_object().is_some_and(|c| {
            c.len() == 1 && c.contains_key("text") && c["text"].as_str() != Some("\n")
        })
    {
        return Some(chunks.remove(0)["text"].clone());
    }
    if chunks.is_empty() {
        return None;
    }
    Some(serde_json::Value::Array(chunks))
}

/// The line's text slice covering display columns `[from, to)` —
/// cluster boundaries are the only legal cut points.
fn slice_by_columns(
    line: &opentui_core::text_buffer::LogicalLine,
    from: u32,
    to: u32,
) -> Option<String> {
    if from >= to {
        return None;
    }
    let mut byte_start = None;
    let mut byte_end = None;
    let mut col = 0u32;
    for cluster in &line.clusters {
        if col >= from && byte_start.is_none() {
            byte_start = Some(cluster.byte_start as usize);
        }
        col += cluster.width;
        if col >= to {
            byte_end = Some(cluster.byte_end() as usize);
            break;
        }
    }
    let (s, e) = (byte_start?, byte_end.unwrap_or(line.text.len()));
    if s >= e {
        return None;
    }
    line.text.get(s..e).map(str::to_owned)
}

fn hex(color: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red(), color.green(), color.blue())
}
