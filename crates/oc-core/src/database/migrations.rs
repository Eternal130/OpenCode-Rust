//! Migration engine: bootstrap + 48 incremental migrations + legacy
//! `__drizzle_migrations` journal seeding.
//!
//! Port of `packages/core/src/database/migration.ts` + `migration/*.ts` (48
//! files, all transliterated). Journal semantics (`core-deep-map.md` §1a):
//!
//! - Journal table `migration(id TEXT PRIMARY KEY, time_completed INTEGER NOT
//!   NOT NULL)`; "already applied" = id present; no hash column.
//! - Classification by scanning `sqlite_master` for opencode tables (not
//!   `sqlite_%`, not `_`-prefixed): `session`/`session_v2` → incremental;
//!   other tables without a session table → die; empty → single-transaction
//!   bootstrap (snapshot DDL + journal + all 48 ids), never running the files.
//! - Each migration runs in its own transaction (`up` + journal insert).
//! - `foreignKeys: false` migrations are wrapped with `PRAGMA foreign_keys =
//!   OFF` before the transaction and restored to `ON` after (restore failure
//!   is fatal, mirroring `Effect.ensuring(...orDie)`).
//! - Legacy seeding only when `migration` is empty but `__drizzle_migrations`
//!   exists: name-column variant bulk-imports names; `hash/created_at` variant
//!   prefix-matches `strftime('%Y%m%d%H%M%S', created_at/1000, 'unixepoch')`
//!   against migration ids and dies on unmatched timestamps.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use super::bootstrap::{
    BOOTSTRAP_SQL, MIGRATION_IDS, MIGRATION_TABLE_IF_NOT_EXISTS_SQL, MIGRATION_TABLE_SQL,
};
use super::error::{DbError, Result};
use super::{run_top_tx, Begin, Tx};

/// Marker identifying the completed pre-split V2 lineage
/// (`20260804233008_loose_psylocke.ts`).
const PREVIOUS_V2_MARKER: &str = "20260730195856_optional_session_title";

/// Key the legacy-credential import merges wellknown origins into.
const WELLKNOWN_SOURCES_KEY: &str = "wellknown:sources";

/// A single named migration.
#[derive(Debug)]
pub struct Migration {
    /// `YYYYMMDDHHMMSS_slug` (drizzle-kit naming).
    pub id: &'static str,
    /// `false` → the runner disables `PRAGMA foreign_keys` around this
    /// migration's transaction.
    pub foreign_keys: bool,
    /// What to run.
    pub body: MigrationBody,
}

/// The executable body of a migration.
#[derive(Debug)]
pub enum MigrationBody {
    /// A fixed sequence of SQL statements executed in order inside the
    /// migration's transaction.
    Statements(&'static [&'static str]),
    /// `20260410174513_workspace-name`: `PRAGMA table_info` column probe.
    WorkspaceName,
    /// `20260511173437_session-metadata`: skip when `session.metadata` already
    /// exists (shipped twice under a second id).
    SessionMetadata,
    /// `20260804233008_loose_psylocke`: pre-split V2 rename vs. re-create.
    LoosePsylocke,
    /// `20260805200742_import_legacy_credentials`: reads `auth.json`.
    ImportLegacyCredentials,
}

/// Inputs migrations may need beyond the connection.
#[derive(Debug, Clone, Default)]
pub struct MigrationOptions {
    /// `Global.data` — required by the legacy-credential import (`auth.json`
    /// lives there).
    pub global_data: Option<PathBuf>,
}

impl MigrationOptions {
    /// Options with the `Global.data` root from [`oc_util::global_roots`].
    #[must_use]
    pub fn with_global_data() -> Self {
        Self { global_data: Some(oc_util::global_roots::resolve().data) }
    }
}

/// Outcome of one `apply` run.
#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    /// Fresh database: the snapshot bootstrap transaction ran.
    pub bootstrapped: bool,
    /// The legacy drizzle journal was found and seeded into `migration`.
    pub seeded_legacy: bool,
    /// Migration ids applied by this run, in order (empty when the journal was
    /// already complete).
    pub applied: Vec<&'static str>,
}

/// `Date.now()` — milliseconds since the unix epoch.
#[must_use]
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => i64::try_from(delta.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// The 48 migrations in strict `migration.gen.ts` order.
pub static MIGRATIONS: &[Migration] = &[
    Migration {
        id: "20260127222353_familiar_lady_ursula",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `project` (`id` text PRIMARY KEY, `worktree` text NOT NULL, `vcs` text, `name` text, `icon_url` text, `icon_color` text, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `time_initialized` integer, `sandboxes` text NOT NULL);",
            "CREATE TABLE `message` (`id` text PRIMARY KEY, `session_id` text NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `data` text NOT NULL, CONSTRAINT `fk_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE);",
            "CREATE TABLE `part` (`id` text PRIMARY KEY, `message_id` text NOT NULL, `session_id` text NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `data` text NOT NULL, CONSTRAINT `fk_part_message_id_message_id_fk` FOREIGN KEY (`message_id`) REFERENCES `message`(`id`) ON DELETE CASCADE);",
            "CREATE TABLE `permission` (`project_id` text PRIMARY KEY, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `data` text NOT NULL, CONSTRAINT `fk_permission_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);",
            "CREATE TABLE `session` (`id` text PRIMARY KEY, `project_id` text NOT NULL, `parent_id` text, `slug` text NOT NULL, `directory` text NOT NULL, `title` text NOT NULL, `version` text NOT NULL, `share_url` text, `summary_additions` integer, `summary_deletions` integer, `summary_files` integer, `summary_diffs` text, `revert` text, `permission` text, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `time_compacting` integer, `time_archived` integer, CONSTRAINT `fk_session_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);",
            "CREATE TABLE `todo` (`session_id` text NOT NULL, `content` text NOT NULL, `status` text NOT NULL, `priority` text NOT NULL, `position` integer NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, CONSTRAINT `todo_pk` PRIMARY KEY(`session_id`, `position`), CONSTRAINT `fk_todo_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE);",
            "CREATE TABLE `session_share` (`session_id` text PRIMARY KEY, `id` text NOT NULL, `secret` text NOT NULL, `url` text NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, CONSTRAINT `fk_session_share_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE);",
            "CREATE INDEX `message_session_idx` ON `message` (`session_id`);",
            "CREATE INDEX `part_message_idx` ON `part` (`message_id`);",
            "CREATE INDEX `part_session_idx` ON `part` (`session_id`);",
            "CREATE INDEX `session_project_idx` ON `session` (`project_id`);",
            "CREATE INDEX `session_parent_idx` ON `session` (`parent_id`);",
            "CREATE INDEX `todo_session_idx` ON `todo` (`session_id`);",
        ]),
    },
    Migration {
        id: "20260211171708_add_project_commands",
        foreign_keys: true,
        body: MigrationBody::Statements(&["ALTER TABLE `project` ADD `commands` text;"]),
    },
    Migration {
        id: "20260213144116_wakeful_the_professor",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `control_account` (`email` text NOT NULL, `url` text NOT NULL, `access_token` text NOT NULL, `refresh_token` text NOT NULL, `token_expiry` integer, `active` integer NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, CONSTRAINT `control_account_pk` PRIMARY KEY(`email`, `url`));",
        ]),
    },
    Migration {
        id: "20260225215848_workspace",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `workspace` (`id` text PRIMARY KEY, `branch` text, `project_id` text NOT NULL, `config` text NOT NULL, CONSTRAINT `fk_workspace_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);",
        ]),
    },
    Migration {
        id: "20260227213759_add_session_workspace_id",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `session` ADD `workspace_id` text;",
            "CREATE INDEX `session_workspace_idx` ON `session` (`workspace_id`);",
        ]),
    },
    Migration {
        id: "20260228203230_blue_harpoon",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `account` (`id` text PRIMARY KEY, `email` text NOT NULL, `url` text NOT NULL, `access_token` text NOT NULL, `refresh_token` text NOT NULL, `token_expiry` integer, `selected_org_id` text, `time_created` integer NOT NULL, `time_updated` integer NOT NULL);",
            "CREATE TABLE `account_state` (`id` integer PRIMARY KEY NOT NULL, `active_account_id` text, FOREIGN KEY (`active_account_id`) REFERENCES `account`(`id`) ON UPDATE no action ON DELETE set null);",
        ]),
    },
    Migration {
        id: "20260303231226_add_workspace_fields",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `workspace` ADD `type` text NOT NULL;",
            "ALTER TABLE `workspace` ADD `name` text;",
            "ALTER TABLE `workspace` ADD `directory` text;",
            "ALTER TABLE `workspace` ADD `extra` text;",
            "ALTER TABLE `workspace` DROP COLUMN `config`;",
        ]),
    },
    Migration {
        id: "20260309230000_move_org_to_state",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `account_state` ADD `active_org_id` text;",
            "UPDATE `account_state` SET `active_org_id` = (SELECT `selected_org_id` FROM `account` WHERE `account`.`id` = `account_state`.`active_account_id`);",
            "ALTER TABLE `account` DROP COLUMN `selected_org_id`;",
        ]),
    },
    Migration {
        id: "20260312043431_session_message_cursor",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DROP INDEX IF EXISTS `message_session_idx`;",
            "DROP INDEX IF EXISTS `part_message_idx`;",
            "CREATE INDEX `message_session_time_created_id_idx` ON `message` (`session_id`,`time_created`,`id`);",
            "CREATE INDEX `part_message_id_id_idx` ON `part` (`message_id`,`id`);",
        ]),
    },
    Migration {
        id: "20260323234822_events",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `event_sequence` (`aggregate_id` text PRIMARY KEY, `seq` integer NOT NULL);",
            "CREATE TABLE `event` (`id` text PRIMARY KEY, `aggregate_id` text NOT NULL, `seq` integer NOT NULL, `type` text NOT NULL, `data` text NOT NULL, CONSTRAINT `fk_event_aggregate_id_event_sequence_aggregate_id_fk` FOREIGN KEY (`aggregate_id`) REFERENCES `event_sequence`(`aggregate_id`) ON DELETE CASCADE);",
        ]),
    },
    Migration {
        id: "20260410174513_workspace-name",
        foreign_keys: true,
        body: MigrationBody::WorkspaceName,
    },
    Migration {
        id: "20260413175956_chief_energizer",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `session_entry` (`id` text PRIMARY KEY, `session_id` text NOT NULL, `type` text NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `data` text NOT NULL, CONSTRAINT `fk_session_entry_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE);",
            "CREATE INDEX `session_entry_session_idx` ON `session_entry` (`session_id`);",
            "CREATE INDEX `session_entry_session_type_idx` ON `session_entry` (`session_id`,`type`);",
            "CREATE INDEX `session_entry_time_created_idx` ON `session_entry` (`time_created`);",
        ]),
    },
    Migration {
        id: "20260423070820_add_icon_url_override",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `project` ADD `icon_url_override` text;",
            "UPDATE `project` SET `icon_url_override` = `icon_url` WHERE `icon_url` IS NOT NULL;",
        ]),
    },
    Migration {
        id: "20260427172553_slow_nightmare",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `session_message` (`id` text PRIMARY KEY, `session_id` text NOT NULL, `type` text NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `data` text NOT NULL, CONSTRAINT `fk_session_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE);",
            "DROP INDEX IF EXISTS `session_entry_session_idx`;",
            "DROP INDEX IF EXISTS `session_entry_session_type_idx`;",
            "DROP INDEX IF EXISTS `session_entry_time_created_idx`;",
            "CREATE INDEX `session_message_session_idx` ON `session_message` (`session_id`);",
            "CREATE INDEX `session_message_session_type_idx` ON `session_message` (`session_id`,`type`);",
            "CREATE INDEX `session_message_time_created_idx` ON `session_message` (`time_created`);",
            "DROP TABLE `session_entry`;",
        ]),
    },
    Migration {
        id: "20260428004200_add_session_path",
        foreign_keys: true,
        body: MigrationBody::Statements(&["ALTER TABLE `session` ADD `path` text;"]),
    },
    Migration {
        id: "20260501142318_next_venus",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `session` ADD `agent` text;",
            "ALTER TABLE `session` ADD `model` text;",
        ]),
    },
    Migration {
        id: "20260504145000_add_sync_owner",
        foreign_keys: true,
        body: MigrationBody::Statements(&["ALTER TABLE `event_sequence` ADD `owner_id` text;"]),
    },
    Migration {
        id: "20260507164347_add_workspace_time",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `workspace` ADD `time_used` integer NOT NULL DEFAULT 0;",
        ]),
    },
    Migration {
        id: "20260510033149_session_usage",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `session` ADD `cost` real DEFAULT 0 NOT NULL;",
            "ALTER TABLE `session` ADD `tokens_input` integer DEFAULT 0 NOT NULL;",
            "ALTER TABLE `session` ADD `tokens_output` integer DEFAULT 0 NOT NULL;",
            "ALTER TABLE `session` ADD `tokens_reasoning` integer DEFAULT 0 NOT NULL;",
            "ALTER TABLE `session` ADD `tokens_cache_read` integer DEFAULT 0 NOT NULL;",
            "ALTER TABLE `session` ADD `tokens_cache_write` integer DEFAULT 0 NOT NULL;",
            "UPDATE session SET cost = coalesce((SELECT sum(coalesce(json_extract(message.data, '$.cost'), 0)) FROM message WHERE message.session_id = session.id AND json_extract(message.data, '$.role') = 'assistant'), 0), tokens_input = coalesce((SELECT sum(coalesce(json_extract(message.data, '$.tokens.input'), 0)) FROM message WHERE message.session_id = session.id AND json_extract(message.data, '$.role') = 'assistant'), 0), tokens_output = coalesce((SELECT sum(coalesce(json_extract(message.data, '$.tokens.output'), 0)) FROM message WHERE message.session_id = session.id AND json_extract(message.data, '$.role') = 'assistant'), 0), tokens_reasoning = coalesce((SELECT sum(coalesce(json_extract(message.data, '$.tokens.reasoning'), 0)) FROM message WHERE message.session_id = session.id AND json_extract(message.data, '$.role') = 'assistant'), 0), tokens_cache_read = coalesce((SELECT sum(coalesce(json_extract(message.data, '$.tokens.cache.read'), 0)) FROM message WHERE message.session_id = session.id AND json_extract(message.data, '$.role') = 'assistant'), 0), tokens_cache_write = coalesce((SELECT sum(coalesce(json_extract(message.data, '$.tokens.cache.write'), 0)) FROM message WHERE message.session_id = session.id AND json_extract(message.data, '$.role') = 'assistant'), 0)",
        ]),
    },
    Migration {
        id: "20260511000411_data_migration_state",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `data_migration` (`name` text PRIMARY KEY, `time_completed` integer NOT NULL);",
        ]),
    },
    Migration {
        id: "20260511173437_session-metadata",
        foreign_keys: true,
        body: MigrationBody::SessionMetadata,
    },
    Migration {
        id: "20260601010001_normalize_storage_paths",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "UPDATE project SET worktree = REPLACE(worktree, char(92), '/') WHERE worktree GLOB '[A-Za-z]:' || char(92) || '*' OR worktree LIKE char(92) || char(92) || '%';",
            "UPDATE project SET sandboxes = REPLACE(sandboxes, char(92) || char(92), '/') WHERE instr(sandboxes, char(92)) > 0 AND (worktree GLOB '[A-Za-z]:*' OR worktree LIKE '//%');",
            "UPDATE session SET directory = REPLACE(directory, char(92), '/') WHERE directory GLOB '[A-Za-z]:' || char(92) || '*' OR directory LIKE char(92) || char(92) || '%';",
            "UPDATE session SET path = REPLACE(path, char(92), '/') WHERE path IS NOT NULL AND instr(path, char(92)) > 0 AND (directory GLOB '[A-Za-z]:*' OR directory LIKE '//%');",
        ]),
    },
    Migration {
        id: "20260601202201_amazing_prowler",
        foreign_keys: true,
        body: MigrationBody::Statements(&["DROP TABLE `permission`;"]),
    },
    Migration {
        id: "20260602002951_lowly_union_jack",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `permission` (`id` text PRIMARY KEY, `project_id` text NOT NULL, `action` text NOT NULL, `resource` text NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, CONSTRAINT `fk_permission_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);",
            "CREATE UNIQUE INDEX `permission_project_action_resource_idx` ON `permission` (`project_id`,`action`,`resource`);",
        ]),
    },
    Migration {
        id: "20260602182828_add_project_directories",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `project_directory` (`project_id` text NOT NULL, `directory` text NOT NULL, `type` text NOT NULL, `time_created` integer NOT NULL, CONSTRAINT `project_directory_pk` PRIMARY KEY(`project_id`, `directory`), CONSTRAINT `fk_project_directory_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);",
        ]),
    },
    Migration {
        id: "20260603001617_session_message_projection_indexes",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DROP INDEX IF EXISTS `session_message_session_idx`;",
            "DROP INDEX IF EXISTS `session_message_session_type_idx`;",
            "CREATE INDEX `event_aggregate_seq_idx` ON `event` (`aggregate_id`,`seq`);",
            "CREATE INDEX `session_message_session_time_created_id_idx` ON `session_message` (`session_id`,`time_created`,`id`);",
            "CREATE INDEX `session_message_session_type_time_created_id_idx` ON `session_message` (`session_id`,`type`,`time_created`,`id`);",
        ]),
    },
    Migration {
        id: "20260603040000_session_message_projection_order",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DELETE FROM `session_message`;",
            "ALTER TABLE `session_message` ADD COLUMN `seq` integer NOT NULL;",
            "DROP INDEX IF EXISTS `session_message_session_type_time_created_id_idx`;",
            "CREATE INDEX `session_message_session_seq_idx` ON `session_message` (`session_id`,`seq`);",
            "CREATE INDEX `session_message_session_type_seq_idx` ON `session_message` (`session_id`,`type`,`seq`);",
        ]),
    },
    Migration {
        id: "20260603141458_session_input_inbox",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `session_input` (`seq` integer PRIMARY KEY AUTOINCREMENT, `id` text NOT NULL UNIQUE, `session_id` text NOT NULL, `prompt` text NOT NULL, `delivery` text NOT NULL, `promoted_seq` integer, `time_created` integer NOT NULL, CONSTRAINT `fk_session_input_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE);",
            "CREATE INDEX `session_input_session_pending_seq_idx` ON `session_input` (`session_id`,`promoted_seq`,`seq`);",
        ]),
    },
    Migration {
        id: "20260603160727_jittery_ezekiel_stane",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DROP INDEX IF EXISTS `session_input_session_pending_seq_idx`;",
            "CREATE INDEX IF NOT EXISTS `event_aggregate_type_seq_idx` ON `event` (`aggregate_id`,`type`,`seq`);",
            "CREATE INDEX IF NOT EXISTS `session_input_session_pending_delivery_seq_idx` ON `session_input` (`session_id`,`promoted_seq`,`delivery`,`seq`);",
            "CREATE INDEX IF NOT EXISTS `session_message_session_time_created_id_idx` ON `session_message` (`session_id`,`time_created`,`id`);",
        ]),
    },
    Migration {
        id: "20260604172448_event_sourced_session_input",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DELETE FROM `session_input`;",
            "DELETE FROM `session_message`;",
            "DELETE FROM `event`;",
            "DELETE FROM `event_sequence`;",
            "UPDATE `session` SET `workspace_id` = NULL;",
            "DELETE FROM `workspace`;",
            "DROP INDEX IF EXISTS `event_aggregate_seq_idx`;",
            "CREATE UNIQUE INDEX `event_aggregate_seq_idx` ON `event` (`aggregate_id`,`seq`);",
            "DROP INDEX IF EXISTS `session_message_session_seq_idx`;",
            "CREATE UNIQUE INDEX `session_message_session_seq_idx` ON `session_message` (`session_id`,`seq`);",
            // PRAGMA inside a transaction is a no-op in SQLite; kept verbatim
            // from the TypeScript migration for fidelity.
            "PRAGMA foreign_keys=OFF;",
            "CREATE TABLE `__new_session_input` (`id` text PRIMARY KEY, `session_id` text NOT NULL, `prompt` text NOT NULL, `delivery` text NOT NULL, `admitted_seq` integer NOT NULL, `promoted_seq` integer, `time_created` integer NOT NULL, CONSTRAINT `fk_session_input_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE);",
            "DROP TABLE `session_input`;",
            "ALTER TABLE `__new_session_input` RENAME TO `session_input`;",
            "PRAGMA foreign_keys=ON;",
            "CREATE INDEX `session_input_session_pending_delivery_seq_idx` ON `session_input` (`session_id`,`promoted_seq`,`delivery`,`admitted_seq`);",
            "CREATE UNIQUE INDEX `session_input_session_admitted_seq_idx` ON `session_input` (`session_id`,`admitted_seq`);",
            "CREATE UNIQUE INDEX `session_input_session_promoted_seq_idx` ON `session_input` (`session_id`,`promoted_seq`);",
        ]),
    },
    Migration {
        id: "20260605003541_add_session_context_snapshot",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `session_context_epoch` (`session_id` text PRIMARY KEY, `baseline` text NOT NULL, `snapshot` text NOT NULL, `baseline_seq` integer NOT NULL, `replacement_seq` integer, `revision` integer DEFAULT 0 NOT NULL, CONSTRAINT `fk_session_context_epoch_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE);",
        ]),
    },
    Migration {
        id: "20260605042240_add_context_epoch_agent",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `session_context_epoch` ADD `agent` text DEFAULT 'build' NOT NULL;",
        ]),
    },
    Migration {
        id: "20260611035744_credential",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `credential` (`id` text PRIMARY KEY, `connector_id` text NOT NULL, `method_id` text NOT NULL, `label` text NOT NULL, `value` text NOT NULL, `active` integer DEFAULT false NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL);",
            "CREATE UNIQUE INDEX `credential_connector_active_idx` ON `credential` (`connector_id`) WHERE \"credential\".\"active\" = 1;",
        ]),
    },
    Migration {
        id: "20260611192811_lush_chimera",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DROP INDEX IF EXISTS `credential_connector_active_idx`;",
            "DROP TABLE `credential`;",
            "CREATE TABLE `credential` (`id` text PRIMARY KEY, `integration_id` text, `label` text NOT NULL, `value` text NOT NULL, `connector_id` text, `method_id` text, `active` integer, `time_created` integer NOT NULL, `time_updated` integer NOT NULL);",
        ]),
    },
    Migration {
        id: "20260612174303_project_dir_strategy",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `project_directory` ADD `strategy` text;",
            "PRAGMA foreign_keys=OFF;",
            "CREATE TABLE `__new_project_directory` (`project_id` text NOT NULL, `directory` text NOT NULL, `type` text, `strategy` text, `time_created` integer NOT NULL, CONSTRAINT `project_directory_pk` PRIMARY KEY(`project_id`, `directory`), CONSTRAINT `fk_project_directory_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);",
            "INSERT INTO `__new_project_directory`(`project_id`, `directory`, `type`, `time_created`) SELECT `project_id`, `directory`, `type`, `time_created` FROM `project_directory`;",
            "DROP TABLE `project_directory`;",
            "ALTER TABLE `__new_project_directory` RENAME TO `project_directory`;",
            "PRAGMA foreign_keys=ON;",
        ]),
    },
    Migration {
        id: "20260622142730_simplify_session_context_epoch",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `session_context_epoch` DROP COLUMN `agent`;",
            "ALTER TABLE `session_context_epoch` DROP COLUMN `replacement_seq`;",
            "ALTER TABLE `session_context_epoch` DROP COLUMN `revision`;",
        ]),
    },
    Migration {
        id: "20260622170816_reset_v2_session_state",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DELETE FROM `session_context_epoch`;",
            "DELETE FROM `session_input`;",
            "DELETE FROM `session_message`;",
            "DELETE FROM `event`;",
            "DELETE FROM `event_sequence`;",
        ]),
    },
    Migration {
        id: "20260622202450_simplify_session_input",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DELETE FROM `session_context_epoch`;",
            "DELETE FROM `session_input`;",
            "DELETE FROM `session_message`;",
            "DELETE FROM `event`;",
            "DELETE FROM `event_sequence`;",
            "UPDATE `session` SET `workspace_id` = NULL WHERE `workspace_id` IS NOT NULL;",
            "DELETE FROM `workspace`;",
        ]),
    },
    Migration {
        id: "20260804233008_loose_psylocke",
        foreign_keys: true,
        body: MigrationBody::LoosePsylocke,
    },
    Migration {
        id: "20260805200742_import_legacy_credentials",
        foreign_keys: true,
        body: MigrationBody::ImportLegacyCredentials,
    },
    Migration {
        id: "20260808023530_workspace_domain",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "DROP TABLE `workspace`;",
            "CREATE TABLE `workspace` (`id` text PRIMARY KEY, `provider` text NOT NULL, `binding` text NOT NULL, `created_at` integer NOT NULL, `last_used_at` integer NOT NULL);",
        ]),
    },
    Migration {
        id: "20260811161259_execution_claim_attempts",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `session_v2` ADD `resume_attempts` integer DEFAULT 0 NOT NULL;",
        ]),
    },
    Migration {
        id: "20260812181746_session_inbox",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `session_inbox` (`id` text PRIMARY KEY, `session_id` text NOT NULL, `type` text NOT NULL, `payload` text NOT NULL, `delivery` text NOT NULL, `enqueued_seq` integer NOT NULL, `time_created` integer NOT NULL, CONSTRAINT `fk_session_inbox_session_id_session_v2_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session_v2`(`id`) ON DELETE CASCADE);",
            "CREATE INDEX `session_inbox_session_delivery_seq_idx` ON `session_inbox` (`session_id`,`delivery`,`enqueued_seq`);",
            "CREATE UNIQUE INDEX `session_inbox_session_enqueued_seq_idx` ON `session_inbox` (`session_id`,`enqueued_seq`);",
        ]),
    },
    Migration {
        id: "20260812213948_worktree",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `worktree` (`project_id` text NOT NULL, `directory` text NOT NULL, `strategy` text, `time_created` integer NOT NULL, CONSTRAINT `worktree_pk` PRIMARY KEY(`project_id`, `directory`), CONSTRAINT `fk_worktree_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);",
            "INSERT INTO `worktree` (`project_id`, `directory`, `strategy`, `time_created`) SELECT `project_id`, `directory`, CASE WHEN `strategy` = 'git_worktree' THEN 'git' WHEN `strategy` IS NOT NULL THEN `strategy` WHEN `type` = 'git_worktree' THEN 'git' END, `time_created` FROM `project_directory`;",
        ]),
    },
    Migration {
        id: "20260819222447_session_viewed_state",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `session_v2` ADD `time_idle` integer;",
            "ALTER TABLE `session_v2` ADD `time_viewed` integer;",
            "ALTER TABLE `session_v2` ADD `idle_outcome` text;",
        ]),
    },
    Migration {
        id: "20260823191254_nullable_workspace_binding",
        foreign_keys: false,
        body: MigrationBody::Statements(&[
            "CREATE TABLE `__new_workspace` (`id` text PRIMARY KEY, `provider` text NOT NULL, `binding` text, `created_at` integer NOT NULL, `last_used_at` integer NOT NULL);",
            "INSERT INTO `__new_workspace`(`id`, `provider`, `binding`, `created_at`, `last_used_at`) SELECT `id`, `provider`, `binding`, `created_at`, `last_used_at` FROM `workspace`;",
            "DROP TABLE `workspace`;",
            "ALTER TABLE `__new_workspace` RENAME TO `workspace`;",
        ]),
    },
    Migration {
        id: "20260910120000_clear_v1_session_permission",
        foreign_keys: true,
        body: MigrationBody::Statements(&["UPDATE `session_v2` SET `permission` = NULL;"]),
    },
    Migration {
        id: "20260923013825_project_time_active",
        foreign_keys: true,
        body: MigrationBody::Statements(&[
            "ALTER TABLE `project` ADD `time_active` integer DEFAULT 0 NOT NULL;",
            "UPDATE `project` SET `time_active` = `time_updated`;",
        ]),
    },
];

/// Applies migrations with the bootstrap classification
/// (`migration.ts:22-52`).
pub fn apply(conn: &mut Connection, opts: &MigrationOptions) -> Result<ApplyReport> {
    let tables = opencode_tables(conn)?;
    if tables.iter().any(|name| name == "session" || name == "session_v2") {
        return apply_only(conn, opts);
    }
    if !tables.is_empty() {
        return Err(DbError::NotEmptyNoSession);
    }
    let now = now_ms();
    run_top_tx(conn, Begin::Immediate, |tx| {
        tx.execute_batch(BOOTSTRAP_SQL)?;
        tx.execute_batch(MIGRATION_TABLE_SQL)?;
        for id in MIGRATION_IDS {
            tx.execute(
                "INSERT INTO migration (id, time_completed) VALUES (?1, ?2)",
                rusqlite::params![id, now],
            )?;
        }
        Ok(())
    })?;
    Ok(ApplyReport { bootstrapped: true, seeded_legacy: false, applied: Vec::new() })
}

/// Incremental path: create the journal if missing, seed it from a legacy
/// drizzle journal when empty, then run every unjournaled migration
/// (`migration.ts:54-146`).
pub fn apply_only(conn: &mut Connection, opts: &MigrationOptions) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();
    conn.execute_batch(MIGRATION_TABLE_IF_NOT_EXISTS_SQL)?;
    let mut completed = journal_ids(conn)?;
    if completed.is_empty()
        && table_exists(conn, "__drizzle_migrations")?
        && seed_legacy_journal(conn, &mut report)?
    {
        completed = journal_ids(conn)?;
    }
    for migration in MIGRATIONS {
        if completed.contains(migration.id) {
            continue;
        }
        if migration.foreign_keys {
            run_migration(conn, migration, opts)?;
        } else {
            // Relax outside the transaction (the pragma is a no-op inside
            // one); restore afterwards regardless of outcome.
            conn.execute_batch("PRAGMA foreign_keys = OFF")?;
            let result = run_migration(conn, migration, opts);
            conn.execute_batch("PRAGMA foreign_keys = ON")?;
            result?;
        }
        report.applied.push(migration.id);
    }
    Ok(report)
}

fn run_migration(
    conn: &mut Connection,
    migration: &Migration,
    opts: &MigrationOptions,
) -> Result<()> {
    run_top_tx(conn, Begin::Immediate, |tx| {
        run_body(tx, &migration.body, opts)?;
        tx.execute(
            "INSERT INTO migration (id, time_completed) VALUES (?1, ?2)",
            rusqlite::params![migration.id, now_ms()],
        )?;
        Ok(())
    })
}

fn run_body(tx: &mut Tx<'_>, body: &MigrationBody, opts: &MigrationOptions) -> Result<()> {
    match body {
        MigrationBody::Statements(statements) => {
            for statement in *statements {
                tx.execute_batch(statement)?;
            }
            Ok(())
        }
        MigrationBody::WorkspaceName => workspace_name(tx),
        MigrationBody::SessionMetadata => session_metadata(tx),
        MigrationBody::LoosePsylocke => loose_psylocke(tx),
        MigrationBody::ImportLegacyCredentials => import_legacy_credentials(tx, opts),
    }
}

/// `20260410174513_workspace-name`.
fn workspace_name(tx: &mut Tx<'_>) -> Result<()> {
    let name = if table_columns(tx.conn(), "workspace")?.iter().any(|c| c == "name") {
        "`name`"
    } else {
        "''"
    };
    tx.execute_batch("PRAGMA foreign_keys=OFF;")?;
    tx.execute_batch(
        "CREATE TABLE `__new_workspace` (`id` text PRIMARY KEY, `type` text NOT NULL, `name` text DEFAULT '' NOT NULL, `branch` text, `directory` text, `extra` text, `project_id` text NOT NULL, CONSTRAINT `fk_workspace_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);",
    )?;
    tx.execute_batch(&format!(
        "INSERT INTO `__new_workspace`(`id`, `type`, `branch`, `name`, `directory`, `extra`, `project_id`) SELECT `id`, `type`, `branch`, {name}, `directory`, `extra`, `project_id` FROM `workspace`;"
    ))?;
    tx.execute_batch("DROP TABLE `workspace`;")?;
    tx.execute_batch("ALTER TABLE `__new_workspace` RENAME TO `workspace`;")?;
    tx.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(())
}

/// `20260511173437_session-metadata` (column shipped twice; skip if present).
fn session_metadata(tx: &mut Tx<'_>) -> Result<()> {
    if table_columns(tx.conn(), "session")?.iter().any(|c| c == "metadata") {
        return Ok(());
    }
    tx.execute_batch("ALTER TABLE `session` ADD `metadata` text;")?;
    Ok(())
}

/// `20260804233008_loose_psylocke`.
fn loose_psylocke(tx: &mut Tx<'_>) -> Result<()> {
    let marker: Option<String> = tx
        .conn()
        .query_row(
            "SELECT id FROM migration WHERE id = ?1",
            rusqlite::params![PREVIOUS_V2_MARKER],
            |row| row.get(0),
        )
        .optional()?;
    if marker.is_some() {
        let v1_only: Option<i64> = tx
            .conn()
            .query_row(
                "SELECT 1 FROM message WHERE NOT EXISTS (SELECT 1 FROM session_message WHERE session_message.session_id = message.session_id) LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if v1_only.is_some() {
            return Err(DbError::V1OnlyHistory);
        }
        tx.execute_batch("DROP INDEX IF EXISTS `session_project_idx`;")?;
        tx.execute_batch("DROP INDEX IF EXISTS `session_workspace_idx`;")?;
        tx.execute_batch("DROP INDEX IF EXISTS `session_parent_idx`;")?;
        tx.execute_batch("DROP INDEX IF EXISTS `session_time_suspended_idx`;")?;
        tx.execute_batch("ALTER TABLE `session` RENAME TO `session_v2`;")?;
        tx.execute_batch("CREATE INDEX `session_v2_project_idx` ON `session_v2` (`project_id`);")?;
        tx.execute_batch(
            "CREATE INDEX `session_v2_workspace_idx` ON `session_v2` (`workspace_id`);",
        )?;
        tx.execute_batch("CREATE INDEX `session_v2_parent_idx` ON `session_v2` (`parent_id`);")?;
        tx.execute_batch(
            "CREATE INDEX `session_v2_time_suspended_idx` ON `session_v2` (`time_suspended`) WHERE \"session_v2\".\"time_suspended\" is not null;",
        )?;
        tx.execute_batch("DROP TABLE IF EXISTS `data_migration`;")?;
        tx.execute_batch("DROP TABLE IF EXISTS `session_context_epoch`;")?;
        tx.execute_batch("DROP TABLE IF EXISTS `session_input`;")?;
        return Ok(());
    }

    tx.execute_batch("CREATE TABLE IF NOT EXISTS `kv` (`key` text PRIMARY KEY, `value` text NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL);")?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS `instruction_blob` (`hash` text PRIMARY KEY, `value` text);",
    )?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS `instruction_entry` (`session_id` text NOT NULL, `key` text NOT NULL, `value` text, `removed` integer DEFAULT false NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, CONSTRAINT `instruction_entry_pk` PRIMARY KEY(`session_id`, `key`), CONSTRAINT `fk_instruction_entry_session_id_session_v2_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session_v2`(`id`) ON DELETE CASCADE);")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS `instruction_state` (`session_id` text PRIMARY KEY, `epoch_start` integer NOT NULL, `through_seq` integer NOT NULL, `initial_values` text NOT NULL, `current_values` text NOT NULL, CONSTRAINT `fk_instruction_state_session_id_session_v2_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session_v2`(`id`) ON DELETE CASCADE);")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS `session_pending` (`id` text PRIMARY KEY, `session_id` text NOT NULL, `type` text NOT NULL, `data` text NOT NULL, `delivery` text, `admitted_seq` integer NOT NULL, `time_created` integer NOT NULL, CONSTRAINT `fk_session_pending_session_id_session_v2_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session_v2`(`id`) ON DELETE CASCADE);")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS `session_v2` (`id` text PRIMARY KEY, `project_id` text NOT NULL, `workspace_id` text, `parent_id` text, `fork_session_id` text, `fork_boundary` text, `slug` text NOT NULL, `directory` text NOT NULL, `path` text, `title` text, `version` text NOT NULL, `share_url` text, `summary_additions` integer, `summary_deletions` integer, `summary_files` integer, `summary_diffs` text, `metadata` text, `cost` real DEFAULT 0 NOT NULL, `tokens_input` integer DEFAULT 0 NOT NULL, `tokens_output` integer DEFAULT 0 NOT NULL, `tokens_reasoning` integer DEFAULT 0 NOT NULL, `tokens_cache_read` integer DEFAULT 0 NOT NULL, `tokens_cache_write` integer DEFAULT 0 NOT NULL, `revert` text, `permission` text, `agent` text, `model` text, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `time_compacting` integer, `time_archived` integer, `time_suspended` integer, CONSTRAINT `fk_session_v2_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE);")?;
    tx.execute_batch("ALTER TABLE `event` ADD `created` integer DEFAULT 0 NOT NULL;")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS `__new_session_message` (`id` text PRIMARY KEY, `session_id` text NOT NULL, `type` text NOT NULL, `seq` integer NOT NULL, `time_created` integer NOT NULL, `time_updated` integer NOT NULL, `data` text NOT NULL, CONSTRAINT `fk_session_message_session_id_session_v2_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session_v2`(`id`) ON DELETE CASCADE);")?;
    tx.execute_batch("DROP TABLE `session_message`;")?;
    tx.execute_batch("ALTER TABLE `__new_session_message` RENAME TO `session_message`;")?;
    tx.execute_batch("CREATE UNIQUE INDEX `session_message_session_seq_idx` ON `session_message` (`session_id`,`seq`);")?;
    tx.execute_batch("CREATE INDEX `session_message_session_type_seq_idx` ON `session_message` (`session_id`,`type`,`seq`);")?;
    tx.execute_batch("CREATE INDEX `session_message_session_time_created_id_idx` ON `session_message` (`session_id`,`time_created`,`id`);")?;
    tx.execute_batch(
        "CREATE INDEX `session_message_time_created_idx` ON `session_message` (`time_created`);",
    )?;
    tx.execute_batch("CREATE INDEX `session_pending_session_delivery_seq_idx` ON `session_pending` (`session_id`,`delivery`,`admitted_seq`);")?;
    tx.execute_batch("CREATE UNIQUE INDEX `session_pending_session_compaction_idx` ON `session_pending` (`session_id`) WHERE \"session_pending\".\"type\" = 'compaction';")?;
    tx.execute_batch("CREATE UNIQUE INDEX `session_pending_session_admitted_seq_idx` ON `session_pending` (`session_id`,`admitted_seq`);")?;
    tx.execute_batch("CREATE INDEX `session_v2_project_idx` ON `session_v2` (`project_id`);")?;
    tx.execute_batch("CREATE INDEX `session_v2_workspace_idx` ON `session_v2` (`workspace_id`);")?;
    tx.execute_batch("CREATE INDEX `session_v2_parent_idx` ON `session_v2` (`parent_id`);")?;
    tx.execute_batch(
        "CREATE INDEX `session_v2_time_suspended_idx` ON `session_v2` (`time_suspended`) WHERE \"session_v2\".\"time_suspended\" is not null;",
    )?;
    tx.execute_batch("DROP TABLE `data_migration`;")?;
    tx.execute_batch("DROP TABLE `session_context_epoch`;")?;
    tx.execute_batch("DROP TABLE `session_input`;")?;
    Ok(())
}

/// Decoded legacy `auth.json` entry (`20260805200742`).
enum LegacyValue {
    Oauth {
        refresh: String,
        access: String,
        expires: i64,
        account_id: Option<String>,
        enterprise_url: Option<String>,
    },
    Api {
        key: String,
        metadata: Option<serde_json::Value>,
    },
    WellKnown {
        /// Required by the legacy schema decode; unused afterwards.
        _key: String,
        token: String,
    },
}

fn decode_legacy_value(raw: &serde_json::Value) -> Option<LegacyValue> {
    let obj = raw.as_object()?;
    match obj.get("type")?.as_str()? {
        "oauth" => {
            let refresh = obj.get("refresh")?.as_str()?.to_string();
            let access = obj.get("access")?.as_str()?.to_string();
            let expires = obj.get("expires")?.as_i64()?;
            if expires < 0 {
                return None;
            }
            let account_id = obj.get("accountId").and_then(|v| v.as_str()).map(str::to_string);
            let enterprise_url =
                obj.get("enterpriseUrl").and_then(|v| v.as_str()).map(str::to_string);
            Some(LegacyValue::Oauth { refresh, access, expires, account_id, enterprise_url })
        }
        "api" => {
            let key = obj.get("key")?.as_str()?.to_string();
            let metadata = match obj.get("metadata") {
                Some(value) if value.is_object() => Some(value.clone()),
                Some(_) => return None, // Record<string, string>: must be an object
                None => None,
            };
            Some(LegacyValue::Api { key, metadata })
        }
        "wellknown" => {
            let key = obj.get("key")?.as_str()?.to_string();
            let token = obj.get("token")?.as_str()?.to_string();
            Some(LegacyValue::WellKnown { _key: key, token })
        }
        _ => None,
    }
}

/// `import_legacy_credentials.ts` — `Integration.MethodID` derivation.
fn legacy_method_id(integration_id: &str) -> &'static str {
    match integration_id {
        "openai" => "chatgpt-browser",
        "github-copilot" | "opencode" | "xai" => "device",
        _ => "oauth",
    }
}

fn legacy_credential_id() -> String {
    use oc_util::determinism::{AscendingIdGen, SystemClock, SystemRng};
    let gen = AscendingIdGen::new(Arc::new(SystemClock), Arc::new(SystemRng));
    format!("cred_{}", gen.generate(false))
}

/// `20260805200742_import_legacy_credentials`.
fn import_legacy_credentials(tx: &mut Tx<'_>, opts: &MigrationOptions) -> Result<()> {
    let data = opts.global_data.as_deref().ok_or(DbError::MissingGlobalData)?;
    let filepath = data.join("auth.json");
    // readFile().catch(() => undefined): any read failure (incl. ENOENT) skips.
    let Ok(content) = fs::read_to_string(&filepath) else {
        return Ok(());
    };
    let input: serde_json::Value = serde_json::from_str(&content)?;
    let Some(map) = input.as_object() else {
        return Err(DbError::LegacyCredentialShape);
    };

    let mut origins: Vec<String> = Vec::new();
    for (id, raw) in map {
        let Some(value) = decode_legacy_value(raw) else {
            continue;
        };
        let integration_id = id.trim_end_matches('/');
        if integration_id.is_empty() {
            continue;
        }
        if matches!(value, LegacyValue::WellKnown { .. }) {
            origins.push(integration_id.to_string());
        }
        if credential_for_integration_exists(tx, integration_id)? {
            continue;
        }
        insert_legacy_credential(tx, integration_id, value)?;
    }
    merge_wellknown_sources(tx, &origins)
}

fn credential_for_integration_exists(tx: &Tx<'_>, integration_id: &str) -> Result<bool> {
    let existing: Option<String> = tx
        .conn()
        .query_row(
            "SELECT id FROM credential WHERE integration_id = ?1",
            rusqlite::params![integration_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(existing.is_some())
}

fn insert_legacy_credential(
    tx: &mut Tx<'_>,
    integration_id: &str,
    value: LegacyValue,
) -> Result<()> {
    let (credential, label) = match value {
        LegacyValue::Api { key, metadata } => {
            let mut credential = serde_json::json!({ "type": "key", "key": key });
            if let Some(metadata) = metadata {
                credential["metadata"] = metadata;
            }
            (credential, "API key")
        }
        LegacyValue::WellKnown { token, .. } => {
            (serde_json::json!({ "type": "key", "key": token }), "API key")
        }
        LegacyValue::Oauth { refresh, access, expires, account_id, enterprise_url } => {
            let mut credential = serde_json::json!({
                "type": "oauth",
                "methodID": legacy_method_id(integration_id),
                "refresh": refresh,
                "access": access,
                "expires": expires,
            });
            if account_id.is_some() || enterprise_url.is_some() {
                let mut metadata = serde_json::Map::new();
                if let Some(account_id) = account_id {
                    metadata.insert("accountID".into(), serde_json::Value::String(account_id));
                }
                if let Some(url) = enterprise_url {
                    metadata.insert("enterpriseUrl".into(), serde_json::Value::String(url));
                }
                credential["metadata"] = serde_json::Value::Object(metadata);
            }
            (credential, "OAuth")
        }
    };
    let now = now_ms();
    tx.execute(
        "INSERT INTO credential (id, integration_id, label, value, time_created, time_updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            legacy_credential_id(),
            integration_id,
            label,
            credential.to_string(),
            now,
            now
        ],
    )?;
    Ok(())
}

/// Merges wellknown origins into the `kv` sources list
/// (`wellknown:sources`, union-preserving upsert).
fn merge_wellknown_sources(tx: &mut Tx<'_>, origins: &[String]) -> Result<()> {
    if origins.is_empty() {
        return Ok(());
    }
    let stored: Option<String> = tx
        .conn()
        .query_row(
            "SELECT value FROM kv WHERE key = ?1",
            rusqlite::params![WELLKNOWN_SOURCES_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let current: Vec<String> = stored
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|value| value.as_array().cloned())
        .map(|items| {
            items.into_iter().filter_map(|item| item.as_str().map(str::to_string)).collect()
        })
        .unwrap_or_default();
    let mut merged: Vec<String> = current;
    for origin in origins {
        if !merged.contains(origin) {
            merged.push(origin.clone());
        }
    }
    let value = serde_json::Value::Array(
        merged.into_iter().map(serde_json::Value::String).collect::<Vec<_>>(),
    )
    .to_string();
    let now = now_ms();
    tx.execute(
        "INSERT INTO kv (key, value, time_created, time_updated) VALUES (?1, ?2, ?3, ?4) ON CONFLICT (key) DO UPDATE SET value = excluded.value, time_updated = excluded.time_updated",
        rusqlite::params![WELLKNOWN_SOURCES_KEY, value, now, now],
    )?;
    Ok(())
}

/// Seeds `migration` from `__drizzle_migrations` (`migration.ts:62-104`).
/// Returns whether seeding ran.
fn seed_legacy_journal(conn: &mut Connection, report: &mut ApplyReport) -> Result<bool> {
    let named = table_columns(conn, "__drizzle_migrations")?.iter().any(|c| c == "name");
    if named {
        conn.execute_batch(&format!(
            "INSERT OR IGNORE INTO migration (id, time_completed) SELECT name, {} FROM __drizzle_migrations WHERE name IS NOT NULL",
            now_ms()
        ))?;
        report.seeded_legacy = true;
        return Ok(true);
    }
    let mut statement = conn.prepare(
        "SELECT created_at, strftime('%Y%m%d%H%M%S', created_at / 1000, 'unixepoch') AS prefix FROM __drizzle_migrations WHERE created_at IS NOT NULL",
    )?;
    let entries: Vec<(i64, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    drop(statement);
    for (created_at, prefix) in entries {
        let migration = MIGRATIONS
            .iter()
            .find(|item| item.id.starts_with(&format!("{prefix}_")))
            .ok_or(DbError::LegacyTimestampUnmatched(created_at))?;
        conn.execute(
            "INSERT OR IGNORE INTO migration (id, time_completed) VALUES (?1, ?2)",
            rusqlite::params![migration.id, now_ms()],
        )?;
    }
    report.seeded_legacy = true;
    Ok(true)
}

fn journal_ids(conn: &Connection) -> Result<HashSet<String>> {
    let mut statement = conn.prepare("SELECT id FROM migration")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    Ok(ids)
}

/// Tables opencode owns in `sqlite_master` (not `sqlite_%`, not `_`-prefixed).
fn opencode_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND substr(name, 1, 1) <> '_'",
    )?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(tables)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(columns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::bootstrap::MIGRATION_IDS;

    #[test]
    fn migration_ids_match_bootstrap_journal_constant() {
        assert_eq!(MIGRATIONS.len(), 48);
        assert_eq!(MIGRATION_IDS.len(), 48);
        for (migration, id) in MIGRATIONS.iter().zip(MIGRATION_IDS.iter()) {
            assert_eq!(migration.id, *id);
        }
    }

    #[test]
    fn only_nullable_workspace_binding_disables_foreign_keys() {
        let flagged: Vec<&str> =
            MIGRATIONS.iter().filter(|m| !m.foreign_keys).map(|m| m.id).collect();
        assert_eq!(flagged, ["20260823191254_nullable_workspace_binding"]);
    }

    #[test]
    fn legacy_method_id_matches_reference() {
        assert_eq!(legacy_method_id("openai"), "chatgpt-browser");
        assert_eq!(legacy_method_id("github-copilot"), "device");
        assert_eq!(legacy_method_id("opencode"), "device");
        assert_eq!(legacy_method_id("xai"), "device");
        assert_eq!(legacy_method_id("linear"), "oauth");
    }

    #[test]
    fn legacy_credential_id_shape() {
        let id = legacy_credential_id();
        assert!(id.starts_with("cred_"), "got {id}");
        assert_eq!(id.len(), 31); // cred_ + 26 chars
    }

    #[test]
    fn decode_legacy_value_shapes() {
        let oauth = serde_json::json!({
            "type": "oauth", "refresh": "r", "access": "a", "expires": 123,
            "accountId": "acc", "enterpriseUrl": "https://x"
        });
        assert!(matches!(decode_legacy_value(&oauth), Some(LegacyValue::Oauth { .. })));

        let negative =
            serde_json::json!({"type": "oauth", "refresh": "r", "access": "a", "expires": -1});
        assert!(decode_legacy_value(&negative).is_none());

        let api = serde_json::json!({"type": "api", "key": "k", "metadata": {"a": "b"}});
        assert!(matches!(decode_legacy_value(&api), Some(LegacyValue::Api { .. })));

        let wellknown = serde_json::json!({"type": "wellknown", "key": "k", "token": "t"});
        assert!(matches!(decode_legacy_value(&wellknown), Some(LegacyValue::WellKnown { .. })));

        assert!(decode_legacy_value(&serde_json::json!({"type": "other"})).is_none());
        assert!(decode_legacy_value(&serde_json::Value::Null).is_none());
    }
}
