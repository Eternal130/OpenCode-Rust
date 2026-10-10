//! Client capability parsing (port of `acp/capabilities.ts`): the
//! three client features the agent behavior gates on.

/// The `_meta` key advertising child-session update support
/// (capabilities.ts:3).
pub const CHILD_SESSION_UPDATES: &str = "opencode/child-session-updates";

/// The parsed client capabilities (capabilities.ts:5-14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// `_meta["opencode/child-session-updates"] === true`.
    pub child_session_updates: bool,
    /// `elicitation.form` present (form elicitation supported).
    pub form_elicitation: bool,
    /// `session.compaction` present (standard compaction updates
    /// supported).
    pub compaction: bool,
}

impl Capabilities {
    /// `parse` (capabilities.ts:7-15) — `None` client capabilities
    /// parse to all-false (the pre-initialize default, agent.ts:123).
    #[must_use]
    pub fn parse(client: Option<&agent_client_protocol::schema::v1::ClientCapabilities>) -> Self {
        let Some(client) = client else { return Self::default() };
        Self {
            child_session_updates: client
                .meta
                .as_ref()
                .and_then(|meta| meta.get(CHILD_SESSION_UPDATES))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            form_elicitation: client
                .elicitation
                .as_ref()
                .is_some_and(|elicitation| elicitation.form.is_some()),
            compaction: client.session.as_ref().is_some_and(|session| session.compaction.is_some()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::ClientCapabilities;

    #[test]
    fn absent_client_capabilities_parse_to_all_false() {
        assert_eq!(Capabilities::parse(None), Capabilities::default());
    }

    #[test]
    fn form_and_compaction_and_child_updates_are_detected() {
        let capabilities = ClientCapabilities::default()
            .elicitation(
                agent_client_protocol::schema::v1::ElicitationCapabilities::new()
                    .form(agent_client_protocol::schema::v1::ElicitationFormCapabilities::new()),
            )
            .session(
                agent_client_protocol::schema::v1::ClientSessionCapabilities::new()
                    .compaction(agent_client_protocol::schema::v1::CompactionCapabilities::new()),
            );
        let capabilities = agent_client_protocol::schema::v1::InitializeRequest::new(
            agent_client_protocol::schema::ProtocolVersion::V1,
        )
        .client_capabilities(capabilities);
        let mut meta = serde_json::Map::new();
        meta.insert(CHILD_SESSION_UPDATES.to_owned(), serde_json::Value::Bool(true));
        let mut with_meta = capabilities.client_capabilities.clone();
        with_meta.meta = Some(meta);
        let parsed = Capabilities::parse(Some(&with_meta));
        assert!(parsed.form_elicitation);
        assert!(parsed.compaction);
        assert!(parsed.child_session_updates);
    }

    #[test]
    fn plain_capabilities_enable_nothing() {
        let parsed = Capabilities::parse(Some(&ClientCapabilities::default()));
        assert_eq!(parsed, Capabilities::default());
    }
}
