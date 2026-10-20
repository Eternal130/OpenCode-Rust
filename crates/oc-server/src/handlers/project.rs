//! Project + worktree + vcs handlers.

#![allow(clippy::must_use_candidate, clippy::similar_names, clippy::type_complexity)]
use std::path::PathBuf;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::common::{no_content, resolve_location, ApiError, SharedState};
use super::session::{query_field, services};
use oc_core::project::ProjectEvents as _;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/project", axum::routing::get(project_list))
        .route("/api/project/:projectID", axum::routing::patch(project_update))
        .route("/api/worktree", axum::routing::delete(worktree_remove))
        .route("/api/worktree", axum::routing::get(worktree_list))
        .route("/api/worktree", axum::routing::post(worktree_create))
        .route("/api/worktree/refresh", axum::routing::post(worktree_refresh))
        .route("/api/vcs", axum::routing::get(vcs_get))
        .route("/api/vcs/base", axum::routing::get(vcs_base))
        .route("/api/vcs/branch", axum::routing::get(vcs_branch_list))
        .route("/api/vcs/diff", axum::routing::get(vcs_diff))
        .route("/api/vcs/init", axum::routing::post(vcs_init))
        .route("/api/vcs/status", axum::routing::get(vcs_status))
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

async fn project_list(State(state): State<SharedState>) -> Result<Response, ApiError> {
    let db = state.engine.db.clone();
    let rows = db
        .transaction(move |tx| {
            let conn = tx.conn();
            let mut statement = conn.prepare(
                "SELECT id, worktree, vcs, name, icon_url, icon_url_override, icon_color, \
                 sandboxes, commands, time_created, time_updated, time_active \
                 FROM project ORDER BY time_active DESC",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, i64>(10)?,
                        row.get::<_, i64>(11)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("project:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let data: Vec<Value> = rows.into_iter().map(|row| project_json(&row)).collect();
    Ok(axum::Json(Value::Array(data)).into_response())
}

type ProjectTuple = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    i64,
    i64,
    i64,
);

fn project_json(row: &ProjectTuple) -> Value {
    let (
        id,
        worktree,
        vcs,
        name,
        icon_url,
        icon_override,
        icon_color,
        sandboxes,
        commands,
        created,
        updated,
        active,
    ) = row;
    let icon = (icon_url.is_some() || icon_override.is_some() || icon_color.is_some()).then(|| {
        json!({
            "url": icon_url,
            "override": icon_override,
            "color": icon_color,
        })
    });
    json!({
        "id": id,
        "canonical": worktree,
        "vcs": vcs,
        "name": name,
        "icon": icon,
        "sandboxes": serde_json::from_str::<Value>(sandboxes).unwrap_or(json!([])),
        "commands": commands.as_deref().and_then(|value| serde_json::from_str::<Value>(value).ok()),
        "time": { "created": created, "updated": updated, "active": active },
    })
}

async fn project_update(
    State(state): State<SharedState>,
    Path(project_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    // `Project.UpdateInput` (schema/project.ts:51-58): absent = untouched,
    // present = write (empty string clears to NULL — project.ts:219-226).
    // `icon.url` is server-owned and never updatable (project.ts:221-223).
    let canonical = body["canonical"].as_str().map(std::path::PathBuf::from);
    if let Some(canonical) = &canonical {
        if !canonical.is_absolute() {
            return Err(ApiError::invalid_request("canonical must be an absolute path"));
        }
    }
    let commands = body["commands"].get("start").and_then(Value::as_str).map(|start| {
        // `commands.start` present: non-empty → {start}, empty → clear.
        (!start.is_empty()).then(|| serde_json::json!({ "start": start }))
    });
    let input = oc_core::project::UpdateInput {
        project_id: project_id.clone(),
        canonical,
        name: body["name"].as_str().map(String::from),
        icon_override: body["icon"].get("override").and_then(Value::as_str).map(String::from),
        icon_color: body["icon"].get("color").and_then(Value::as_str).map(String::from),
        commands,
    };
    // DB-D4 (project.ts:215-239): persist through the store (real row,
    // v2 empty-string-clear semantics, time_updated bump) …
    let info = state.engine.projects.update(&input).await.map_err(|error| {
        crate::errors::debug_cause("project:unknown_ref", &error);
        ApiError::project_not_found(&project_id)
    })?;
    // … announce `project.updated` with the full Info (fromRow) …
    oc_core::project::BusProjectEvents::new((*state.engine.bus).clone())
        .project_updated(&info)
        .await;
    // … and return the REAL row (never a fabricated echo).
    Ok(axum::Json(info.to_json()).into_response())
}

// ---------------------------------------------------------------------------
// Worktrees
// ---------------------------------------------------------------------------

async fn worktrees_for(
    state: &SharedState,
    project_id: &str,
) -> Result<Vec<(String, String, String)>, ApiError> {
    let db = state.engine.db.clone();
    let project = project_id.to_owned();
    db.transaction(move |tx| {
        let conn = tx.conn();
        let mut statement = conn.prepare(
            "SELECT project_id, directory, coalesce(strategy, '') FROM worktree WHERE project_id = ?1 ORDER BY directory",
        )?;
        let rows = statement
            .query_map((project,), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|error| { crate::errors::debug_cause("project:unknown_ref", &error); ApiError::unknown_ref() })
}

async fn worktree_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let Some(project_id) = query_field(&uri, "projectID") else {
        return Err(ApiError::invalid_request("projectID is required"));
    };
    if project_id != services.project.id {
        return Err(ApiError::project_not_found(&project_id));
    }
    // v2 `Worktree.List` is a BARE ARRAY of `{directory, strategy?}`
    // (schema/worktree.ts `List = Schema.Array(Directory)` — the core
    // ops.list rows); the strategy rides along for the TUI's
    // managed-root ordering (dialog-workspaces.tsx).
    let directories: Vec<Value> = worktrees_for(&state, &project_id)
        .await?
        .into_iter()
        .map(|(_, directory, strategy)| {
            let mut entry = json!({ "directory": directory });
            if !strategy.is_empty() {
                entry["strategy"] = json!(strategy);
            }
            entry
        })
        .collect();
    Ok(axum::Json(Value::Array(directories)).into_response())
}

/// v2 `Slug.create()` (util/slug.ts) — a random adjective-noun pair.
mod slug {
    use rand::seq::SliceRandom;

    const ADJECTIVES: [&str; 29] = [
        "brave", "calm", "clever", "cosmic", "crisp", "curious", "eager", "gentle", "glowing",
        "happy", "hidden", "jolly", "kind", "lucky", "mighty", "misty", "neon", "nimble",
        "playful", "proud", "quick", "quiet", "shiny", "silent", "stellar", "sunny", "swift",
        "tidy", "witty",
    ];
    const NOUNS: [&str; 31] = [
        "cabin", "cactus", "canyon", "circuit", "comet", "eagle", "engine", "falcon", "forest",
        "garden", "harbor", "island", "knight", "lagoon", "meadow", "moon", "mountain", "nebula",
        "orchid", "otter", "panda", "pixel", "planet", "river", "rocket", "sailor", "squid",
        "star", "tiger", "wizard", "wolf",
    ];

    pub fn create() -> String {
        let mut rng = rand::thread_rng();
        format!(
            "{}-{}",
            ADJECTIVES.choose(&mut rng).unwrap_or(&"swift"),
            NOUNS.choose(&mut rng).unwrap_or(&"comet")
        )
    }
}

/// `canonical` (worktree/directory.ts): the on-disk canonical path, or
/// `Worktree.DirectoryUnavailableError` when the directory does not exist.
fn canonical_directory(directory: &std::path::Path) -> Result<std::path::PathBuf, ApiError> {
    std::fs::canonicalize(directory).map_err(|_| {
        ApiError::worktree(format!("Worktree directory unavailable: {}", directory.display()), None)
    })
}

/// The configured worktree parent (v2 `ConfigWorktreePlugin`,
/// config/plugin/worktree.ts): the last config document owning
/// `worktree.directory` — `~/` expands against home, relatives resolve
/// against the project's canonical checkout.
fn configured_worktree_directory(
    state: &SharedState,
    project_worktree: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let loader = oc_core::config::Loader::new(
        project_worktree.to_path_buf(),
        oc_core::config::GlobalDirs {
            config: state.engine.home.join(".config").join("opencode"),
            home: state.engine.home.clone(),
        },
    );
    let snapshot = loader.load();
    let directory = oc_core::config::latest(&snapshot.entries, |info| {
        info.worktree.as_ref().map(|wt| wt.directory.clone())
    })?;
    let path = if let Some(rest) = directory.strip_prefix("~/") {
        state.engine.home.join(rest)
    } else {
        project_worktree.join(directory)
    };
    Some(path)
}

#[allow(clippy::too_many_lines)] // one v2 Effect chain (worktree.ts:192-238)
async fn worktree_create(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    // v2 `Worktree.create` (worktree.ts:192-238) with the git strategy
    // (worktree/git.ts): source validation → parent resolution → slug
    // name + collision suffixes → `git worktree add --detach` → row
    // (replace) + `worktree.updated` → optional start command.
    let project_id = body["projectID"].as_str().unwrap_or_default().to_owned();
    if project_id.is_empty() {
        return Err(ApiError::invalid_request("projectID is required"));
    }
    let name = body["name"].as_str().map(str::trim);
    if name == Some("") {
        return Err(ApiError::worktree("Worktree name is required", None));
    }
    let branch = body["branch"].as_str().map(str::trim).filter(|branch| !branch.is_empty());
    let from = body["from"].as_str().map(std::path::PathBuf::from);
    let parent_override =
        body["directory"].as_str().map(str::trim).filter(|dir| !dir.is_empty()).map(PathBuf::from);

    let db = state.engine.db.clone();
    let lookup_id = project_id.clone();
    let project = db
        .transaction(move |tx| {
            Ok(tx
                .conn()
                .query_row(
                    "SELECT worktree, commands FROM project WHERE id = ?1",
                    (lookup_id,),
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .ok())
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("project:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let Some((project_worktree, commands)) = project else {
        return Err(ApiError::project_not_found(&project_id));
    };
    let project_worktree = std::path::PathBuf::from(project_worktree);

    // The source directory (v2 `source`): canonical `from` or the
    // project's canonical checkout; anything else must already be a
    // registered row (`SourceDirectoryNotFoundError`).
    let source = canonical_directory(from.as_ref().unwrap_or(&project_worktree))?;
    if source != project_worktree {
        let source_str = source.to_string_lossy().into_owned();
        let lookup_project = project_id.clone();
        let known = db
            .transaction(move |tx| {
                Ok(tx
                    .conn()
                    .query_row(
                        "SELECT 1 FROM worktree WHERE project_id = ?1 AND directory = ?2",
                        (lookup_project, source_str),
                        |_| Ok(()),
                    )
                    .is_ok())
            })
            .await
            .unwrap_or(false);
        if !known {
            return Err(ApiError::worktree(
                format!("Worktree source not found: {}", source.display()),
                None,
            ));
        }
    }

    // Parent directory: explicit override → config `worktree.directory` →
    // `<data>/worktree/<project id prefix>` (v2 strategies.ts initial
    // state; `global.data` ≙ the engine data root).
    let parent = parent_override
        .or_else(|| configured_worktree_directory(&state, &project_worktree))
        .unwrap_or_else(|| {
            let prefix: String = project_id.chars().take(6).collect();
            state.engine.data_root.join("worktree").join(prefix)
        });
    std::fs::create_dir_all(&parent).map_err(|error| {
        crate::errors::debug_cause("worktree:mkdir", &error);
        ApiError::worktree(format!("Worktree directory unavailable: {}", parent.display()), None)
    })?;

    // Name + collision suffixes (v2: `name-2` … `name-10`, then
    // `DestinationExistsError`).
    let name = name.map_or_else(slug::create, String::from);
    let mut worktree_directory = None;
    for suffix in 1..=10u32 {
        let candidate =
            if suffix == 1 { parent.join(&name) } else { parent.join(format!("{name}-{suffix}")) };
        if !candidate.exists() {
            worktree_directory = Some(candidate);
            break;
        }
    }
    let Some(worktree_directory) = worktree_directory else {
        return Err(ApiError::worktree(
            format!(
                "Worktree destination already exists: {}",
                parent.join(format!("{name}-10")).display()
            ),
            None,
        ));
    };

    // The git strategy (worktree/git.ts → git.ts:657): discover the
    // repository from the source, then
    // `git worktree add --detach -- <directory> <ref | HEAD>`.
    let inside_repo = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&source)
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success());
    if !inside_repo {
        return Err(ApiError::worktree(
            format!("Worktree directory unavailable: {}", source.display()),
            None,
        ));
    }
    let reference = branch.unwrap_or("HEAD");
    let added = std::process::Command::new("git")
        .args(["worktree", "add", "--detach", "--"])
        .arg(&worktree_directory)
        .arg(reference)
        .current_dir(&source)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| {
            crate::errors::debug_cause("worktree:spawn", &error);
            ApiError::worktree(format!("Git failed: {error}"), None)
        })?;
    if !added.status.success() {
        let message = {
            let stderr = String::from_utf8_lossy(&added.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&added.stdout).trim().to_string();
            (!stderr.is_empty())
                .then_some(stderr)
                .or((!stdout.is_empty()).then_some(stdout))
                .unwrap_or_else(|| "Git failed".to_string())
        };
        return Err(ApiError::worktree(message, None));
    }
    let directory = canonical_directory(&worktree_directory)?;
    let directory_str = directory.to_string_lossy().into_owned();

    // Row (v2 ops.create with `replace: true` — the git strategy owns the
    // row) + `worktree.updated` when the write landed.
    let row_project_id = project_id.clone();
    let row_directory = directory_str.clone();
    let written = db
        .transaction(move |tx| {
            Ok(tx.execute(
                "INSERT INTO worktree (project_id, directory, strategy, time_created) \
                 VALUES (?1, ?2, 'git', ?3) \
                 ON CONFLICT (project_id, directory) DO UPDATE SET strategy = 'git'",
                (row_project_id, row_directory, oc_core::database::migrations::now_ms()),
            )? != 0)
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("project:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if written {
        publish_worktree_updated(&state, &project_id).await;
    }

    // The project's `commands.start` (v2 worktree.ts:231-246): bash -lc in
    // the new worktree with the OPENCODE_WORKTREE_* env; a failure fails
    // the create.
    let start = commands
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|decoded| {
            decoded.get("start").and_then(Value::as_str).map(|command| command.trim().to_string())
        })
        .filter(|command| !command.is_empty());
    if let Some(command) = start {
        let output = std::process::Command::new("bash")
            .args(["-lc", command.as_str()])
            .current_dir(&directory)
            .env("OPENCODE_WORKTREE_BASE", &source)
            .env("OPENCODE_WORKTREE_PATH", &directory)
            .stdin(std::process::Stdio::null())
            .output();
        match output {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let message = (!stderr.is_empty())
                    .then_some(stderr)
                    .or((!stdout.is_empty()).then_some(stdout))
                    .unwrap_or_else(|| "Start command failed".to_string());
                return Err(ApiError::worktree(message, None));
            }
            Err(error) => {
                crate::errors::debug_cause("worktree:start", &error);
                return Err(ApiError::worktree(format!("Start command failed: {error}"), None));
            }
        }
    }

    Ok(axum::Json(json!({ "directory": directory_str })).into_response())
}

/// `worktree.updated` `{projectID}` — v2 publishes it after every
/// row-owning mutation (`changed(projectID, true)`, worktree.ts:128-131 +
/// create:228 / remove:270), routed to the project's location.
async fn publish_worktree_updated(state: &SharedState, project_id: &str) {
    let Some(definition) =
        oc_core::bus::registry::by_type(oc_core::bus::registry::WORKTREE_UPDATED)
    else {
        return;
    };
    let project_id = project_id.to_owned();
    let lookup_id = project_id.clone();
    let directory: Option<String> = state
        .engine
        .db
        .transaction(move |tx| {
            Ok(tx
                .conn()
                .query_row("SELECT worktree FROM project WHERE id = ?1", (lookup_id,), |row| {
                    row.get(0)
                })
                .ok())
        })
        .await
        .ok()
        .flatten();
    let location = directory.map(|directory| oc_schema::location::Ref {
        directory: oc_schema::common::AbsolutePath(directory),
        workspace_id: None,
    });
    let options = oc_core::bus::PublishOptions { location, ..Default::default() };
    let _ = state
        .engine
        .bus
        .publish(definition, serde_json::json!({ "projectID": project_id }), options)
        .await;
}

#[allow(clippy::too_many_lines)] // one v2 Effect chain (worktree.ts:273-327)
async fn worktree_refresh(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    // v2 worktree.ts:273-327 — project row → strategy `load` (the
    // canonical checkout must be a directory) → stored rows with on-disk
    // checks → discovery over the unowned/canonical roots → ONE
    // transaction (upsert discovered, prune vanished) →
    // `worktree.updated` when anything changed.
    let project_id = body["projectID"].as_str().unwrap_or_default().to_owned();
    if project_id.is_empty() {
        return Err(ApiError::invalid_request("projectID is required"));
    }
    let db = state.engine.db.clone();
    let lookup = project_id.clone();
    let project = db
        .transaction(move |tx| {
            Ok(tx
                .conn()
                .query_row("SELECT worktree FROM project WHERE id = ?1", (lookup,), |row| {
                    row.get::<_, String>(0)
                })
                .ok())
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("worktree:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let Some(project_worktree) = project else {
        return Err(ApiError::project_not_found(&project_id));
    };
    let project_worktree = std::path::PathBuf::from(project_worktree);
    // `load(row.worktree)` — DirectoryUnavailableError when the canonical
    // checkout is gone (worktree.ts:132-134).
    if !project_worktree.is_dir() {
        return Err(ApiError::worktree(
            format!("Worktree directory unavailable: {}", project_worktree.display()),
            None,
        ));
    }

    // Stored rows + on-disk existence (ops.list + the `checked` pass).
    let lookup = project_id.clone();
    let stored: Vec<(String, Option<String>)> = db
        .transaction(move |tx| {
            let mut statement = tx.conn().prepare(
                "SELECT directory, strategy FROM worktree WHERE project_id = ?1 \
                 ORDER BY time_created DESC, directory ASC",
            )?;
            let rows = statement
                .query_map((lookup,), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("worktree:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let checked: Vec<(std::path::PathBuf, Option<String>, bool)> = stored
        .into_iter()
        .map(|(directory, strategy)| {
            let directory = std::path::PathBuf::from(directory);
            let exists = directory.is_dir();
            (directory, strategy, exists)
        })
        .collect();

    // Discovery roots: the canonical checkout plus every unowned row's
    // directory (managed children are enumerated by their backend).
    let mut roots: Vec<std::path::PathBuf> = vec![project_worktree.clone()];
    roots.extend(
        checked
            .iter()
            .filter(|(_, strategy, exists)| *exists && strategy.is_none())
            .map(|(directory, _, _)| directory.clone()),
    );

    // `git worktree list --porcelain` per root (the git strategy — the
    // only registered backend). Strategy failures are swallowed like the
    // ref's logWarning → []; each entry is canonicalized, skipping
    // directories that vanished mid-scan. The first entry is the main
    // checkout (type "root", unowned); the rest are linked worktrees
    // (type "worktree" → strategy `git`).
    let mut discovered: Vec<(std::path::PathBuf, Option<&'static str>)> = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let Some(output) = git_output(&root, &["worktree", "list", "--porcelain"]) else {
            continue;
        };
        for (index, line) in output.lines().filter(|line| line.starts_with("worktree ")).enumerate()
        {
            let raw = line["worktree ".len()..].trim();
            let path = if std::path::Path::new(raw).is_absolute() {
                std::path::PathBuf::from(raw)
            } else {
                root.join(raw)
            };
            let Ok(real) = std::fs::canonicalize(&path) else {
                continue;
            };
            if discovered.iter().any(|(directory, _)| *directory == real) {
                continue;
            }
            let strategy = (index > 0).then_some("git");
            discovered.push((real, strategy));
        }
    }

    // Rows whose directory no longer exists are pruned.
    let vanished: Vec<std::path::PathBuf> = checked
        .iter()
        .filter(|(_, _, exists)| !exists)
        .map(|(directory, _, _)| directory.clone())
        .collect();

    // One transaction: upsert the discovered facts (never replacing
    // another strategy's ownership — ops.create's setWhere) and delete
    // the vanished rows (ops.remove), counting only landed writes.
    let tx_project = project_id.clone();
    let changes = db
        .transaction(move |tx| {
            let conn = tx.conn();
            let now = oc_core::database::migrations::now_ms();
            let mut updated: Vec<String> = Vec::new();
            for (directory, strategy) in discovered {
                let row = conn.query_row(
                    "INSERT INTO worktree (project_id, directory, strategy, time_created) \
                     VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT (project_id, directory) DO UPDATE SET strategy = excluded.strategy \
                     WHERE ?3 IS NOT NULL AND worktree.strategy IS NULL \
                     RETURNING directory",
                    (&tx_project, directory.to_string_lossy(), strategy, now),
                    |row| row.get::<_, String>(0),
                );
                if let Ok(directory) = row {
                    updated.push(directory);
                }
            }
            let mut removed: Vec<String> = Vec::new();
            for directory in vanished {
                let row = conn.query_row(
                    "DELETE FROM worktree WHERE project_id = ?1 AND directory = ?2 \
                     RETURNING directory",
                    (&tx_project, directory.to_string_lossy()),
                    |row| row.get::<_, String>(0),
                );
                if let Ok(directory) = row {
                    removed.push(directory);
                }
            }
            Ok((updated.len(), removed.len()))
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("worktree:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;

    // `changed(projectID, update)` — announce only when a write landed.
    if changes.0 > 0 || changes.1 > 0 {
        publish_worktree_updated(&state, &project_id).await;
    }
    Ok(no_content())
}

#[allow(clippy::too_many_lines)] // one v2 Effect chain (worktree.ts:257-271)
async fn worktree_remove(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    // v2 worktree.ts:257-271 — project row → canonicalize → stored
    // strategy check → strategy remove (`git worktree remove [--force]`,
    // git.ts:669-681) → row delete + `worktree.updated`.
    let (project_id, directory, force) = (
        body["projectID"].as_str().unwrap_or_default().to_owned(),
        body["directory"].as_str().unwrap_or_default().to_owned(),
        body["force"].as_bool().unwrap_or(false),
    );
    if project_id.is_empty() {
        return Err(ApiError::invalid_request("projectID is required"));
    }
    let db = state.engine.db.clone();
    let lookup_project = project_id.clone();
    let project = db
        .transaction(move |tx| {
            Ok(tx
                .conn()
                .query_row("SELECT worktree FROM project WHERE id = ?1", (lookup_project,), |row| {
                    row.get::<_, String>(0)
                })
                .ok())
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("worktree:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let Some(project_worktree) = project else {
        return Err(ApiError::project_not_found(&project_id));
    };
    let project_worktree = std::path::PathBuf::from(project_worktree);

    // Canonical on-disk path (DirectoryUnavailableError when missing).
    let worktree_directory = canonical_directory(std::path::Path::new(&directory))?;
    let directory_str = worktree_directory.to_string_lossy().into_owned();

    // A saved row with a strategy owns the directory (InvalidDirectoryError
    // otherwise); the git strategy is the only registered backend (create
    // stores `'git'`).
    let lookup_directory = directory_str.clone();
    let strategy_project = project_id.clone();
    let stored: Option<String> = db
        .transaction(move |tx| {
            let value = tx
                .conn()
                .query_row(
                    "SELECT strategy FROM worktree WHERE project_id = ?1 AND directory = ?2",
                    (strategy_project.as_str(), lookup_directory.as_str()),
                    |row| row.get::<_, String>(0),
                )
                .ok();
            Ok(value)
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("worktree:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if stored.as_deref().is_none_or(str::is_empty) {
        return Err(ApiError::worktree(
            format!("Invalid worktree directory: {directory_str}"),
            None,
        ));
    }

    // `git worktree remove [--force] <directory>` from the repository's
    // common dir (git.ts:621-633 + 669-681: discover from the worktree,
    // run from commonDirectory).
    let common = git_output(
        &worktree_directory,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .map(|output| std::path::PathBuf::from(output.trim()))
    .filter(|path| path.is_dir())
    .unwrap_or(project_worktree);
    let mut args = vec!["worktree".to_owned(), "remove".to_owned()];
    if force {
        args.push("--force".to_owned());
    }
    args.push(directory_str.clone());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let refs = refs.as_slice();
    let removal = std::process::Command::new("git")
        .args(refs)
        .current_dir(&common)
        .stdin(std::process::Stdio::null())
        .output();
    match removal {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let message = (!stderr.is_empty())
                .then_some(stderr)
                .or((!stdout.is_empty()).then_some(stdout))
                .unwrap_or_else(|| "Git failed".to_owned());
            // git.ts:630-633 — the dirty-tree failure demands --force. The
            // v2 regex covers the English messages; the literal `--force`
            // hint rides every locale's dirty-tree error.
            let dirty = message.contains("contains modified or untracked files")
                || message.contains("is dirty")
                || message.contains("--force");
            return Err(ApiError::worktree(message, Some(dirty)));
        }
        Err(error) => {
            crate::errors::debug_cause("worktree:spawn", &error);
            return Err(ApiError::worktree(format!("Git failed: {error}"), None));
        }
    }

    // Row delete + `worktree.updated` (worktree.ts:270-271) — by the
    // canonical directory the row stores.
    let delete_project = project_id.clone();
    let delete_directory = directory_str;
    let removed = db
        .transaction(move |tx| {
            tx.execute(
                "DELETE FROM worktree WHERE project_id = ?1 AND directory = ?2",
                (delete_project, delete_directory),
            )
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("worktree:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if removed > 0 {
        publish_worktree_updated(&state, &project_id).await;
    }
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// VCS
// ---------------------------------------------------------------------------

async fn vcs_get(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    // The wire schema (`VcsInfo`: `branch.current` — the generated
    // client's `VcsBranch` and the ref's `Info` agree; a flat string
    // here failed every typed client's deserialization).
    let data = match &services.project.vcs {
        Some(vcs) => json!({
            "branch": {
                "current": git_output(services.project.canonical.as_path(), &["rev-parse", "--abbrev-ref", "HEAD"])
                    .map(|output| output.trim().to_owned())
                    .unwrap_or_default(),
            },
            "provider": vcs.kind(),
        }),
        None => Value::Null,
    };
    Ok(super::common::json_location_data(&location.as_path().to_string_lossy(), data))
}

async fn vcs_base(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // v2 vcs.base (handlers/vcs.ts + plugin/vcs/git.ts:286-317): reflog
    // "branch: Created from <origin>" first, default-branch fallback
    // second, DiffError → 503 ServiceUnavailableError{service:"vcs"}.
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let scope = services.project.canonical.as_path();
    let unavailable =
        |message: &str| ApiError::service_unavailable(message.to_owned(), Some("vcs"));

    if !git_has_head(scope) {
        return Ok(super::common::json_location_data(
            &location.as_path().to_string_lossy(),
            Value::Null,
        ));
    }
    let Some(current) = git_current_branch(scope) else {
        return Err(unavailable("Choose a review base"));
    };

    // Creation hints from the branch reflog (renamed branches lose the
    // simple creation story).
    let history = git_reflog_creation(scope, &current);
    let renamed = history.iter().any(|(_, message)| {
        message.strip_prefix("Branch: renamed ").is_some_and(|rest| !rest.is_empty())
    });
    let creation = if renamed {
        None
    } else {
        // reflog lists newest first; `find` takes the newest creation hint.
        history.iter().find_map(|(commit, message)| {
            message
                .strip_prefix("branch: Created from ")
                .filter(|origin| !origin.is_empty())
                .map(|origin| (commit.clone(), origin.to_owned()))
        })
    };
    if let Some((commit, origin)) = creation {
        if let Some((name, ref_)) = git_named_ref(scope, &origin) {
            if name != current
                && git_is_ancestor(scope, &commit, "HEAD")
                && git_is_ancestor(scope, &commit, &ref_)
            {
                return Ok(super::common::json_location_data(
                    &location.as_path().to_string_lossy(),
                    json!({ "name": name, "ref": ref_, "source": "reflog" }),
                ));
            }
        }
    }

    let Some((name, default_ref)) = git_default_branch(scope) else {
        return Err(unavailable("Choose a review base"));
    };
    if name != current {
        return Err(unavailable("Choose a review base"));
    }
    match git_named_ref(scope, &default_ref) {
        Some((_, ref_)) => Ok(super::common::json_location_data(
            &location.as_path().to_string_lossy(),
            json!({ "name": name, "ref": ref_, "source": "default" }),
        )),
        None => Err(unavailable("The default review base is unavailable")),
    }
}

/// `git rev-parse --verify HEAD` succeeds (git.ts:318-321).
fn git_has_head(scope: &std::path::Path) -> bool {
    git_status(scope, &["rev-parse", "--verify", "HEAD"])
}

/// The checked-out branch short name (git.ts:220-224).
fn git_current_branch(scope: &std::path::Path) -> Option<String> {
    git_output(scope, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map(|output| output.trim().to_owned())
        .filter(|branch| !branch.is_empty())
}

/// `git reflog show --max-count=256 --format=%H%x00%gs refs/heads/<branch>`
/// parsed into (commit, message) pairs (git.ts:291-300).
fn git_reflog_creation(scope: &std::path::Path, branch: &str) -> Vec<(String, String)> {
    let output = git_output(
        scope,
        &[
            "reflog",
            "show",
            "--max-count=256",
            "--format=%H%x00%gs",
            &format!("refs/heads/{branch}"),
        ],
    )
    .unwrap_or_default();
    output
        .lines()
        .filter_map(|line| {
            let (commit, message) = line.split_once('\u{0}')?;
            Some((commit.to_owned(), message.to_owned()))
        })
        .collect()
}

/// `namedRef` (git.ts:271-283): a creation hint must identify a branch
/// (not HEAD, an object id, or a revision expression).
fn git_named_ref(scope: &std::path::Path, input: &str) -> Option<(String, String)> {
    if input == "HEAD"
        || input.ends_with("/HEAD")
        || input.chars().any(|c| matches!(c, '~' | '^' | ':' | '@' | '{' | '}' | ' '))
    {
        return None;
    }
    let full = git_output(
        scope,
        &["rev-parse", "--symbolic-full-name", "--verify", "--end-of-options", input],
    )?
    .trim()
    .to_owned();
    let is_head_ref = full.starts_with("refs/heads/") && !full.ends_with("/HEAD");
    let is_remote_ref = full.starts_with("refs/remotes/")
        && !full.ends_with("/HEAD")
        && full.matches('/').count() > 2;
    if !is_head_ref && !is_remote_ref {
        return None;
    }
    if !git_status(
        scope,
        &["rev-parse", "--verify", "--end-of-options", &format!("{full}^{{commit}}")],
    ) {
        return None;
    }
    let name = full
        .strip_prefix("refs/heads/")
        .map(str::to_owned)
        .or_else(|| {
            full.strip_prefix("refs/remotes/").map(|rest| {
                rest.split_once('/').map_or_else(|| rest.to_owned(), |(_, tail)| tail.to_owned())
            })
        })
        .unwrap_or_else(|| full.clone());
    Some((name, full))
}

/// `defaultBranch` (git.ts:244-260): primary remote's HEAD → configured
/// `init.defaultBranch` present locally → main → master.
fn git_default_branch(scope: &std::path::Path) -> Option<(String, String)> {
    let remotes = git_lines(scope, &["remote"]);
    let primary = if remotes.iter().any(|remote| remote == "origin") {
        Some("origin".to_owned())
    } else if remotes.len() == 1 {
        remotes.first().cloned()
    } else if remotes.iter().any(|remote| remote == "upstream") {
        Some("upstream".to_owned())
    } else {
        remotes.first().cloned()
    };
    if let Some(remote) = primary {
        if let Some(head) =
            git_output(scope, &["symbolic-ref", &format!("refs/remotes/{remote}/HEAD")])
        {
            let reference = head.trim().strip_prefix("refs/remotes/").unwrap_or_default();
            if let Some(name) =
                reference.strip_prefix(&format!("{remote}/")).filter(|name| !name.is_empty())
            {
                return Some((name.to_owned(), reference.to_owned()));
            }
        }
    }
    let locals = git_lines(scope, &["for-each-ref", "--format=%(refname:short)", "refs/heads"]);
    if let Some(configured) = git_output(scope, &["config", "init.defaultBranch"]) {
        let name = configured.trim();
        if !name.is_empty() && locals.iter().any(|local| local == name) {
            return Some((name.to_owned(), name.to_owned()));
        }
    }
    if locals.iter().any(|local| local == "main") {
        return Some(("main".to_owned(), "main".to_owned()));
    }
    if locals.iter().any(|local| local == "master") {
        return Some(("master".to_owned(), "master".to_owned()));
    }
    None
}

/// `merge-base --is-ancestor commit ref` (git.ts:268-272).
fn git_is_ancestor(scope: &std::path::Path, commit: &str, reference: &str) -> bool {
    let hex = commit.len() >= 40 && commit.chars().all(|c| c.is_ascii_hexdigit());
    hex && git_status(scope, &["merge-base", "--is-ancestor", commit, reference])
}

fn git_lines(scope: &std::path::Path, args: &[&str]) -> Vec<String> {
    git_output(scope, args)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

fn git_status(scope: &std::path::Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(scope)
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

async fn vcs_branch_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // v2 vcs.branch.list → `Vcs.BranchList` = bare string[]
    // (git.ts:228-241): case-insensitive for-each-ref over local+remote
    // namespaces, committerdate-then-refname ordering, /HEAD filtered.
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let search = query_field(&uri, "search");
    let limit = query_field(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .map_or(50usize, |value| value.min(100));
    let scope = services.project.canonical.as_path();

    let escaped = search.as_ref().map(|value| {
        let trimmed = value.trim();
        let mut out = String::with_capacity(trimmed.len());
        for character in trimmed.chars() {
            if matches!(character, '*' | '?' | '[' | '\\' | ']') {
                out.push('\\');
            }
            out.push(character);
        }
        out
    });
    let mut args = vec![
        "for-each-ref".to_owned(),
        "--ignore-case".to_owned(),
        "--sort=refname".to_owned(),
        "--sort=-committerdate".to_owned(),
        "--format=%(refname:short)".to_owned(),
        format!("--count={limit}"),
    ];
    match escaped.as_deref() {
        Some(pattern) if !pattern.is_empty() => {
            args.push(format!("refs/heads/*{pattern}*"));
            args.push(format!("refs/remotes/*{pattern}*"));
        }
        _ => {
            args.push("refs/heads".to_owned());
            args.push("refs/remotes".to_owned());
        }
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = git_output(scope, refs.as_slice()).unwrap_or_default();
    let branches: Vec<Value> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.ends_with("/HEAD"))
        .map(|line| json!(line))
        .collect();
    Ok(super::common::json_location_data(
        &location.as_path().to_string_lossy(),
        Value::Array(branches),
    ))
}

// ---------------------------------------------------------------------------
// VCS-3 — diff (plugin/vcs/git.ts:101-117 + 344-608)
// ---------------------------------------------------------------------------

/// `MAX_PATCH_BYTES` / `MAX_TOTAL_PATCH_BYTES` (vcs/patch.ts:10-11).
const MAX_PATCH_BYTES: usize = 10_000_000;
const MAX_TOTAL_PATCH_BYTES: usize = 10_000_000;
/// `PATCH_CONTEXT_LINES` (vcs/patch.ts:7) — effectively full context.
const PATCH_CONTEXT_LINES: i64 = 2_147_483_647;

/// One changed file (git.ts:129-133).
#[derive(Debug, Clone)]
struct DiffItem {
    file: String,
    code: String,
    status: &'static str,
}

/// The jsdiff `emptyPatch` rendering (vcs/patch.ts:33-34), pinned against
/// the reference's own `diff` package output.
fn empty_patch(file: &str) -> String {
    format!("Index: {file}\n===================================================================\n--- {file}\t\n+++ {file}\t\n")
}

/// `kind` (git.ts:154-160): porcelain/status letter → wire status.
fn diff_kind(code: &str) -> &'static str {
    if code == "??" {
        return "added";
    }
    if code.contains('U') {
        return "modified";
    }
    if code.contains('A') && !code.contains('D') {
        return "added";
    }
    if code.contains('D') && !code.contains('A') {
        return "deleted";
    }
    "modified"
}

/// NUL-split output filter (git.ts:152).
fn nuls(text: &str) -> Vec<String> {
    text.split('\u{0}').filter(|item| !item.is_empty()).map(String::from).collect()
}

#[allow(clippy::too_many_lines)] // mirrors the v2 batch pipeline (git.ts:101-608)
async fn vcs_diff(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let base_param = query_field(&uri, "base");
    let context = query_field(&uri, "context").and_then(|value| value.parse::<i64>().ok());
    let mode = query_field(&uri, "mode").unwrap_or_else(|| "working".to_owned());
    if !matches!(mode.as_str(), "working" | "branch" | "committed") {
        return Err(ApiError::invalid_request(
            "Expected \"working\" | \"branch\" | \"committed\"\n  at [\"mode\"]".to_owned(),
        ));
    }
    let scope = services.project.canonical.as_path();
    let unavailable =
        |message: &str| ApiError::service_unavailable(message.to_owned(), Some("vcs"));

    let context = context.unwrap_or(PATCH_CONTEXT_LINES);

    // Mode routing (git.ts:101-117).
    let (reference, target): (Option<String>, Option<&str>) = if mode == "working" {
        (git_has_head(scope).then(|| "HEAD".to_owned()), None)
    } else if !git_has_head(scope) {
        if mode == "committed" {
            return Ok(super::common::json_location_data(
                &location.as_path().to_string_lossy(),
                json!([]),
            ));
        }
        // branch-mode without HEAD diffs the untracked status scan.
        (None, None)
    } else {
        let Some(base) = base_param
            .clone()
            .or_else(|| git_default_branch(scope).map(|(_, reference)| reference))
        else {
            return Err(unavailable("No review base available"));
        };
        let Some(reference) = git_merge_base(scope, &base) else {
            return Err(unavailable(&format!("No merge base available for {base}")));
        };
        if mode == "committed" {
            (Some(reference), Some("HEAD"))
        } else {
            (Some(reference), None)
        }
    };

    // File list + stats + untracked (git.ts:344-371 + statusNames).
    let mut items = match &reference {
        Some(reference) => {
            let mut names = git_diff_names(scope, reference, target);
            // diffNames covers tracked changes; working-scope diffs add
            // the untracked files from status (git.ts:601-609).
            if target.is_none() {
                names.extend(git_status_items(scope).into_iter().filter(|item| item.code == "??"));
            }
            names
        }
        None => git_status_items(scope),
    };
    items.sort_by(|a, b| a.file.cmp(&b.file));
    items.dedup_by(|a, b| a.file == b.file);

    // Per-file stats (numstat; untracked via --no-index).
    let mut stats: std::collections::HashMap<String, (i64, i64)> = match &reference {
        Some(reference) => git_numstat(scope, reference, target),
        None => std::collections::HashMap::new(),
    };
    for item in &items {
        if item.code == "??" && !stats.contains_key(&item.file) {
            if let Some(stat) = git_stat_untracked(scope, &item.file) {
                stats.insert(item.file.clone(), stat);
            }
        }
    }

    // Batched patches (one git diff invocation, capped, split per file)
    // with the per-file native fallback (git.ts:490-560).
    let mut batch: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut batch_capped = false;
    if let Some(reference) = &reference {
        if !items.is_empty() {
            let mut args = vec![
                "diff".to_owned(),
                "--patch".to_owned(),
                "--no-ext-diff".to_owned(),
                "--no-renames".to_owned(),
                format!("--unified={context}"),
                reference.clone(),
            ];
            if let Some(target) = target {
                args.push(target.to_owned());
            }
            args.push("--".to_owned());
            args.push(".".to_owned());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let output = std::process::Command::new("git")
                .args(refs.as_slice())
                .current_dir(scope)
                .stdin(std::process::Stdio::null())
                .output();
            if let Ok(output) = output {
                let text = String::from_utf8_lossy(&output.stdout).into_owned();
                batch_capped = text.len() > MAX_TOTAL_PATCH_BYTES;
                for (file, chunk) in split_git_patch(&text) {
                    *batch.entry(file).or_default() += &chunk;
                }
            }
        }
    }

    // Assemble FileDiff.Info[] under the byte budgets (git.ts:568-608).
    let mut files: Vec<Value> = Vec::new();
    let mut total = 0usize;
    let mut capped = false;
    for item in &items {
        let stat = stats.get(&item.file).copied().unwrap_or((0, 0));
        let patch = if capped {
            empty_patch(&item.file)
        } else {
            match batch.get(&item.file) {
                Some(patch) => patch.clone(),
                None => {
                    if item.code != "??" && batch_capped {
                        empty_patch(&item.file)
                    } else {
                        native_patch(scope, reference.as_deref(), item, target, context)
                    }
                }
            }
        };
        let (patch, this_capped) = if capped {
            (patch, true)
        } else if total + patch.len() <= MAX_TOTAL_PATCH_BYTES {
            (patch, false)
        } else {
            (empty_patch(&item.file), true)
        };
        capped = capped || this_capped;
        if !capped {
            total += patch.len();
            capped = total >= MAX_TOTAL_PATCH_BYTES;
        }
        files.push(json!({
            "file": item.file,
            "patch": patch,
            "additions": stat.0,
            "deletions": stat.1,
            "status": item.status,
        }));
    }
    Ok(super::common::json_location_data(
        &location.as_path().to_string_lossy(),
        Value::Array(files),
    ))
}

/// `diff --name-status -z` (git.ts:344-359).
fn git_diff_names(scope: &std::path::Path, reference: &str, target: Option<&str>) -> Vec<DiffItem> {
    let mut args = vec![
        "diff".to_owned(),
        "--no-ext-diff".to_owned(),
        "--no-renames".to_owned(),
        "--name-status".to_owned(),
        "-z".to_owned(),
        reference.to_owned(),
    ];
    if let Some(target) = target {
        args.push(target.to_owned());
    }
    args.push("--".to_owned());
    args.push(".".to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(output) = git_output(scope, refs.as_slice()) else {
        return Vec::new();
    };
    let list = nuls(&output);
    let mut items = Vec::new();
    let mut index = 0;
    while index + 1 < list.len() {
        items.push(DiffItem {
            code: list[index].clone(),
            file: list[index + 1].clone(),
            status: diff_kind(&list[index]),
        });
        index += 2;
    }
    items
}

/// `status --porcelain -z` untracked scan (git.ts:353-367).
fn git_status_items(scope: &std::path::Path) -> Vec<DiffItem> {
    let Some(output) = git_output(
        scope,
        &["status", "--porcelain=v1", "--untracked-files=all", "--no-renames", "-z", "--", "."],
    ) else {
        return Vec::new();
    };
    nuls(&output)
        .into_iter()
        .filter_map(|entry| {
            let file = entry.get(3..)?.to_owned();
            let code = entry.get(..2)?.to_owned();
            (!file.is_empty()).then(|| DiffItem {
                code: code.clone(),
                file,
                status: diff_kind(&code),
            })
        })
        .collect()
}

/// `diff --numstat -z` (git.ts:361-384).
fn git_numstat(
    scope: &std::path::Path,
    reference: &str,
    target: Option<&str>,
) -> std::collections::HashMap<String, (i64, i64)> {
    let mut args = vec![
        "diff".to_owned(),
        "--no-ext-diff".to_owned(),
        "--no-renames".to_owned(),
        "--numstat".to_owned(),
        "-z".to_owned(),
        reference.to_owned(),
    ];
    if let Some(target) = target {
        args.push(target.to_owned());
    }
    args.push("--".to_owned());
    args.push(".".to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(output) = git_output(scope, refs.as_slice()) else {
        return std::collections::HashMap::new();
    };
    let mut map = std::collections::HashMap::new();
    for entry in nuls(&output) {
        let Some((additions, rest)) = entry.split_once('\t') else { continue };
        let Some((deletions, file)) = rest.split_once('\t') else { continue };
        if file.is_empty() {
            continue;
        }
        let parse = |value: &str| match value {
            "-" => 0,
            other => other.parse::<i64>().unwrap_or(0),
        };
        map.insert(file.to_owned(), (parse(additions), parse(deletions)));
    }
    map
}

/// `diff --no-index --numstat /dev/null <file>` for untracked additions
/// (git.ts:442-458).
fn git_stat_untracked(scope: &std::path::Path, file: &str) -> Option<(i64, i64)> {
    let output = std::process::Command::new("git")
        .args(["diff", "--no-index", "--numstat", "--", "/dev/null", file])
        .current_dir(scope)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut parts = text.split('\t');
    let addition = parts.next()?;
    let deletion = parts.next()?;
    let parse = |value: &str| match value {
        "-" => 0,
        other => other.parse::<i64>().unwrap_or(0),
    };
    Some((parse(addition), parse(deletion)))
}

/// Per-file native patch (git.ts:386-404 / 406-421 / 423-440).
fn native_patch(
    scope: &std::path::Path,
    reference: Option<&str>,
    item: &DiffItem,
    target: Option<&str>,
    context: i64,
) -> String {
    let mut args = vec![
        "diff".to_owned(),
        "--patch".to_owned(),
        "--no-ext-diff".to_owned(),
        "--no-renames".to_owned(),
        format!("--unified={context}"),
    ];
    let output = if item.code == "??" || reference.is_none() {
        args.extend([
            "--no-index".to_owned(),
            "--".to_owned(),
            "/dev/null".to_owned(),
            item.file.clone(),
        ]);
        std::process::Command::new("git")
            .args(args.iter().map(String::as_str).collect::<Vec<_>>().as_slice())
            .current_dir(scope)
            .stdin(std::process::Stdio::null())
            .output()
    } else {
        args.push(reference.unwrap_or("HEAD").to_owned());
        if let Some(target) = target {
            args.push(target.to_owned());
        }
        args.push("--".to_owned());
        args.push(item.file.clone());
        std::process::Command::new("git")
            .args(args.iter().map(String::as_str).collect::<Vec<_>>().as_slice())
            .current_dir(scope)
            .stdin(std::process::Stdio::null())
            .output()
    };
    match output {
        Ok(output) if output.stdout.len() <= MAX_PATCH_BYTES && !output.stdout.is_empty() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        _ => empty_patch(&item.file),
    }
}

/// `splitGitPatch` + `chunksByFile` (vcs/patch.ts:93-110): split a
/// multi-file `git diff` at `diff --git` headers and map each chunk to
/// its file (from the `+++ ` line, falling back to the header's b-side).
fn split_git_patch(text: &str) -> Vec<(String, String)> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            starts.push(offset);
        }
        offset += line.len();
    }
    if starts.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(text.len());
        chunks.push(&text[*start..end]);
    }
    chunks
        .into_iter()
        .filter_map(|chunk| {
            let file = chunk.lines().find_map(|line| {
                let path = line.strip_prefix("+++ ")?;
                let path = path.split('\t').next().unwrap_or(path);
                let path = path.strip_prefix("b/").unwrap_or(path);
                (!path.is_empty() && path != "/dev/null").then(|| path.to_owned())
            });
            let file = file.or_else(|| {
                chunk.lines().find_map(|line| {
                    let header = line.strip_prefix("diff --git ")?;
                    let (_, b) = header.split_once(" b/")?;
                    (!b.trim().is_empty()).then(|| b.trim().to_owned())
                })
            })?;
            Some((file, chunk.to_owned()))
        })
        .collect()
}

/// `mergeBase` (git.ts:325-333): resolve, then `merge-base <ref> HEAD`.
fn git_merge_base(scope: &std::path::Path, base: &str) -> Option<String> {
    let resolved = git_output(
        scope,
        &["rev-parse", "--verify", "--end-of-options", &format!("{base}^{{commit}}")],
    )?
    .trim()
    .to_owned();
    if resolved.is_empty() {
        return None;
    }
    let merged = git_output(scope, &["merge-base", &resolved, "HEAD"])?.trim().to_owned();
    (!merged.is_empty()).then_some(merged)
}

async fn vcs_status(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let output =
        git_output(services.project.canonical.as_path(), &["status", "--porcelain=v1", "-z"])
            .unwrap_or_default();
    let mut data = Vec::new();
    for record in output.split('\0') {
        if record.len() < 4 {
            continue;
        }
        let (code, path) = record.split_at(3);
        let path = path.trim_start();
        data.push(json!({
            "file": path,
            "status": status_code_to_kind(code),
            "additions": 0,
            "deletions": 0,
        }));
    }
    Ok(super::common::json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

fn status_code_to_kind(code: &str) -> &'static str {
    match code.trim() {
        "A" | "A " | "AM " | "??" => "added",
        "D" | "D " | "AD" => "deleted",
        // Everything else (M/MM/AM/R/C and unknown codes) reads as
        // modified — the v2 mapping buckets renames/copies there too.
        _ => "modified",
    }
}

async fn vcs_init(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let provider = query_field(&uri, "provider");
    if let Some(provider) = provider {
        if provider != "git" {
            return Err(ApiError::vcs_init_not_supported(&provider));
        }
    }
    // The location's services may pre-date the init (cached bundle with
    // vcs = None); the filesystem is the source of truth.
    if services.project.canonical.join(".git").exists() || services.project.vcs.is_some() {
        return Err(ApiError::conflict("Repository already exists"));
    }
    let output = std::process::Command::new("git")
        .arg("init")
        .current_dir(services.project.canonical.as_path())
        .output()
        .map_err(|error| {
            crate::errors::debug_cause("project:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if !output.status.success() {
        return Err(ApiError::unknown_ref());
    }
    Ok(no_content())
}

fn git_output(directory: &std::path::Path, args: &[&str]) -> Option<String> {
    let output =
        std::process::Command::new("git").args(args).current_dir(directory).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
