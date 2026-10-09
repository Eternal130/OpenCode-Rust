/**
 * Canonical golden-frame serialization.
 *
 * Text grid: the exact bytes of `TestRendererSetup.captureCharFrame()`
 * (rows joined with "\n"; trailing padding preserved).
 *
 * ANSI stream: deterministic per-cell-style serialization of
 * `TestRendererSetup.captureSpans()`:
 *   - one output line per screen row, joined with "\n", trailing "\n"
 *   - at each span whose style differs from the previous span on the line,
 *     emit ONE full-state SGR:  ESC [ 0 ; <attrs...> ; <fg params> ; <bg params> m
 *       attrs (ECMA-48): 1 bold, 2 dim, 3 italic, 4 underline, 5 blink,
 *                         7 inverse, 8 hidden, 9 strikethrough
 *       fg: 39 (default) | 38;5;N (indexed) | 38;2;R;G;B (rgb, 0-255)
 *       bg: 49 (default) | 48;5;N           | 48;2;R;G;B
 *     a fully-default span emits no SGR at all
 *   - if the line ends in a non-default style, terminate with ESC [ 0 m
 *
 * This format is position-independent, trivially parseable, and byte-stable.
 * The Rust port implements the identical format (tools/parity/src/tui_frame.rs
 * parser + crates/oc-tui/src/spike serializer) — keep the three in sync.
 */
import type { CapturedFrame, CapturedSpan } from "@opentui/core"

const ATTR_BOLD = 1 << 0
const ATTR_DIM = 1 << 1
const ATTR_ITALIC = 1 << 2
const ATTR_UNDERLINE = 1 << 3
const ATTR_BLINK = 1 << 4
const ATTR_INVERSE = 1 << 5
const ATTR_HIDDEN = 1 << 6
const ATTR_STRIKE = 1 << 7

export interface CellStyle {
  attrs: number
  fgKind: "default" | "indexed" | "rgb"
  fgIndex: number
  fgRgb: [number, number, number]
  bgKind: "default" | "indexed" | "rgb"
  bgIndex: number
  bgRgb: [number, number, number]
}

export function defaultStyle(): CellStyle {
  return {
    attrs: 0,
    fgKind: "default",
    fgIndex: 0,
    fgRgb: [0, 0, 0],
    bgKind: "default",
    bgIndex: 0,
    bgRgb: [0, 0, 0],
  }
}

export function spanStyle(span: CapturedSpan): CellStyle {
  const style = defaultStyle()
  style.attrs = (span.attributes ?? 0) & 0xff
  applyColor(span.fg, "fg", style)
  applyColor(span.bg, "bg", style)
  return style
}

function applyColor(color: CapturedSpan["fg"], channel: "fg" | "bg", style: CellStyle) {
  if (!color) return
  const intent = color.intent ?? "rgb"
  const [r, g, b] = color.toInts()
  if (intent === "default") return
  if (intent === "indexed") {
    style[`${channel}Kind`] = "indexed" as never
    style[`${channel}Index`] = color.slot ?? 0
    return
  }
  style[`${channel}Kind`] = "rgb" as never
  style[`${channel}Rgb`] = [r, g, b]
}

export function sameStyle(a: CellStyle, b: CellStyle): boolean {
  return (
    a.attrs === b.attrs &&
    a.fgKind === b.fgKind &&
    a.bgKind === b.bgKind &&
    (a.fgKind !== "indexed" || a.fgIndex === b.fgIndex) &&
    (a.bgKind !== "indexed" || a.bgIndex === b.bgIndex) &&
    (a.fgKind !== "rgb" || a.fgRgb[0] === b.fgRgb[0] && a.fgRgb[1] === b.fgRgb[1] && a.fgRgb[2] === b.fgRgb[2]) &&
    (a.bgKind !== "rgb" || a.bgRgb[0] === b.bgRgb[0] && a.bgRgb[1] === b.bgRgb[1] && a.bgRgb[2] === b.bgRgb[2])
  )
}

function colorParams(kind: CellStyle["fgKind"], index: number, rgb: [number, number, number], base: number): string[] {
  if (kind === "default") return [String(base + 9)] // 39 / 49
  if (kind === "indexed") return [String(base + 8), "5", String(index)] // 38;5;N / 48;5;N
  return [String(base + 8), "2", String(rgb[0]), String(rgb[1]), String(rgb[2])] // 38;2;R;G;B / 48;2;R;G;B
}

export function sgr(style: CellStyle): string {
  const params: string[] = ["0"]
  if (style.attrs & ATTR_BOLD) params.push("1")
  if (style.attrs & ATTR_DIM) params.push("2")
  if (style.attrs & ATTR_ITALIC) params.push("3")
  if (style.attrs & ATTR_UNDERLINE) params.push("4")
  if (style.attrs & ATTR_BLINK) params.push("5")
  if (style.attrs & ATTR_INVERSE) params.push("7")
  if (style.attrs & ATTR_HIDDEN) params.push("8")
  if (style.attrs & ATTR_STRIKE) params.push("9")
  params.push(...colorParams(style.fgKind, style.fgIndex, style.fgRgb, 30))
  params.push(...colorParams(style.bgKind, style.bgIndex, style.bgRgb, 40))
  return `\x1b[${params.join(";")}m`
}

function isDefault(style: CellStyle) {
  return sameStyle(style, defaultStyle())
}

export function serializeAnsi(frame: CapturedFrame): string {
  const lines: string[] = []
  for (const line of frame.lines) {
    let current = defaultStyle()
    let out = ""
    for (const span of line.spans) {
      const style = spanStyle(span)
      if (!sameStyle(current, style)) {
        out += sgr(style)
        current = style
      }
      out += span.text
    }
    if (!isDefault(current)) out += "\x1b[0m"
    lines.push(out)
  }
  return `${lines.join("\n")}\n`
}

export function sha256(data: string | Uint8Array): string {
  const hasher = new Bun.CryptoHasher("sha256")
  hasher.update(data)
  return hasher.digest("hex")
}
