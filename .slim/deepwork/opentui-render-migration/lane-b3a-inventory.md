# Lane B3a — crossterm event surface vs opentui coverage (inventory, commit 1)

Sources: `app.rs` (crossterm usage at c3a7bee), opentui-core @4ff646d
(`terminal/{session,pumps,probe,theme,tty,modes}.rs`, `renderer.rs`,
`events.rs`, `parse_keypress.rs`, `parse_mouse.rs`, `paste.rs`).

## crossterm surface actually consumed by the main TUI loop

| Surface (crossterm) | Where used | opentui coverage | Adapter action |
|---|---|---|---|
| `Event::Key` (Press only; Char+CTRL/ALT/SHIFT chords, Enter/Esc/Backspace/Delete/Tab/BackTab, arrows, Home/End/PageUp/PageDown, F(n)) | `crossterm_key_to_spec` (app.rs:9714) → `app.press(spec)` | `parse_keypress` (public) produces the same vocabulary (`name` = "return"/"escape"/"backspace"/"up"/…/single char, `ctrl`/`meta`/`shift` flags; `[Z` → shift+tab via `is_shift_key`) | NEW `opentui_key_to_spec(&ParsedKey)` replaces `crossterm_key_to_spec`, byte-input parity |
| Lone `ESC` flushed at read-chunk end | crossterm `parse_event(input_available=false)` → `Esc` immediately | **GAP**: `CliRenderer::feed_bytes` holds lone ESC(s) pending forever (`complete_unit_len`: "lone ESC(s): timeout resolution (follow-up port)") | Adapter pre-splits units itself (own `complete_unit` copy) and flushes all-ESC pending at chunk end — crossterm parity. Feasible because `parse_keypress` is public |
| Mouse: Down/Up/Moved/Drag(button) + Scroll(Up/Down/Left/Right), Left/Middle/Right, SHIFT/ALT/CTRL modifiers | `App::mouse_event(&crossterm::event::MouseEvent)` | `parse_mouse::MouseParser` (public) covers SGR+X10 incl. `MouseModifiers{shift,alt,ctrl}`, Drag-vs-Move classification, 4 scroll directions. **GAP**: a headless `CliRenderer` DRAINS mouse sequences into its (empty) renderable tree with no consumer event | Adapter intercepts complete mouse units with its own `MouseParser` before feeding the renderer; converts to oc-tui-owned mirror `MouseEvent` (same field/variant names as crossterm's) so `App::mouse_event` + tests keep their shape |
| `Event::Paste(String)` (bracketed `?2004`) | `app.paste_terminal_text` | `paste::BracketedPasteCollector` + `decode_paste_bytes` (public); renderer's own collector dispatches via `KeyHandler.on_paste` | Adapter runs its own collector (avoids the renderer double-dispatch) |
| `Event::FocusGained/FocusLost` (`?1004`) | attention marker + ?997 stand-in | `CliRenderEvent::TerminalFocus/TerminalBlur` from `Terminal::feed_input` | direct map into the app event enum |
| `Event::Resize(w,h)` | app.rs:9584 | `Terminal::check_resize` (TIOCGWINSZ poll) — B3a routes resize through the 50 ms input loop (mandate) | feed as app Resize event |
| Boot probes: OSC 11 bg, kitty `?u` query, DA1 attr-4 sixel (theme.rs `probe_terminal`, 800 ms window) | input thread + `resolve_protocol` | `Terminal::probe_capabilities` burst (OSC 10/11, DECRPM family, xtversion, kitty graphics) + `CliRenderEvent::Capabilities` (`caps.sixel` = DA1 attr 4) and `ThemeMode` (OSC 10/11 pair reduction) | probe retired from theme.rs; sixel/theme decisions ride Capabilities/ThemeMode events, conservative defaults (dark / env-seeded graphics) until events arrive |
| Live theme flip (?997 stand-in: focus-triggered OSC 11 re-probe, 10 s cooldown) | FocusGained arm | theme watch (`Terminal::feed_input` post-settle) reduces late OSC 10/11 pairs to ThemeMode; re-query bytes are oc-tui's | adapter writes the OSC 10/11 re-query on FocusGained (cooldown kept) — opentui has no public re-query trigger |
| kitty push `CSI >5u` only after `?u` answer | terminal_proto KITTY_QUERY/KITTY_PUSH | probe reports `caps.kitty_keyboard`; **no post-setup push API** (SetupOptions is setup-only; `enable_detected_features` skips the kitty re-push, gap-registered upstream) | keep KITTY_PUSH/KITTY_POP as oc-tui bytes (opentui-uncovered); parser flag switched at the same moment |

## Byte sequences opentui does NOT own (kept as oc-tui constants)

* kitty keyboard push/pop (`CSI >5u` / `CSI <u`) — post-setup negotiation.
* live mouse flip (`?1000h ?1002h ?1003h ?1006h` set + reverse) — `SetupOptions.use_mouse` covers setup only.
* OSC 10/11 focus re-query (the ?997 stand-in).
* kitty graphics delete-all (`_Ga=d,d=a`) — teardown-only.
* DECSCUSR + OSC 12 (cursor style/color), OSC 0 title — app-driven emissions.
* SIGHUP/SIGTERM write(2)-only restore — cannot run opentui's registry (locks) in a signal handler; kept static and conservative.

## Verdict

**No unfillable gaps.** The two real gaps (lone-ESC timeout; mouse events
swallowed by a headless CliRenderer) are both fillable in the oc-tui
adapter layer with public opentui APIs (`parse_keypress`,
`parse_mouse::MouseParser`, `paste::*`). Proceed with assembly.

## Routing decision (drives the implementation)

The CliRenderer's feed lane is used for what it is good at — probe/theme
routing and focus events — while the adapter parses keys/mouse/paste from
its own pending buffer over the same read chunks (deterministic
double-parse of one stream; the renderer side dispatches to no key/mouse
listeners, so nothing double-fires). `CliRendererConfig.use_mouse = false`
so the renderer silently consumes the mouse bytes the adapter already
handled.
