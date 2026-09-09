# Pideck

A native Windows project and terminal workspace, built with Rust and GPUI 0.2.2. Its light editorial workbench follows the [final Paper references](design/paper/README.md).

Switch projects with the top project selector. The sidebar has **Projects**, **Files**, and **Git** tabs; the main pane combines editable files, terminals, and read-only diffs in one tab strip.

Each terminal starts in its project's directory. Run `codex`, `claude`, or another command after installing that CLI independently. Terminals continue running when their tab or project is hidden.

## Files and Git

Open files from the tree, edit them, and choose **Save** or press `Ctrl+S`. Changed tabs show a dirty marker. Closing changed files offers Save, Discard, or Cancel; closing a project or window offers **Save all and close**. Saves check for external changes and report conflicts while keeping your edits open.

The Git sidebar shows the branch and changed files, with read-only staged and working-tree diffs. Untracked files open in the editor. It does not stage, commit, or modify Git history.

The editor accepts UTF-8 text files up to 2 MiB. Each directory listing is capped at 2,000 entries; the sidebar reports truncation.

## Run from source

Use Windows with the stable Rust MSVC toolchain and Windows SDK/build tools. The declared Rust syntax floor is 1.88. Cargo.lock pins GPUI 0.2.2.

From the repository, launch with:

```powershell
cargo run --locked
```

Pideck embeds Geist interface text, Newsreader headings, and JetBrains Mono technical text. The paper, linen and evergreen palette follows the [final Paper design](design/paper/README.md). No font preparation step, Node.js, or Pi installation is required to build or launch the app. Tools you run inside its terminals have their own installation requirements.

A release build uses:

```powershell
cargo build --release --locked
```

## Project state and restoration

While the app stays open, each project retains its sidebar selection, expanded directories and scroll position, open file buffers and cursor positions, active tab, and terminal sessions. Switching projects reuses those views and processes.

Across app restarts, only project folders, terminal layout, selected terminal/project, and sidebar visibility are restored. This layout is stored in `terminal-workspace.json` beside the settings file, normally under `%APPDATA%\Pideck`. `PI_GUI_SETTINGS_PATH` relocates the settings file and adjacent layout file.

Restarting creates fresh shells. Open files, file buffers and cursor positions, expanded directories, terminal output, running processes, and CLI conversations are not restored from disk. Save files before closing; use each CLI's own session features when available.

If a saved layout cannot be read or restored, automatic saving pauses to preserve it. Repair the file and reopen the app, or choose **Replace saved layout** to save the current workspace instead. Closing without saving keeps the previous file.

Workspace settings stay local. Pideck adds no telemetry, analytics, or remote reporting; commands you run have their own network and privacy behavior.

## Terminal input

Drag to select text, double-click to select a word, or triple-click to select a line. Hold `Alt` while dragging for a rectangular selection. When a terminal application captures the mouse, hold `Shift` to select text or scroll the terminal history instead.

The terminal supports native Unicode/IME composition, bracketed paste, application mouse input, and scrollback. Copy uses the selected text, or the visible screen when nothing is selected.

## Keyboard

| Action | Keys |
|---|---|
| Add project folder | `Ctrl+Shift+O` |
| New terminal | `Ctrl+Shift+T` |
| Close active tab | `Ctrl+Shift+W` |
| Next / previous tab | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Save active file | `Ctrl+S` |
| Previous / next project | `Ctrl+Alt+Up` / `Ctrl+Alt+Down` |
| Toggle project sidebar | `Ctrl+Shift+B` |
| Focus terminal | `` Ctrl+` `` |
| Focus project sidebar | `F6` |
| Copy terminal selection or visible screen | `Ctrl+Shift+C` |
| Select all terminal history | `Ctrl+Shift+A` |
| Paste into terminal | `Ctrl+Shift+V` |
| Scroll terminal history up / down | `Shift+PageUp` / `Shift+PageDown` |

## Appearance

Choose **Paper**, **Linen**, **Graphite**, or **Midnight** from the appearance picker beside **New terminal**. Paper matches the supplied design; Linen is warm and light, Graphite is charcoal with sage accents, and Midnight uses a dark blue palette.

Theme changes apply to open files, diffs, and terminals without restarting shells or clearing their history. The preference stays local in `appearance.json` beside the workspace layout. A malformed preference is preserved until you explicitly choose a theme. The picker supports Tab, arrow keys, Enter, and Escape.

## Development

The app reuses [GPUI 0.2.2](https://docs.rs/crate/gpui/0.2.2/source/) for native rendering, [GPUI Component 0.5.1](https://docs.rs/crate/gpui-component/0.5.1/source/) for the editor, and [Alacritty Terminal 0.26.0](https://docs.rs/crate/alacritty_terminal/0.26.0/source/) with `portable-pty` for terminals. GPUI Component brings in Zed's [sum-tree 0.2.0](https://docs.rs/crate/zed-sum-tree/0.2.0/source/) transitively. These linked crates are Apache-2.0 licensed; no GPL-licensed Zed editor source was copied for this integration.

For code changes, finish implementation before running the relevant final checks:

```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
```

See [AGENTS.md](AGENTS.md) for contributor rules and [GPUI.md](GPUI.md) for the version-specific native UI reference.
