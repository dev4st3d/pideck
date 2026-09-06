# Pideck

A native Windows workspace for the Pi coding agent, built with Rust and GPUI 0.2.2. Pi owns agent execution, sessions, provider credentials and extension semantics.

## Workspace

The Workbench layout keeps projects and sessions on the left, conversation and input in the center, and a dismissible inspector on the right. Navigation yields to the conversation when the window is narrow. History and the inspector do not consume the center at the same time.

Graphite and Paper are the two supported appearances. Both use a shared type scale, restrained semantic accents, readable status text, a 960-logical-pixel reading measure and visible keyboard focus. Existing theme preferences map to the corresponding dark or light appearance; system-installed fonts remain configurable.

- Multiline composer with grapheme navigation, IME, clipboard, undo/redo, file/image attachments, `@` files and `/` commands.
- Separate Send, Steer, Queue and Stop behavior. Input stays editable while acceptance is pending; a late acknowledgement cannot clear a newer revision or newly changed attachments.
- Session-owned local draft checkpoints retain text, selection, undo/redo, attachments, saved inputs and transcript position across normal application closure. Recovery never sends a prompt automatically.
- Streaming Markdown, selectable transcript text, tool details, image previews and read-only Git changes/diffs.
- Searchable model selection, thinking controls, provider authentication, command palette, settings, history, resources and task/subagent/goal inspector integrations.
- Embedded multi-tab PTY terminal and a bounded pool of supervised background session runtimes.
- No telemetry, analytics or remote reporting. Authentication fields are not draft-checkpointed.

## Requirements

| Component | Requirement |
|---|---|
| OS | Windows; native application validation remains outstanding |
| Rust | Stable MSVC toolchain, Windows SDK/build tools, rustfmt and Clippy; declared syntax floor 1.88 |
| GPUI | 0.2.2 in the included Cargo.lock |
| Pi | `@earendil-works/pi-coding-agent@0.85.1` |
| Node | Stable 22.19.0 or newer |

There is no application `package.json` or frontend npm build. Node is used for Pi, its SDK sidecar, bridge tests and the optional source checks/benchmark. Dependencies and runtimes are not included in this source ZIP.

## Run from source

Install the prerequisites, then run from the extracted repository:

```powershell
npm install -g @earendil-works/pi-coding-agent@0.85.1
cargo run --locked
```

A release-mode source build uses:

```powershell
cargo build --release --locked
```

The launch directory joins the project sidebar. Existing Pi sessions and credentials remain Pi-owned. Open a project, select or create a session, then use **Settings → Providers** to authenticate and **Models** to choose an available model.

The supplied source retains the existing executable discovery, strict version checks, rejected-project-trust launch policy, queue cancellation and process supervision. It does not install extensions automatically or modify a user's project to demonstrate a feature.

## Local drafts and recovery

Drafts are stored beside the app settings file, normally under `%APPDATA%\Pideck\drafts-v1`. `PI_GUI_SETTINGS_PATH` changes the settings location and therefore the adjacent drafts directory. These are app-owned checkpoints, not Pi session files.

The contents are **local plaintext**, including draft text, undo history, attachment snapshots/base64 image data and source paths. Protect this directory like your Pi session directory. No credential input fields are saved by this mechanism. No file in a user project is created for draft storage.

Every two seconds, changed session drafts are offered to a bounded background writer; unchanged drafts are not reserialized. Normal close waits asynchronously for queued writes and checks for intervening edits. Failures keep the window open with recovery feedback. A forced process/OS termination can lose changes after the last completed checkpoint; this is not a crash-proof or encrypted vault.

An unreadable, mismatched-owner or unsupported-version checkpoint is retained rather than overwritten. The feedback identifies a corrupt checkpoint when available. Back it up and repair or move that file, then select **Retry storage**. Never delete a Pi session as part of local draft recovery. Restored uncertain submissions require inspecting the conversation before deciding whether to send again.

While an attachment picker/read is active, session switching is temporarily blocked so the result cannot land in a different session. Cleared Pi queue messages remain available under **Restore next**, which appends instead of replacing a current draft.

## Keyboard essentials

| Action | Keys |
|---|---|
| Command palette | `Ctrl+Shift+P` |
| Project navigation / inspector | `Ctrl+B` / `Ctrl+I` |
| Connect / Retry / Stop | `Ctrl+Alt+C` / `Ctrl+Alt+R` / `Ctrl+Alt+S` |
| Workspace terminal | `` Ctrl+` `` |
| Attach files | `Ctrl+O`, or drag onto the composer |
| Send or steer / newline / queue | `Enter` / `Shift+Enter` / `Alt+Enter` |
| Restore a saved input | `Ctrl+Shift+R` |
| Direct Bash / Bash excluded from context | `!command` / `!!command` |
| Abort current run | `Escape` |
| Hotkey help | `Ctrl+/` |

The full keyboard and launch-policy map is in [info/README.md](info/README.md).

## Extension integrations

Supported Pi extension UI requests use native dialogs, status lines, widgets, title updates and palette commands. Custom TUI components remain unsupported rather than appearing as dead native controls.

The snapshot's reference versions are retained, **not live-certified in this delivery**:

| Extension | Snapshot reference | Integration |
|---|---:|---|
| `@tintinweb/pi-tasks` | 0.7.2 | Dependencies, blockers, outputs, guarded execute/stop |
| `@tintinweb/pi-subagents` | 0.15.2 | Lifecycle, queue, concurrency, schedules, worktrees, memory, steer/stop/resume |
| `@narumitw/pi-goal` | 0.51.0 | Objective, limits, budget, queue and guarded goal actions |
| `@juicesharp/rpiv-ask-user-question` | 2.5.1 | Native multi-question, choice, note and multi-select flows |

`pi-bar` 0.3.39 is the snapshot reference for GUI-session exclusion; TUI packages remain installed. Public SDK compatibility checks and synthetic fixtures are not a substitute for exercising those installed extensions.

## Validation

On a configured Windows machine:

```powershell
.\scripts\validate.ps1
```

The script checks source/asset contracts, formatting, all Rust targets/tests, bridge tests and Clippy, using the included lockfile. It does not install dependencies or publish anything.

Bridge tests and isolated catalog benchmark can also run directly:

```powershell
node --test (Get-ChildItem -Path bridge -Filter *.test.mjs).FullName
node scripts/verify-source.mjs
node scripts/bench-resource-index.mjs
```

The benchmark excludes Pi startup, filesystem loading, networking and native rendering.

## Documentation

- [info/README.md](info/README.md): runtime policy and architecture map.
- [bridge/README.md](bridge/README.md): public SDK bridge, IPC, trust and resource indexing.
- [AGENTS.md](AGENTS.md): contributor instructions. Its referenced `GPUI.md` was absent from the supplied snapshot; this delivery does not claim otherwise.
