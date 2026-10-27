//! In-TUI image display (R-07) — the Rust port of v2's transcript
//! image strip (`SessionImages`, ref session/index.tsx:2456): up to
//! three thumbnails above the user-message text, `+N more` overflow,
//! rendered through the terminal's graphics protocol.
//!
//! v2 delegates emission to opentui's `<image protocol="auto">`
//! element; the Rust TUI emits the escape sequences itself after the
//! cell grid draws (ratatui's backend writer):
//!
//! * **kitty graphics** — PNG passes through undecoded
//!   (`f=100`): one `a=T` transmit-and-display command per placement,
//!   sized in *cells* (`c`/`r`), chunked at 4096 payload bytes with
//!   `m=1` continuations, `q=2` (no reply), stable per-image ids so a
//!   redraw replaces in place and `a=d` clears the ones that left.
//! * **sixel** — detected, and *raw sixel bytes* (DCS `q` … `ST`)
//!   pass through at the cursor. PNG → sixel transcoding (RB-07
//!   residual) lives in [`sixel`]: a decodable PNG quantizes to ≤ 16
//!   color registers and emits a standard DCS `q` sequence at the
//!   cursor; a PNG that fails to decode keeps the **blocks**
//!   rendering (v2's own no-resolution path:
//!   `resolveImageRenderProtocol` maps `sixel` without a pixel
//!   resolution to `blocks`), exactly like a terminal with no
//!   protocol.
//! * **fallback** — the blocks renderer (R-08,
//!   [`crate::image_blocks`]): the decoded PNG quantizes to
//!   Unicode block glyphs with truecolor fg/bg — the port of
//!   opentui's `blocks` image protocol, which is what v2's
//!   `protocol="auto"` resolves to on terminals without kitty
//!   graphics / sixel. The bordered box + label placeholder
//!   remains only for payloads that fail to decode (v2's onError
//!   → "No preview", mini/scrollback.surface.ts:390).
//!
//! Detection: v2/opentui probes the terminal at runtime (DA1 /
//! graphics-capability queries). Since lane R0 the boot capability
//! probe (`crate::theme::probe_terminal`) performs that synchronous
//! DA1 round-trip in its existing read window and feeds the verdict
//! into [`resolve_protocol`]: attribute `4` in the `CSI ? {attrs} c`
//! reply means sixel (e.g. xterm `-ti vt340`'s
//! `\x1b[?62;4;6;8;15;22c`); an answered DA1 without it rules sixel
//! out; no answer (pipes, dumb terminals) falls back to the
//! environment heuristics — `KITTY_WINDOW_ID`/`TERM=xterm-kitty*`
//! (kitty), `TERM`/`TERM_PROGRAM` naming ghostty (kitty graphics),
//! `TERM` naming sixel (`*sixel*`, mlterm). Kitty stays env-decided:
//! kitty's DA1 reply (`\x1b[?62;6;9;15;22c`) never carries the sixel
//! attribute, and its graphics protocol is not a DA1-visible one —
//! `KITTY_WINDOW_ID` is authoritative (ref-equivalent fast path).
//! The [`GraphicsProtocol::detect_from`] seam keeps the env half a
//! pure function of env (tests inject maps); [`resolve_protocol`]
//! merges env + probe verdict as a pure decision table.

use std::collections::BTreeMap;
use std::io::Write;

pub mod sixel;

/// The terminal's image protocol (`protocol="auto"`'s resolution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphicsProtocol {
    Kitty,
    Sixel,
    /// No graphics support (or an unsupported data/protocol pairing):
    /// the placeholder box renders instead.
    #[default]
    None,
}

/// The image bytes one placement draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageData {
    /// Encoded PNG (kitty `f=100` passes it through verbatim).
    Png(Vec<u8>),
    /// Raw sixel bytes (DCS `q…ST`) — passthrough only.
    Sixel(Vec<u8>),
}

/// One image placed over a cell rect of the frame. `id` is stable per
/// image payload (the retransmit/replace seam; kitty ids must be
/// nonzero).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePlacement {
    pub id: u32,
    pub data: ImageData,
    /// 0-based cell column (the frame's coordinate space).
    pub x: u16,
    /// 0-based cell row.
    pub y: u16,
    /// Width in cells (v2 thumbs are `height * 2`: cells are ~1:2).
    pub cols: u16,
    /// Height in cells.
    pub rows: u16,
}

/// `GraphicsProtocol::detect` from a env-style map (`None` = unset).
#[must_use]
pub fn detect_from_env(env: &BTreeMap<String, String>) -> GraphicsProtocol {
    let term = env.get("TERM").cloned().unwrap_or_default();
    let term_lower = term.to_ascii_lowercase();
    let program = env.get("TERM_PROGRAM").cloned().unwrap_or_default();
    // kitty itself (the window id is set inside kitty; the TERM names
    // it for shells that preserve it) and ghostty (kitty graphics
    // support) take the kitty protocol.
    if env.contains_key("KITTY_WINDOW_ID")
        || term_lower.starts_with("xterm-kitty")
        || term_lower == "ghostty"
        || program == "ghostty"
    {
        return GraphicsProtocol::Kitty;
    }
    if term_lower.contains("sixel") || term_lower.starts_with("mlterm") {
        return GraphicsProtocol::Sixel;
    }
    GraphicsProtocol::None
}

/// Detect from the process environment.
#[must_use]
pub fn detect_protocol() -> GraphicsProtocol {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    detect_from_env(&env)
}

/// The RB-07 decision table: merge the env heuristic with the boot
/// DA1 probe's sixel verdict ([`crate::theme::TerminalProbe::da1_sixel`]).
///
/// * env says **kitty** (or ghostty) → `Kitty` — authoritative fast
///   path, unchanged (kitty's DA1 reply never carries the sixel
///   attribute and its graphics protocol is not DA1-visible).
/// * DA1 **answered** (`Some`) → the probe is authoritative for
///   sixel: `Some(true)` → `Sixel`, `Some(false)` → `None` (a
///   terminal that answers DA1 without attribute 4 has no sixel,
///   even under a `*sixel*`-named `TERM`).
/// * DA1 **unanswered** (`None` — headless pipes, dumb terminals,
///   probes swallowed by a multiplexer) → the env heuristic stands.
#[must_use]
pub fn resolve_protocol(env: GraphicsProtocol, da1_sixel: Option<bool>) -> GraphicsProtocol {
    if env == GraphicsProtocol::Kitty {
        return GraphicsProtocol::Kitty;
    }
    match da1_sixel {
        Some(true) => GraphicsProtocol::Sixel,
        Some(false) => GraphicsProtocol::None,
        None => env,
    }
}

/// FNV-1a over the payload — the stable placement id (forced nonzero:
/// kitty treats 0 as unspecified).
#[must_use]
pub fn image_id(data: &ImageData) -> u32 {
    let bytes = match data {
        ImageData::Png(bytes) | ImageData::Sixel(bytes) => bytes,
    };
    let mut hash: u32 = 0x811C_9DC5;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash | 1
}

/// The kitty APC payload escape for one placement: transmit-and-
/// display (`a=T`) a PNG (`f=100`) at `c`×`r` cells with a stable id,
/// `q=2` (quiet), chunked per the spec (payload splits ≤ 4096 bytes,
/// `m=1` marks continuation).
#[must_use]
pub fn kitty_display_escape(placement: &ImagePlacement) -> Vec<u8> {
    let ImageData::Png(bytes) = &placement.data else {
        return Vec::new();
    };
    // std-free base64 reuse (the clipboard encoder).
    let payload = crate::clipboard::base64_encode::encode(bytes);
    let mut out = Vec::with_capacity(payload.len() + 96);
    for (index, chunk) in payload.as_bytes().chunks(4096).enumerate() {
        out.extend_from_slice(b"\x1b_G");
        if index == 0 {
            out.extend_from_slice(
                format!(
                    "a=T,f=100,c={},r={},i={},q=2;",
                    placement.cols, placement.rows, placement.id
                )
                .as_bytes(),
            );
        } else {
            out.extend_from_slice(b"m=1;");
        }
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

/// The kitty delete escape for one image id (`a=d,d=i`).
#[must_use]
pub fn kitty_delete_escape(id: u32) -> Vec<u8> {
    format!("\x1b_Ga=d,d=i,i={id},q=2\x1b\\").into_bytes()
}

/// CUP (`CSI y+1 ; x+1 H`) — the cursor move before an emission.
#[must_use]
pub fn cursor_move_escape(x: u16, y: u16) -> Vec<u8> {
    format!("\x1b[{};{}H", y + 1, x + 1).into_bytes()
}

/// The sixel emission for one placement (positioned at the cursor):
/// raw sixel bytes pass through verbatim; PNG bytes transcode
/// through [`sixel::encode_sixel_from_png`] (RB-07) — an undecodable
/// PNG emits nothing and the placement reads as undrawable.
#[must_use]
pub fn sixel_display_escape(placement: &ImagePlacement) -> Vec<u8> {
    let body = match &placement.data {
        ImageData::Sixel(bytes) => Some(bytes.clone()),
        ImageData::Png(bytes) => sixel::encode_sixel_from_png(bytes),
    };
    let Some(body) = body else {
        return Vec::new();
    };
    let mut out = cursor_move_escape(placement.x, placement.y);
    out.extend_from_slice(&body);
    out
}

/// Which placements this protocol can actually draw (the placeholder
/// covers the rest).
#[must_use]
pub fn drawable(protocol: GraphicsProtocol, data: &ImageData) -> bool {
    match (protocol, data) {
        (GraphicsProtocol::Kitty, ImageData::Png(_))
        | (GraphicsProtocol::Sixel, ImageData::Sixel(_)) => true,
        // Sixel + PNG draws only when the transcode path can decode
        // the payload (RB-07); garbage keeps the blocks fallback.
        (GraphicsProtocol::Sixel, ImageData::Png(bytes)) => {
            crate::image_blocks::decode_png(bytes).is_some()
        }
        _ => false,
    }
}

/// Emit one frame's placements to `writer` (the run loop's post-draw
/// step, after ratatui flushed the cell grid): for each *drawable*
/// placement, CUP to its rect then emit the protocol bytes. Returns
/// the ids it drew (the caller's next-frame deletion set).
///
/// Deterministic and terminal-free: tests capture a `Vec<u8>`.
pub fn emit_frame_images<W: Write>(
    writer: &mut W,
    protocol: GraphicsProtocol,
    placements: &[ImagePlacement],
) -> std::io::Result<Vec<u32>> {
    let mut drawn = Vec::new();
    for placement in placements {
        if !drawable(protocol, &placement.data) {
            continue;
        }
        match protocol {
            GraphicsProtocol::Kitty => {
                writer.write_all(&cursor_move_escape(placement.x, placement.y))?;
                writer.write_all(&kitty_display_escape(placement))?;
            }
            GraphicsProtocol::Sixel => {
                writer.write_all(&sixel_display_escape(placement))?;
            }
            GraphicsProtocol::None => {}
        }
        drawn.push(placement.id);
    }
    writer.flush()?;
    Ok(drawn)
}

/// Emit the `a=d` deletes for ids that left the frame.
pub fn emit_image_deletes<W: Write>(writer: &mut W, ids: &[u32]) -> std::io::Result<()> {
    for id in ids {
        writer.write_all(&kitty_delete_escape(*id))?;
    }
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn detection_maps_the_environment() {
        assert_eq!(detect_from_env(&env(&[("TERM", "xterm-kitty")])), GraphicsProtocol::Kitty);
        assert_eq!(
            detect_from_env(&env(&[("KITTY_WINDOW_ID", "1"), ("TERM", "xterm-256color")])),
            GraphicsProtocol::Kitty
        );
        assert_eq!(detect_from_env(&env(&[("TERM", "xterm-ghostty")])), GraphicsProtocol::None);
        assert_eq!(detect_from_env(&env(&[("TERM_PROGRAM", "ghostty")])), GraphicsProtocol::Kitty);
        assert_eq!(
            detect_from_env(&env(&[("TERM", "xterm-256color-sixel")])),
            GraphicsProtocol::Sixel
        );
        assert_eq!(detect_from_env(&env(&[("TERM", "mlterm")])), GraphicsProtocol::Sixel);
        assert_eq!(
            detect_from_env(&env(&[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")])),
            GraphicsProtocol::None
        );
        assert_eq!(detect_from_env(&BTreeMap::new()), GraphicsProtocol::None);
    }

    #[test]
    fn resolution_merges_env_and_da1_probe() {
        // Env kitty/ghostty stays authoritative (kitty's DA1 never
        // reports the sixel attribute).
        assert_eq!(resolve_protocol(GraphicsProtocol::Kitty, Some(false)), GraphicsProtocol::Kitty);
        assert_eq!(resolve_protocol(GraphicsProtocol::Kitty, None), GraphicsProtocol::Kitty);
        // DA1 answered: the probe is authoritative for sixel — both
        // the upgrade (plain xterm-256color + attr 4) and the
        // downgrade (a *sixel*-named TERM on a terminal that answers
        // without attr 4).
        assert_eq!(resolve_protocol(GraphicsProtocol::None, Some(true)), GraphicsProtocol::Sixel);
        assert_eq!(resolve_protocol(GraphicsProtocol::Sixel, Some(true)), GraphicsProtocol::Sixel);
        assert_eq!(resolve_protocol(GraphicsProtocol::Sixel, Some(false)), GraphicsProtocol::None);
        assert_eq!(resolve_protocol(GraphicsProtocol::None, Some(false)), GraphicsProtocol::None);
        // DA1 unanswered: env fallback stands.
        assert_eq!(resolve_protocol(GraphicsProtocol::Sixel, None), GraphicsProtocol::Sixel);
        assert_eq!(resolve_protocol(GraphicsProtocol::None, None), GraphicsProtocol::None);
    }

    #[test]
    fn ids_are_stable_and_nonzero() {
        let png = ImageData::Png(vec![1, 2, 3]);
        assert_eq!(image_id(&png), image_id(&png));
        assert_ne!(image_id(&png), image_id(&ImageData::Png(vec![3, 2, 1])));
        assert_ne!(image_id(&png), 0);
    }

    #[test]
    fn kitty_escape_is_the_spec_shape() {
        let placement = ImagePlacement {
            id: 7,
            data: ImageData::Png(vec![0xFF, 0x00, 0xFF]),
            x: 4,
            y: 9,
            cols: 16,
            rows: 8,
        };
        let bytes = kitty_display_escape(&placement);
        // a=T transmit+display, PNG, cell-sized, quiet, id; then b64.
        let expected = format!(
            "\x1b_Ga=T,f=100,c=16,r=8,i=7,q=2;{}\x1b\\",
            crate::clipboard::base64_encode::encode(&[0xFF, 0x00, 0xFF])
        );
        assert_eq!(bytes, expected.as_bytes());
        assert_eq!(cursor_move_escape(4, 9), b"\x1b[10;5H");
        assert_eq!(kitty_delete_escape(7), b"\x1b_Ga=d,d=i,i=7,q=2\x1b\\");
    }

    #[test]
    fn kitty_escape_chunks_large_payloads() {
        // 3073 bytes → 4098 base64 chars ⇒ two APCs (4096 + 2), the
        // second marked m=1; the concatenated payload equals the full
        // base64.
        let png = vec![0xABu8; 3073];
        let placement = ImagePlacement {
            id: 3,
            data: ImageData::Png(png.clone()),
            x: 0,
            y: 0,
            cols: 8,
            rows: 4,
        };
        let bytes = kitty_display_escape(&placement);
        let text = String::from_utf8(bytes.clone()).unwrap();
        let st_count = text.matches("\x1b\\").count();
        assert_eq!(st_count, 2, "two APCs (payload + one continuation)");
        assert!(text.contains("\x1b_Ga=T,f=100,"), "first chunk carries the options");
        let second_start = text.find("\x1b_Gm=1;").expect("continuation APC");
        let head_payload =
            text[text.find(';').map(|i| i + 1).unwrap()..second_start].trim_end_matches("\x1b\\");
        let tail_payload = &text[second_start + "\x1b_Gm=1;".len()..text.len() - 2];
        assert_eq!(head_payload.len(), 4096, "first chunk is exactly the 4096-byte budget");
        assert_eq!(
            format!("{head_payload}{tail_payload}"),
            crate::clipboard::base64_encode::encode(&png),
            "chunks concatenate to the full base64 payload"
        );
    }

    #[test]
    fn sixel_transcodes_decodable_png_and_skips_garbage() {
        // RB-07: a real PNG renders under Sixel (CUP + transcoded DCS),
        // an undecodable one stays with the blocks fallback.
        let png_bytes = include_bytes!("../../tests/fixtures/r07-tiny-4x2.png").to_vec();
        let good = ImagePlacement {
            id: 21, data: ImageData::Png(png_bytes.clone()), x: 1, y: 1, cols: 4, rows: 2,
        };
        let bad = ImagePlacement {
            id: 22, data: ImageData::Png(vec![1, 2, 3]), x: 1, y: 1, cols: 4, rows: 2,
        };
        assert!(drawable(GraphicsProtocol::Sixel, &good.data));
        assert!(!drawable(GraphicsProtocol::Sixel, &bad.data));
        let mut out = Vec::new();
        let drawn =
            emit_frame_images(&mut out, GraphicsProtocol::Sixel, &[good.clone(), bad]).unwrap();
        assert_eq!(drawn, vec![21], "only the decodable PNG draws");
        let body = sixel_display_escape(&good);
        assert_eq!(out, body);
        assert!(body.ends_with(b"\x1b\\"));
        // Non-sixel terminals are unaffected: PNG stays undrawable
        // under Kitty's counterpart check is covered above; None never
        // draws.
        assert!(!drawable(GraphicsProtocol::None, &good.data));
        assert!(!drawable(GraphicsProtocol::Kitty, &ImageData::Sixel(b"x".to_vec())));
    }

    #[test]
    fn emit_writes_only_drawable_placements() {
        let png =
            ImagePlacement { id: 11, data: ImageData::Png(vec![1]), x: 2, y: 3, cols: 8, rows: 4 };
        let sixel = ImagePlacement {
            id: 13,
            data: ImageData::Sixel(b"\x1bPq...\x1b\\".to_vec()),
            x: 40,
            y: 3,
            cols: 8,
            rows: 4,
        };
        // Kitty: PNG draws (CUP + APC), sixel bytes do not.
        let mut out = Vec::new();
        let drawn =
            emit_frame_images(&mut out, GraphicsProtocol::Kitty, &[png.clone(), sixel.clone()])
                .unwrap();
        assert_eq!(drawn, vec![11]);
        // CUP to the rect, then the PNG's APC — and nothing else (the
        // sixel placement does not emit under kitty).
        let expected =
            [cursor_move_escape(png.x, png.y).as_slice(), kitty_display_escape(&png).as_slice()]
                .concat();
        assert_eq!(out, expected);

        // Sixel: raw sixel passes through at the cursor, PNG does not.
        let mut out = Vec::new();
        let drawn =
            emit_frame_images(&mut out, GraphicsProtocol::Sixel, &[png, sixel.clone()]).unwrap();
        assert_eq!(drawn, vec![13]);
        assert_eq!(out, sixel_display_escape(&sixel));

        // No protocol: nothing emits.
        let mut out = Vec::new();
        let drawn = emit_frame_images(&mut out, GraphicsProtocol::None, &[sixel]).unwrap();
        assert_eq!(drawn, Vec::<u32>::new());
        assert_eq!(out, Vec::<u8>::new());

        // Deletes emit per id.
        let mut out = Vec::new();
        emit_image_deletes(&mut out, &[11, 13]).unwrap();
        assert_eq!(out, [kitty_delete_escape(11), kitty_delete_escape(13)].concat());
    }
}
