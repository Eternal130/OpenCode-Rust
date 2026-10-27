//! Error type for the database service (`src/database/`).

/// A recoverable session-level conflict raised inside a projector (or
/// publish-commit) transaction and tunneled out through [`DbError`].
///
/// These used to travel as sentinel-prefixed strings inside
/// [`DbError::Pragma`]; the typed variants make every raise/catch site
/// pattern-matched (a missed strip or prefix collision can no longer turn
/// a recoverable conflict into an undiagnosable database error).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionConflict {
    /// `session.created`/`session.forked` lost the projection race — the
    /// caller re-reads the existing session identity.
    #[error("session already projected: {0}")]
    AlreadyProjected(String),
    /// `SessionInbox.LifecycleConflict` — admission/delivery/cancel lost a
    /// race; the conflicting inbox id is carried.
    #[error("session inbox lifecycle conflict: {0}")]
    InboxConflict(String),
    /// An optimistic-concurrency title rename lost its expected-sequence
    /// race (`\u{0}title-changed\u{1}` sentinel); the caller retries with a
    /// fresh expectation.
    #[error("session title changed concurrently: {0}")]
    TitleChanged(String),
}

/// Errors surfaced by the database layer.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// Underlying rusqlite failure.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// The blocking task running a database operation failed to join.
    #[error("database task failed: {0}")]
    Join(#[from] tokio::task::JoinError),

    /// Filesystem failure while preparing the database file.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON (de)serialization failure on a JSON-typed column.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// `sqlite_master` contains non-opencode tables but no `session`/`session_v2`.
    #[error("Database is not empty and has no session table")]
    NotEmptyNoSession,

    /// A `__drizzle_migrations` row's `created_at` matches no known migration id.
    #[error("Legacy migration timestamp {0} does not match any known migration")]
    LegacyTimestampUnmatched(i64),

    /// `20260804233008_loose_psylocke` found V1-only history in a pre-split V2 database.
    #[error("Previous V2 database contains V1-only session history")]
    V1OnlyHistory,

    /// `20260805200742_import_legacy_credentials` found a non-object auth.json.
    #[error("Legacy credential file must contain an object")]
    LegacyCredentialShape,

    /// A migration requiring `Global.data` ran without it.
    #[error("migration requires the global data directory (Global.data)")]
    MissingGlobalData,

    /// A path column rejected a non-absolute value.
    #[error("Path is not absolute: {0}")]
    NotAbsolute(String),

    /// A boot-time pragma did not take effect.
    #[error("pragma failed: {0}")]
    Pragma(String),

    /// A recoverable session conflict tunneled out of a projector
    /// transaction (typed; see [`SessionConflict`]).
    #[error(transparent)]
    SessionConflict(#[from] SessionConflict),
}

/// Result alias for the database layer.
pub type Result<T> = std::result::Result<T, DbError>;
