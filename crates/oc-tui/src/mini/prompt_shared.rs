//! The prompt-command surface — port of
//! `packages/tui/src/mini/prompt.shared.ts` (the command predicates)
//! plus the history ring (`prompt.shared.ts:10-70`).

/// `HISTORY_LIMIT` (`prompt.shared.ts:10`).
pub const HISTORY_LIMIT: usize = 200;

/// `EXIT_COMMANDS` (`prompt.shared.ts:72`).
pub const EXIT_COMMANDS: &[&str] = &["exit", "quit", "q"];

/// `isExitCommand` (`prompt.shared.ts:74-77`): `:q` or `/exit`
/// `/quit` `/q`.
#[must_use]
pub fn is_exit_command(input: &str) -> bool {
    let text = input.trim().to_lowercase();
    text == ":q" || EXIT_COMMANDS.iter().any(|name| text == format!("/{name}"))
}

/// `isNewCommand` (`prompt.shared.ts:79`).
#[must_use]
pub fn is_new_command(input: &str) -> bool {
    input.trim().to_lowercase() == "/new"
}

/// `isCompactCommand` (`prompt.shared.ts:83`).
#[must_use]
pub fn is_compact_command(input: &str) -> bool {
    input.trim().to_lowercase() == "/compact"
}

/// The prompt history ring (`PromptHistoryState`,
/// `prompt.shared.ts:17-22`): stores past prompts, `browse` tracks the
/// up-navigation position (`None` = not browsing), `draft` holds the
/// in-progress text saved when navigation begins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptHistory {
    pub items: Vec<String>,
    pub browse: Option<usize>,
    pub draft: String,
}

impl PromptHistory {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push an accepted prompt (`promptCopy` + append, capped at
    /// `HISTORY_LIMIT`).
    pub fn push(&mut self, text: &str) {
        if let Some(position) = self.items.iter().position(|item| item == text) {
            self.items.remove(position);
        }
        self.items.push(text.to_owned());
        if self.items.len() > HISTORY_LIMIT {
            self.items.remove(0);
        }
        self.browse = None;
        self.draft.clear();
    }

    /// Arrow-up at cursor 0: save the draft once, then walk backwards
    /// through history (`prompt.shared.ts` navigation helpers).
    #[must_use]
    pub fn up(&mut self, current: &str) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        self.browse = match self.browse {
            None => {
                self.draft.clear();
                self.draft.push_str(current);
                Some(self.items.len() - 1)
            }
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
        self.items.get(self.browse?).cloned()
    }

    /// Arrow-down: walk forward; past the end restores the draft.
    #[must_use]
    pub fn down(&mut self) -> Option<String> {
        let index = self.browse?;
        if index + 1 >= self.items.len() {
            self.browse = None;
            return Some(std::mem::take(&mut self.draft));
        }
        self.browse = Some(index + 1);
        self.items.get(index + 1).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_commands_match_exactly() {
        assert!(is_exit_command("/exit"));
        assert!(is_exit_command("  /Quit "));
        assert!(is_exit_command(":q"));
        assert!(is_exit_command("/q"));
        assert!(!is_exit_command("q"));
        assert!(!is_exit_command("/exit now"));
        assert!(!is_exit_command("exit"));
    }

    #[test]
    fn new_and_compact_commands() {
        assert!(is_new_command("/new"));
        assert!(is_new_command(" /NEW "));
        assert!(!is_new_command("/newest"));
        assert!(is_compact_command("/compact"));
        assert!(!is_compact_command("/compact/x"));
    }

    #[test]
    fn history_ring_walks_and_restores_draft() {
        let mut history = PromptHistory::new();
        history.push("first");
        history.push("second");
        assert_eq!(history.up("typing").as_deref(), Some("second"));
        assert_eq!(history.up("").as_deref(), Some("first"));
        assert_eq!(history.up("").as_deref(), Some("first"), "clamps at oldest");
        assert_eq!(history.down().as_deref(), Some("second"));
        assert_eq!(history.down().as_deref(), Some("typing"), "past the end restores draft");
        assert_eq!(history.browse, None);
    }
}
