# Pideck — technical notes

Companion to the root [README](../README.md). This page covers launch policy, keyboard shortcuts, shell capabilities, and where code lives.

## Launch policy

Install the tested Pi package:

```powershell
npm install -g @earendil-works/pi-coding-agent@0.83.0
cargo run
```

If Cargo is not on `PATH` on Windows:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run
```

The launch folder is added to the persistent project sidebar. The app reopens the last available active project and stores project expansion and last-thread state in `pideck-projects.json` under Pi's agent directory.

The active project connects with:

- `ProjectTrust::Reject` (`--no-approve`)
- a persisted session directory resolved with Pi precedence (`PI_CODING_AGENT_SESSION_DIR`, configured `sessionDir`, then the encoded default under the agent directory)
- installed extensions, skills, and prompt templates discovered for the command catalog under rejected project trust
- TUI chrome extensions replaced by native UI (`activity-rail`, `box-editor`, `quiet-topbar`, `compact-resources`, `pi-bar`) omitted from the GUI Pi process only — they stay installed for the TUI and are never deleted
- themes disabled (native shell owns appearance); context files (`AGENTS.md` / `CLAUDE.md`) load the same way as stock Pi
- built-in Pi tools enabled so generic tool cards can observe execution
- offline mode disabled so Pi can resolve existing Pi-managed credentials and models

Tool events are observational, not permission prompts. Tools and direct Bash run with the user's normal account permissions. Returned `fullOutputPath` values are untrusted: the app never reads or executes them automatically, and only passes them as process arguments after an explicit Reveal or Open folder action.

If no model is available, open **Settings → Providers**, authenticate, refresh catalogs, and choose a model. Pi remains the credential owner. The GUI transports provider prompts but never includes credential values in catalog state or diagnostics.


## Keyboard

| Action | Shortcut |
|---|---|
| Connect | `Ctrl+Alt+C` |
| Retry | `Ctrl+Alt+R` |
| Stop | `Ctrl+Alt+S` |
| Open command palette | `Ctrl+Shift+P` |
| Attach files | `Ctrl+O` or drag onto the composer |
| Toggle workspace terminal | Ctrl + backtick |
| `@` file completion (in composer) | type `@` then `↑` `↓` `Enter` / `Esc` |
| `/` command completion (in composer) | type `/` then `↑` `↓` `Enter` / `Esc` |
| Show native hotkey help | `Ctrl+/` |
| Increase font size | `Ctrl++` (or `Ctrl+=`) |
| Decrease font size | `Ctrl+-` |
| Send while idle / steer while running | `Enter` |
| Insert newline | `Shift+Enter` |
| Queue follow-up while running | `Alt+Enter` |
| Run direct Bash / exclude it from context | `!command` / `!!command` |
| Abort active agent run or direct Bash scope | `Escape` |
| Move within an extension select/confirm dialog | Arrow keys |
| Accept the highlighted extension choice | `Enter` or `Space` |
| Cancel an extension dialog | `Escape` |
| Keep focus inside an extension dialog | `Tab` or `Shift+Tab` |
| Send a subagent steer/resume message | `Enter` |
| Close the subagent conversation | `Escape` |
| Activate visible recovery action | `Enter` or `Space` |
| Next focus | `Tab` |
| Previous focus | `Shift+Tab` |

Typography uses installed system fonts. Open `/settings` and choose separate Main, Sans, and Mono families in the Type tab; selections apply immediately and persist locally. Shell theme picks from the title-bar theme menu also apply immediately and persist in the same local settings file.

## Shell capabilities

Workers own discovery, readiness, hydration, prompts, streaming, tools, retries, compaction, session catalogs, direct Bash, SDK session/model/resource operations, orchestration, command discovery, and shutdown. The GPUI controller observes normalized state and never performs I/O from `render`. Surfaces show only live values or explicit Loading / Awaiting / Unknown / stale / stopped / error.

### Workspace and sessions

- Title bar: active session, lifecycle, cost
- Collapsible project sidebar: background thread catalogs, native folder picking, project switch/remove, live per-thread work status
- Workspace-filtered v1–v3 JSONL thread catalogs with loading, empty, inaccessible, corrupt, refreshing/stale, and active-switch states
- Atomic new/switch/rename/export that keeps the old transcript read-only until Pi confirms a replacement; uncertain prompts are never replayed
- Parallel saved threads: reuse an idle same-project runtime when safe, otherwise start an independent supervised runtime; sidebar shows working / cancelling / opening / attention
- Hover a non-active thread to move it to the Recycle Bin (Windows); active, busy, and out-of-catalog files stay protected

### Composer and input

- Multiline composer with model and thinking pickers, grapheme-safe editing, IME, clipboard, selection, undo, wrap, scroll
- Visible pending / accepted / rejected / uncertain delivery state
- Multi-file picker and drag/drop: images use Pi RPC image transport; small UTF-8 text/code is snapshotted into bounded named prompt blocks; larger readable files stay live path references; unsupported binaries are rejected without blocking the UI
- Idle send uses `prompt`; while running, primary action uses `steer` and follow-ups use `follow_up`
- `!` runs direct Bash via RPC; `!!` sets `excludeFromContext=true`. Escape / abort routes to `abort_bash` while Bash is active, independent of agent `abort`
- Empty and rapid-duplicate submissions rejected; prompt drafts clear only after matching accepted response; Bash drafts clear when local execution is recorded
- Steering/follow-up queue visibility, scoped abort, retry countdown/attempt/final-error, manual compaction with optional focus, auto-compaction/auto-retry controls

### Transcript and tools

- Markdown user / assistant / thinking; custom messages; branch/compaction summaries; tool and Bash cards; lifecycle notices; in-place partial updates
- Expandable sanitized tool args, bounded text/diff previews, image results, copy, elapsed time, truncation metadata, explicit Reveal/Open folder for full output paths
- Read-only Git workspace change summary after completed responses (per-file counts, untracked/binary awareness, expandable rows, bounded file-oriented diff viewer)
- Selectable transcript text; auto-follow only when near bottom
- Provider/model/time/stop/usage metadata with private payloads excluded from normalized diagnostics
- Markdown links are styled but not yet clickable; no share UI

### History and SDK bridge

- History panel focused on the active tip: stock fork-before-tip and clone-path actions with explicit same-file/new-file confirmations
- Negotiated Pi SDK 0.83.0 stdio JSONL bridge for same-file navigation, optional branch summaries, labels, active-path JSONL export, and safe import into a new session file. Unsupported bridge actions stay hidden

Details: [bridge/README.md](../bridge/README.md).

### Models and providers

- Cached-first provider/model catalogs with background refresh and stale/per-provider failure handling
- Searchable model switcher and thinking chips on the prompt box; Providers / Models / Thinking / Usage settings
- Provider-owned API-key, browser, device-code, text, select, progress, cancel, and logout flows
- Secret input masked and redacted from Rust debug output; catalogs expose no credential values, env values, headers, base URLs, or raw provider errors
- Active session model/thinking (stock RPC) kept separate from Pi's persisted defaults/model cycle order (SDK settings)
- Nullable current context, lifetime token/cache/reasoning totals, estimated cost; zero pricing labeled unpriced, not free

### Commands

- `/` autocomplete and `Ctrl+Shift+P` palette: native actions plus Pi-discovered extension, prompt-template, and skill commands
- Grouped by kind; retain duplicate/suffixed names; show installed scope/origin/source/path provenance
- `@` file autocomplete against the active workspace (directory drill-in, fuzzy search, quoted paths, shared keyboard nav)
- Native model/session/tree/fork/clone/compact/export/copy/abort/settings/help actions never round-trip through the model
- Discovered extension/prompt/skill commands always invoke through Pi's `prompt` RPC; prompt/skill use `streamingBehavior` while a run is active; extension commands execute immediately as Pi specifies
- Known TUI-only built-ins (`/login`, `/logout`, `/resume`, `/share`, `/theme`, layout commands such as `/box`, `/rail`, `/topbar`) rejected or omitted locally
- Native RPC notifications as dismissible in-app notices

### Extension UI host

- Stock `select`, `confirm`, `input`, and `editor` dialogs in a FIFO modal queue with focus containment, Escape cancel, answer-type validation, one response per request ID, and timeout/process/session tombstones
- Keyed `setStatus` and text-line `setWidget` replace/clear; above/below-composer widgets; native window `setTitle`; last-write-wins `set_editor_text`; redacted extension errors (source basename + event context only)
- Unsupported: `custom()` overlays/components, component-factory widgets, custom editor/header/footer, TUI renderers, themes, process-local extension event bus
- Extension dialogs are untrusted application content, not secure permission prompts (stock requests carry no verified provenance)

### Resource Center

- Capability-gated inventory: extensions, tools, skills, prompt templates, themes, packages, context files, dynamically registered providers
- Scope, loaded/disabled/error, source path, provenance, trust, diagnostics, active-tool state, reload
- Project trust rejected for the sidecar execution plane: project resources are inventoried through Pi's public SDK but project extension/package code is never loaded
- Package install/remove/update/config advertised as unsupported until confirmation, progress, pin/filter, and rollback-safe error UX exist
- Resource Center is deliberately read-mostly

### Orchestration

- Session-scoped Inspector for `pi-tasks`, `pi-subagents`, and `pi-goal`: task dependency/blocker/output and guarded execute/stop; live subagent lifecycle/queue/concurrency/schedules/worktrees/memory and steer/stop/resume; goal objective/status/budget/elapsed and guarded pause/resume/edit/clear
- Conversation-style subagent overlay from any Inspector agent row: bounded live transcript tail, stale-ID and reconnect states, worktree/result metadata, agent-scoped composer

Main-session queue items are authoritative and read-only (stock Pi RPC has no remove/restore).

## Architecture map

| Path | Role |
|---|---|
| `src/app.rs` | Window bootstrap, service injection, keybindings, automatic connection, send-only window-close shutdown |
| `src/actions.rs` | Logical runtime and focus actions |
| `src/command_catalog.rs` | Native commands, provenance, filtering, duplicate identity, stale state, TUI-only guards |
| `src/controller.rs` | GPUI-owned controller plus pure attempt/generation gate |
| `src/services/runtime_worker.rs` | Injectable service boundary and responsive worker coordinator |
| `src/services/session_catalog.rs` | Streaming v1–v3 JSONL metadata scanner, directory precedence, workspace filtering, stale-scan worker |
| `src/services/git_diff.rs` | Bounded read-only Git and untracked inspection for post-response change snapshots |
| `src/services/terminal.rs`, `src/views/terminal.rs` | Lazy bounded PTY transport and multi-instance terminal tab panel |
| `src/model_runtime.rs` | Secret-free provider/model catalog, auth prompt state machine, thinking/pricing/streaming policy |
| `src/services/sdk_bridge.rs`, `bridge/` | Negotiated stdio JSONL SDK sidecar (session gaps, ModelRuntime, resources, orchestration transport) |
| `src/orchestration.rs`, `bridge/orchestration-*.mjs` | Typed task/subagent/goal snapshots, guards, event-bus actions, bounded live subagent transcripts |
| `src/resource_center.rs` | Secret-free resource inventory, trust/load state, package mutation policy, UI filters |
| `src/services/rpc/` | Pi 0.83.0 wire contract, strict JSONL framing, correlated client, runtime adapter |
| `src/services/pi_process/` | Executable discovery, capability probing, launch policy, process-tree supervision |
| `src/state/runtime.rs` | Normalized owned runtime/transcript state, stamped inputs, requests, effects |
| `src/state/reducer.rs` | Pure lifecycle, hydration, streaming reconciliation, tool, queue, and extension reducer |
| `src/state.rs` | Owned shell projections with stale/unknown semantics |
| `src/views/root.rs` | Observed runtime shell, conversation ownership, scroll pinning, draft/acceptance correlation |
| `src/views/conversation.rs`, `src/views/markdown.rs` | Turn-grouped live transcript, selectable text, safe CommonMark, metadata, notices |
| `src/views/tool_card.rs` | Generic tool/Bash payload normalization, previews, image/diff, copy, output-path actions |
| `src/views/diff_summary.rs` | Response-tail changed-file disclosure and bounded file-oriented diff viewer |
| `src/views/composer/` | Native GPUI input handler, text buffer, multiline layout/paint |
| `src/views/controls.rs` | Focus-visible recovery control and inspector rows |

### Runtime pool and recovery

The worker uses a controller attempt generation in addition to `ConnectionGeneration` and `SessionEpoch`. A retry starts a fresh process and generation; prior-attempt startup/results cannot overwrite it, and a late obsolete client is stopped immediately. Recovery hydrates state and never resends a prompt.

`RootView` owns a bounded pool of thread runtimes (up to eight live). Idle draft-free navigation reuses a same-project process when safe; running and connecting threads stay supervised in the background; retained controllers keep hydrated transcript/history/model state; LRU eviction only when the pool needs room; unsent drafts stay attached to their thread. Each runtime/SDK bridge pair gets a unique orchestration endpoint so parallel threads in the same workspace cannot replace each other's adapter connection.

Initial, recovery, and session-replacement hydration request `get_state` first, then messages, durable entries, session statistics, commands, available models, and fork candidates. Tree state is rebuilt from flat durable entries (not Pi's recursively nested tree response) so long sessions cannot exceed the JSON decoder recursion limit. Opening a saved session starts a fresh supervised connection on that file and retires the prior generation off the UI thread. Fork, clone, import, and same-file navigation advance the session epoch before rebuilding every session-bound surface. Recovery resumes the persisted session and requests entries after the last durable cursor; an invalid cursor falls back to one full rebuild. Optional facet failures preserve prior valid values. Window/controller release requests shutdown without waiting on the GPUI event loop.
