//! Command palette skeleton: the palette-visible commands from the
//! keymap registry, grouped, with their leader-chord binding displays —
//! the data surface `ctrl+p` opens (`command-palette.tsx` +
//! `ui/dialog-select` foundation).
//!
//! The dialog-select frame (fuzzy filter, selection, scrolling, exact
//! dialog geometry) is deferred to 7.2 — this wave ships the registry
//! listing + binding formatting the dialog renders from.

use crate::keymap::{CommandDef, CommandGroup, Keymap};

/// One palette row: title, group, and formatted bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub title: String,
    pub group: &'static str,
    pub command: &'static str,
    pub bindings: Vec<String>,
}

/// The palette listing: palette-visible commands in registry order,
/// carrying their default binding displays ("ctrl+x n").
#[must_use]
pub fn palette_entries(keymap: &Keymap) -> Vec<PaletteEntry> {
    keymap
        .palette_commands()
        .iter()
        .map(|def: &&CommandDef| PaletteEntry {
            title: title_of(def),
            group: def.group.map_or_else(|| "Other", CommandGroup::as_str),
            command: def.command,
            bindings: keymap.bindings_for(def.command),
        })
        .collect()
}

/// Palette title: the command description fallback mirrors the
/// reference (`title ?? id`).
fn title_of(def: &CommandDef) -> String {
    def.description.trim_end_matches(" the session").trim_end_matches(" the application").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_lists_grouped_commands_with_bindings() {
        let keymap = Keymap::new();
        let entries = palette_entries(&keymap);
        let new_session = entries
            .iter()
            .find(|entry| entry.command == "session.new")
            .expect("session.new in palette");
        assert_eq!(new_session.group, "Session");
        assert_eq!(new_session.bindings, vec!["ctrl+x n".to_owned()]);
        let model = entries.iter().find(|entry| entry.command == "model.list").expect("model.list");
        assert_eq!(model.group, "Agent");
        // Every palette entry resolves back to a registered command.
        for entry in &entries {
            assert!(keymap.command_def(entry.command).is_some());
        }
    }
}
