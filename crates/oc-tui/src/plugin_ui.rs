//! The plugin UI-request application seam (wave 3's slot bridge): the
//! neutral event shape the embedder converts the plugin host's drained
//! [`UiRequest`]s into, and [`apply_plugin_ui`] — the single entry
//! point that lands each one in the store. Mirrors the wave-1
//! [`crate::store::EventTap`] pattern: oc-tui defines the native
//! surface; the app layer owns the ~10-line conversion glue (oc-tui
//! has no src dependency on the host crate).
//!
//! The toast lands through the SAME bus-event path the server's
//! `tui.toast.show` uses — one reducer, one queue.

use crate::data::BusEvent;
use crate::store::{PluginDialogContent, PluginDialogOption, Store};

/// One plugin → TUI UI request, in oc-tui's native shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginUiEvent {
    Toast {
        title: Option<String>,
        message: String,
        variant: String,
        duration_ms: u64,
    },
    /// One sidebar section per plugin (empty title + empty rows
    /// removes it). `clickable` marks rows carrying the plugin's own
    /// `onMouseUp` (the v2 TUI-plugin surface).
    SidebarSection {
        title: String,
        rows: Vec<String>,
        clickable: Vec<bool>,
    },
    /// A plugin's navigation ask (`route.navigate("session",
    /// {sessionID})` from a plugin row's activate handler — beta.13
    /// bug 2's plugin-world path).
    NavigateToSession {
        session_id: String,
    },
    /// A plugin-route navigation ask (`ctx.ui.router.navigate(
    /// {type:"plugin", name, data?})` — api.tsx:211-224): the
    /// registered page becomes the visible screen (content
    /// materializes through the host's bounded `route.render`).
    NavigateToPluginRoute {
        name: String,
        data: Option<serde_json::Value>,
    },
    /// One footer hint segment per plugin (empty text removes it).
    FooterHint {
        text: String,
    },
    /// A fully-built dialog open (renders natively from `content`).
    DialogOpen {
        content: PluginDialogContent,
    },
}

impl PluginUiEvent {
    #[must_use]
    pub fn toast(message: impl Into<String>) -> Self {
        Self::Toast {
            title: None,
            message: message.into(),
            variant: "info".to_owned(),
            duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
        }
    }
}

/// Apply one UI event to the store (the pump's per-drain step).
pub fn apply_plugin_ui(store: &Store, plugin_id: &str, event: &PluginUiEvent) {
    match event {
        PluginUiEvent::Toast { title, message, variant, duration_ms } => {
            // The same reducer the server event path uses. The empty
            // event id deliberately skips replay dedup — these are
            // synthesized one-shots, never replayed frames.
            let mut data = serde_json::Map::new();
            if let Some(title) = title {
                data.insert("title".into(), serde_json::Value::String(title.clone()));
            }
            data.insert("message".into(), serde_json::Value::String(message.clone()));
            data.insert("variant".into(), serde_json::Value::String(variant.clone()));
            data.insert("duration".into(), serde_json::Value::from(*duration_ms));
            store.ingest_all([crate::data::Ingest::Event(BusEvent::new(
                "tui.toast.show",
                serde_json::Value::Object(data),
            ))]);
        }
        PluginUiEvent::SidebarSection { title, rows, clickable } => {
            let section = crate::components::sidebar::SidebarSection {
                plugin_id: plugin_id.to_owned(),
                title: title.clone(),
                rows: rows.clone(),
                clickable: clickable.clone(),
            };
            store.set_plugin_sidebar_section(&section);
        }
        PluginUiEvent::NavigateToSession { session_id } => {
            store.navigate(crate::store::Route::Session { session_id: session_id.clone() });
        }
        PluginUiEvent::NavigateToPluginRoute { name, data } => {
            store.open_plugin_route(plugin_id, name, data.clone());
        }
        PluginUiEvent::FooterHint { text } => store.set_plugin_footer_hint(plugin_id, text),
        PluginUiEvent::DialogOpen { content } => {
            let id = format!("plugin:{plugin_id}:open");
            store.open_plugin_dialog_with(
                &id,
                PluginDialogContent {
                    title: content.title.clone(),
                    rows: content.rows.clone(),
                    options: content
                        .options
                        .iter()
                        .map(|option| PluginDialogOption {
                            title: option.title.clone(),
                            value: option.value.clone(),
                            description: option.description.clone(),
                        })
                        .collect(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;

    #[test]
    fn toast_events_land_in_the_queue() {
        let store = Store::new(Runtime::new());
        apply_plugin_ui(
            &store,
            "omos",
            &PluginUiEvent::Toast {
                title: Some("Preset".into()),
                message: "switched".into(),
                variant: "success".into(),
                duration_ms: 9000,
            },
        );
        let toasts = store.toasts();
        let toast = toasts.current().expect("the toast shows immediately");
        assert_eq!(toast.message, "switched");
        assert_eq!(toast.title.as_deref(), Some("Preset"));
        assert_eq!(toast.variant, "success");
        assert_eq!(toast.duration_ms, 9000);
    }

    #[test]
    fn sidebar_and_hint_upsert_per_plugin() {
        let store = Store::new(Runtime::new());
        apply_plugin_ui(
            &store,
            "omos",
            &PluginUiEvent::SidebarSection {
                title: "Preset".into(),
                rows: vec!["zhipuai".into()],
                clickable: vec![],
            },
        );
        apply_plugin_ui(&store, "omos", &PluginUiEvent::FooterHint { text: "omo ready".into() });
        apply_plugin_ui(
            &store,
            "other",
            &PluginUiEvent::SidebarSection {
                title: "Other".into(),
                rows: vec![],
                clickable: vec![],
            },
        );
        let sections = store.plugin_sidebar_sections();
        assert_eq!(sections.len(), 2, "one section per plugin");
        assert_eq!(store.plugin_footer_hints(), ["omo ready".to_owned()]);
        // Latest wins per plugin; empty removes.
        apply_plugin_ui(
            &store,
            "omos",
            &PluginUiEvent::SidebarSection {
                title: "Preset2".into(),
                rows: vec![],
                clickable: vec![],
            },
        );
        assert_eq!(store.plugin_sidebar_sections().len(), 2);
        apply_plugin_ui(
            &store,
            "omos",
            &PluginUiEvent::SidebarSection {
                title: String::new(),
                rows: vec![],
                clickable: vec![],
            },
        );
        apply_plugin_ui(&store, "omos", &PluginUiEvent::FooterHint { text: String::new() });
        assert_eq!(store.plugin_sidebar_sections().len(), 1, "the omos section removed");
        assert_eq!(store.plugin_footer_hints(), Vec::<String>::new());
    }
    #[test]
    fn dialog_open_materializes_and_opens() {
        let store = Store::new(Runtime::new());
        apply_plugin_ui(
            &store,
            "omos",
            &PluginUiEvent::DialogOpen {
                content: PluginDialogContent {
                    title: "About".into(),
                    rows: vec!["v3.0.3".into()],
                    options: vec![],
                },
            },
        );
        let top = store.top_dialog().expect("the dialog opened");
        assert!(matches!(top.kind, crate::store::DialogKind::Plugin { .. }));
    }
}
