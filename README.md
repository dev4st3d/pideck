# Pideck

A native Windows workspace for the Pi coding agent, built with Rust and GPUI 0.2.2. Pi owns agent execution, sessions and provider credentials.

Requires **Pi 0.85.1** (`@earendil-works/pi-coding-agent@0.85.1`).

## Features

- Collapsible project sidebar with multi-thread catalogs, live background-work status, and session switch / rename / export
- Multiline composer with drag-and-drop attachments, `@` file completion, `/` command completion, and direct Bash (`!` / `!!`)
- Steer mid-run with `Enter`, queue follow-ups with `Alt+Enter`, delivery state always visible
- Streaming Markdown transcript with tool cards, expandable args, diff and image previews, copy, and elapsed time
- Read-only Git change summary with a bounded per-file diff viewer after each response
- Provider authentication, searchable model switcher, and thinking controls — Pideck never stores credentials
- Command palette (`Ctrl+Shift+P`) merging native actions with discovered extension, skill, and prompt-template commands
- Embedded PTY terminal, keyboard-first recovery (connect / retry / stop), and hotkey help (`Ctrl+/`)
- Resource Center inventory for extensions, tools, skills, prompt templates, themes, and packages
- No telemetry, no analytics, no remote reporting

## Supported extensions

Supported Pi extension UI requests are mapped to native controls: `select`, `confirm`, `input`, and `editor` dialogs become native windows, status lines and widgets render in place, window titles update the title bar, and extension commands join the palette.

**Inspector integrations retained from the supplied snapshot**

These are the snapshot's reference extension versions, not newly certified Pi 0.85.1 combinations. The adapter fixture tests exercise their protocol projections; live extension recertification is outstanding.

| Extension | Snapshot reference | Documented Pi range | Interface |
|---|---:|---:|---|
| `@tintinweb/pi-tasks` | 0.7.2 | `>=0.80.0` | Task lists with dependencies, blockers, and outputs; guarded execute and stop |
| `@tintinweb/pi-subagents` | 0.15.2 | `>=0.80.0` | Live lifecycle, queue, concurrency, schedules, worktrees, and memory; steer, stop, and resume agents; conversation overlay with a bounded live transcript |
| `@narumitw/pi-goal` | 0.51.0 | `>=0.80.6` | Objective, wait state, safety limits, queue, budget, and elapsed time; guarded pause, resume, edit, and clear |
| `@juicesharp/rpiv-ask-user-question` | 2.5.1 | `*` | Multi-question flows, choices, previews, notes, and multi-select answered through native dialogs |

`pi-bar` 0.3.39 is the snapshot reference for the native-shell exclusion policy. It stays installed for Pi's TUI but is omitted from GUI sessions because PiDeck supplies the native status shell.

## Requirements

| | |
|---|---|
| OS | Windows |
| Rust | Current stable with rustfmt and Clippy; declared syntax floor 1.88 (see `rust-toolchain.toml`) |
| Pi | `@earendil-works/pi-coding-agent@0.85.1` |
| Node | 22.19+; required only for Pi and the SDK bridge sidecar |

## Install prerequisites on Windows

Install a current stable Rust toolchain with the MSVC build tools, and Node 22.19.0 or newer. The source targets the official npm Pi package, not a frontend build toolchain. No application `package.json` or `npm install` in this repository is needed.

## Quick start from source

```powershell
npm install -g @earendil-works/pi-coding-agent@0.85.1
cargo run --locked
```

If Cargo is not on `PATH`:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run
```

On first launch, pick or open a project — it joins the sidebar and reopens next time. Existing Pi credentials are reused; you can also authenticate from **Settings → Providers**.

The main composer now keeps its height while focus moves. Session switches retain editor selection, undo history and transcript scroll position. Stop clears Pi's queue before aborting. Cleared messages appear as saved inputs and can be appended to the current draft with **Restore next** or **Ctrl+Shift+R**; typed text is not overwritten.

## Keyboard essentials

| Action | Keys |
|---|---|
| Command palette | `Ctrl+Shift+P` |
| Connect · Retry · Stop | `Ctrl+Alt+C` · `Ctrl+Alt+R` · `Ctrl+Alt+S` |
| Workspace terminal | `` Ctrl+` `` |
| Attach files | `Ctrl+O` or drag onto the composer |
| Send / steer · newline · queue follow-up | `Enter` · `Shift+Enter` · `Alt+Enter` |
| Direct Bash · Bash excluded from context | `!cmd` · `!!cmd` |
| Abort the active run | `Escape` |
| Hotkey help | `Ctrl+/` |

The full map lives in [info/README.md](info/README.md).

## Documentation

| Doc | Contents |
|---|---|
| [info/README.md](info/README.md) | Launch policy, keyboard map, architecture map |
| [AGENTS.md](AGENTS.md) | Conventions for contributors and coding agents |

## Development

```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
```

Dev builds use `opt-level = 1` so GPUI rendering stays fluid without a full release profile; the hot rendering crates compile at `opt-level = 3`.
