//! Composer golden acceptance: the five state goldens
//! (`composer-placeholder-empty` / `-typed-multiline` / `-cjk-input` /
//! `-attachment-chip` / `-autocomplete-open`) at 80 and 40 columns —
//! text grid AND canonical ANSI byte-exact at the manifest depth
//! (`truecolor-canonical`).
//!
//! The states mirror `tui-dump/scenarios/composer.tsx`: drafts are set
//! through the reference's own `PromptRef.set` path, and only the
//! autocomplete state drives real keys (`reset()` then typing `/re`).

#![cfg(test)]
#![allow(clippy::cast_possible_truncation)]

use parity::tui_frame as pf;

use crate::components::composer::autocomplete::{filter_commands, SlashRegistry};
use crate::components::composer::render::{
    render_prompt_frame, AutocompleteView, FooterHints, PromptView,
};
use crate::components::composer::{Composer, ComposerKey, FileMention, PromptDraft};
use crate::theme::ThemeState;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

fn manifest() -> pf::Manifest {
    pf::load_manifest(&goldens()).expect("golden manifest")
}

fn scenario(id: &str) -> pf::ManifestScenario {
    manifest()
        .scenarios
        .into_iter()
        .find(|scenario| scenario.id == id)
        .unwrap_or_else(|| panic!("scenario {id} in manifest"))
}

/// The harness mount: padding-top 2, the session fixture's footer
/// location, unbound hint shortcuts (they render as ` agents` /
/// ` commands`), the placeholder list pinned to entry 0.
fn harness_view<'a>(
    composer: &'a Composer,
    options: &'a [crate::components::composer::autocomplete::AutocompleteOption],
) -> PromptView<'a> {
    let autocomplete = composer.autocomplete.visible.map(|_| AutocompleteView {
        options,
        selected: composer.autocomplete.selected,
        title_width: composer.autocomplete.registry.display_width(),
    });
    PromptView {
        draft: &composer.draft,
        placeholder: composer.placeholder_text(),
        autocomplete,
        footer: FooterHints {
            location: "/tmp/opencode/packages/tui:main".into(),
            agent_shortcut: String::new(),
            command_shortcut: String::new(),
            ..FooterHints::default()
        },
        top: 2,
        rail: None,
        caret: None,
    }
}

fn autocomplete_query(composer: &Composer) -> String {
    let caret = composer.caret;
    crate::components::composer::autocomplete::query_between(
        &composer.draft.text,
        composer.autocomplete.trigger_index,
        caret,
    )
}

fn assert_matches_golden(id: &str, width: usize, height: usize, composer: &Composer) {
    let scenario = scenario(id);
    let variant = scenario.variants.iter().find(|v| v.width == width).expect("variant");
    assert_eq!(variant.height, height);
    let theme = ThemeState::v2_dark();
    let options = if composer.autocomplete.visible.is_some() {
        filter_commands(&composer.autocomplete.registry, &autocomplete_query(composer))
    } else {
        Vec::new()
    };
    let frame = render_prompt_frame(&harness_view(composer, &options), width, height, &theme);
    let text_grid = frame.text_grid();
    let golden_grid = pf::load_text_grid(&goldens(), id, width).expect("golden text grid");
    let rendered = pf::TextGrid { rows: text_grid, height, width };
    let diff = pf::compare_text_grids(&golden_grid, &rendered);
    assert!(
        diff.first_divergence.is_none(),
        "{id}@{width} text grid diverged: {:?} ({:.2}% matching)\nrendered:\n{}\ngolden:\n{}",
        diff.first_divergence,
        diff.match_pct(),
        rendered.rows.join("\n"),
        golden_grid.rows.join("\n")
    );
    let ansi = crate::render::serialize_canonical(&frame);
    let golden_ansi =
        std::fs::read_to_string(goldens().join(format!("{id}.w{width}.ansi"))).expect("ansi");
    let depth = manifest().comparison_depth_for(&scenario);
    let byte_diff = pf::compare_ansi_bytes_at(&golden_ansi, &ansi, depth).expect("ansi parses");
    assert!(
        byte_diff.equal,
        "{id}@{width} ansi differs at manifest depth {depth}: first divergence {:?}",
        byte_diff.first_divergent_offset
    );
}

fn empty_composer() -> Composer {
    Composer {
        placeholders: vec!["Fix a TODO in the codebase".into()],
        autocomplete: crate::components::composer::AutocompleteState::with_registry(
            SlashRegistry::golden_harness(),
        ),
        ..Composer::default()
    }
}

fn typed_composer() -> Composer {
    let mut composer = empty_composer();
    composer.set(PromptDraft {
        text: "Refactor the parser module\nkeep the public API stable\nadd regression tests".into(),
        ..PromptDraft::default()
    });
    composer
}

fn cjk_composer() -> Composer {
    let mut composer = empty_composer();
    composer.set(PromptDraft {
        text: "検証：日本語プロンプトの入力（English まじり）".into(),
        ..PromptDraft::default()
    });
    composer
}

fn chip_composer() -> Composer {
    let mut composer = empty_composer();
    let text = "Compare [Image 1] with the current design";
    let start = text.find("[Image 1]").expect("mention");
    let end = start + "[Image 1]".len();
    composer.set(PromptDraft {
        text: text.into(),
        files: vec![FileMention {
            name: "screenshot.png".into(),
            start: text[..start].chars().count(),
            end: text[..end].chars().count(),
            payload: None,
            mention: Some("[Image 1]".into()),
        }],
        ..PromptDraft::default()
    });
    composer
}

fn autocomplete_composer() -> Composer {
    // reset() then typing "/re" — the reference's key-driven state.
    let mut composer = empty_composer();
    composer.reset();
    for key in [ComposerKey::Char('/'), ComposerKey::Char('r'), ComposerKey::Char('e')] {
        composer.handle_key(&key);
    }
    composer
}

#[test]
fn composer_placeholder_empty_matches_golden() {
    for (width, height) in [(80, 20), (40, 20)] {
        assert_matches_golden("composer-placeholder-empty", width, height, &empty_composer());
    }
}

#[test]
fn composer_typed_multiline_matches_golden() {
    for (width, height) in [(80, 20), (40, 20)] {
        assert_matches_golden("composer-typed-multiline", width, height, &typed_composer());
    }
}

#[test]
fn composer_cjk_input_matches_golden() {
    for (width, height) in [(80, 20), (40, 20)] {
        assert_matches_golden("composer-cjk-input", width, height, &cjk_composer());
    }
}

#[test]
fn composer_attachment_chip_matches_golden() {
    for (width, height) in [(80, 20), (40, 20)] {
        assert_matches_golden("composer-attachment-chip", width, height, &chip_composer());
    }
}

#[test]
fn composer_autocomplete_open_matches_golden() {
    for (width, height) in [(80, 20), (40, 20)] {
        assert_matches_golden(
            "composer-autocomplete-open",
            width,
            height,
            &autocomplete_composer(),
        );
    }
}

/// The autocomplete flow beyond the frame: typing opens, filtering
/// ranks /redo first, enter accepts the command into the draft.
#[test]
fn autocomplete_accept_flow() {
    let mut composer = autocomplete_composer();
    assert!(composer.autocomplete.visible.is_some(), "open after /re");
    let options = filter_commands(&composer.autocomplete.registry, "re");
    assert_eq!(options.first().map(|option| option.display.as_str()), Some("/redo"));
    composer.autocomplete = composer.autocomplete.clone().with_options(options);
    composer.handle_key(&ComposerKey::Enter);
    assert_eq!(composer.draft.text, "/redo ");
    assert!(composer.autocomplete.visible.is_none(), "accepted closes the popup");
    // Escape dismisses without inserting.
    let mut dismissed = empty_composer();
    for key in [ComposerKey::Char('/'), ComposerKey::Char('r')] {
        dismissed.handle_key(&key);
    }
    assert!(dismissed.autocomplete.visible.is_some());
    dismissed.handle_key(&ComposerKey::Escape);
    assert!(dismissed.autocomplete.visible.is_none());
    assert_eq!(dismissed.draft.text, "/r");
}
