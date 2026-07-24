# Pi GUI

Native Windows-first desktop shell for Pi, built with Rust and GPUI 0.2.2.

## Current maturity

Phases 9 through 16 complete run-control recovery UX, Pi's persisted session lifecycle, native branch history, the model/provider experience, the unified command system, the stock RPC extension UI host, the audited SDK resource plane, and native orchestration for installed Pi task, subagent, and goal extensions. Startup, discovery, correlated readiness, hydration, prompt acceptance, message streaming, tools, retries, compaction, session catalog scanning, direct Bash, SDK session/model/resource operations, orchestration snapshots/actions, command discovery, and shutdown all run behind dedicated workers. The GPUI-owned controller observes normalized state and never performs I/O from `render`.

The visible shell now shows only live or explicitly Loading, Awaiting, Unknown, stale, stopped, and error values:

- active session, lifecycle, and cost in the title bar;
- canonical current workspace and connection/recovery state in the main area;
- context, input/output tokens, cache usage, cost, model, and thinking in the inspector;
- one applicable Connect, Retry, or Stop action with pointer and keyboard paths;
- a native multiline composer with model and thinking pickers in the prompt chrome, grapheme-safe editing, IME, clipboard, selection, undo, wrapping, scrolling, and visible pending/accepted/rejected/uncertain delivery state;
- the authoritative current transcript with Markdown-rendered user input, assistant output, and thinking; visible custom messages; branch/compaction summaries; generic running/success/error/cancelled tool and Bash cards; lifecycle notices; and in-place accumulated partial updates;
- expandable sanitized tool arguments, bounded text/diff previews, image results, opaque detail fallback, copy controls, elapsed time, truncation metadata, and explicit full-output Reveal/Open folder actions;
- provider/model/time/stop/usage metadata with raw provider diagnostics and private payloads excluded from normalized diagnostics;
- selectable transcript text and near-bottom-only auto-follow, preserving manual scroll and selection during streaming.
- complete steering/follow-up queue visibility and delivery modes, scoped abort controls, retry countdown/attempt/final-error state, manual compaction with optional focus instructions, and auto-compaction/auto-retry controls;
- a real workspace-filtered v1-v3 JSONL session catalog with loading, empty, inaccessible, corrupt, refreshing/stale, and active-switch states;
- atomic new/switch/rename/export operations that retain the old transcript read-only until Pi confirms a replacement and never replay an uncertain prompt.
- a searchable, foldable, filterable session tree with the authoritative active leaf, keyboard navigation, entry details, stock fork-before-message and clone-path actions, and explicit same-file/new-file confirmations;
- a negotiated Pi SDK 0.82.0 stdio JSONL bridge for same-file navigation, optional branch summaries, labels, active-path JSONL export, and safe import into a new session file. Unsupported bridge actions stay hidden.
- cached-first provider and model catalogs with background refresh, stale/per-provider failures, a searchable model switcher and thinking chips attached to the prompt box, and Providers/Models/Thinking/Usage settings;
- provider-owned API-key, browser, device-code, text, select, progress, cancel, and logout flows. Secret input is masked and redacted from Rust debug output; catalogs expose no credential values, resolved environment values, headers, base URLs, or raw provider errors;
- honest separation between the active session model/thinking state (stock RPC) and Pi's persisted defaults/model cycle order (SDK settings), plus nullable current context, lifetime token/cache/reasoning totals, and estimated cost with zero pricing labeled as unpriced rather than free.
- `/` autocomplete plus a `Ctrl+Shift+P` palette merging native actions with Pi-discovered extension, prompt-template, and skill commands. Results are grouped by kind, retain duplicate/suffixed names, and show installed scope/origin/source/path provenance;
- native model/session/tree/fork/clone/compact/export/copy/abort/settings/help actions that never round-trip through the model, with argument hints for native commands and a refresh command for retained stale catalogs.
- native RPC notifications are shown as dismissible in-app notices, while installed TUI-only layout commands such as `/box`, `/rail`, and `/topbar` are omitted and guarded from model delivery.
- stock `select`, `confirm`, `input`, and `editor` extension dialogs in a deterministic FIFO modal queue, with focus containment, Escape cancellation, answer-type validation, one response per request ID, and timeout/process/session tombstones that prevent late or duplicate answers;
- keyed `setStatus` and text-line `setWidget` replacement/clear semantics, above/below-composer widget placement, native window `setTitle`, immediate last-write-wins `set_editor_text`, and redacted extension errors that retain only source basename and event context;
- an explicit RPC capability boundary: `custom()` overlays/components, component-factory widgets, custom editor/header/footer, TUI renderers, themes, and the process-local extension event bus are unsupported. Extension dialogs are untrusted application content, not secure permission prompts, because stock requests carry no verified provenance.
- a capability-gated Resource Center inventory for extensions, tools, skills, prompt templates, themes, packages, context files, and dynamically registered providers, including global/project/package scope, loaded/disabled/error state, source path, provenance, trust, diagnostics, active-tool state, and reload;
- rejected project trust for the sidecar execution plane: project resources are inventoried through Pi's public SDK but project extension/package code is never loaded. Missing packages use Pi's explicit non-installing resolution path;
- package install/remove/update/config advertised as unsupported until arbitrary-code confirmation, progress, pin/filter handling, and rollback-safe error UX exist.
- a session-scoped orchestration Inspector backed by `pi-tasks`, `pi-subagents`, and `pi-goal` stores and event-bus APIs: task dependency/blocker/output details and guarded execute/stop actions; live subagent lifecycle, queue, concurrency, schedules, worktrees, memory, and steer/stop/resume actions; and goal objective, status, token budget, elapsed time, queue, pause/resume/edit/clear actions;
- a conversation-style subagent overlay opened from any Inspector agent row, with the authoritative bounded live transcript tail, visible stale-ID and reconnect states, worktree/result metadata, and an agent-scoped composer. Generic transcript tool cards remain unchanged.

Accepted idle input uses `prompt`. While Pi is running, the primary action uses `steer` and follow-ups use `follow_up`. A composer line beginning with `!` executes the remaining text through Pi's direct `bash` RPC; `!!` does the same with `excludeFromContext=true`. Direct Bash is recorded locally until the authoritative session message reconciles it. Escape and the visible abort action route to `abort_bash` while direct Bash is active, independently from agent `abort`. Empty and rapid duplicate submissions are rejected. Prompt drafts clear only after the matching accepted response; Bash drafts clear when the local execution is recorded. Rejection and uncertain disconnect never trigger replay.

Discovered extension, prompt-template, and skill commands always invoke through Pi's `prompt` RPC. Prompt/skill commands use `streamingBehavior` while a run is active; extension commands execute immediately as Pi specifies. Exact dynamic-command arguments are preserved. Known TUI-only built-ins such as `/login`, `/logout`, `/resume`, `/share`, and `/theme` are rejected locally instead of becoming ordinary model prompts.

Rendered Markdown links are styled but not yet clickable. There is no share or concurrent top-level session process UI. Resource Center management is deliberately read-mostly; package mutation remains unavailable. Main-session queue items are authoritative and read-only because stock Pi RPC has no remove/restore operation. Reversible trash stays unavailable until both catalog ownership and a platform trash provider are proven.

## Prerequisite and launch

Install the tested Pi package. Existing Pi credentials are detected automatically, and providers can also be configured from the app's Providers settings:

```powershell
npm install -g @earendil-works/pi-coding-agent@0.82.0
cargo run
```

If Cargo is not on `PATH` on Windows:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run
```

The app connects automatically using the canonical current working directory with:

- `ProjectTrust::Reject` (`--no-approve`);
- a persisted session directory resolved with Pi precedence (`PI_CODING_AGENT_SESSION_DIR`, configured `sessionDir`, then the encoded default under the agent directory);
- installed extensions, skills, and prompt templates discovered for the command catalog under rejected project trust;
- themes and context files disabled because the native shell does not consume them;
- built-in Pi tools enabled so generic tool cards can observe their execution;
- offline mode disabled so Pi can resolve existing Pi-managed credentials and models.

Tool events are observational, not a permission prompt. Tools and direct Bash run with the user's normal account permissions. Returned `fullOutputPath` values are treated as untrusted: the app never reads or executes them automatically, and only passes them as direct process arguments after an explicit Reveal or Open folder action.

If no model is available, open Settings > Providers, authenticate, refresh catalogs, and choose a model. Pi remains the credential owner. The GUI transports provider prompts but never includes credential values in catalog state or diagnostics.

## Keyboard

| Action | Shortcut |
|---|---|
| Connect | `Ctrl+Alt+C` |
| Retry | `Ctrl+Alt+R` |
| Stop | `Ctrl+Alt+S` |
| Open command palette | `Ctrl+Shift+P` |
| Show native hotkey help | `Ctrl+/` |
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

The interface preserves the warm-charcoal Switzer/Bonny identity, with Cascadia Mono reserved for runtime data.

## Architecture

- `src/app.rs` - window bootstrap, service injection, keybindings, automatic connection, and send-only window-close shutdown
- `src/actions.rs` - logical runtime and focus actions
- `src/command_catalog.rs` - declarative native commands, installed provenance, filtering, duplicate identity, stale state, and TUI-only guards
- `src/controller.rs` - GPUI-owned controller plus pure attempt/generation gate
- `src/services/runtime_worker.rs` - injectable GPUI-independent service boundary and responsive worker coordinator
- `src/services/session_catalog.rs` - strict streaming v1-v3 JSONL metadata scanner, directory precedence, canonical workspace filtering, corruption reporting, and stale-scan worker
- `src/model_runtime.rs` - secret-free provider/model catalog, cached refresh, auth prompt state machine, sparse thinking, pricing-tier, and streaming-change policy
- `src/services/sdk_bridge.rs`, `bridge/pi-bridge.mjs`, and `bridge/protocol.schema.json` - negotiated, versioned, cancellable stdio JSONL SDK sidecar for the Phase 11 session gaps, Phase 12 ModelRuntime/settings/auth gaps, Phase 15 resource inventory/reload plane, and Phase 16 orchestration adapter transport
- `src/orchestration.rs`, `bridge/orchestration-adapter.mjs`, and `bridge/orchestration-core.mjs` - typed task/subagent/goal snapshots, stale/session guards, Pi event-bus actions, task DAG checks, schedule restoration, and bounded live subagent transcripts
- `src/resource_center.rs` - secret-free resource inventory contract, trust/load state, package mutation policy, and UI filters
- `src/services/rpc/` - Pi 0.82.0 wire contract, strict JSONL framing, correlated client, and runtime adapter
- `src/services/pi_process/` - executable discovery, capability probing, launch policy, and process-tree supervision
- `src/state/runtime.rs` - normalized owned runtime/transcript state, safe message metadata, stamped inputs, requests, and effects
- `src/state/reducer.rs` - pure lifecycle, hydration, streaming reconciliation, tool, queue, and extension reducer
- `src/state.rs` - truthful owned shell projections with stale/unknown semantics
- `src/views/root.rs` - observed runtime shell, conversation ownership, scroll pinning, and draft/acceptance correlation
- `src/views/conversation.rs` and `src/views/markdown.rs` - turn-grouped live transcript, stable selectable text entities, safe CommonMark presentation, metadata, and notices
- `src/views/tool_card.rs` - generic tool/Bash payload normalization, bounded previews, image/diff rendering, copy, and explicit output-path actions
- `src/views/composer/` - native GPUI input handler, text buffer, multiline layout/paint, and focused tests
- `src/views/controls.rs` - focus-visible recovery control and inspector rows

The worker uses a controller attempt generation in addition to `ConnectionGeneration` and `SessionEpoch`. A retry starts a fresh process and generation; prior-attempt startup/results cannot overwrite it, and a late obsolete client is stopped immediately. Recovery hydrates state and never resends a prompt.

Initial, recovery, and session-replacement hydration request `get_state` first, then messages, durable entries, session statistics, commands, available models, and fork candidates. Tree state is rebuilt from the flat durable entries instead of requesting Pi's recursively nested tree response, so long sessions cannot exceed the JSON decoder's recursion limit. Opening a saved session starts a fresh supervised connection directly on that file and retires the prior generation off the UI thread, preventing an in-process session handoff from leaving the shell disconnected. Fork, clone, import, and same-file navigation advance the session epoch before rebuilding every session-bound surface. Recovery resumes the persisted session and requests entries after the last durable cursor; an invalid cursor falls back to one full rebuild. Optional facet failures preserve prior valid values and do not make an otherwise ready connection unusable. Window/controller release requests shutdown without waiting on the GPUI event loop.
