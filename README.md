# Pi GUI

Native Windows-first desktop shell for Pi, built with Rust and GPUI 0.2.2.

## Current maturity

Phase 6 connects a native multiline composer to the supervised external Pi 0.80.10 runtime. Startup, discovery, correlated readiness, hydration, prompt acceptance, event reads, retries, and shutdown all run behind a dedicated standard-thread worker. The GPUI-owned controller observes normalized reducer state and never performs I/O from `render`.

The visible shell now shows only live or explicitly Loading, Awaiting, Unknown, stale, stopped, and error values:

- active session, model, thinking level, lifecycle, and cost in the title bar;
- canonical current workspace and connection/recovery state in the main area;
- context, input/output tokens, cache usage, cost, model, and thinking in the inspector;
- one applicable Connect, Retry, or Stop action with pointer and keyboard paths;
- a native multiline composer with grapheme-safe editing, IME, clipboard, selection, undo, wrapping, scrolling, and visible pending/accepted/rejected/uncertain delivery state.

Accepted idle input uses `prompt`. While Pi is running, the primary action uses `steer` and follow-ups use `follow_up`; Escape sends `abort`. Empty and rapid duplicate submissions are rejected. The draft clears only after the matching accepted response and remains intact after rejection or uncertain disconnect.

There is no model switching, session browser, auth UI, transcript rendering, task/subagent view, resource browser, full queue management UI, or fake backend content. Those capabilities remain later phases.

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
- tools disabled (`--no-tools`);
- offline mode disabled so Pi can resolve existing Pi-managed credentials and models.

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
| Abort active run | `Escape` |
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
- `src/state/runtime.rs` - normalized owned runtime state, stamped inputs, requests, and effects
- `src/state/reducer.rs` - pure lifecycle, hydration, streaming, tool, queue, and extension reducer
- `src/state.rs` - truthful owned shell projections with stale/unknown semantics
- `src/views/root.rs` - observed runtime shell and draft/acceptance correlation
- `src/views/composer/` - native GPUI input handler, text buffer, multiline layout/paint, and focused tests
- `src/views/controls.rs` - focus-visible recovery control and inspector rows

The worker uses a controller attempt generation in addition to `ConnectionGeneration` and `SessionEpoch`. A retry starts a fresh process and generation; prior-attempt startup/results cannot overwrite it, and a late obsolete client is stopped immediately. Recovery hydrates state and never resends a prompt.

Initial and recovery hydration request `get_state` first, then messages, durable entries, session statistics, commands, available models, and tree state. Optional facet failures preserve prior valid values and do not make an otherwise ready connection unusable. Window/controller release requests shutdown without waiting for `RpcClient::stop` on the GPUI event loop.
