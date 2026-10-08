//! Stable identifiers (port of `src/schema/ids.ts`).
//!
//! The TS side brands strings (`ModelID`, `ProviderID`); in Rust these are
//! transparent newtypes so they stay `serde`-transparent on the wire.

use serde::{Deserialize, Serialize};

/// Stable string identifier for a protocol implementation.
pub type ProtocolId = String;

/// Stable string identifier for the runnable route.
pub type RouteId = String;

/// Stable string identifier for the model being addressed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct ModelId(pub String);

impl ModelId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable string identifier for the provider (facade) owning a route.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Provider-assigned response identifier.
pub type ResponseId = String;

/// Identifier of one streamed content block (text/reasoning fragment).
pub type ContentBlockId = String;

/// Identifier of one tool call.
pub type ToolCallId = String;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_transparent_on_the_wire() {
        assert_eq!(serde_json::to_string(&ModelId::new("m1")).unwrap(), "\"m1\"");
        assert_eq!(serde_json::from_str::<ProviderId>("\"p1\"").unwrap(), ProviderId::new("p1"));
    }
}
