/**
 * Keypress-replay scripts (gate-8 non-frame acceptance / gate-9 addition e).
 *
 * Static, deterministic JSON scripts capturing the key-event sequences the
 * reference tests actually drive for three flows, with the state transitions
 * those tests assert — the fixtures for the future Rust key-replay harness.
 * These are NOT frame goldens: acceptance is replaying the keys against the
 * port and checking the expected route/dialog/tab state. Every step below
 * was replayed against the booted reference app (full-app fixture) before
 * being pinned: the expectations record observed behavior, not intent.
 *
 * Format: { id, description, source, keys, steps: [{ keys: [...], expect }] }
 * - `keys` names the token encoding: "kitty" — opentui kittyKeyboard key
 *   names as accepted by the reference mock-input (`pressKey`): single
 *   characters, "ctrl+x" style modifiers passed as modifiers (encoded here
 *   as "ctrl+x" tokens), and KeyCodes aliases like "BACKSPACE".
 * - `expect.frameIncludes`/`frameExcludes` use the stable labels the cited
 *   tests match on; `dialogs` is the modal stack depth; `exit` means the
 *   app.exit binding ran (renderer torn down).
 */

export interface ReplayStep {
  keys: string[]
  expect: {
    /** Substrings the rendered frame must contain after the keys. */
    frameIncludes?: string[]
    /** Substrings the rendered frame must NOT contain after the keys. */
    frameExcludes?: string[]
    /** Which surface holds input focus. */
    focus?: "prompt" | "dialog" | "transcript" | "none"
    /** The top route after the keys. */
    route?: { type: "home" | "session"; sessionID?: string }
    /** The modal dialog stack depth (0 = no dialog). */
    dialogs?: number
    /** App exit: the app.exit binding ran (renderer torn down). */
    exit?: boolean
  }
}

export interface ReplayScript {
  id: string
  description: string
  source: string
  keys: "kitty"
  steps: ReplayStep[]
}

export const replayScripts: ReplayScript[] = [
  {
    id: "exit-leader-q",
    description:
      "Leader-q exit sequence: with text in the prompt the app.exit binding is disabled (leader+q merely types q); once the draft is cleared, <leader> (ctrl+x) followed by q dispatches app.exit and tears the renderer down",
    source:
      "test/app-lifecycle.test.tsx (exit ordering) + src/config/keybind.ts (app.exit = 'ctrl+c,ctrl+d,<leader>q', leader default ctrl+x) + src/app.tsx app.exit layer enabled only while the prompt is empty",
    keys: "kitty",
    steps: [
      {
        keys: ["d", "r", "a", "f", "t"],
        expect: { frameIncludes: ["draft"], dialogs: 0, focus: "prompt", exit: false },
      },
      {
        // Binding disabled with text: the q lands in the textarea, no exit.
        keys: ["ctrl+x", "q"],
        expect: { frameIncludes: ["draftq"], dialogs: 0, focus: "prompt", exit: false },
      },
      {
        keys: ["BACKSPACE", "BACKSPACE", "BACKSPACE", "BACKSPACE", "BACKSPACE", "BACKSPACE"],
        expect: { frameExcludes: ["draftq"], dialogs: 0, focus: "prompt", exit: false },
      },
      {
        // Empty prompt re-arms the binding: leader+q exits the app.
        keys: ["ctrl+x", "q"],
        expect: { exit: true },
      },
    ],
  },
  {
    id: "palette-ctrl-p",
    description:
      "Command palette flow: ctrl+p opens the palette (suggested commands visible), typed text fuzzy-filters the list, Enter runs the selected command — here 'Rename session', which replaces the palette with the rename dialog",
    source:
      "test/cli/tui/command-palette.test.tsx (open → filter → enter select) + src/app.tsx COMMAND_PALETTE_COMMAND via command.palette.show (default ctrl+p)",
    keys: "kitty",
    steps: [
      {
        keys: ["ctrl+p"],
        expect: { frameIncludes: ["New session"], dialogs: 1, focus: "dialog" },
      },
      {
        keys: ["r", "e", "n", "a", "m", "e"],
        expect: { frameIncludes: ["Rename session"], frameExcludes: ["New session"], dialogs: 1, focus: "dialog" },
      },
      {
        // Running the rename command swaps the palette for DialogSessionRename.
        keys: ["enter"],
        expect: { frameIncludes: ["Rename session"], dialogs: 1, focus: "dialog" },
      },
    ],
  },
  {
    id: "permission-once-always-deny",
    description:
      "Permission prompt replies: the prompt lists Allow once / Always allow (when the request carries save patterns) / Reject with 'once' selected; Enter replies once, left/right move the selection, Escape opens the reject stage whose free-text reason Enter confirms",
    source:
      "test/cli/tui/input-scope.test.tsx (Enter → {decision:'once'}; Escape → reject stage keeping the reason draft) + src/routes/session/permission.tsx SessionQuestion (left/h, right/l, escapeKey=reject; app.exit shadowed while open)",
    keys: "kitty",
    steps: [
      {
        // Permission prompt mounts (request with save patterns): three options.
        keys: [],
        expect: { frameIncludes: ["Permission required", "Allow once", "Always allow", "Reject"], focus: "dialog" },
      },
      {
        keys: ["right"],
        expect: { frameIncludes: ["Always allow"], focus: "dialog" },
      },
      {
        keys: ["left", "enter"],
        // Default selection restored, Enter replies 'once' and clears the prompt.
        expect: { frameExcludes: ["Permission required"], focus: "prompt" },
      },
      {
        // A second request arrives; Escape moves to the reject stage.
        keys: [],
        expect: { frameIncludes: ["Permission required"], focus: "dialog" },
      },
      {
        keys: ["escape"],
        expect: { frameIncludes: ["Reject"], focus: "dialog" },
      },
      {
        // Free-text rejection reason, confirmed with Enter.
        keys: ["c", "h", "o", "o", "s", "e", " ", "a", "n", "o", "t", "h", "e", "r", " ", "w", "a", "y", "enter"],
        expect: { frameExcludes: ["Permission required"], focus: "prompt" },
      },
    ],
  },

  {
    id: "session-list-leader-l",
    description: "Session list dialog: <leader>l opens the Sessions dialog (search + the project's sessions listed, delete/rename + all-projects footer); typing filters the search field; Escape closes back to the prompt",
    source: "recorded against the booted reference app (createAppFixture, sessions Alpha/Beta served by the fixture server) — src/app.tsx session.list via DialogSessions; keys ctrl+x then l per config/keybind.ts session.list = '<leader>l'",
    keys: "kitty",
    steps: [
      {
            "keys": [
                  "ctrl+x",
                  "l"
            ],
            "expect": {
                  "frameIncludes": [
                        "Sessions",
                        "Alpha",
                        "Beta"
                  ],
                  "dialogs": 1,
                  "focus": "dialog"
            }
      },
      {
            "keys": [
                  "B",
                  "e",
                  "t",
                  "a"
            ],
            "expect": {
                  "frameIncludes": [
                        "Beta"
                  ],
                  "dialogs": 1,
                  "focus": "dialog"
            }
      },
      {
            "keys": [
                  "escape"
            ],
            "expect": {
                  "frameExcludes": [
                        "Sessions"
                  ],
                  "dialogs": 0,
                  "focus": "prompt"
            }
      }
],
  },

  {
    id: "theme-dialog-select",
    description: "Theme selection: the command palette filtered to 'theme' runs 'Switch theme' opening the Themes dialog (registry names sorted, the current theme marked); filtering to 'nord' narrows to the one row and Enter selects it, closing the dialog",
    source: "recorded against the booted reference app (createAppFixture) — src/app.tsx theme.switch via the palette; component/dialog-theme-list.tsx lists Object.keys(themes.all()).sort() with themes.set(opt.value) on select",
    keys: "kitty",
    steps: [
      {
            "keys": [
                  "ctrl+p"
            ],
            "expect": {
                  "frameIncludes": [
                        "Commands"
                  ],
                  "dialogs": 1,
                  "focus": "dialog"
            }
      },
      {
            "keys": [
                  "t",
                  "h",
                  "e",
                  "m",
                  "e"
            ],
            "expect": {
                  "frameIncludes": [
                        "Switch theme"
                  ],
                  "dialogs": 1,
                  "focus": "dialog"
            }
      },
      {
            "keys": [
                  "enter"
            ],
            "expect": {
                  "frameIncludes": [
                        "Themes",
                        "opencode"
                  ],
                  "dialogs": 1,
                  "focus": "dialog"
            }
      },
      {
            "keys": [
                  "n",
                  "o",
                  "r",
                  "d"
            ],
            "expect": {
                  "frameIncludes": [
                        "nord"
                  ],
                  "dialogs": 1,
                  "focus": "dialog"
            }
      },
      {
            "keys": [
                  "enter"
            ],
            "expect": {
                  "frameExcludes": [
                        "Themes"
                  ],
                  "dialogs": 0,
                  "focus": "prompt"
            }
      }
],
  },
]
