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
use crate::components::sidebar::SidebarMode;
use crate::components::toast::ToastOptions;
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
/// `PARITY_UPDATE=1` (re)writes the goldens instead of comparing —
/// the sanctioned refresh path for intentionally changed scenarios
/// (same contract as `parity::golden::update_mode`).
fn update_mode() -> bool {
    std::env::var("PARITY_UPDATE").is_ok_and(|value| value == "1")
}

fn assert_golden(id: &str, width: usize, frame: &Frame) {
    let (rows, cols, depth) = variant(id, width);
    assert_eq!((frame.height, frame.width), (rows, cols), "{id}@{width} dims");
    if update_mode() {
        std::fs::write(
            goldens().join(format!("{id}.w{width}.txt")),
            frame.text_grid().join("\n") + "\n",
        )
        .expect("write golden txt");
        std::fs::write(goldens().join(format!("{id}.w{width}.ansi")), serialize_canonical(frame))
            .expect("write golden ansi");
        return;
    }
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
        kind: super::form::FieldKind::Text {
            placeholder: None,
            options: None,
            custom: None,
            min_length: None,
            max_length: None,
        },
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

/// `toast-over-session` (tier-c1) — the **full-frame** golden (the F4
/// upgrade from the region-level pin): the complete session route
/// (transcript + tabs-off + the REAL composer via
/// [`ComposerSlot::Sibling`]) with the toast composited over the live
/// content — panel interior cells replace the frame, the two `┃`
/// rails keep the UNDERLYING background (the surface is transparent at
/// its edges: the golden pins them over the user-message fill on rows
/// 1-3 and over the blank base on rows 4-5).
#[test]
fn toast_over_session_matches_the_full_frame() {
    let toast = ToastOptions {
        title: Some("Fixture notice".into()),
        message: "Toast rendered over session content".into(),
        variant: "info",
        duration_ms: 60_000,
        action_label: None,
        action_command: None,
        modal: false,
    };
    let theme_state = crate::theme::ThemeState::v2_dark();
    let screen = crate::components::session_screen::SessionScreen {
        tabs: None,
        sidebar: SidebarMode::Auto,
        sidebar_view: crate::components::sidebar::SidebarView::fixture(
            "Toast fixture",
            crate::components::session_screen::FIXTURE_LOCATION,
        ),
        follow: crate::components::message::list::FollowState::Sticky,
        location: crate::components::session_screen::FIXTURE_LOCATION,
    };
    let composer = crate::components::session_screen::session_composer_frame(
        &crate::components::composer::PromptDraft::default(),
        76,
        &theme_state,
        crate::components::session_screen::FIXTURE_LOCATION,
    );
    let mut frame = crate::components::session_screen::paint_session_screen(
        &crate::components::message::list::message_groups_fixture(),
        &crate::components::message::list::TranscriptOptions::default(),
        &screen,
        &theme_state,
        80,
        30,
        crate::components::session_screen::ComposerSlot::Sibling(&composer),
    );
    let footer = FooterTheme::from_resolved(&theme_state.resolved.tokens);
    super::super::toast::overlay_toast_over_frame(
        &mut frame,
        &toast,
        0,
        &footer,
        &theme_state.resolved.tokens,
    );
    assert_golden("toast-over-session", 80, &frame);
}

/// The unstyled toast surface is directly constructible by other
/// surfaces (session screens paint it over live content).
#[test]
fn toast_paints_onto_an_existing_canvas() {
    let theme = FooterTheme::fallback_dark();
    let tokens = crate::theme::ThemeState::v2_dark().resolved.tokens;
    let frame = crate::components::toast_solid::render_toast_over_blank_solid(
        &ToastOptions::info("short"),
        0,
        40,
        10,
        &theme,
        &tokens,
    );
    assert!(frame.text_grid()[1].contains('┃'));
    assert!(frame.text_grid()[2].contains("short"));
    let _ = Span::new("", crate::render::CellColor::Default, crate::render::CellColor::Default);
}
