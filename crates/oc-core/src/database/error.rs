//! Error type for the database service (`src/database/`).

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
}

/// Result alias for the database layer.
pub type Result<T> = std::result::Result<T, DbError>;
