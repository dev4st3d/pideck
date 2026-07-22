# Pi GUI

Native Windows-first desktop shell for Pi, built with Rust and GPUI 0.2.2.

## Current maturity

Phase 8 connects the native conversation surface to the supervised external Pi 0.80.10 runtime, including generic tool execution cards and direct RPC Bash. Startup, discovery, correlated readiness, hydration, prompt acceptance, message streaming, tools, retries, direct Bash, and shutdown all run behind a dedicated standard-thread worker. The GPUI-owned controller observes normalized reducer state and never performs I/O from `render`.

The visible shell now shows only live or explicitly Loading, Awaiting, Unknown, stale, stopped, and error values:

- active session, model, thinking level, lifecycle, and cost in the title bar;
- canonical current workspace and connection/recovery state in the main area;
- context, input/output tokens, cache usage, cost, model, and thinking in the inspector;
- one applicable Connect, Retry, or Stop action with pointer and keyboard paths;
- a native multiline composer with grapheme-safe editing, IME, clipboard, selection, undo, wrapping, scrolling, and visible pending/accepted/rejected/uncertain delivery state;
- the authoritative current transcript with user and assistant text, thinking, visible custom messages, branch/compaction summaries, generic running/success/error/cancelled tool and Bash cards, lifecycle notices, and in-place accumulated partial updates;
- expandable sanitized tool arguments, bounded text/diff previews, image results, opaque detail fallback, copy controls, elapsed time, truncation metadata, and explicit full-output Reveal/Open folder actions;
- provider/model/time/stop/usage metadata with raw provider diagnostics and private payloads excluded from normalized diagnostics;
- selectable transcript text and near-bottom-only auto-follow, preserving manual scroll and selection during streaming.

Accepted idle input uses `prompt`. While Pi is running, the primary action uses `steer` and follow-ups use `follow_up`. A composer line beginning with `!` executes the remaining text through Pi's direct `bash` RPC; `!!` does the same with `excludeFromContext=true`. Direct Bash is recorded locally until the authoritative session message reconciles it. Escape and the visible abort action route to `abort_bash` while direct Bash is active, independently from agent `abort`. Empty and rapid duplicate submissions are rejected. Prompt drafts clear only after the matching accepted response; Bash drafts clear when the local execution is recorded. Rejection and uncertain disconnect never trigger replay.

There is no model switching, session browser, auth UI, rich Markdown/link rendering, task/subagent view, resource browser, full queue management UI, or fake backend content. Those capabilities remain later phases.

## Prerequisite and launch

Install the tested Pi package and configure any provider credentials through Pi itself:

```powershell
npm install -g @earendil-works/pi-coding-agent@0.80.10
cargo run
```

If Cargo is not on `PATH` on Windows:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run
```

The app connects automatically using the canonical current working directory with:

- `ProjectTrust::Reject` (`--no-approve`);
- an ephemeral session (`--no-session`);
- extensions, skills, prompt templates, themes, and context files disabled;
- built-in Pi tools enabled so generic tool cards can observe their execution;
- offline mode disabled so Pi can resolve existing Pi-managed credentials and models.

Tool events are observational, not a permission prompt. Tools and direct Bash run with the user's normal account permissions. Returned `fullOutputPath` values are treated as untrusted: the app never reads or executes them automatically, and only passes them as direct process arguments after an explicit Reveal or Open folder action.

If no model is available, configure credentials in Pi and choose Retry. The GUI does not read, display, or store credential values.

## Keyboard

| Action | Shortcut |
|---|---|
| Connect | `Ctrl+Alt+C` |
| Retry | `Ctrl+Alt+R` |
| Stop | `Ctrl+Alt+S` |
| Send while idle / steer while running | `Enter` |
| Insert newline | `Shift+Enter` |
| Queue follow-up while running | `Alt+Enter` |
| Run direct Bash / exclude it from context | `!command` / `!!command` |
| Abort active agent run or direct Bash scope | `Escape` |
| Activate visible recovery action | `Enter` or `Space` |
| Next focus | `Tab` |
| Previous focus | `Shift+Tab` |

The interface preserves the warm-charcoal Switzer/Tanker identity, with Cascadia Mono reserved for runtime data.

## Architecture

- `src/app.rs` - window bootstrap, service injection, keybindings, automatic connection, and send-only window-close shutdown
- `src/actions.rs` - logical runtime and focus actions
- `src/controller.rs` - GPUI-owned controller plus pure attempt/generation gate
- `src/services/runtime_worker.rs` - injectable GPUI-independent service boundary and responsive worker coordinator
- `src/services/rpc/` - Pi 0.80.10 wire contract, strict JSONL framing, correlated client, and runtime adapter
- `src/services/pi_process/` - executable discovery, capability probing, launch policy, and process-tree supervision
- `src/state/runtime.rs` - normalized owned runtime/transcript state, safe message metadata, stamped inputs, requests, and effects
- `src/state/reducer.rs` - pure lifecycle, hydration, streaming reconciliation, tool, queue, and extension reducer
- `src/state.rs` - truthful owned shell projections with stale/unknown semantics
- `src/views/root.rs` - observed runtime shell, conversation ownership, scroll pinning, and draft/acceptance correlation
- `src/views/conversation.rs` - turn-grouped live transcript, stable text entities, safe metadata, notices, and selectable plain text
- `src/views/tool_card.rs` - generic tool/Bash payload normalization, bounded previews, image/diff rendering, copy, and explicit output-path actions
- `src/views/composer/` - native GPUI input handler, text buffer, multiline layout/paint, and focused tests
- `src/views/controls.rs` - focus-visible recovery control and inspector rows

The worker uses a controller attempt generation in addition to `ConnectionGeneration` and `SessionEpoch`. A retry starts a fresh process and generation; prior-attempt startup/results cannot overwrite it, and a late obsolete client is stopped immediately. Recovery hydrates state and never resends a prompt.

Initial and recovery hydration request `get_state` first, then messages, durable entries, session statistics, commands, available models, and tree state. Optional facet failures preserve prior valid values and do not make an otherwise ready connection unusable. Window/controller release requests shutdown without waiting for `RpcClient::stop` on the GPUI event loop.
