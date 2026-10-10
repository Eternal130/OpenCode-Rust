//! Mini footer dock goldens: the footer family's frames in the split-
//! footer viewport sizes the mini renderer actually mounts — the
//! composer at `FOOTER_HEIGHT` (4 rows, `runtime.lifecycle.ts:41`),
//! the permission body at `base + PERMISSION_ROWS` and the form body
//! at `base + FORM_ROWS` (`footer.ts:117-118` with `base = 3`).
//!
//! The goldens pin text-grid rows + the canonical ANSI
//! (`serialize_canonical`) at 80 cells; update with
//! `PARITY_UPDATE=1 cargo test -p oc-tui --test mini_footer_golden`.

use oc_tui::components::footer::form::{FormField, FormRequest};
use oc_tui::components::footer::permission::PermissionRequest;
use oc_tui::components::footer::theme::FooterTheme;
use oc_tui::mini::lifecycle::FooterPane;
use oc_tui::mini::{footer_base, FOOTER_HEIGHT, FORM_ROWS, PERMISSION_ROWS};
use oc_tui::render::serialize_canonical;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/mini-frames")
}

fn assert_golden(name: &str, pane: &FooterPane, width: usize, height: usize) {
    let frame = pane.render_frame(width, height);
    assert_eq!((frame.height, frame.width), (height, width), "{name} dims");
    let grid = frame.text_grid().join("\n");
    let ansi = serialize_canonical(&frame);
    let payload = format!("{grid}\n---ANSI---\n{ansi}");
    let path = goldens().join(format!("{name}.txt"));
    if parity::golden::update_mode() {
        std::fs::create_dir_all(goldens()).expect("golden dir");
        std::fs::write(&path, &payload).expect("write golden");
    }
    let golden = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("golden {name} (run with PARITY_UPDATE=1): {error}"));
    assert_eq!(golden, payload, "{name} frame diverged from the golden");
}

fn pane() -> FooterPane {
    let mut pane = FooterPane::new(FooterTheme::fallback_dark());
    pane.agent = Some("Build".into());
    pane.model_label = Some("gpt-5".into());
    pane
}

fn permission(id: &str) -> PermissionRequest {
    PermissionRequest {
        id: id.into(),
        session_id: "ses_min".into(),
        action: "bash".into(),
        resources: vec!["bash(echo hi)".into()],
        ..PermissionRequest::default()
    }
}

fn question_form(id: &str) -> FormRequest {
    FormRequest {
        id: id.into(),
        session_id: "ses_min".into(),
        title: "Question".into(),
        fields: vec![FormField::text("answer", "What should the agent do next?")],
    }
}

/// The composer dock: rail + transparent textarea, statusline below.
#[test]
fn golden_prompt_dock_at_footer_height() {
    assert_eq!(FOOTER_HEIGHT, 4, "the dock height the renderer mounts");
    assert_golden("prompt-dock-w80", &pane(), 80, FOOTER_HEIGHT);
}

/// A draft paints inside the transparent textarea region.
#[test]
fn golden_prompt_dock_with_draft() {
    let mut footer = pane();
    for key in "list files".chars() {
        let _ = footer.press(oc_tui::mini::lifecycle::FooterKey::Printable(key));
    }
    assert_golden("prompt-dock-draft-w80", &footer, 80, FOOTER_HEIGHT);
}

/// The permission blocker grows the dock to `base + PERMISSION_ROWS`.
#[test]
fn golden_permission_dock() {
    let mut footer = pane();
    footer.permission_asked(permission("prm_dock"));
    assert_eq!(footer.pane_height(), footer_base(FOOTER_HEIGHT) + PERMISSION_ROWS);
    assert_golden("permission-dock-w80", &footer, 80, footer.pane_height());
}

/// The form blocker grows the dock to `base + FORM_ROWS`.
#[test]
fn golden_form_dock() {
    let mut footer = pane();
    footer.form_created(question_form("frm_dock"));
    assert_eq!(footer.pane_height(), footer_base(FOOTER_HEIGHT) + FORM_ROWS);
    assert_golden("form-dock-w80", &footer, 80, footer.pane_height());
}
