//! Plugin → TUI UI requests (wave 3's slot bridge): the guest-side
//! `ctx.ui.*` surface lands here as plain Rust data. Two kinds:
//!
//! * **Runtime requests** ([`UiRequest`]) — pushed onto a queue on the
//!   supervisor's IO reader (deadlock rule 1: answered inline, reply
//!   enqueued, never blocking the pipe) and drained by the embedder
//!   ([`crate::PluginHost::drain_ui_requests`]) between frames. The
//!   TUI applies them through its store seams — a toast becomes the
//!   already-reduced `tui.toast.show` bus event, a dialog open carries
//!   the FULLY-BUILT content structure (the plugin built it
//!   synchronously guest-side; the enclosing dispatch/registration
//!   hard timeout bounded that build), so rendering afterwards is
//!   native with zero per-frame IPC.
//! * **Registration entries** (group rules, home quick actions, dialog
//!   providers) — stored natively in
//!   [`crate::registrations::Registrations`] and carried by the
//!   [`crate::RegistrationSnapshot`], exactly like commands/themes.

use serde_json::{json, Value};

/// The plugin toast default duration (the TUI's `tui.toast.show`
/// default, mirrored so the queue carries a resolved value).
pub const DEFAULT_TOAST_DURATION_MS: u64 = 5000;

/// One option row of a plugin dialog's select semantics (v2
/// `DialogSelectOption` projected to the native bridge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDialogOption {
    /// The rendered row title.
    pub title: String,
    /// The submitted value (a command id for dispatch-backed dialogs).
    pub value: String,
    /// Optional trailing description.
    pub description: String,
    /// G-512 — the option's own `category` (context.ts:360): the ref
    /// groups rows by it; a category-less option renders flat.
    pub category: Option<String>,
}

/// The dialog content structure a plugin supplies at open time (the
/// delegation payload): a title, informational rows, and optional
/// select semantics. Built synchronously plugin-side; rendered
/// natively afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginDialogSpec {
    pub title: String,
    /// Static information rows (rendered verbatim when no options).
    pub rows: Vec<String>,
    /// When non-empty the dialog is a select (filter/move/submit run
    /// through the native dialog pipeline; submit dispatches the
    /// option's value).
    pub options: Vec<PluginDialogOption>,
    /// `ctx.ui.dialog.show/set` presentation fields (api.tsx:329-341
    /// — G-475): the active dialog's `size` (`medium`|`large`|
    /// `xlarge`) and `centered` flag. `None`/`false` keep the native
    /// defaults (the ref's `dialog.setSize/setCentered`).
    pub size: Option<String>,
    pub centered: bool,
}

impl PluginDialogSpec {
    /// Parse the guest's JSON shape
    /// (`{title, rows: [..], options: [{title, value, description?}]}`).
    /// Unknown shapes degrade to an empty spec (contained, like every
    /// registration surface).
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let str_field =
            |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        Self {
            title: str_field("title"),
            rows: value
                .get("rows")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default(),
            options: value
                .get("options")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter(|option| {
                            option.get("title").and_then(Value::as_str).is_some()
                                && option.get("value").is_some()
                        })
                        .map(|option| PluginDialogOption {
                            title: option
                                .get("title")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            // The ref's select values are arbitrary
                            // JSON; the native select dialog keys on
                            // strings — non-strings serialize to their
                            // JSON text (bounded projection, G-475).
                            value: option
                                .get("value")
                                .map(|value| match value {
                                    Value::String(text) => text.clone(),
                                    other => other.to_string(),
                                })
                                .unwrap_or_default(),
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            category: option
                                .get("category")
                                .and_then(Value::as_str)
                                .filter(|category| !category.is_empty())
                                .map(str::to_owned),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            size: value
                .get("size")
                .and_then(Value::as_str)
                .filter(|size| matches!(*size, "medium" | "large" | "xlarge"))
                .map(str::to_owned),
            centered: value.get("centered").and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

/// The flattened content of one plugin slot claim / plugin route
/// render (the guest's render function ran synchronously; the tree
/// flattened to the native section shape — same shape the
/// `sidebar_content` bridge produces, ref
/// `packages/tui/src/plugin/api.tsx:298-312`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginSectionSpec {
    pub title: String,
    pub rows: Vec<String>,
    /// Per-row clickability (the render's `onMouseUp` handlers —
    /// addressable through [`crate::PluginHost::click_slot_claim`]).
    pub clickable: Vec<bool>,
}

impl PluginSectionSpec {
    /// Parse the guest's `{title, rows: [{text, click}]}` reply.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let title = value.get("title").and_then(Value::as_str).unwrap_or_default().to_owned();
        let mut rows = Vec::new();
        let mut clickable = Vec::new();
        if let Some(entries) = value.get("rows").and_then(Value::as_array) {
            for entry in entries {
                if let Some(text) = entry.as_str() {
                    rows.push(text.to_owned());
                    clickable.push(false);
                } else {
                    rows.push(
                        entry.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    );
                    clickable.push(entry.get("click").and_then(Value::as_bool).unwrap_or(false));
                }
            }
        }
        Self { title, rows, clickable }
    }
}

/// One runtime plugin → TUI UI request (drained between frames).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiRequest {
    /// `ctx.ui.toast.show({title?, message, variant?, duration?,
    /// sessionID?})` — the TUI reduces it through the existing
    /// `tui.toast.show` event path (the toast queue). G-475: the
    /// sessionID arm (api.tsx:108-131) crosses raw — the REDUCER
    /// decides plain-vs-action (it owns the route + the session
    /// table; the guest cannot).
    Toast {
        plugin_id: String,
        title: Option<String>,
        message: String,
        variant: String,
        duration_ms: u64,
        /// `options.sessionID` (api.tsx:277 — when this session's
        /// family is not open, the toast offers to open it).
        session_id: Option<String>,
    },
    /// `ctx.ui.sidebar.setSection({title, rows})` — one section per
    /// plugin (latest wins); `removeSection` sends empty rows. The
    /// v2 TUI-plugin surface (beta.13 bug 1) lands here too, with
    /// per-row clickability (the plugin's own `onMouseUp` handlers,
    /// addressable through [`crate::PluginHost::click_sidebar_row`]).
    SidebarSection {
        plugin_id: String,
        title: String,
        rows: Vec<String>,
        /// Per-row clickability (same length as `rows`; shorter means
        /// the tail is plain text).
        clickable: Vec<bool>,
    },
    /// The plugin bridge's navigation ask (`route.navigate("session",
    /// {sessionID})` from a plugin row's activate handler — beta.13
    /// bug 2's plugin-world path).
    NavigateToSession { session_id: String },
    /// G-475 bug2 — `ctx.ui.router.navigate({type: "home"})`
    /// (api.tsx:219-225 → host.route.navigate(destination), the
    /// Destination union's home arm — route.tsx:7-12). The port's
    /// bridge arm was missing entirely.
    NavigateHome,
    /// `ctx.ui.footer.setHint({text})` — one hint segment per plugin
    /// (latest wins; empty text removes it).
    FooterHint { plugin_id: String, text: String },
    /// `ctx.ui.dialog.open(spec)` — the fully-built dialog content
    /// (the plugin built it synchronously; the TUI opens it natively).
    DialogOpen { plugin_id: String, spec: PluginDialogSpec },
    /// `ctx.ui.router.navigate({type: "plugin", name, data?})` (the
    /// plugin-route navigation ask, api.tsx:211-224): the plugin's
    /// named page (registered through `ctx.ui.router.register`)
    /// becomes the visible screen.
    NavigateToPluginRoute { plugin_id: String, name: String, data: Option<Value> },
    /// TUI-25: `ctx.ui.alert({title, message})` (api.tsx:344-349) —
    /// the generic alert opens natively; the guest's promise settles
    /// when the TUI resolves (confirm or any close → null) through
    /// [`crate::PluginHost::resolve_ui_request`]. `request_id` is the
    /// GUEST's jsonrpc request id (the deferred reply's key).
    Alert { request_id: u64, plugin_id: String, title: String, message: String },
    /// TUI-25: `ctx.ui.confirm({title, message, label?})`
    /// (api.tsx:350-366) — resolves `true`/`false` (the active pill)
    /// or null (an unresolved close).
    Confirm {
        request_id: u64,
        plugin_id: String,
        title: String,
        message: String,
        confirm_label: Option<String>,
        cancel_label: Option<String>,
    },
    /// G-237: `ctx.ui.prompt({title, description?, placeholder?,
    /// value?})` (api.tsx:367-386) — the generic text-input prompt;
    /// resolves the typed string on submit, null on any unresolved
    /// close (`settle` over the same deferred-reply seam as
    /// alert/confirm).
    Prompt {
        request_id: u64,
        plugin_id: String,
        title: String,
        description: Option<String>,
        placeholder: Option<String>,
        value: Option<String>,
    },
    /// G-239/G-331: `ctx.ui.panel.open(name, {presentation?})` (the
    /// v2 UI face, context.ts:501-508) — the DEFERRED-reply seam
    /// (alert/confirm/prompt's pattern): the TUI applies the open and
    /// the reply resolves `true`/`false` (false when the TUI is not
    /// on a session route or the registry is inactive). `request_id`
    /// is the GUEST's jsonrpc request id.
    PanelOpen {
        request_id: u64,
        plugin_id: String,
        name: String,
        /// `true` when the plugin requested `presentation:
        /// "fullscreen"` (the default is the docked "panel").
        fullscreen: bool,
    },
    /// G-239/G-331: `ctx.ui.panel.close()` — owner-scoped (only the
    /// owning plugin's panel closes), fire-and-forget like the toast
    /// queue arm.
    PanelClose { plugin_id: String },
    /// G-475 — `ctx.ui.dialog.show(render, onClose?)` (api.tsx:334-336
    /// → dialog.replace): the guest flattened the JSX render into the
    /// native spec; the TUI opens it. `request_id` settles the guest's
    /// `onClose` (any close → `null`, the deferred-reply seam
    /// alert/confirm/prompt already use).
    DialogShow { request_id: u64, plugin_id: String, spec: PluginDialogSpec },
    /// G-475 — `ctx.ui.dialog.set({size?, centered?})`
    /// (api.tsx:337-340 → setSize/setCentered): mutates the ACTIVE
    /// dialog's presentation.
    DialogSet { plugin_id: String, size: Option<String>, centered: bool },
    /// G-475 — `ctx.ui.dialog.clear()` (api.tsx:341-343 →
    /// dialog.clear): closes the active dialog.
    DialogClear { plugin_id: String },
    /// G-475 — `ctx.ui.dialog.select(options)` (api.tsx:387-427):
    /// the promise resolves the selected option's value (any close →
    /// null) through the deferred-reply seam. The search arm is the
    /// native filter (the guest's `search(query, options)` closure
    /// cannot cross per-keystroke — bounded projection). The actions
    /// ride as [`PluginSelectAction`] rows; triggering one asks the
    /// guest back through `dialog.action` (the `click_sidebar_row`
    /// pattern) and does NOT close the dialog.
    DialogSelect {
        request_id: u64,
        plugin_id: String,
        title: String,
        placeholder: Option<String>,
        options: Vec<PluginDialogOption>,
        current: Option<String>,
        actions: Vec<PluginSelectAction>,
    },
    /// G-475 — `ctx.ui.tabs.{open,focus,move,close}` (api.tsx:245-281
    /// → host.sessionTabs.*): the tab mutations, answered by the
    /// store's own tab seams (`open_tab`/`select_tab`/`move_tab`/
    /// `close_tab`).
    TabsOp { plugin_id: String, op: TabsOpKind, session_id: Option<String>, index: usize },
    /// G-475 — `ctx.ui.model.variant.set(variant)` (api.tsx:288-296 →
    /// host.local.model.variant.set): selects a variant of the
    /// current model (`None` = the model default).
    ModelVariantSet { plugin_id: String, variant: Option<String> },
}

/// The `ctx.ui.tabs` mutation shapes (api.tsx:245-281).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabsOpKind {
    /// `tabs.open(sessionID)` — open without focusing.
    Open,
    /// `tabs.focus(sessionID)` — open + focus.
    Focus,
    /// `tabs.move(sessionID, index)`.
    Move,
    /// `tabs.close(sessionID?)` — the active tab when omitted.
    Close,
}

/// G-513 — the live route view pushed to the guest (`tui.route`):
/// the REF's `Route` shape (plugin/tui/context.ts:142-152) — home,
/// session (`sessionID`), or an open plugin route page
/// (`id`/`name`/`data?`). `ctx.ui.router.current()` returns it
/// verbatim (api.tsx:226-228 `host.route.data`).
#[derive(Debug, Clone, PartialEq)]
pub enum TuiRouteView {
    Home,
    Session {
        session_id: String,
    },
    Plugin {
        plugin_id: String,
        name: String,
        data: Option<Value>,
    },
}

impl TuiRouteView {
    /// The `tui.route` notification params (the wire shape the guest
    /// builds its `router.current()` view from).
    #[must_use]
    pub fn notification_params(&self) -> Value {
        match self {
            Self::Home => json!({ "type": "home" }),
            Self::Session { session_id } => json!({ "type": "session", "sessionID": session_id }),
            Self::Plugin { plugin_id, name, data } => json!({
                "type": "plugin",
                "id": plugin_id,
                "name": name,
                "data": data.clone().unwrap_or(Value::Null),
            }),
        }
    }
}

/// One footer action of a plugin select dialog (api.tsx:407-417's
/// `DialogSelectAction` projection: the trigger routes back through
/// the guest's `dialog.action` dispatch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSelectAction {
    /// The keybinding that triggers the action (e.g. "ctrl+d").
    /// G-512: bind-less actions cross (the ref's `shownActions`
    /// keeps them, dialog-select.tsx:154-155 — they carry no footer
    /// label, so `visibleActions`'s label filter drops the row and
    /// they stay untriggerable, exactly like the ref).
    pub bind: Option<String>,
    /// The row title.
    pub title: String,
    /// `left` | `right` (the footer side; default left).
    pub side: String,
    /// `true` when the action needs a selected option
    /// (`selection: "required"`); `"none"` actions set this false.
    pub selection_required: bool,
}

/// Parse the guest's `ui.dialog.select` payload into the native
/// select spec (title/placeholder/options/current/actions — the
/// same bounded decoding [`PluginDialogSpec::from_json`] applies to
/// option rows).
#[must_use]
pub fn parse_select_spec(
    value: &Value,
) -> (String, Option<String>, Vec<PluginDialogOption>, Option<String>, Vec<PluginSelectAction>) {
    let title = value.get("title").and_then(Value::as_str).unwrap_or_default().to_owned();
    let placeholder = value
        .get("placeholder")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned);
    let options: Vec<PluginDialogOption> = value
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter(|option| {
                    option.get("title").and_then(Value::as_str).is_some()
                        && option.get("value").is_some()
                })
                .map(|option| PluginDialogOption {
                    title: option
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    value: option
                        .get("value")
                        .map(|value| match value {
                            Value::String(text) => text.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default(),
                    description: option
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    category: option
                        .get("category")
                        .and_then(Value::as_str)
                        .filter(|category| !category.is_empty())
                        .map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default();
    let current = value.get("current").map(|value| match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    });
    let actions: Vec<PluginSelectAction> = value
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                // G-512 — bind-less actions cross (the ref's
                // shownActions keeps them; the title is the only
                // required field); only title-less rows drop.
                .filter(|action| action.get("title").and_then(Value::as_str).is_some())
                .map(|action| PluginSelectAction {
                    bind: action
                        .get("bind")
                        .and_then(Value::as_str)
                        .filter(|bind| !bind.is_empty())
                        .map(str::to_owned),
                    title: action
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    side: action.get("side").and_then(Value::as_str).unwrap_or("left").to_owned(),
                    selection_required: action.get("selection").and_then(Value::as_str)
                        != Some("none"),
                })
                .collect()
        })
        .unwrap_or_default();
    (title, placeholder, options, current, actions)
}

impl UiRequest {
    /// The originating plugin (every request carries provenance).
    #[must_use]
    pub fn plugin_id(&self) -> &str {
        match self {
            UiRequest::Toast { plugin_id, .. }
            | UiRequest::SidebarSection { plugin_id, .. }
            | UiRequest::FooterHint { plugin_id, .. }
            | UiRequest::DialogOpen { plugin_id, .. }
            | UiRequest::NavigateToPluginRoute { plugin_id, .. }
            | UiRequest::Alert { plugin_id, .. }
            | UiRequest::Confirm { plugin_id, .. }
            | UiRequest::Prompt { plugin_id, .. }
            | UiRequest::PanelOpen { plugin_id, .. }
            | UiRequest::PanelClose { plugin_id }
            | UiRequest::DialogShow { plugin_id, .. }
            | UiRequest::DialogSet { plugin_id, .. }
            | UiRequest::DialogClear { plugin_id }
            | UiRequest::DialogSelect { plugin_id, .. }
            | UiRequest::TabsOp { plugin_id, .. }
            | UiRequest::ModelVariantSet { plugin_id, .. } => plugin_id,
            UiRequest::NavigateToSession { .. } | UiRequest::NavigateHome => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dialog_spec_parses_the_guest_shape() {
        let spec = PluginDialogSpec::from_json(&json!({
            "title": "Pick a preset",
            "rows": ["row one", "row two"],
            "options": [
                { "title": "Zhipu", "value": "preset.zhipu", "description": "own/glm" },
                { "title": "missing value" },
            ]
        }));
        assert_eq!(spec.title, "Pick a preset");
        assert_eq!(spec.rows, ["row one", "row two"]);
        assert_eq!(spec.options.len(), 1, "shape-invalid options drop");
        assert_eq!(spec.options[0].value, "preset.zhipu");
        assert_eq!(spec.options[0].description, "own/glm");
    }

    #[test]
    fn dialog_spec_degrades_on_foreign_shapes() {
        let spec = PluginDialogSpec::from_json(&json!("nonsense"));
        assert_eq!(spec, PluginDialogSpec::default());
    }
}
