# Pideck

A native Windows project and terminal workspace, built with Rust and GPUI 0.2.2. Its neutral workbench follows the [Pideck reimagined handoff](design/pideck-reimagined/README.md).

Switch projects with the top project selector. The sidebar has **Projects**, **Files**, and **Git** tabs; the main pane combines editable files, terminals, and read-only diffs in one tab strip.

Each terminal starts in its project's directory. Run `codex`, `claude`, or another command after installing that CLI independently. Terminals continue running when their tab or project is hidden.

## Files and Git

Open files from the tree, edit them, and choose **Save** or press `Ctrl+S`. Changed tabs show a dirty marker. Closing changed files offers Save, Discard, or Cancel; closing a project or window offers **Save all and close**. Saves check for external changes and report conflicts while keeping your edits open.

The Git sidebar shows the branch and changed files, with read-only staged and working-tree diffs. Untracked text files also have a diff preview. Choose Unified or Split, move between changes with Alt+Up/Down, and use Open file to edit. It does not stage, commit, or modify Git history.

The editor accepts UTF-8 text files up to 2 MiB. PNG, JPEG, GIF, WebP, BMP, and TIFF files open in a read-only viewer (32 MiB or 50 megapixels). Each directory listing is capped at 2,000 entries; the sidebar reports truncation.

## Run from source

Use Windows with the stable Rust MSVC toolchain and Windows SDK/build tools. The declared Rust syntax floor is 1.88. Cargo.lock pins GPUI 0.2.2.

From the repository, launch with:

```powershell
cargo run --locked
```

Pideck embeds DM Sans interface text, Instrument Serif for the wordmark, and IBM Plex Mono for terminals, code, and counts. The four neutral themes follow the [final design handoff](design/pideck-reimagined/README.md). No font preparation step, Node.js, or Pi installation is required to build or launch the app. Tools you run inside its terminals have their own installation requirements.

A release build uses:

```powershell
cargo build --release --locked
```

## App updates

Windows copies installed with **PiDeck Setup** check GitHub Releases for updates on startup. Use **Check for updates** in the status bar’s **⋯** menu to check manually, then **Update and restart** to download and verify a new version.

Before restarting, Pideck asks about unsaved files and stopping running terminals, then saves the workspace layout. Cancelling keeps the app open; a downloaded update can be applied later with **Restart to update**. File or layout save failures must be resolved before the update can restart the app. Restarting creates fresh shells, not resumed commands or terminal history.

Source builds and unpackaged executables do not support in-app updates. Downloading an update does not silently apply it on the next launch; restarting to apply it remains an explicit action.

## Project state and restoration

While the app stays open, each project retains its sidebar selection, expanded directories and scroll position, open file buffers and cursor positions, active tab, and terminal sessions. Switching projects reuses those views and processes.

Across app restarts, project folders, terminal layout, selected terminal/project, sidebar visibility, chosen sidebar width, project checklists, and checklist visibility are restored. This data is stored in `terminal-workspace.json` beside the settings file, normally under `%APPDATA%\Pideck`. `PI_GUI_SETTINGS_PATH` relocates the settings file and adjacent layout file.

Restarting creates fresh shells. Open files, file buffers and cursor positions, expanded directories, terminal output, running processes, and CLI conversations are not restored from disk. Save files before closing; use each CLI's own session features when available.

If a saved layout cannot be read or restored, automatic saving pauses to preserve it. Repair the file and reopen the app, or choose **Replace saved layout** to save the current workspace instead. Closing without saving keeps the previous file.

Workspace settings stay local. Pideck adds no telemetry, analytics, or remote reporting; commands you run have their own network and privacy behavior.

Files, projects, and Git changes have independent filters. File search is bounded to 20,000 visited entries and 2,000 matches and never follows directory links; incomplete searches show a notice. Filtering keeps matching ancestors. Clearing Files or Git filters restores the previous expansion, selection, and scroll position.

Drag the sidebar divider between 256 and 420 logical pixels. Keyboard focus on the divider supports Left/Right to resize and Home to reset. The default is 288 pixels, 336 for Git, and 256 in compact windows. Project actions live in each project's row menu.

## Project checklist

Use the **checklist icon** in the toolbar or `Ctrl+Shift+L` to toggle the right inspector. It uses the headerless outline with numbered Instrument Serif section headings. Each project has its own reminders; committed edits and collapsed branches save locally with the workspace. New projects start with an empty Workspace section.

Click **Add a reminder**, type, and press Enter to save. The adjacent **+** creates a section. Reminder menus contain only **Add subtask**, **Rename**, and **Delete**; section menus contain **Rename** and **Delete**. Indenting and outdenting remain available from the keyboard. Completing a parent completes its subtree; parent checkboxes and counts follow their children. Deleting a subtree or section offers **Undo**, and `Ctrl+Z` restores recent edits while the checklist is focused. Removing a project also removes its saved checklist, as stated in the removal prompt.

With the checklist tree focused: arrows navigate and fold branches, Space checks, Enter adds a reminder, `Ctrl+Enter` adds a subtask, `Tab` / `Shift+Tab` indent or outdent, F2 renames, Delete removes, and `Ctrl+Shift+N` creates a section. Escape cancels text editing; `Ctrl+\`` returns to the terminal. Long labels show their full text in a tooltip. On compact windows, opening the inspector temporarily hides the left sidebar to reserve terminal space; the sidebar preference is preserved. Below 720 logical pixels the inspector is temporarily hidden as well.

## Terminal input

Drag to select text, double-click to select a word, or triple-click to select a line. Hold `Alt` while dragging for a rectangular selection. When a terminal application captures the mouse, hold `Shift` to select text or scroll the terminal history instead.

The terminal supports native Unicode/IME composition, bracketed paste, application mouse input, and scrollback. Copy uses the selected text, or the visible screen when nothing is selected.

## Keyboard

The explorer uses neutral outline icons with separate disclosure, filename, and status columns. Click to select a file; double-click or Enter to open. Ctrl-click toggles selection and Shift-click selects a range. Right-click for file operations, path copying, hidden files, and reveal in Windows Explorer. New file/folder naming stays inside the panel.

With explorer focus: `Ctrl+C/X/V` copies/cuts/pastes files, `Ctrl+A` selects visible entries, `Ctrl+D` duplicates, `F2` renames, `Delete` asks to move items to the Recycle Bin, `Ctrl+N` creates a file, and `Ctrl+Shift+N` creates a folder. Drag within the tree to move; hold Ctrl to copy. External drops and Windows file-clipboard pastes copy into the selected folder. Cut is internal to Pideck. Existing destinations are never intentionally overwritten; interrupted copies may leave completed or partial destination files, with the source retained. Directory links are not recursively copied. Cross-drive moves require copying first.

Filesystem changes refresh the explorer and Git status automatically; `F5` refreshes manually. The compact Git tree separates unstaged and staged changes. Click the branch name (or press `B` with Git-panel focus) to choose a local branch, then use Tab/Enter to select it. Switching preserves Git's conflict checks and never forces, stashes, or fetches. Save dirty editor buffers first. Git hooks are disabled for this operation.

| Action | Keys |
|---|---|
| Add project folder | `Ctrl+Shift+O` |
| New terminal | `Ctrl+Shift+T` |
| Close active tab | `Ctrl+Shift+W` |
| Next / previous tab | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Previous / next change (diff focus) | `Alt+Up` / `Alt+Down` |
| Unified / split diff (diff focus) | `Alt+U` / `Alt+S` |
| Refresh selected diff | `F5` |
| Save active file | `Ctrl+S` |
| Previous / next project | `Ctrl+Alt+Up` / `Ctrl+Alt+Down` |
| Toggle project sidebar | `Ctrl+Shift+B` |
| Toggle project checklist | `Ctrl+Shift+L` |
| Focus terminal | `` Ctrl+` `` |
| Focus project sidebar | `F6` |
| Copy terminal selection or visible screen | `Ctrl+Shift+C` |
| Select all terminal history | `Ctrl+Shift+A` |
| Paste into terminal | `Ctrl+Shift+V` |
| Scroll terminal history up / down | `Shift+PageUp` / `Shift+PageDown` |

## Appearance

Choose a theme from the appearance picker beside **New terminal**:

- **Black** — near-black surfaces.
- **Light** — neutral white surfaces.
- **Graphite** — dark gray surfaces.
- **Charcoal** — softer charcoal surfaces.
- **Flint** — warm dark, a step below Stone.
- **Stone** — warm, lifted dark (the default).

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
