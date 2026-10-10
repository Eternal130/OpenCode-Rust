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
    /// plugin (latest wins); `removeSection` sends empty rows.
    SidebarSection { plugin_id: String, title: String, rows: Vec<String> },
    /// `ctx.ui.footer.setHint({text})` — one hint segment per plugin
    /// (latest wins; empty text removes it).
    FooterHint { plugin_id: String, text: String },
    /// `ctx.ui.dialog.open(spec)` — the fully-built dialog content
    /// (the plugin built it synchronously; the TUI opens it natively).
    DialogOpen { plugin_id: String, spec: PluginDialogSpec },
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
