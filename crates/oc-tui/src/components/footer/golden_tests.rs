//! Footer-family golden acceptance: text-grid **and** canonical-ANSI
//! byte comparisons against the tier-a footer corpus
//! (`footer-{prompt-idle,permission,form,command-menu,model-select,
//! agent-select,variant-select,settings}`, `permission-reject-field`,
//! `form-authorization`) plus the `toast-over-session` overlay region —
//! all at the manifest's comparison depth, through
//! `tools/parity::tui_frame`.

#![cfg(test)]
#![allow(clippy::cast_possible_truncation)]

use super::form::{FormField, FormRequest};
use super::form_authorization::{render_form_authorization, FormAuthorizationState};
use super::panels::{
    render_agent_select, render_command_menu, render_model_select, render_settings,
    render_variant_select, MiniSettings, RunAgent, RunModel, RunProvider,
};
use super::permission::{
    create_permission_body_state, render_permission_body, render_reject_field, PermissionRequest,
};
use super::prompt_idle::{render_prompt_idle, PromptIdleState};
use crate::components::footer::canvas::{Canvas, Span};
use crate::components::footer::form::{render_form_body, FormBodyState};
use crate::components::footer::theme::FooterTheme;
use crate::components::toast::{render_toast_fallback_dark, ToastOptions};
use crate::render::{serialize_canonical, Frame};
use parity::tui_frame as pf;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

fn manifest() -> pf::Manifest {
    pf::load_manifest(&goldens()).expect("golden manifest")
}

fn variant(id: &str, width: usize) -> (usize, usize, pf::ComparisonDepth) {
    let scenario = manifest()
        .scenarios
        .into_iter()
        .find(|scenario| scenario.id == id)
        .unwrap_or_else(|| panic!("scenario {id}"));
    let depth = manifest().comparison_depth_for(&scenario);
    let variant = scenario
        .variants
        .iter()
        .find(|variant| variant.width == width)
        .unwrap_or_else(|| panic!("{id} variant @{width}"));
    (variant.rows, variant.cols, depth)
}

/// Assert the rendered frame matches the golden text grid exactly and
/// the canonical ANSI byte-for-byte at the manifest depth.
fn assert_golden(id: &str, width: usize, frame: &Frame) {
    let (rows, cols, depth) = variant(id, width);
    assert_eq!((frame.height, frame.width), (rows, cols), "{id}@{width} dims");
    let golden = pf::load_text_grid(&goldens(), id, width).expect("text grid");
    let rendered =
        pf::TextGrid { rows: frame.text_grid(), height: frame.height, width: frame.width };
    let diff = pf::compare_text_grids(&golden, &rendered);
    assert!(
        diff.first_divergence.is_none(),
        "{id}@{width} text grid diverged: {:?} ({:.2}%)",
        diff.first_divergence,
        diff.match_pct()
    );
    let golden_ansi =
        std::fs::read_to_string(goldens().join(format!("{id}.w{width}.ansi"))).expect("ansi");
    let ansi = serialize_canonical(frame);
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "{id}@{width} ansi differs at depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

fn permission_fixture() -> PermissionRequest {
    PermissionRequest {
        id: "per_dump".into(),
        session_id: "ses_dump".into(),
        action: "read".into(),
        resources: vec!["src/index.ts".into()],
        ..PermissionRequest::default()
    }
}

fn deployment_form() -> FormRequest {
    let field = |key: &str, title: &str, required: bool| FormField {
        key: key.into(),
        title: Some(title.into()),
        description: None,
        required,
        kind: super::form::FieldKind::Text { placeholder: None },
    };
    FormRequest {
        id: "frm_preempted".into(),
        session_id: "ses_child".into(),
        title: "Deployment".into(),
        fields: vec![field("service", "Service", true), field("notes", "Notes", true)],
    }
}

fn provider_fixture() -> RunProvider {
    RunProvider {
        id: "opencode".into(),
        name: "opencode".into(),
        models: vec![
            RunModel {
                id: "gpt-5".into(),
                name: "GPT-5".into(),
                cost_input: 1.0,
                deprecated: false,
            },
            RunModel {
                id: "gpt-free".into(),
                name: "GPT Free".into(),
                cost_input: 0.0,
                deprecated: false,
            },
            RunModel {
                id: "old".into(),
                name: "Old Model".into(),
                cost_input: 1.0,
                deprecated: true,
            },
        ],
    }
}

fn agents_fixture() -> Vec<RunAgent> {
    vec![
        RunAgent {
            id: "build".into(),
            name: "Build".into(),
            description: Some("Build software".into()),
            mode: "all".into(),
            hidden: false,
        },
        RunAgent {
            id: "review".into(),
            name: "Review".into(),
            description: Some("Review changes".into()),
            mode: "primary".into(),
            hidden: false,
        },
        RunAgent {
            id: "explore".into(),
            name: "Explore".into(),
            description: None,
            mode: "subagent".into(),
            hidden: false,
        },
        RunAgent {
            id: "secret".into(),
            name: "Secret".into(),
            description: None,
            mode: "all".into(),
            hidden: true,
        },
    ]
}

fn settings_fixture() -> MiniSettings {
    super::panels::resolve_mini_settings()
}

#[test]
fn footer_prompt_idle_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    for width in [80usize, 160] {
        let frame = render_prompt_idle(&PromptIdleState::default(), width, 12, &theme);
        assert_golden("footer-prompt-idle", width, &frame);
    }
}

#[test]
fn footer_permission_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    let request = permission_fixture();
    let state = create_permission_body_state(&request);
    for width in [80usize, 160] {
        let frame = render_permission_body(&request, &state, width, 16, &theme);
        assert_golden("footer-permission", width, &frame);
    }
}

#[test]
fn footer_form_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    let request = deployment_form();
    let state = FormBodyState::new(&request);
    for width in [80usize, 160] {
        let frame = render_form_body(&request, &state, width, 16, &theme);
        assert_golden("footer-form", width, &frame);
    }
}

#[test]
fn permission_reject_field_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    for width in [80usize, 160] {
        let frame = render_reject_field(true, width, 18, &theme);
        assert_golden("permission-reject-field", width, &frame);
    }
}

#[test]
fn footer_command_menu_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    for width in [80usize, 160] {
        let frame = render_command_menu(width, 17, &theme);
        assert_golden("footer-command-menu", width, &frame);
    }
}

#[test]
fn footer_model_select_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    for width in [80usize, 160] {
        let frame = render_model_select(
            &[provider_fixture()],
            Some(("opencode", "gpt-5")),
            width,
            17,
            &theme,
        );
        assert_golden("footer-model-select", width, &frame);
    }
}

#[test]
fn footer_agent_select_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    for width in [80usize, 160] {
        let frame = render_agent_select(&agents_fixture(), Some("review"), width, 16, &theme);
        assert_golden("footer-agent-select", width, &frame);
    }
}

#[test]
fn footer_variant_select_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    let variants = vec!["high".to_owned(), "minimal".to_owned()];
    for width in [80usize, 160] {
        let frame = render_variant_select(&variants, Some("high"), width, 16, &theme);
        assert_golden("footer-variant-select", width, &frame);
    }
}

#[test]
fn footer_settings_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    for width in [80usize, 160] {
        let frame = render_settings(&settings_fixture(), width, 16, &theme);
        assert_golden("footer-settings", width, &frame);
    }
}

#[test]
fn form_authorization_matches_golden() {
    let theme = FooterTheme::fallback_dark();
    let form = FormRequest {
        id: "frm_test".into(),
        session_id: "ses_test".into(),
        title: "Authorization required".into(),
        fields: vec![FormField {
            key: "authorization".into(),
            title: Some("Authorize access".into()),
            description: None,
            required: false,
            kind: super::form::FieldKind::External { url: "https://example.com/authorize".into() },
        }],
    };
    for width in [80usize, 160] {
        let frame =
            render_form_authorization(&form, &FormAuthorizationState::new(), width, 20, &theme);
        assert_golden("form-authorization", width, &frame);
    }
}

/// The toast overlay region of `toast-over-session` (tier-c1): the
/// full frame needs the session route (message-flow lane) and the
/// composer (wave 2), so this pins the toast's own cells against the
/// golden sub-rect (rows 1..5 at w80). The panel **interior**
/// (x38..76 — columns the toast surface fully owns) compares whole
/// styles; the two rail columns compare glyph + foreground only, since
/// their background is compositing with whatever renders behind (the
/// session screen — another lane's surface).
#[test]
fn toast_over_session_region_matches_golden() {
    let toast = ToastOptions {
        title: Some("Fixture notice".into()),
        message: "Toast rendered over session content".into(),
        variant: "info",
        duration_ms: 60_000,
        action_label: None,
    };
    let frame = render_toast_fallback_dark(&toast, 0, 80, 30);
    let golden = pf::load_ansi_frame(&goldens(), "toast-over-session", 80).expect("ansi golden");
    let rendered = pf::parse_ansi(&serialize_canonical(&frame), Some((30, 80))).expect("parses");
    for row in 1..=5 {
        for col in 37..=77 {
            let expected = golden.cell(row, col);
            let actual = rendered.cell(row, col);
            if (38..=76).contains(&col) {
                assert_eq!(
                    (expected.ch, expected.style.fg, expected.style.bg, expected.style.attrs),
                    (actual.ch, actual.style.fg, actual.style.bg, actual.style.attrs),
                    "toast interior cell ({row},{col}) — golden {expected:?} vs rendered {actual:?}"
                );
            } else {
                assert_eq!(
                    (expected.ch, expected.style.fg),
                    (actual.ch, actual.style.fg),
                    "toast rail cell ({row},{col}) — golden {expected:?} vs rendered {actual:?}"
                );
            }
        }
    }
}

/// The unstyled toast surface is directly constructible by other
/// surfaces (session screens paint it over live content).
#[test]
fn toast_paints_onto_an_existing_canvas() {
    let mut canvas = Canvas::new(40, 10);
    canvas.fill_bg(0, 0, 40, crate::render::CellColor::Rgb(10, 10, 10));
    let theme = FooterTheme::fallback_dark();
    super::super::toast::paint_toast(
        &mut canvas,
        &ToastOptions::info("short"),
        0,
        40,
        &theme,
        crate::render::CellColor::Rgb(86, 182, 194),
    );
    let frame = canvas.into_frame();
    assert!(frame.text_grid()[1].contains('┃'));
    assert!(frame.text_grid()[2].contains("short"));
    let _ = Span::new("", crate::render::CellColor::Default, crate::render::CellColor::Default);
}
