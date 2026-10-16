//! Mini scrollback color goldens: the STYLED exit splash — the one
//! scrollback surface mini paints with per-cell colors (`splash.ts`'s
//! `draw()`/`cells()` half-block mark + the `Session`/`Continue`
//! body) — under the fallback-dark theme triple mapped through v2's
//! oklab quantizer (`mini/theme.ts:172-175` → `nearestIndexed`,
//! `theme.ts:95-109`).
//!
//! The golden pins each row's text AND per-span fg/bg so a quantizer
//! regression (wrong index, truecolor leak, tie-break drift) fails
//! here; update with
//! `PARITY_UPDATE=1 cargo test -p oc-tui --test mini_scrollback_golden`.

use oc_tui::components::footer::theme::FooterTheme;
use oc_tui::layout::StyledSpan;
use oc_tui::mini::splash::{exit_splash_styled_rows, SplashColors};
use ratatui::style::Color;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/mini-frames")
}

/// Serialize one styled row: `text⟨fg=…|bg=…⟩` per span (the color
/// vocabulary the quantizer emits — `Idx(n)` for indexed, `Rgb(r,g,b)`
/// would FAIL the no-truecolor expectation below before ever diffing).
fn encode_span(span: &StyledSpan) -> String {
    let fg = match span.style.fg {
        Some(Color::Indexed(n)) => format!("idx:{n}"),
        Some(Color::Rgb(r, g, b)) => format!("RGB-LEAK({r},{g},{b})"),
        Some(other) => format!("{other:?}"),
        None => "none".to_owned(),
    };
    let bg = match span.style.bg {
        Some(Color::Indexed(n)) => format!("idx:{n}"),
        Some(Color::Rgb(r, g, b)) => format!("RGB-LEAK({r},{g},{b})"),
        Some(other) => format!("{other:?}"),
        None => "none".to_owned(),
    };
    let bold =
        if span.style.add_modifier.contains(ratatui::style::Modifier::BOLD) { "+bold" } else { "" };
    format!("{}⟨fg={fg}|bg={bg}{bold}⟩", span.text)
}

fn assert_golden(name: &str, rows: &[Vec<StyledSpan>]) {
    let mut payload = String::new();
    for row in rows {
        for span in row {
            payload.push_str(&encode_span(span));
        }
        payload.push('\n');
    }
    assert!(
        !payload.contains("RGB-LEAK"),
        "{name}: scrollback colors must be oklab-quantized xterm-256 indexes"
    );
    let path = goldens().join(format!("{name}.txt"));
    if parity::golden::update_mode() {
        std::fs::create_dir_all(goldens()).expect("golden dir");
        std::fs::write(&path, &payload).expect("write golden");
    }
    let golden = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("golden {name} (run with PARITY_UPDATE=1): {error}"));
    assert_eq!(golden, payload, "{name} styled scrollback diverged from the golden");
}

/// The colored exit banner: mark glyphs in `left` (muted → idx 8),
/// `_` cells shadowed (shade → idx 233), title/command in `right`
/// (text → idx 255) + bold — exactly the oklab quantizer's picks.
#[test]
fn golden_exit_splash_colored_scrollback() {
    let colors = SplashColors::from_footer_theme(&FooterTheme::fallback_dark());
    assert_eq!(colors.left, Color::Indexed(8), "muted #808080 → oklab idx 8");
    assert_eq!(colors.right, Color::Indexed(255), "text #eeeeee → oklab idx 255");
    assert_eq!(colors.shadow, Color::Indexed(233), "shade #141414 → oklab idx 233");
    let rows = exit_splash_styled_rows(Some("fix parser"), "ses_123", false, 80, &colors);
    assert_golden("exit-splash-colored-w80", &rows);
}

/// The mono theme stays codeless (default fg/bg — `RUN_THEME_MONO`,
/// `theme.ts:203-255`): the quantizer never sees it. `Color::Reset`
/// is the default-fg intent (no SGR color emitted), not a code.
#[test]
fn golden_exit_splash_mono_has_no_color_codes() {
    let rows =
        exit_splash_styled_rows(Some("fix parser"), "ses_123", true, 80, &SplashColors::mono());
    for row in &rows {
        for span in row {
            assert!(
                matches!(span.style.fg, None | Some(Color::Reset)),
                "mono rows carry no fg: {} ({:?})",
                span.text,
                span.style.fg
            );
            assert!(
                matches!(span.style.bg, None | Some(Color::Reset)),
                "mono rows carry no bg: {} ({:?})",
                span.text,
                span.style.bg
            );
        }
    }
}
