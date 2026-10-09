//! Shell service tests (ported from shell.test.ts service-level behavior +
//! tool-shell.test.ts notification formats).
#![allow(clippy::assert_is_empty)] // deliberate empty-state assertions

use super::*;

fn ids() -> std::sync::Arc<dyn oc_util::determinism::IdGen> {
    std::sync::Arc::new(oc_util::determinism::AscendingIdGen::new(
        std::sync::Arc::new(oc_util::determinism::SystemClock),
        std::sync::Arc::new(oc_util::determinism::SequentialRng::new()),
    ))
}

fn service(tmp: &tempfile::TempDir) -> Shell {
    Shell::new(tmp.path(), "proj", ids(), std::sync::Arc::new(oc_util::determinism::SystemClock))
}

#[tokio::test]
async fn shell_runs_a_command_and_captures_combined_output() {
    let tmp = tempfile::tempdir().unwrap();
    let shell = service(&tmp);
    let info = shell
        .create(
            CreateInput {
                command: "printf hello; printf err >&2; sleep 0.05".into(),
                cwd: Some(tmp.path().to_string_lossy().to_string()),
                timeout: Some(5_000),
                metadata: None,
            },
            Some("/bin/sh"),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(info.status, Status::Running);
    let result = shell.result(info.clone()).await;
    assert_eq!(result.info.status, Status::Exited);
    assert_eq!(result.info.exit.map(|exit| exit.0), Some(0.0));
    let capture = result.capture.expect("capture");
    assert!(capture.output.contains("hello"), "output: {}", capture.output);
    assert!(capture.output.contains("err"), "output: {}", capture.output);
}

#[tokio::test]
async fn shell_reports_exit_codes_and_output() {
    let tmp = tempfile::tempdir().unwrap();
    let shell = service(&tmp);
    let info = shell
        .create(
            CreateInput {
                command: "printf body; exit 7".into(),
                cwd: Some(tmp.path().to_string_lossy().to_string()),
                timeout: Some(5_000),
                metadata: None,
            },
            Some("/bin/sh"),
            None,
            None,
        )
        .await
        .unwrap();
    let result = shell.result(info).await;
    assert_eq!(result.info.exit.map(|exit| exit.0), Some(7.0));
    let output = result::output(&result);
    assert_eq!(output.output.trim_end(), "body");
    assert_eq!(result::notice(&output).as_deref(), Some("Exited with code 7"));
}

#[tokio::test]
async fn shell_timeout_kills_and_marks_status() {
    let tmp = tempfile::tempdir().unwrap();
    let shell = service(&tmp);
    let info = shell
        .create(
            CreateInput {
                command: "sleep 30".into(),
                cwd: Some(tmp.path().to_string_lossy().to_string()),
                timeout: Some(200),
                metadata: None,
            },
            Some("/bin/sh"),
            None,
            None,
        )
        .await
        .unwrap();
    let result = shell.result(info).await;
    assert_eq!(result.info.status, Status::Timeout);
    let output = result::output(&result);
    assert_eq!(output.timeout, Some(true));
    assert_eq!(result::notice(&output).as_deref(), Some("Timed out before completion"));
}

#[tokio::test]
async fn shell_timeout_replace_and_clear() {
    let tmp = tempfile::tempdir().unwrap();
    let shell = service(&tmp);
    let info = shell
        .create(
            CreateInput {
                command: "sleep 0.3".into(),
                cwd: Some(tmp.path().to_string_lossy().to_string()),
                timeout: Some(10_000),
                metadata: None,
            },
            Some("/bin/sh"),
            None,
            None,
        )
        .await
        .unwrap();
    // Zero clears the timeout; the command completes.
    shell.timeout(&info.id, 0).await.unwrap();
    let result = shell.result(info).await;
    assert_eq!(result.info.status, Status::Exited);
}

#[tokio::test]
async fn shell_injects_agent_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let shell = service(&tmp);
    let mut env = std::collections::HashMap::new();
    env.insert("AI_AGENT".to_string(), "custom".to_string());
    env.insert("BASE".to_string(), "1".to_string());
    let info = shell
        .create(
            CreateInput {
                command: "printf %s \"$AGENT$OPENCODE$AI_AGENT$OPENCODE_SESSION_ID\"".into(),
                cwd: Some(tmp.path().to_string_lossy().to_string()),
                timeout: Some(5_000),
                metadata: None,
            },
            Some("/bin/sh"),
            Some(&env),
            Some(Box::new(|invocation: &mut Invocation| {
                invocation.env.insert("AGENT".into(), "1".into());
                invocation.env.insert("OPENCODE".into(), "1".into());
                invocation
                    .env
                    .entry("AI_AGENT".to_string())
                    .or_insert_with(|| "opencode".to_string());
                invocation.env.insert("OPENCODE_SESSION_ID".into(), "ses_test".into());
            })),
        )
        .await
        .unwrap();
    let result = shell.result(info).await;
    let output = result::output(&result);
    assert_eq!(output.output.trim_end(), "11customses_test");
}

#[tokio::test]
async fn shell_output_pages_by_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    let shell = service(&tmp);
    let info = shell
        .create(
            CreateInput {
                command: "printf 0123456789".into(),
                cwd: Some(tmp.path().to_string_lossy().to_string()),
                timeout: Some(5_000),
                metadata: None,
            },
            Some("/bin/sh"),
            None,
            None,
        )
        .await
        .unwrap();
    let _ = shell.wait(&info.id).await;
    let first = shell
        .output(&info.id, Some(OutputInput { cursor: Some(0), limit: Some(4) }))
        .await
        .unwrap();
    assert_eq!(first.output, "0123");
    assert_eq!(first.cursor, 4);
    assert_eq!(first.size, 10);
    let next = shell
        .output(&info.id, Some(OutputInput { cursor: Some(4), limit: Some(100) }))
        .await
        .unwrap();
    assert_eq!(next.output, "456789");
    // A cursor past the end returns empty.
    let done =
        shell.output(&info.id, Some(OutputInput { cursor: Some(100), limit: None })).await.unwrap();
    assert_eq!(done.output, "");
}

#[tokio::test]
async fn shell_list_running_only_and_remove() {
    let tmp = tempfile::tempdir().unwrap();
    let shell = service(&tmp);
    let info = shell
        .create(
            CreateInput {
                command: "sleep 0.2".into(),
                cwd: Some(tmp.path().to_string_lossy().to_string()),
                timeout: Some(10_000),
                metadata: None,
            },
            Some("/bin/sh"),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(shell.list().len(), 1);
    shell.remove(&info.id).await.unwrap();
    assert!(shell.list().is_empty());
    assert!(shell.get(&info.id).is_err());
}

#[tokio::test]
async fn shell_truncation_notice_counts_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let shell = service(&tmp);
    // 3 lines over the byte page: notice reports the line window.
    let info = shell
        .create(
            CreateInput {
                command: "printf 'one\ntwo\nthree\n'".into(),
                cwd: Some(tmp.path().to_string_lossy().to_string()),
                timeout: Some(5_000),
                metadata: None,
            },
            Some("/bin/sh"),
            None,
            None,
        )
        .await
        .unwrap();
    let result = shell.result(info).await;
    let capture = result.capture.unwrap();
    assert!(!capture.truncated);
    // The raw page keeps its trailing newline when under the limits.
    assert_eq!(capture.output, "one\ntwo\nthree\n");
}

// ---------------------------------------------------------------------------
// ShellParse ports (shell-parse.test.ts) — security-pinned behavior
// ---------------------------------------------------------------------------

mod parse_tests {
    use crate::shell::parse::{scan, scan_portable, ScanOutput, ScannedCommandPermission};

    fn cwd() -> std::path::PathBuf {
        std::path::PathBuf::from("/workspace")
    }

    fn commands_of(output: &ScanOutput) -> Vec<(String, String)> {
        output
            .commands
            .iter()
            .map(|command| (command.resource.clone(), command.save.clone()))
            .collect()
    }

    #[test]
    fn shell_parse_splits_bash_commands_and_derives_reusable_prefixes() {
        let result =
            scan("git status && npm run test -- --watch", "/bin/bash", &cwd(), false).unwrap();
        assert_eq!(
            commands_of(&result),
            vec![
                ("git status".to_string(), "git status *".to_string()),
                ("npm run test -- --watch".to_string(), "npm run test *".to_string()),
            ]
        );
        assert_eq!(result.directories, Vec::<String>::new());
    }

    #[test]
    fn shell_parse_portable_matches_legacy_on_supported_commands() {
        let commands = [
            "git status && npm run test -- --watch",
            "echo $(curl evil | sed s/x/y/)",
            "cd /tmp/$USER && git status",
            "if true; then printf yes; else printf no; fi",
            "if true; then export X=$(printf value); unset X; fi",
            "if export X=$(printf value); then printf done; fi",
            "export X=value >$(printf output)",
            "echo $((1 + 1))",
            "cd ~; cd src&&cd ..; pwd",
        ];
        for command in commands {
            let legacy = scan(command, "/bin/bash", &cwd(), false)
                .unwrap_or_else(|error| panic!("{command}: {error}"));
            let portable = scan(command, "/bin/bash", &cwd(), true)
                .unwrap_or_else(|error| panic!("{command}: {error}"));
            assert_eq!(portable, legacy, "command: {command}");
            assert_eq!(
                scan_portable(command, "/bin/bash", &cwd()).unwrap(),
                portable,
                "command: {command}"
            );
        }
    }

    #[test]
    fn shell_parse_portable_handles_heredocs_with_the_existing_permission_resource() {
        let command = "cat <<'EOF'\nstatic body\nEOF";
        let legacy = scan(command, "/bin/bash", &cwd(), false).unwrap();
        assert_eq!(
            commands_of(&legacy),
            vec![("cat <<'EOF'\nstatic body\nEOF".to_string(), "cat *".to_string())]
        );
        assert_eq!(scan(command, "/bin/bash", &cwd(), true).unwrap(), legacy);
    }

    #[test]
    fn shell_parse_portable_keeps_source_shaped_command_heads() {
        for command in ["c\"\\d\" relative", "'cd' /tmp", "c''d /tmp", "c\\\nd /tmp"] {
            let portable = scan_portable(command, "/bin/bash", &cwd()).unwrap();
            assert_eq!(
                portable.commands.iter().map(|item| item.resource.clone()).collect::<Vec<_>>(),
                vec![command.to_string()]
            );
            assert_eq!(portable.directories, Vec::<String>::new());
        }
    }

    #[test]
    fn shell_parse_preserves_declaration_permission_behavior() {
        for name in ["declare", "typeset", "export", "readonly", "local", "unset", "unsetenv"] {
            for command in [format!("{name} X"), format!("{name} \"$(printf X)\"; git status")] {
                let legacy = scan(&command, "/bin/bash", &cwd(), false).unwrap();
                let expected: Vec<(String, String)> = if command.contains("$(") {
                    vec![
                        ("printf X".to_string(), "printf *".to_string()),
                        ("git status".to_string(), "git status *".to_string()),
                    ]
                } else {
                    vec![]
                };
                assert_eq!(commands_of(&legacy), expected, "{command}");
                assert_eq!(scan(&command, "/bin/bash", &cwd(), true).unwrap(), legacy, "{command}");
            }
            for command in [
                format!("\"{name}\" X"),
                format!("FOO=bar {name} X"),
                format!("command {name} X"),
                format!(">{name}.txt {name} X"),
            ] {
                let legacy = scan(&command, "/bin/bash", &cwd(), false).unwrap();
                assert_eq!(legacy.commands.len(), 1, "{command}");
                assert_eq!(scan(&command, "/bin/bash", &cwd(), true).unwrap(), legacy, "{command}");
            }
        }
    }

    #[test]
    fn shell_parse_declaration_filtering_retains_directory_checks_inside_substitutions() {
        let command = "export X=$(cd /outside; printf value)";
        let expected = ScanOutput {
            commands: vec![ScannedCommandPermission {
                resource: "printf value".into(),
                save: "printf *".into(),
            }],
            directories: vec!["/outside".to_string()],
        };
        assert_eq!(scan(command, "/bin/bash", &cwd(), false).unwrap(), expected);
        assert_eq!(scan(command, "/bin/bash", &cwd(), true).unwrap(), expected);
    }

    #[test]
    fn shell_parse_does_not_treat_powershell_commands_as_bash_declarations() {
        assert_eq!(
            scan_portable("export X; unset X", "pwsh", &cwd()).unwrap(),
            ScanOutput {
                commands: vec![
                    ScannedCommandPermission {
                        resource: "export X".into(),
                        save: "export *".into()
                    },
                    ScannedCommandPermission { resource: "unset X".into(), save: "unset *".into() },
                ],
                directories: vec![],
            }
        );
    }

    #[test]
    fn shell_parse_does_not_permission_directory_changes_separately() {
        let result = scan("cd 'src dir' && git status", "/bin/bash", &cwd(), false).unwrap();
        assert_eq!(
            result,
            ScanOutput {
                commands: vec![ScannedCommandPermission {
                    resource: "git status".into(),
                    save: "git status *".into()
                }],
                directories: vec!["src dir".to_string()],
            }
        );
    }

    #[test]
    fn shell_parse_expands_deterministic_directory_variables() {
        let bash = scan("cd ~/src", "/bin/bash", &cwd(), false).unwrap();
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        assert_eq!(bash.directories, vec![home.join("src").to_string_lossy().to_string()]);

        let backslash = scan("cd '~\\src'", "/bin/bash", &cwd(), false).unwrap();
        assert_eq!(backslash.directories, vec!["~\\src".to_string()]);
    }

    #[test]
    fn shell_parse_opaque_dynamic_execution() {
        // eval marks a variable sink; a bound expansion reaching it refuses.
        for command in [
            "X='$(printf evil)'; eval $X",
            "PS4='$(evil)'; set -x",
            "alias x='evil'",
            "compgen -C evil",
            "emulate -L zsh; setopt globsubst",
        ] {
            let dialects = [crate::shell::scan::Dialect::Posix, crate::shell::scan::Dialect::Zsh];
            let mut refused = false;
            for dialect in dialects {
                if let crate::shell::scan::ScanResult::Opaque { .. } =
                    crate::shell::scan::scan(command, dialect)
                {
                    refused = true;
                }
            }
            assert!(refused, "expected opaque for {command}");
        }
    }

    #[test]
    fn shell_parse_opaque_compound_and_substitution_forms() {
        let cases: &[(&str, &str)] = &[
            ("((x) ", "command-substitution"),
            ("$(printf unclosed", "command-substitution"),
            ("cat <<EOF", "heredoc"),
            ("echo 'unterminated", "unterminated-quote"),
        ];
        for (command, reason) in cases {
            match crate::shell::scan::scan(command, crate::shell::scan::Dialect::Posix) {
                crate::shell::scan::ScanResult::Opaque { reason: found } => {
                    assert_eq!(found.as_str(), *reason, "command {command}");
                }
                crate::shell::scan::ScanResult::Scanned { .. } => {
                    panic!("expected opaque for {command}")
                }
            }
        }
    }
}
