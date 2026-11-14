// B2-fix plugin-fallback payload: deterministically broken on BOTH
// sides — the module top level throws, so every loader (ref 2.0.24
// <dir>/tui extension probe; rust guest dynamic import) lands in the
// failure state and the failure-STATE rendering becomes the diff face.
throw new Error("fixture: intentionally broken plugin (diff-ext plugin-fallback)");
