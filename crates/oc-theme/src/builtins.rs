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

/// GENERATED from the v2.0.24 reference assets
/// (`packages/tui/src/theme/assets/`): every builtin theme asset's
/// sha256, byte level (34 assets = 33 v1 + the v2 default). A digest
/// failing here means the embedded table drifted from the reference
/// SOURCE VALUES; regenerate only deliberately alongside a ref
/// upgrade (lane M-theme's table-parity gate).
#[cfg(test)]
const REF_ASSET_DIGESTS: &[(&str, &str)] = &[
    ("v1/aura.json", "3877fc874509286f934bc99d053ed158279623fae8747f61e588a342fc1ffa59"),
    ("v1/ayu.json", "c6f22767ad334e1b1b071fb4027a707b869a36a13627dee28961b49ef7a379df"),
    ("v1/carbonfox.json", "e75eca27f53a2274f82fc6767883d8b67988e9788da26dc408f67f562df4d55c"),
    (
        "v1/catppuccin-frappe.json",
        "f05740e3f637840caec38ed77368ee20e24d8c8e191c08f5c56eb04a627b58d8",
    ),
    (
        "v1/catppuccin-macchiato.json",
        "1f8ed24b6d5ce9ebee68f5af0bc0c3e62e0347f36958c3364703816802c1ecee",
    ),
    ("v1/catppuccin.json", "28c0a4c96604b480e7e041699f8fdd7e8a586c82a8fd903d674ed08ad44b39db"),
    ("v1/cobalt2.json", "08f33731f2d3434e874d8c431d6410bb8ebcc6c642dc890d0a0d3b21b5b1c607"),
    ("v1/cursor.json", "ba025a1cb28d1b510b5855a1bc8c14a03e09ddd849fade8d30477752c3b46441"),
    ("v1/dracula.json", "c721f9a3fb2ed273d0ffd8acdc5fd6c0a4d974d837ea80a19f619738055a319a"),
    ("v1/everforest.json", "67ff970d72877d9e827a01560ba8727f7ff1cf6c7cadf4b9213aae4b026e1970"),
    ("v1/flexoki.json", "73047c4544f190d6de213e1194057facdcf8fb7b84007084e2fbe86246d9893c"),
    ("v1/github.json", "b0cd4a988115e7705dd52cab0ea18464ffdfe76bad2a0a1e817c12839849c53d"),
    ("v1/gruvbox.json", "749d91f778a9aa7419c5a9fa4e1117c0b8fa6d8907fa867156af3f35f7323eab"),
    ("v1/kanagawa.json", "9aa73db869c3bd4b2fdd3181b91635f5185382284526b39c62f06d394b3f3839"),
    ("v1/lucent-orng.json", "e1d051beffcd1ef3e62734d3621d28c21a6bca44cf5ec9b47c44af86860c91fc"),
    ("v1/material.json", "d987cc3d31797617b4a517e46b26cddba46ed56fecc276fa1a778b2274914d1f"),
    ("v1/matrix.json", "77136eadbd19690dfbad154d1695d6893b96f383a77b8ace232082078196c541"),
    ("v1/mercury.json", "cfa03a5f745636085d5b98e4d212efe31a4b3245c7b2236ee2463ffd180ee4a5"),
    ("v1/monokai.json", "f0623af87201c4d2cd6874ca8bfb41d1949c68f22a03811a8ad56450fc3bed0a"),
    ("v1/nightowl.json", "18cad6067ccfd42cfc2d3c249fe90d44a8f99f20b8e1fb5372ad1a37c16de83f"),
    ("v1/nord.json", "2a4074516dcb08080cd97694884809a0d047063c894b71ed03a5ce20eb561284"),
    ("v1/one-dark.json", "92c07adf68ebd34c3b804b69dc24cd4e8ad6d71573e9239493cbbf86bb51fc70"),
    ("v1/opencode.json", "7d118db9abe1cf5f130fe99831406e5f0a5de6e2900e6ccda2f8e1aee3d1afd7"),
    ("v1/orng.json", "db9e776dc081944aa950936a170b5f1ad2898ff1daa543aeff492dd788c6134e"),
    ("v1/osaka-jade.json", "f26d302b9ca61ac2131f89d89e20b86eb903f38b42a24bad8e31287ab0fba510"),
    ("v1/palenight.json", "baf6bb21968a70a9c85a8881f43cb207d9f8167e5c2854896075e28bcfe6b00c"),
    ("v1/rosepine.json", "549e7aa10e75bcc39e8469bcfacca97681860856c439d509a140ba6fefe66988"),
    ("v1/solarized.json", "ebcbc744c069429afb8fa5c7146833de03b097ef9eb46cdbe2179c2b73f3e9fa"),
    ("v1/synthwave84.json", "7b7d0c5d5203046eccc3b49934ed89dab8a407be3fe0af081453fb7d905d80c0"),
    ("v1/tokyonight.json", "8dd2620d45535dc6297343b684e81ec00add795edd1f6b6f9396421fd90aa75f"),
    ("v2/opencode.json", "2673cbe2e68c6eb8ae5477f6f06fd1676331084385017acbd3f367107bebe9f1"),
    ("v1/vercel.json", "094eeb863e762cdb9aea2c32c3e4813973c9bbb90f7d8196da180d781544fd1b"),
    ("v1/vesper.json", "4e6eb4fceed17999ed1f8189ad24ed192370abf9ec2d37c56c5f401148c449d0"),
    ("v1/zenburn.json", "ee0facb4e0090b3eedc945e4f7957f872070206c7380c81fcffa46103c3c4900"),
];

#[cfg(test)]
mod parity_tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Lane M-theme's table-parity gate: every embedded asset is
    /// byte-identical to the reference's source asset (the digests
    /// above were generated FROM the ref tree, not from ours).
    #[test]
    fn embedded_assets_match_the_reference_source_bytes() {
        let mut checked = 0;
        for (name, digest) in REF_ASSET_DIGESTS {
            let source = if *name == "v2/opencode.json" {
                BUILTIN_V2_OPENCODE
            } else {
                let key = name.trim_start_matches("v1/").trim_end_matches(".json");
                builtin_v1_theme(key).unwrap_or_else(|| panic!("asset {name} missing"))
            };
            let actual = Sha256::digest(source.as_bytes());
            assert_eq!(format!("{actual:x}"), *digest, "asset {name} drifted from the ref source");
            checked += 1;
        }
        assert_eq!(checked, 34, "the full v2.0.24 builtin table");
        assert_eq!(BUILTIN_V1_THEMES.len() + 1, REF_ASSET_DIGESTS.len());
    }
}
