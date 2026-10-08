//! Builtin theme assets, embedded at compile time.
//!
//! * `assets/v1/*.json` — the 33 classic v1 themes (migrate with
//!   [`crate::v1_migrate::migrate_v1`]). Note the upstream TUI registers 32
//!   of them as defaults: the v1 `opencode.json` is legacy and shadowed by
//!   the v2 default below.
//! * `assets/v2/opencode.json` — the default v2 theme (parse with
//!   [`crate::resolve::parse_theme_document`]).

/// A builtin theme asset: `(name, JSON source)`.
pub type BuiltinAsset = (&'static str, &'static str);

/// All 33 embedded v1 theme assets, sorted by name.
pub const BUILTIN_V1_THEMES: &[BuiltinAsset] = &[
    ("aura", include_str!("../assets/v1/aura.json")),
    ("ayu", include_str!("../assets/v1/ayu.json")),
    ("carbonfox", include_str!("../assets/v1/carbonfox.json")),
    ("catppuccin", include_str!("../assets/v1/catppuccin.json")),
    ("catppuccin-frappe", include_str!("../assets/v1/catppuccin-frappe.json")),
    ("catppuccin-macchiato", include_str!("../assets/v1/catppuccin-macchiato.json")),
    ("cobalt2", include_str!("../assets/v1/cobalt2.json")),
    ("cursor", include_str!("../assets/v1/cursor.json")),
    ("dracula", include_str!("../assets/v1/dracula.json")),
    ("everforest", include_str!("../assets/v1/everforest.json")),
    ("flexoki", include_str!("../assets/v1/flexoki.json")),
    ("github", include_str!("../assets/v1/github.json")),
    ("gruvbox", include_str!("../assets/v1/gruvbox.json")),
    ("kanagawa", include_str!("../assets/v1/kanagawa.json")),
    ("lucent-orng", include_str!("../assets/v1/lucent-orng.json")),
    ("material", include_str!("../assets/v1/material.json")),
    ("matrix", include_str!("../assets/v1/matrix.json")),
    ("mercury", include_str!("../assets/v1/mercury.json")),
    ("monokai", include_str!("../assets/v1/monokai.json")),
    ("nightowl", include_str!("../assets/v1/nightowl.json")),
    ("nord", include_str!("../assets/v1/nord.json")),
    ("one-dark", include_str!("../assets/v1/one-dark.json")),
    ("opencode", include_str!("../assets/v1/opencode.json")),
    ("orng", include_str!("../assets/v1/orng.json")),
    ("osaka-jade", include_str!("../assets/v1/osaka-jade.json")),
    ("palenight", include_str!("../assets/v1/palenight.json")),
    ("rosepine", include_str!("../assets/v1/rosepine.json")),
    ("solarized", include_str!("../assets/v1/solarized.json")),
    ("synthwave84", include_str!("../assets/v1/synthwave84.json")),
    ("tokyonight", include_str!("../assets/v1/tokyonight.json")),
    ("vercel", include_str!("../assets/v1/vercel.json")),
    ("vesper", include_str!("../assets/v1/vesper.json")),
    ("zenburn", include_str!("../assets/v1/zenburn.json")),
];

/// The embedded v2 default theme asset.
pub const BUILTIN_V2_OPENCODE: &str = include_str!("../assets/v2/opencode.json");

/// Names of the builtin v1 themes (sorted).
pub fn builtin_v1_names() -> Vec<&'static str> {
    BUILTIN_V1_THEMES.iter().map(|(name, _)| *name).collect()
}

/// Look up a builtin v1 theme by its asset name.
pub fn builtin_v1_theme(name: &str) -> Option<&'static str> {
    BUILTIN_V1_THEMES.iter().find(|(key, _)| *key == name).map(|(_, source)| *source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_all_assets() {
        assert_eq!(BUILTIN_V1_THEMES.len(), 33);
        for (name, source) in BUILTIN_V1_THEMES {
            assert!(source.len() > 10, "asset {name} is suspiciously small");
            assert!(
                serde_json::from_str::<serde_json::Value>(source).is_ok(),
                "asset {name} is not valid JSON"
            );
        }
        assert!(serde_json::from_str::<serde_json::Value>(BUILTIN_V2_OPENCODE).is_ok());
        assert!(builtin_v1_theme("dracula").is_some());
        assert!(builtin_v1_theme("nonexistent").is_none());
    }
}
