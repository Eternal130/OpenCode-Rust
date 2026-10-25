//! oc-theme: opencode theme token schema, resolve pipeline, v1 migration,
//! syntax rule generation and builtin theme assets.
//!
//! Port of `@opencode/theme` (tui) from opencode v2.0.24:
//!
//! * [`schema`] / [`resolve`] — the v2 theme document model
//!   (`ThemeDocument { base, light?, dark? }`), validation rules and the
//!   resolve pipeline to typed [`types::ResolvedTheme`] tokens.
//! * [`v1`] / [`v1_migrate`] — the legacy flat theme format and its OKLCH
//!   migration to v2 documents.
//! * [`syntax`] — `generateSyntax` highlighting rules.
//! * [`builtins`] — the 34 embedded theme assets.
//! * [`discovery`] — custom theme discovery (`discovery.ts` +
//!   `config-directories.ts`): `<dir>/themes/*.json` scans across config
//!   directories, later directories overriding earlier names.
//! * [`system`] — system dark/light detection (`system.ts terminalMode`):
//!   the background-luma classification rule; the OSC 11 probe itself
//!   lives in oc-tui with the renderer/tty (see [`system`] docs).
//!
//! The theme registry that merges builtins with discovered custom themes
//! (`theme/index.ts listThemes`, precedence defaults < custom < system)
//! lives in oc-tui where the resolved themes are consumed.

// The color math is a line-by-line port of the TypeScript pipeline; numeric
// casts mirror the reference arithmetic exactly and are safe by construction.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
// Ported code keeps the reference implementation's short bindings.
#![allow(clippy::similar_names)]
#![allow(clippy::many_single_char_names)]
// Pure query helpers on simple data.
#![allow(clippy::must_use_candidate)]
// The migrated definition builder mirrors the TS source layout.
#![allow(clippy::too_many_lines)]

pub mod builtins;
pub mod color;
pub mod discovery;
pub mod expand;
pub mod resolve;
pub mod schema;
pub mod syntax;
pub mod system;
pub mod types;
pub mod v1;
pub mod v1_migrate;

pub use color::{ints_to_oklch, oklch_to_hex, rgb_to_oklch, Oklch, Rgba};
pub use discovery::{config_directories, discover_themes, DiscoveredTheme};
pub use expand::{expand_theme, expand_tokens, merge_theme};
pub use resolve::{
    parse_theme_document, resolve_theme, resolve_theme_document, select_theme_mode, ThemeDocument,
};
pub use schema::ThemeError;
pub use syntax::{convert_theme_to_styles, generate_syntax, SyntaxRule, SyntaxStyleDef};
pub use system::mode_for_background;
pub use types::{
    ActionStates, ActionVariant, FeedbackKind, HueMap, HueScale, Mode, ResolvedTheme,
    ResolvedThemeTokens, SemanticHue, StatefulColor, SurfaceName,
};
pub use v1::resolve_v1;
pub use v1_migrate::migrate_v1;
