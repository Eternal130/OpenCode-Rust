//! operationId -> client namespace mapping, mirroring the v2 SDK method
//! groups (server, debug, migration, location, agent, plugin, session,
//! message, model, generate, provider, integration, websearch, credential,
//! permission, form, fs, shell, mcp, reference, project, worktree, vcs,
//! config, skill, rpc, event, pty, persistent_pty — plus `command`).

use crate::naming::snake_ident;

/// Returns (namespace module path, method name inside the namespace).
pub fn classify(operation_id: &str) -> (Vec<String>, String) {
    let segments: Vec<&str> = operation_id.split('.').collect();
    let (module, tail): (Vec<&str>, &[&str]) = match segments.as_slice() {
        ["server"] => (vec!["server"], &[]),
        ["server", "experimental", "persistentPty", rest @ ..] => (vec!["persistent_pty"], rest),
        ["persistentPty", rest @ ..] => (vec!["persistent_pty"], rest),
        ["server", rest @ ..] => (vec!["server"], rest),
        ["debug", "location", rest @ ..] => (vec!["debug", "location"], rest),
        ["debug", rest @ ..] => (vec!["debug"], rest),
        ["experimental", "config", rest @ ..] => (vec!["config"], rest),
        ["experimental", "fs", rest @ ..] => (vec!["fs"], rest),
        ["experimental", "generate", rest @ ..] => (vec!["generate"], rest),
        ["experimental", "integration", rest @ ..] => (vec!["integration"], rest),
        ["experimental", "mcp", rest @ ..] => (vec!["mcp"], rest),
        ["experimental", "migration", "v1", rest @ ..] => (vec!["migration"], rest),
        ["experimental", "session", "instructions", rest @ ..] => {
            (vec!["session", "instructions"], rest)
        }
        ["experimental", "session", rest @ ..] => (vec!["session"], rest),
        ["session", "message", rest @ ..] => (vec!["message"], rest),
        ["session", "inbox", rest @ ..] => (vec!["session", "inbox"], rest),
        ["session", "form", rest @ ..] => (vec!["session", "form"], rest),
        ["session", "revert", rest @ ..] => (vec!["session", "revert"], rest),
        ["session", "instructions", rest @ ..] => (vec!["session", "instructions"], rest),
        ["session", rest @ ..] => (vec!["session"], rest),
        ["integration", "wellknown", rest @ ..] => (vec!["integration", "wellknown"], rest),
        ["integration", "connect", rest @ ..] => (vec!["integration", "connect"], rest),
        ["integration", "oauth", rest @ ..] => (vec!["integration", "oauth"], rest),
        ["integration", "command", rest @ ..] => (vec!["integration", "command"], rest),
        ["integration", rest @ ..] => (vec!["integration"], rest),
        ["permission", "request", rest @ ..] => (vec!["permission", "request"], rest),
        ["permission", "saved", rest @ ..] => (vec!["permission", "saved"], rest),
        ["permission", rest @ ..] => (vec!["permission"], rest),
        ["mcp", "resource", rest @ ..] => (vec!["mcp", "resource"], rest),
        ["pty", "connect", "token"] => (vec!["pty"], &["connect_token"]),
        ["vcs", "branch", rest @ ..] => (vec!["vcs", "branch"], rest),
        ["websearch", rest @ ..] => (vec!["websearch"], rest),
        [first, rest @ ..] => (vec![first], rest),
        [] => (vec!["misc"], &[]),
    };
    let method = if tail.is_empty() {
        snake_ident(segments.last().copied().unwrap_or("call"))
    } else {
        tail.iter().map(|s| snake_ident(s)).collect::<Vec<_>>().join("_")
    };
    (module.into_iter().map(str::to_owned).collect(), method)
}

#[cfg(test)]
mod tests {
    use super::classify;

    fn parts(operation_id: &str) -> (String, String) {
        let (module, method) = classify(operation_id);
        (module.join("::"), method)
    }

    #[test]
    fn classification() {
        assert_eq!(parts("server.info"), ("server".to_owned(), "info".to_owned()));
        assert_eq!(
            parts("server.experimental.persistentPty.handoff"),
            ("persistent_pty".to_owned(), "handoff".to_owned())
        );
        assert_eq!(parts("session.message.list"), ("message".to_owned(), "list".to_owned()));
        assert_eq!(
            parts("session.revert.clear"),
            ("session::revert".to_owned(), "clear".to_owned())
        );
        assert_eq!(parts("experimental.fs.write"), ("fs".to_owned(), "write".to_owned()));
        assert_eq!(
            parts("experimental.migration.v1.status"),
            ("migration".to_owned(), "status".to_owned())
        );
        assert_eq!(parts("pty.connect.token"), ("pty".to_owned(), "connect_token".to_owned()));
        assert_eq!(
            parts("integration.oauth.complete"),
            ("integration::oauth".to_owned(), "complete".to_owned())
        );
    }
}
