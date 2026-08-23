# PiDeck

PiDeck is a native desktop shell for the Pi coding agent. It uses Rust and GPUI for the application surface, keeps the runtime state in deterministic reducers, and uses a bounded Node sidecar only for Pi SDK capabilities that are not available through the stock RPC runtime.

## What the app provides

- A virtualized, turn-grouped conversation with live tool activity and selectable output.
- A workspace sidebar for projects and threads, with keyboard navigation and reversible thread deletion.
- A compact, transcript-aligned message editor with model, thinking, attachment, queue, Bash, and command controls.
- A right-side session-details sheet for usage, orchestration, queue, and run controls.
- A live Git diff surface that refreshes while the agent is working and preserves the last valid snapshot.
- An embedded terminal, history browser, model/provider settings, resource inventory, and update flow.

## Development

### Requirements

- Rust stable 1.85 or newer.
- A supported desktop environment for GPUI. The current application setup is Windows-first.
- Node.js for the Pi SDK sidecar tests and for running the materialized bridge during development.
- A compatible Pi coding-agent installation for end-to-end runtime use.

### Run

```sh
cargo run
```

The development profile intentionally optimizes the renderer-facing dependencies so `cargo run` is representative enough for UI work while preserving useful debug information.

### Validate

With the Rust toolchain available:

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
node --test bridge/*.test.mjs
```

For a toolchain-independent repository audit:

```sh
python scripts/verify_static.py
find bridge -name '*.mjs' -print0 | xargs -0 -n1 node --check
```

## Keyboard paths

| Action | Shortcut |
| --- | --- |
| Focus prompt | `Ctrl+L` |
| Toggle workspace sidebar | `Ctrl+B` |
| Toggle session inspector | `Ctrl+I` |
| Toggle terminal | <kbd>Ctrl</kbd>+<kbd>`</kbd> |
| Command palette | `Ctrl+Shift+P` |
| Hotkey help | `Ctrl+/` |
| Attach files | `Ctrl+O` |
| Send or steer | `Enter` |
| Queue follow-up | `Alt+Enter` |
| Insert newline | `Shift+Enter` |
| Abort or dismiss the active overlay | `Esc` |

## Architecture

The application keeps domain state outside rendering and makes stale-result rejection explicit:

- `src/controller.rs` owns GPUI-facing runtime orchestration.
- `src/controller/runtime_flow.rs` batches replaceable stream updates within one display frame without reordering lifecycle records.
- `src/state/` contains the UI-independent runtime model and reducer.
- `src/services/` owns filesystem, process, terminal, Git, RPC, and sidecar boundaries.
- `src/views/root/` composes the shell from independent sidebar, conversation, composer, terminal, inspector, diff, and overlay modules.
- `bridge/` contains the bounded JSONL SDK sidecar and orchestration adapter.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/UI_SYSTEM.md](docs/UI_SYSTEM.md), [docs/ZED_UI_REWORK.md](docs/ZED_UI_REWORK.md), and [STATIC_VERIFICATION.md](STATIC_VERIFICATION.md) for the detailed boundaries, design rationale, and validation record.

## Privacy

PiDeck does not add telemetry, analytics, tracking, or remote reporting. Runtime and project data remain inside the user-selected workspace and the local Pi process boundary unless a configured model provider necessarily receives a prompt.
