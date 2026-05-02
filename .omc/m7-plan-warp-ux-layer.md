# M7+ Plan — Warp-shape UX Layer

**Status**: Draft (2026-05-02). Trigger: iteration-19 honest accounting (`.omc/v1-prep-uiux-verification.md` + user push-back "這個介面是 warp??") confirmed that M0–M6 built the Warp **engine** (PTY, Vulkan grid, DCS-hook Block model, BYOK AI client, Termux runtime) but **did not** build the Warp **UX layer** (sidebar, agent-first prompt screen, Block-as-card rendering, prompt-style input box, model picker, search overlay, tab manager). v1.0 cannot truthfully claim "Warp on Android" until M7 ships.

## §0. Reference UI

User-supplied screenshot (Warp Desktop, 2026-05-02) — observed elements that drive this plan:

| Region | Real Warp |
|---|---|
| Top-left | Window controls + sidebar toggle + tools / palette icons |
| Top-center | "Search sessions, agents, files…" search bar |
| Top-right | New tab + inbox + account avatar |
| Left sidebar | "Search tabs…" filter + tabs list ("New agent conversation" with `~` cwd hint) |
| Pane header | Back arrow + ESC hint "ESC for terminal" + agent label "New agent conversation" + per-tab menu |
| Pane center | Empty agent canvas (waiting for first prompt) |
| Pane lower-mid | "New Oz agent conversation" hero + "Send a prompt below to start a new conversation in ~" + keyboard hints (⌘↵ start, ⌥⌘↵ cloud agent, /model, esc back) |
| Pane bottom hints | `?` for help, `/` for commands, `⌘Y` open conversation, `⇧⌘+` for code review |
| Bottom input box | "Warp anything e.g. Deploy my React app to Vercel and set up environment variables" |
| Bottom-left toolbar | working-dir picker (📁 ~), font-size (A⁺) |
| Bottom-right toolbar | model picker ("auto (cost-efficient)"), `/remote-control` indicator, microphone, attach |

These are concrete, codeable elements. M7–M10 each tackles a coherent slice.

## §1. Compose vs. continue-Vulkan decision

The big architectural fork. Options:

### Option A — Compose chrome around Vulkan terminal pane
Use Jetpack Compose for the sidebar, top bar, prompt box, model picker, and tabs. The terminal pane stays as a `SurfaceView` rendered by `warpui::platform::android` (the engine we already built). Compose wraps the SurfaceView via `AndroidView`.

**Pros**:
- Reuses every M0–M6 engine investment as-is (PTY, font shaping, Block model, DCS parser, dynamic_grid renderer)
- Compose is the official 2026-era Android idiom; Material 3 themes give us light / dark / dynamic-color for free
- Fast iteration on layout (preview, recomposition) — Vulkan rebuild cycles are slow in comparison
- Clear separation: chrome owns layout, terminal owns rendering

**Cons**:
- Two rendering systems coexist; SurfaceView z-order rules are subtle (z-fight with Compose drawer overlays)
- The `warpui` upstream is moving toward a unified GPUI scene tree — splitting it sacrifices long-term parity with Warp Desktop's pure-GPUI model
- Compose binary size (~3-4 MB) adds to APK after Termux bootstrap is included

### Option B — Pure Vulkan, port Warp's GPUI primitives
Continue with `warpui::platform::android` as the single rendering path. Implement sidebar, prompt box, model picker, etc., as GPUI primitives in Rust, mirroring Warp Desktop's `app/src/ui/` modules.

**Pros**:
- One rendering system, one event loop, one input model — cleaner architecture
- Maximally-faithful to upstream `warpdotdev/Warp` — cherry-picks across UI commits become viable
- Avoids the mixed Compose / SurfaceView gotchas

**Cons**:
- 3-5× more code to write than Option A — every primitive (button, list, scrollable panel, drawer, text input field) has to be drawn by hand on Vulkan
- Custom IME bridge is harder — Compose has `BasicTextField` doing the heavy lifting; pure Vulkan needs a hidden Android `View` for `InputConnection` (we already have `WarpInputView` for the terminal, but a prompt input field needs a separate one with different focus semantics)
- Material theming, dynamic color, accessibility services (TalkBack, font-size) are non-trivial without Compose
- Solo-dev: 6× the code = the project doesn't ship in 2026

### Option C — Compose-only re-implementation
Drop `warpui::platform::android` entirely; render Block cards and terminal output via Compose `Canvas` + `BasicText`. Use a normal `tokio` PTY in a Service.

**Pros**:
- Easiest path to a Material 3 polished Warp lookalike
- Fast iteration

**Cons**:
- Throws away M0–M6 engine work entirely (Vulkan renderer, font shaping, DCS parser plumbing)
- Diverges from Warp Desktop hard — cherry-picks become impossible
- Loses the "warpui runs on Android natively" thesis that justifies the project's existence

### Recommendation: Option A

Compose chrome around the Vulkan terminal pane. Reuses every engine investment, ships fast enough for solo-dev, gives us Material 3 + accessibility + theming without writing a UI toolkit. The mixed-rendering subtleties are real but constrained to: (a) a single SurfaceView z-position, (b) Compose drawer / sheet overlays drawn ABOVE the SurfaceView via `AndroidView` lifecycle. Termux Plus, Tabby, and other non-Warp Android terminals all use this exact pattern.

**Decision gate**: this picks the architectural shape. After we commit, M7 implementation can start; reversing later costs ~1 week.

## §2. Milestone breakdown

### M7 — Compose chrome scaffold (1.5–2 weeks)

Goal: replace `setContentView(frame)` with a Compose-based scaffold that hosts the existing `SurfaceView` + `WarpInputView` + `AccessoryRow` inside a `NavigationDrawer`-shaped layout.

Stories:

- **M7-S01** — Add `androidx.compose:compose-bom` + `androidx.activity:activity-compose` dependencies. Build green, no UX change yet (still uses the legacy `FrameLayout`).
- **M7-S02** — `WarpScaffold` Composable with a `ModalNavigationDrawer` (drawer + topBar + bottomBar + content). The content slot hosts an `AndroidView { frame }` so the existing terminal pane keeps rendering. Drawer + topBar + bottomBar are placeholders (blank panels with a Material 3 theme).
- **M7-S03** — Top search bar (`OutlinedTextField` with leading search icon, "Search sessions, agents, files…" placeholder). Non-functional in M7; M7-S08 wires search.
- **M7-S04** — Drawer with placeholder tab list. One tab per active PTY session (currently always "terminal_mode"). Tap → hide drawer. New-tab "+" button — non-functional in M7.
- **M7-S05** — Material 3 theme tokens (light + dark + system); status bar / nav bar tinted to match. Replaces the current hard-black SurfaceView clear color.
- **M7-S06** — Tab management — actual multi-tab. New tab spawns a new PTY cmd_id; switching tabs swaps the active grid in the SurfaceView. PtyManager already supports multi-cmd_id.
- **M7-S07** — Settings entry point (gear icon in top bar) → existing `SettingsActivity` (M6 BYOK form). Just a navigation hookup.
- **M7-S08** — Search overlay implementation: tabs list filter + grep-the-scrollback for "files" results.

Acceptance: launcher tap shows a Material 3 surface with sidebar / top bar / bottom bar; the terminal grid sits in the content area; existing IME + AccessoryRow + Block model continue to function.

### M8 — Block-as-card rendering (2–3 weeks)

Goal: replace flat-grid output with discrete, interactive Block cards. The Block model already exists (M3-S07); M8 is the UI surface.

Stories:

- **M8-S01** — `BlockCard` Composable: command header (with re-run / copy / explain icons), output body (`AndroidView` hosting a per-block Vulkan pane OR a Compose `BasicText` if the block fits within a perf budget — measure), exit-code badge (green / red / yellow chip).
- **M8-S02** — Block list as `LazyColumn` of `BlockCard`s. Replaces the single fullscreen grid as the default content slot in `WarpScaffold`. The "live" block (currently active command) renders the Vulkan grid; finished blocks are static Compose cards.
- **M8-S03** — Per-block actions: copy command, copy output, re-run, explain (forwards to the M6 Sonnet agent), share. The "explain" path already exists (M5-S03 BlockActionsSheet) — port the actions to the new card surface.
- **M8-S04** — Block selection / multi-select for "explain these blocks together". M5-S03 lays the groundwork; M8 surfaces it.
- **M8-S05** — Live-block streaming: as PTY output flows into the Block, the card grows incrementally. Uses the existing dirty-bit + `terminalTakeDirtyAndPushFrame` plumbing — but routed into the live `AndroidView` Vulkan pane instead of the fullscreen grid.
- **M8-S06** — Switch between "card mode" and "raw terminal mode" (vim / nvim / less force raw). Detect via the existing alt-screen ANSI sequence (`ESC[?1049h`) — when alt-screen is active, swap the live card for a fullscreen Vulkan grid.

Acceptance: `ls -la /` produces a Block card with the file list inside; `vim test.txt` switches to fullscreen raw mode; `:q` returns to card mode; the previous Block card is preserved above.

### M9 — Prompt-first agent screen (2–3 weeks)

Goal: replace "raw mksh prompt is the default entry point" with "an agent-first prompt-input screen is the default entry point". Matches the `New Oz agent conversation` UX in the reference screenshot.

Stories:

- **M9-S01** — `PromptComposer` Composable replacing the `AccessoryRow` + Gboard combo as the bottom input. Uses a Compose `TextField` with placeholder "Warp anything e.g. …", `IME_ACTION_SEND` enter, and a model picker chip on the trailing edge. The existing `WarpInputView` stays around for the terminal grid's own IME handling — `PromptComposer` is a separate Compose surface targeting the AGENT input.
- **M9-S02** — Working-directory picker (📁 ~ icon, opens a directory chooser bottom sheet). Tracks current `cwd` per-tab so the agent + terminal share state.
- **M9-S03** — Model picker chip ("auto (cost-efficient)") with dropdown listing Haiku / Sonnet / Opus + "/model" command palette equivalent.
- **M9-S04** — `/`-prefixed slash-command palette (`/model`, `/copy`, `/explain`, `/clear`, `/agent`). Compose `Popup` with autocomplete fed by a static list initially, extensible later.
- **M9-S05** — Agent-first home screen: when no PTY output yet, show "New Oz agent conversation" hero panel with keyboard-hint chips. First prompt creates a new agent block. ESC switches to the raw mksh terminal (still available as a fallback).
- **M9-S06** — Agent → Block bridging: agent responses render as cards in the same `LazyColumn` (M8-S02) interleaved with shell command blocks. The user sees one unified timeline.

Acceptance: launcher tap shows the agent home screen; first prompt produces an agent reply (using the M6 BYOK Haiku/Sonnet client); ESC returns to the raw mksh; the agent block + shell blocks coexist in the timeline.

### M10 — Polish + accessibility + low-end (1.5–2 weeks)

Goal: ship-readiness pass over the M7-M9 surface.

Stories:

- **M10-S01** — TalkBack labels on every interactive element (drawer, top bar, prompt composer, block cards, model picker).
- **M10-S02** — Font-size + dynamic-text sliders (Settings); per-block monospace font respected.
- **M10-S03** — Light / dark / system theme + accent color (Material You dynamic color on Android 12+).
- **M10-S04** — Low-end device gates: Pixel 4a (Adreno 618) ships an "engine-mode-only" fallback (skip Compose chrome, use the legacy FrameLayout) when GPU detection fails the M2 budget. Plan-Amendment-3 baseline holds.
- **M10-S05** — End-to-end Espresso-style integration test: launch app → type "echo hi" → verify block card with output → tap explain → verify agent response.
- **M10-S06** — APK size budget: Compose adds ~3-4 MB; current 7.4 MB → ~11-12 MB target. Stays well under the 80 MB / 120 MB-with-Termux gates.

Acceptance: ship gate for v1.0 — same gates as the iteration-18 v1-release-kickoff doc, refreshed with M7-M9 deliverables included.

## §3. Sequencing

Strict dependency order: M7 → M8 → M9 → M10. M9 depends on M8's Block card (agent responses need the same card surface). M10 is a polish pass after.

Parallelization opportunities (solo-dev caveats):
- **M7-S05 (theme)** can be drafted alongside M7-S02 since it's asset / token work
- **M8-S03 (per-block actions)** can be drafted while M8-S02 lays out the LazyColumn — both touch the `BlockCard` Composable

Total estimated effort: **7–10 weeks**, solo-dev. Compresses to ~5 weeks if helped by additional implementer agents.

## §4. Open architectural questions

1. **Which Compose version?** BOM `2025.10.00`+ for Material 3 + nested-scroll fixes; pin via `compose-bom` so the version matrix stays consistent.
2. **`AndroidView` lifecycle for the SurfaceView swap**: re-attaching SurfaceHolder.Callback across `WarpScaffold` recompositions is fiddly; M7-S02 should use `remember { SurfaceView(...) }` + `factory = { it }` to keep the same instance across recomposition. Verify swapchain doesn't re-init mid-recompose.
3. **Block card output rendering: Compose Text vs nested SurfaceView?** Test both at M8-S01 against a 100-block scrollback with mixed Latin / CJK content. Compose `BasicText` is simpler if it hits 60fps; SurfaceView nesting is faster for long output but z-order fragile.
4. **Agent prompt input vs terminal input**: M6's GhostSuggestController is bound to `WarpInputView` (the terminal IME). M9 introduces a SEPARATE Compose `TextField` for the agent prompt — does the ghost-suggest pipeline get reused, or do we keep the agent prompt simpler (no per-keystroke ghost) and reserve ghost-text for the terminal? Recommendation: terminal-only for ghost-text in M9; agent prompt uses model selector + slash commands instead.
5. **State management**: `ViewModel`s per tab? A single app-wide `WarpAppState` holding the tab list + active session? Compose's `rememberSaveable` for cross-rotation persistence. Worth a 30-min spike before M7-S01.

## §5. Non-goals (explicitly out of scope for M7-M10)

- Voice input (RecognizerIntent) — v2+ scope per the existing kickoff doc.
- Multi-window / split-pane within a single tab — v3+ optional.
- Cloud agent (`⌥⌘↵`) — depends on remote infra we don't have.
- `/remote-control` indicator integration — depends on Warp's remote-control protocol which is not open-source.
- Block search (cmd+f within blocks) — v1.1 polish.

## §6. Open questions for the user (decision gates)

These need user input before M7-S01 starts:

1. **Compose vs pure-Vulkan?** Recommendation: Option A (Compose chrome). Decisions get baked in at M7-S02; reversing costs ~1 week.
2. **Ship the engine preview as v0.7-engine NOW, separately from the v1.0 Warp-shape ship?** Option (a) ship v0.7-engine on iteration-19 commits + plan v1.0 around M7-M10 finishing. Option (b) hold all releases until M10 ships ~10 weeks. Option (c) ship intermediate previews (v0.8 after M7, v0.9 after M8, v1.0 after M10).
3. **Solo-dev cadence**: realistic time budget per week? Affects whether M7-M10 is 7 weeks or 10+ weeks.
4. **Re-baseline of v1-release-kickoff.md** — that doc still treats v1.0 as imminent. After this M7 plan lands, kickoff doc needs §3 rewritten to add M7-M10 as ship gates.

## §7. Why this is the right time to plan M7+ honestly

The iteration-18 verification surfaced the Warp-engine-vs-Warp-UX gap. iteration 19 (this iteration) confirmed the input wiring fix (commit `c6a7359`) made the engine actually usable for typed input, but the user-facing UI is still raw mksh + Gboard. Without M7-M10 the project ships a Termux-shaped terminal with the Warp engine running underneath — accurate but mis-marketed if called "Warp on Mobile v1.0".

Doing this plan as a doc-only deliverable (no code yet) lets the user pick the path forward (option 1/2/3 from the iteration-19 turn) before any compose dependency lands, while keeping the autonomous-iteration cadence productive.
