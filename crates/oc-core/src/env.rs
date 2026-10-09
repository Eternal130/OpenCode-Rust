//! Port of `packages/core/src/session/environment.ts` — per-session
//! persistent environment variables merged over the process environment for
//! every shell invocation owned by a session (v2: non-workspace locations
//! with a valid session id).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

/// `SessionEnvironment.Variables`.
pub type Variables = HashMap<String, String>;

/// The session environment service (clone shares state).
#[derive(Clone, Default)]
pub struct SessionEnv {
    environments: Arc<RwLock<HashMap<String, Variables>>>,
}

impl SessionEnv {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `SessionEnvironment.get`.
    #[must_use]
    pub fn get(&self, session_id: &str) -> Option<Variables> {
        self.environments.read().get(session_id).cloned()
    }

    /// `SessionEnvironment.set` — replaces the stored variables.
    pub fn set(&self, session_id: &str, variables: Variables) {
        self.environments.write().insert(session_id.to_string(), variables);
    }

    /// `SessionEnvironment.clear`.
    pub fn clear(&self, session_id: &str) {
        self.environments.write().remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_stores_replaces_and_clears_per_session() {
        let env = SessionEnv::new();
        assert!(env.get("ses_1").is_none());
        env.set("ses_1", HashMap::from([("FOO".to_string(), "bar".to_string())]));
        assert_eq!(env.get("ses_1").unwrap()["FOO"], "bar");
        env.set("ses_1", HashMap::from([("BAZ".to_string(), "1".to_string())]));
        assert_eq!(env.get("ses_1").unwrap().len(), 1);
        env.clear("ses_1");
        assert!(env.get("ses_1").is_none());
    }
}
