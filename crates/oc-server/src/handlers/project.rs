//! Project + worktree + vcs handlers.

#![allow(clippy::must_use_candidate, clippy::similar_names, clippy::type_complexity)]
use std::path::PathBuf;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::common::{no_content, resolve_location, ApiError, SharedState};
use super::session::{query_field, services};

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
    let db = state.engine.db.clone();
    let (id, name, icon_url_override, icon_color) = (
        project_id.clone(),
        body["name"].as_str().map(String::from),
        body["icon"].get("override").and_then(Value::as_str).map(String::from),
        body["icon"].get("color").and_then(Value::as_str).map(String::from),
    );
    let updated = db
        .transaction(move |tx| {
            tx.execute(
                "UPDATE project SET \
                   name = coalesce(?2, name), \
                   icon_url_override = coalesce(?3, icon_url_override), \
                   icon_color = coalesce(?4, icon_color), \
                   time_updated = ?5 \
                 WHERE id = ?1",
                (id, name, icon_url_override, icon_color, oc_core::database::migrations::now_ms()),
            )
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("project:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if updated == 0 {
        return Err(ApiError::project_not_found(&project_id));
    }
    let row = (
        project_id.clone(),
        String::new(),
        None,
        body["name"].as_str().map(String::from),
        None,
        body["icon"].get("override").and_then(Value::as_str).map(String::from),
        body["icon"].get("color").and_then(Value::as_str).map(String::from),
        "[]".to_owned(),
        None,
        0,
        0,
        0,
    );
    Ok(axum::Json(project_json(&row)).into_response())
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

fn body_project_id(body: &Value) -> String {
    body["projectID"].as_str().unwrap_or_default().to_owned()
}

/// v2 `Slug.create()` (util/slug.ts) — a random adjective-noun pair.
mod slug {
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
        use rand::seq::SliceRandom;
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
        ApiError::worktree(
            "directory_unavailable",
            format!("Worktree directory unavailable: {}", directory.display()),
            None,
        )
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
    let directory =
        oc_core::config::latest(&snapshot.entries, |info| info.worktree.as_ref().map(|wt| wt.directory.clone()))?;
    let path = if let Some(rest) = directory.strip_prefix("~/") {
        state.engine.home.join(rest)
    } else {
        project_worktree.join(directory)
    };
    Some(path)
}

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
        return Err(ApiError::worktree("invalid_name", "Worktree name is required", None));
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
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                        ))
                    },
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
                "source_not_found",
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
        ApiError::worktree(
            "directory_unavailable",
            format!("Worktree directory unavailable: {}", parent.display()),
            None,
        )
    })?;

    // Name + collision suffixes (v2: `name-2` … `name-10`, then
    // `DestinationExistsError`).
    let name = name.map(String::from).unwrap_or_else(slug::create);
    let mut worktree_directory = None;
    for suffix in 1..=10u32 {
        let candidate = if suffix == 1 {
            parent.join(&name)
        } else {
            parent.join(format!("{name}-{suffix}"))
        };
        if !candidate.exists() {
            worktree_directory = Some(candidate);
            break;
        }
    }
    let Some(worktree_directory) = worktree_directory else {
        return Err(ApiError::worktree(
            "destination_exists",
            format!("Worktree destination already exists: {}", parent.join(format!("{name}-10")).display()),
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
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !inside_repo {
        return Err(ApiError::worktree(
            "directory_unavailable",
            format!("Worktree directory unavailable: {}", source.display()),
            None,
        ));
    }
    let reference = branch.as_deref().unwrap_or("HEAD");
    let added = std::process::Command::new("git")
        .args(["worktree", "add", "--detach", "--"])
        .arg(&worktree_directory)
        .arg(reference)
        .current_dir(&source)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| {
            crate::errors::debug_cause("worktree:spawn", &error);
            ApiError::worktree("operation", format!("Git failed: {error}"), None)
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
        return Err(ApiError::worktree("operation", message, None));
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
                return Err(ApiError::worktree("operation", message, None));
            }
            Err(error) => {
                crate::errors::debug_cause("worktree:start", &error);
                return Err(ApiError::worktree("operation", format!("Start command failed: {error}"), None));
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

async fn worktree_refresh(axum::Json(_body): axum::Json<Value>) -> Result<Response, ApiError> {
    // Refresh re-scans git worktrees into the table (vcs lane surface,
    // TODO(phase-9)); the row set is already queryable.
    Ok(no_content())
}

async fn worktree_remove(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (project_id, directory, force) = (
        body["projectID"].as_str().unwrap_or_default().to_owned(),
        body["directory"].as_str().unwrap_or_default().to_owned(),
        body["force"].as_bool().unwrap_or(false),
    );
    let db = state.engine.db.clone();
    let outer_directory = directory.clone();
    let removed = db
        .transaction(move |tx| {
            tx.execute(
                "DELETE FROM worktree WHERE project_id = ?1 AND directory = ?2",
                (project_id, directory),
            )
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("project:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    if removed == 0 && !force {
        return Err(ApiError::worktree(
            "not_found",
            format!("Worktree not found: {outer_directory}"),
            None,
        ));
    }
    // worktree.ts:270 — removal of an owned row publishes `worktree.updated`.
    if removed > 0 {
        publish_worktree_updated(&state, &body_project_id(&body)).await;
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
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let base = git_output(services.project.canonical.as_path(), &["merge-base", "HEAD", "@{u}"])
        .map_or(Value::Null, |output| {
            let name = output.trim().to_owned();
            json!({ "name": name, "ref": name })
        });
    Ok(super::common::json_location_data(&location.as_path().to_string_lossy(), base))
}

async fn vcs_branch_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let search = query_field(&uri, "search");
    let limit = query_field(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .map_or(50usize, |value| value.min(100));
    let output =
        git_output(services.project.canonical.as_path(), &["branch", "--format=%(refname:short)"])
            .unwrap_or_default();
    let mut branches: Vec<String> = output.lines().map(str::trim).map(String::from).collect();
    if let Some(search) = &search {
        let lowered = search.to_lowercase();
        branches.retain(|branch| branch.to_lowercase().contains(&lowered));
    }
    branches.truncate(limit);
    Ok(super::common::json_location_data(
        &location.as_path().to_string_lossy(),
        json!({ "branches": branches }),
    ))
}

async fn vcs_diff(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let base = query_field(&uri, "base");
    let context = query_field(&uri, "context").unwrap_or_else(|| "3".to_owned());
    let mode = query_field(&uri, "mode");
    let mut args = vec!["diff".to_owned(), format!("--unified={context}")];
    if mode.as_deref() == Some("staged") {
        args.push("--cached".to_owned());
    }
    if let Some(base) = base {
        args.push(base);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = git_output(services.project.canonical.as_path(), &refs).unwrap_or_default();
    // A raw unified-diff string array (one hunk per entry) is the v2 shape;
    // the file-level diff parse rides the file_diff lane (TODO(phase-9)).
    let hunks: Vec<Value> = if output.trim().is_empty() {
        Vec::new()
    } else {
        vec![json!({ "path": "diff", "status": { "type": "modified" }, "chunks": [output] })]
    };
    Ok(super::common::json_location_data(
        &location.as_path().to_string_lossy(),
        Value::Array(hunks),
    ))
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
