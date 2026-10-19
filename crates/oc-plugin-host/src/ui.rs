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

use serde_json::Value;

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
                                && option.get("value").and_then(Value::as_str).is_some()
                        })
                        .map(|option| PluginDialogOption {
                            title: option
                                .get("title")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            value: option
                                .get("value")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
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
    /// `ctx.ui.toast.show({title?, message, variant?, duration?})` —
    /// the TUI reduces it through the existing `tui.toast.show` event
    /// path (the toast queue).
    Toast {
        plugin_id: String,
        title: Option<String>,
        message: String,
        variant: String,
        duration_ms: u64,
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
            | UiRequest::NavigateToPluginRoute { plugin_id, .. } => plugin_id,
            UiRequest::NavigateToSession { .. } => "",
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
