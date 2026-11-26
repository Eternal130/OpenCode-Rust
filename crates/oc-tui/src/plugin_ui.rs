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
        /// G-475 — the sessionID arm (api.tsx:108-131): `None` = a
        /// plain toast; `Some` lets the reducer decide plain-vs-action
        /// (it owns the route + session tables).
        session_id: Option<String>,
    },
    /// One sidebar section per plugin (empty title + empty rows
    /// removes it). `clickable` marks rows carrying the plugin's own
    /// `onMouseUp` (the v2 TUI-plugin surface).
    SidebarSection { title: String, rows: Vec<String>, clickable: Vec<bool> },
    /// A plugin's navigation ask (`route.navigate("session",
    /// {sessionID})` from a plugin row's activate handler — beta.13
    /// bug 2's plugin-world path).
    NavigateToSession { session_id: String },
    /// G-475 bug2 — `ctx.ui.router.navigate({type: "home"})`
    /// (api.tsx:219-225): the home arm.
    NavigateHome,
    /// A plugin-route navigation ask (`ctx.ui.router.navigate(
    /// {type:"plugin", name, data?})` — api.tsx:211-224): the
    /// registered page becomes the visible screen (content
    /// materializes through the host's bounded `route.render`).
    NavigateToPluginRoute { name: String, data: Option<serde_json::Value> },
    /// One footer hint segment per plugin (empty text removes it).
    FooterHint { text: String },
    /// A fully-built dialog open (renders natively from `content`).
    DialogOpen { content: PluginDialogContent },
    /// G-475 — `ctx.ui.dialog.show(render, onClose?)` (api.tsx:334-336):
    /// the flattened content + the presentation; `request_id` settles
    /// the guest's `onClose` on any close.
    DialogShow {
        request_id: u64,
        content: PluginDialogContent,
        size: Option<String>,
        centered: bool,
    },
    /// G-475 — `ctx.ui.dialog.set({size?, centered?})`
    /// (api.tsx:337-340): the ACTIVE dialog's presentation.
    DialogSet { size: Option<String>, centered: bool },
    /// G-475 — `ctx.ui.dialog.clear()` (api.tsx:341-343).
    DialogClear,
    /// G-475 — `ctx.ui.dialog.select(options)` (api.tsx:387-427): the
    /// value promise settles through `request_id`; the actions render
    /// as footer rows (their triggers stage the guest dispatch).
    DialogSelect {
        request_id: u64,
        title: String,
        placeholder: Option<String>,
        options: Vec<PluginDialogOption>,
        current: Option<String>,
        actions: Vec<oc_plugin_host::PluginSelectAction>,
    },
    /// TUI-25: `ctx.ui.alert({title, message})` — the generic alert
    /// opens natively; `request_id` settles the guest's promise
    /// (confirm or any close → null) through the plugin pump.
    Alert { request_id: u64, title: String, message: String },
    /// TUI-25: `ctx.ui.confirm({title, message, label?})` — resolves
    /// true/false (the active pill) or null (an unresolved close).
    Confirm {
        request_id: u64,
        title: String,
        message: String,
        confirm_label: Option<String>,
        cancel_label: Option<String>,
    },
    /// G-237: `ctx.ui.prompt({title, description?, placeholder?,
    /// value?})` — the generic text-input prompt; resolves the typed
    /// string on submit, null on dismiss.
    Prompt {
        request_id: u64,
        title: String,
        description: Option<String>,
        placeholder: Option<String>,
        value: Option<String>,
    },
    /// G-239/G-331: `ctx.ui.panel.open(name, {presentation?})` — the
    /// session panel opens on the current session route; the guest's
    /// deferred promise resolves with the route-check boolean
    /// (`request_id` rides the same resolution queue as the dialogs).
    PanelOpen { request_id: u64, name: String, fullscreen: bool },
    /// G-239/G-331: `ctx.ui.panel.close()` — the owner-scoped release
    /// (only the requesting plugin's panel closes).
    PanelClose,
    /// G-475 — `ctx.ui.tabs.{open,focus,move,close}` (api.tsx:245-281
    /// → host.sessionTabs.*): the store's tab seams answer.
    TabsOp { op: oc_plugin_host::TabsOpKind, session_id: Option<String>, index: usize },
    /// G-475 — `ctx.ui.model.variant.set(variant)` (api.tsx:288-296).
    ModelVariantSet { variant: Option<String> },
}

impl PluginUiEvent {
    #[must_use]
    pub fn toast(message: impl Into<String>) -> Self {
        Self::Toast {
            title: None,
            message: message.into(),
            variant: "info".to_owned(),
            duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
            session_id: None,
        }
    }
}

/// Apply one UI event to the store (the pump's per-drain step).
#[allow(clippy::too_many_lines)] // one arm per UI request (G-475 grew the table)
pub fn apply_plugin_ui(store: &Store, plugin_id: &str, event: &PluginUiEvent) {
    match event {
        PluginUiEvent::Toast { title, message, variant, duration_ms, session_id } => {
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
            // G-475 — the sessionID arm crosses raw (the reducer owns
            // the route/session tables and decides plain-vs-action).
            if let Some(session) = session_id {
                data.insert("sessionID".into(), serde_json::Value::String(session.clone()));
            }
            store.ingest_all([crate::data::Ingest::Event(BusEvent::new(
                "tui.toast.show",
                serde_json::Value::Object(data),
            ))]);
        }
        PluginUiEvent::SidebarSection { title, rows, clickable } => {
            // G-435-residual followup: the payload's flattened section
            // rows carry the slot column's OWN title-pad row as a
            // leading empty (the omo-slim section arrives as
            // ["","",content…] while the ref's Solid slot column
            // renders exactly ONE spacer above the first content row —
            // channel-8 plugin-panel-open@320x80 oracle). The native
            // slot origin paints that pad (the sidebar title's
            // paddingBottom), so strip ONE leading empty row or the
            // pad renders twice.
            let mut rows = rows.clone();
            if rows.first().is_some_and(String::is_empty) {
                rows.remove(0);
            }
            let section = crate::components::sidebar::SidebarSection {
                plugin_id: plugin_id.to_owned(),
                title: title.clone(),
                rows,
                clickable: clickable.clone(),
            };
            store.set_plugin_sidebar_section(&section);
        }
        PluginUiEvent::NavigateToSession { session_id } => {
            store.navigate(crate::store::Route::session(session_id.clone()));
        }
        // G-475 bug2 — the router navigate home arm (api.tsx:219-225).
        PluginUiEvent::NavigateHome => {
            store.navigate(crate::store::Route::Home);
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

                    placeholder: None,
                },
            );
        }
        // G-475 — the dialog.show face: the flattened content opens
        // natively; the request_id settles the guest's onClose on any
        // close (the deferred-reply seam).
        PluginUiEvent::DialogShow { request_id, content, size, centered } => {
            store.open_plugin_dialog_show(
                plugin_id,
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

                    placeholder: None,
                },
                *request_id,
                size.as_deref(),
                *centered,
            );
        }
        // G-475 — dialog.set: the ACTIVE dialog's presentation.
        PluginUiEvent::DialogSet { size, centered } => {
            store.set_plugin_dialog_presentation(size.as_deref(), *centered);
        }
        // G-505 — dialog.clear (api.tsx:341-343 → dialog.tsx:159-169):
        // EVERY stack entry's close semantics run (onClose fires for
        // each — here: the theme revert + the plugin request settle),
        // the stack empties, and size/centered reset with it. The
        // port's faithful `clear_dialogs()` seam.
        PluginUiEvent::DialogClear => {
            store.clear_dialogs();
        }
        // G-475 — the dialog.select face: the native select over the
        // projected options; the value promise settles on submit /
        // any close; the actions ride as the footer rows.
        PluginUiEvent::DialogSelect {
            request_id,
            title,
            placeholder,
            options,
            current,
            actions,
        } => {
            store.open_plugin_select(
                plugin_id,
                *request_id,
                title,
                placeholder.as_deref(),
                options,
                current.as_deref(),
                actions,
            );
        }
        // TUI-25: the generic alert/confirm tones open through the
        // store's dialog kinds; the guest promise's settle rides the
        // `request_id` (the close/submit hooks enqueue the resolution
        // and the plugin pump drains it back into the host).
        PluginUiEvent::Alert { request_id, title, message } => {
            store.open_alert(title, message, Some(*request_id), None);
        }
        PluginUiEvent::Confirm { request_id, title, message, confirm_label, cancel_label } => {
            store.open_confirm(
                title,
                message,
                confirm_label.as_deref().unwrap_or("confirm"),
                cancel_label.as_deref().unwrap_or("cancel"),
                Some(*request_id),
                None,
                None,
            );
        }
        // G-237: the generic prompt opens natively; the typed value /
        // null settles the guest's promise through the same
        // `request_id` queue.
        PluginUiEvent::Prompt { request_id, title, description, placeholder, value } => {
            store.open_prompt(
                title,
                description.as_deref().unwrap_or(""),
                placeholder.as_deref().unwrap_or(""),
                value.as_deref().unwrap_or(""),
                Some(*request_id),
                None,
            );
        }
        // G-239/G-331: the open applies through the store seam and
        // the route-check boolean settles the guest's deferred
        // promise through the same resolution queue the dialogs use.
        PluginUiEvent::PanelOpen { request_id, name, fullscreen } => {
            let opened = store.open_session_panel(plugin_id, name, *fullscreen);
            store.enqueue_ui_dialog_resolution(*request_id, serde_json::Value::Bool(opened));
        }
        // The owner-scoped close (`release`, panel.tsx:33-36).
        PluginUiEvent::PanelClose => {
            store.release_session_panel(plugin_id);
        }
        // G-475 — the tab mutations (api.tsx:245-281): the store's
        // own tab seams (open/select/move/close), gated exactly like
        // the ref (disabled tabs → no-op; the guest already answered
        // `false` from the pushed view).
        PluginUiEvent::TabsOp { op, session_id, index } => {
            if !store.tabs_enabled() {
                return;
            }
            match op {
                oc_plugin_host::TabsOpKind::Open => {
                    if let Some(session) = session_id {
                        store.open_tab(session, None);
                    }
                }
                oc_plugin_host::TabsOpKind::Focus => {
                    if let Some(session) = session_id {
                        store.open_tab(session, None);
                        store.select_tab(session);
                    }
                }
                oc_plugin_host::TabsOpKind::Move => {
                    if let Some(session) = session_id {
                        let target = store.session_root(session);
                        store.move_tab(&target, *index);
                    }
                }
                oc_plugin_host::TabsOpKind::Close => {
                    let target = session_id.clone().or_else(|| store.tabs().active(&store.route()));
                    if let Some(target) = target {
                        store.close_tab(Some(&target));
                    }
                }
            }
        }
        // G-475 — the model variant selection (api.tsx:288-296): the
        // composer-key routing follows the ref's local model seam.
        PluginUiEvent::ModelVariantSet { variant } => {
            let key = match store.route() {
                crate::store::Route::Session { session_id, .. } => session_id,
                crate::store::Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
            };
            if store.selection_for(&key).model.is_some() {
                store.set_current_model_variant(variant.as_deref());
            }
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
                session_id: None,
            },
        );
        let toasts = store.toasts();
        let toast = toasts.current().expect("the toast shows immediately");
        assert_eq!(toast.message, "switched");
        assert_eq!(toast.title.as_deref(), Some("Preset"));
        assert_eq!(toast.variant, "success");
        assert_eq!(toast.duration_ms, 9000);
    }

    /// G-475 — the sessionID arm: a toast for a session whose family is
    /// NOT the open route offers `Open` (the action navigates there);
    /// the open family keeps the plain toast.
    #[test]
    fn toast_session_id_arm_offers_open_for_background_sessions() {
        let store = Store::new(Runtime::new());
        // Home route: every session is background → the action arm.
        apply_plugin_ui(
            &store,
            "v21",
            &PluginUiEvent::Toast {
                title: None,
                message: "background work".into(),
                variant: "info".into(),
                duration_ms: 600_000,
                session_id: Some("ses_bg".into()),
            },
        );
        let queue = store.toasts();
        let toast = queue.current().expect("the toast shows");
        assert_eq!(toast.action_label.as_deref(), Some("Open"), "the affordance");
        assert_eq!(toast.action_session.as_deref(), Some("ses_bg"));
        // The activation carries the session target (api.tsx:126-130's
        // `run: navigate` — the app layer runs it through navigate).
        let activation = store.toast_activate().expect("an activation");
        assert_eq!(activation, crate::components::toast::ToastActivation::Session("ses_bg".into()));
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

                    placeholder: None,
                },
            },
        );
        let top = store.top_dialog().expect("the dialog opened");
        assert!(matches!(top.kind, crate::store::DialogKind::Plugin { .. }));
    }
}
