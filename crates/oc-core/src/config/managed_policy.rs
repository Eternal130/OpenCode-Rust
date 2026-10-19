//! Port of `packages/core/src/managed-policy.ts` + the evaluation half
//! of `packages/core/src/config/plugin/policy.ts` (CONFIG-C7).
//!
//! v2's policy runtime has three statement sources, merged in one
//! order (policy.ts:26-40):
//!
//! 1. the AUTHORED config documents' `experimental.policies`, documents
//!    reversed so the user-global document outranks the repository's;
//! 2. the connected Console's organization statements, which follow
//!    every authored one and have the final say;
//! 3. v1 migration products, which land in the same
//!    `experimental.policies` slot through config normalization.
//!
//! Each authored statement carries the message
//! `Blocked by configuration policy`; organization statements carry
//! `Blocked by {organization}'s policy` (or `Blocked by your
//! organization's policy` when the connection has no name).
//!
//! Two execution surfaces consume the merged list:
//!
//! * `provider.use` deny — the provider is pruned from the location's
//!   bindings and model resolution (`PolicyModelResolver`, the
//!   provider catalog transform equivalent);
//! * `permission` deny — the permission engine's evaluate hook denies
//!   matching `{action}:{resource}` requests with the policy message.

use oc_schema::config::policy::{Action, Effect, Policy};
use oc_schema::config::Document;
use oc_util::wildcard;
use std::sync::Arc;

/// One runtime policy statement — the wire [`Policy`] plus the denial
/// message its source contributes (policy.ts:31-40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub action: Action,
    pub resource: String,
    pub effect: Effect,
    /// `Blocked by configuration policy` (authored) or
    /// `Blocked by {organization}'s policy` (Console).
    pub message: String,
}

impl Statement {
    fn from_policy(policy: &Policy, message: &str) -> Self {
        Self {
            action: policy.action,
            resource: policy.resource.clone(),
            effect: policy.effect,
            message: message.to_owned(),
        }
    }
}

/// The Console-side managed state (managed-policy.ts:9-32): the
/// organization statements the connected Console compiled for whoever
/// it authenticated, plus the organization name for denial messages.
/// `set` REPLACES the whole state — statements never merge across
/// connections (a credential switch, disconnect, or 404 clears them).
#[derive(Debug, Default)]
pub struct ManagedPolicy {
    inner: parking_lot::RwLock<ManagedState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedState {
    pub statements: Vec<Policy>,
    pub organization: Option<String>,
}

impl ManagedPolicy {
    #[must_use]
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The current state (`current` — synchronous so catalog transforms
    /// can consult the statements while they run).
    #[must_use]
    pub fn current(&self) -> ManagedState {
        self.inner.read().clone()
    }

    /// Replace the whole state (`set`).
    pub fn set(&self, state: ManagedState) {
        *self.inner.write() = state;
    }
}

/// The merged policy list (policy.ts:26-40): authored documents
/// REVERSED (user-global outranks repository) with the configuration
/// message, then the Console's organization statements with the final
/// say and the organization message.
#[must_use]
pub fn policies(managed: &ManagedPolicy, documents: &[Document]) -> Vec<Statement> {
    let mut merged: Vec<Statement> = documents
        .iter()
        .rev()
        .flat_map(|document| {
            document
                .info
                .experimental
                .as_ref()
                .and_then(|experimental| experimental.policies.as_ref())
                .into_iter()
                .flatten()
                .map(|policy| Statement::from_policy(policy, "Blocked by configuration policy"))
                .collect::<Vec<_>>()
        })
        .collect();
    let state = managed.current();
    if !state.statements.is_empty() {
        let message = state.organization.as_deref().map_or_else(
            || "Blocked by your organization's policy".to_owned(),
            |organization| format!("Blocked by {organization}'s policy"),
        );
        merged
            .extend(state.statements.iter().map(|policy| Statement::from_policy(policy, &message)));
    }
    merged
}

/// The `provider.use` deny verdict for one provider id (policy.ts:44-52
/// — `findLast` over the merged list, `Wildcard.match(providerID,
/// policy.resource)`): the LAST matching statement wins; a later
/// `allow` overrides an earlier `deny`.
#[must_use]
pub fn provider_denied<'a>(policies: &'a [Statement], provider_id: &str) -> Option<&'a Statement> {
    policies
        .iter()
        .rev()
        .find(|statement| {
            statement.action == Action::ProviderUse
                && wildcard::matches(provider_id, &statement.resource)
        })
        .filter(|statement| statement.effect == Effect::Deny)
}

/// The `permission` deny verdict for one request (policy.ts:54-67):
/// each resource pairs with the request's action as
/// `{action}:{resource}`; the LAST matching statement per resource is
/// consulted and the FIRST denied verdict across the resources wins.
#[must_use]
pub fn permission_denied<'a>(
    policies: &'a [Statement],
    action: &str,
    resources: &[String],
) -> Option<&'a Statement> {
    resources.iter().find_map(|resource| {
        policies
            .iter()
            .rev()
            .find(|statement| {
                statement.action == Action::Permission
                    && wildcard::matches(&format!("{action}:{resource}"), &statement.resource)
            })
            .filter(|statement| statement.effect == Effect::Deny)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(action: Action, resource: &str, effect: Effect) -> Policy {
        Policy { action, resource: resource.to_owned(), effect }
    }

    fn document(policies: Vec<Policy>) -> Document {
        Document {
            info: oc_schema::config::Info {
                experimental: Some(oc_schema::config::Experimental {
                    policies: Some(policies),
                    ..Default::default()
                }),
                ..Default::default()
            },
            path: None,
        }
    }

    #[test]
    fn authored_documents_merge_reversed_with_console_last() {
        let managed = ManagedPolicy::shared();
        managed.set(ManagedState {
            statements: vec![policy(Action::ProviderUse, "console-*", Effect::Deny)],
            organization: Some("Acme".to_owned()),
        });
        let merged = policies(
            &managed,
            &[
                document(vec![policy(Action::Permission, "edit:src/*", Effect::Allow)]),
                document(vec![policy(Action::ProviderUse, "repo-*", Effect::Deny)]),
            ],
        );
        // Project document first (reversed), then global, then console.
        let resources: Vec<&str> = merged.iter().map(|s| s.resource.as_str()).collect();
        assert_eq!(resources, ["repo-*", "edit:src/*", "console-*"]);
        assert_eq!(merged[0].message, "Blocked by configuration policy");
        assert_eq!(merged[2].message, "Blocked by Acme's policy");
        // No organization name: the generic message.
        managed.set(ManagedState {
            statements: vec![policy(Action::Permission, "bash:*", Effect::Deny)],
            organization: None,
        });
        let merged = policies(&managed, &[]);
        assert_eq!(merged[0].message, "Blocked by your organization's policy");
    }

    #[test]
    fn provider_deny_last_match_with_wildcards() {
        let statements = vec![
            Statement::from_policy(&policy(Action::ProviderUse, "open*", Effect::Deny), "m"),
            Statement::from_policy(&policy(Action::ProviderUse, "openai", Effect::Allow), "m"),
        ];
        // The later allow overrides the earlier deny (findLast).
        assert!(provider_denied(&statements, "openai").is_none());
        // The wildcard still catches siblings.
        let denied = provider_denied(&statements, "opensub").expect("denied");
        assert_eq!(denied.resource, "open*");
        // permission statements never prune providers.
        let statements = vec![Statement::from_policy(
            &policy(Action::Permission, "provider.use", Effect::Deny),
            "m",
        )];
        assert!(provider_denied(&statements, "provider.use").is_none());
    }

    #[test]
    fn permission_deny_pairs_action_with_resource() {
        let statements = vec![Statement::from_policy(
            &policy(Action::Permission, "edit:secret*", Effect::Deny),
            "no secrets",
        )];
        let denied = permission_denied(&statements, "edit", &["src/main.rs".to_owned()]).is_none();
        assert!(denied, "non-matching resource is untouched");
        let denied =
            permission_denied(&statements, "edit", &["secret/keys.rs".to_owned()]).expect("denied");
        assert_eq!(denied.message, "no secrets");
        // A different action never matches the same resource.
        assert!(permission_denied(&statements, "bash", &["secret/keys.rs".to_owned()]).is_none());
        // ANY resource denying denies the whole request (first find).
        let denied = permission_denied(
            &statements,
            "edit",
            &["src/a.rs".to_owned(), "secret/b.rs".to_owned()],
        );
        assert!(denied.is_some());
    }
}
