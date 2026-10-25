# Vendored `oc-uqr` — Rust port of `uqr` 0.1.3's QR encoder (F15)

The QR block the ref renders in the pair surfaces:

- TUI: `packages/tui/src/component/dialog-pair.tsx:4,82` —
  `renderUnicodeCompact(links[0], { border: 1 })` (uqr via node_modules).
- CLI: `packages/cli/src/commands/handlers/pair.ts:5,34` —
  `renderUnicodeCompact(links[0], { border: 2 })`, each row indented by 2.

`uqr` 0.1.3 (MIT, https://github.com/unjs/uqr) is itself a port of
Nayuki's "QR Code generator library" (MIT,
https://www.nayuki.io/page/qr-code-generator-library); the encoder
core (`encodeSegments`, the ECC tables, `addEccAndInterleave`,
`drawCodewords`, the 8 mask patterns, the penalty-based auto-mask)
is Nayuki's algorithm verbatim.

This crate is a faithful Rust translation of the uqr 0.1.3 dist
(`node_modules/uqr/dist/index.mjs` as pinned by ref
`packages/tui/package.json`), limited to the surface the ref uses:
`encode` (string segments, auto mask, `border`) and
`renderUnicodeCompact` (half-block palette). Module matrices are
byte-identical to uqr 0.1.3 — pinned by unit tests against golden
vectors generated from the ref-pinned `node_modules/uqr` itself
(`src/golden.json`; regen script below).

Divergences (deliberate, none observable on the ref surfaces):
- `ecc` defaults to `L` and `boost_ecc` to `false` exactly as uqr's
  `encode()` defaults (`index.mjs` `encode` destructure) — both fixed
  here because ref never overrides them.
- `renderANSI`/`renderSVG`/`invert`/`onEncoded`/byte-array input are
  not ported (unused by ref).
- JS `toUtf8ByteArray` encodes via `encodeURI` percent escapes —
  identical to standard UTF-8 byte encoding for every input, so the
  Rust port uses `str::as_bytes`.

Regenerating `src/golden.json` (from the ref checkout):

    cd /tmp/opencode/ref/packages/tui
    node -e '
      const uqr = require("./node_modules/uqr/dist/index.cjs");
      const cases = [["http://localhost:4096/auth/connect/pair-code-123",1],
                     ["http://localhost:4096/auth/connect/pair-code-123",2],
                     ["https://opencode.ai/auth/connect/abc123",1],
                     ["1234567890",1],["HELLO WORLD 123",1],["",1],["A",1],
                     ["héllo wörld",1],
                     // v5 long-URL coverage — keep in sync with golden.json.
                     ["https://example.com/some/very/long/path/that/pushes/the/version/up?query=1&more=2#frag",1]];
      const out = [];
      for (const [text, border] of cases) {
        const r = uqr.encode(text, { border });
        out.push({ text, border, version: r.version, size: r.size,
                   mask: r.maskPattern, data: r.data });
        out.push({ compactFor: text, border,
                   compact: uqr.renderUnicodeCompact(text, { border }) });
      }
      require("fs").writeFileSync("/tmp/qr-golden.json", JSON.stringify(out));
    '

License: MIT (uqr 0.1.1-0.1.3 line; © Anthony Fu; the underlying
algorithm © Nayuki).
