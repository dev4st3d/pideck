# Pideck: final Paper reference

These are the user's approved screenshots from the Paper design session. They supersede the older Graphite conversation reference for the native project workspace.

- `01-shell.png`: shared header and footer. The middle of this artboard was unfinished; it is not a proposed empty/loading state.
- `02-terminal.png`: Explorer, mixed document tabs, real terminal surface, bottom navigation and shared status bar.
- `03-git-diff.png`: Git sidebar and a read-only side-by-side diff.

The original Paper artboards are 1440 × 900 logical pixels. The supplied screenshots are scaled exports; do not treat their physical dimensions as native layout units. The geometry below comes from the authored Paper styles and the final corrections in the design session. Paper exports were unavailable during implementation because the weekly quota was exhausted.

| Region | Logical pixels |
| --- | --- |
| Native title bar | 30 high, 16 horizontal inset |
| Toolbar | 52 high |
| Sidebar and brand region | 256 wide, 16 content inset |
| Main toolbar/content inset | 24 |
| Mixed tab strip | 40 high |
| Sidebar navigation | 42 high |
| Shared status bar | 30 high |
| File rows | 28 high |
| Git rows | 40 high with 8 separation |
| Terminal text | 13 size, 23 line height |
| Diff text | 12 size, 24 line height |

Typography: Newsreader Medium for the 28px wordmark and 36px Git title; Geist regular/medium for controls; JetBrains Mono for code and technical labels. The bundled static Newsreader optical family is named `Newsreader 16pt`. Source URLs, licenses and hashes are recorded in `../fonts/sources.json`.

Palette: paper `#F6F5F0`, linen `#ECEBE4`, ink `#242824`, muted text `#646A62`, rules `#D5D8CF`, evergreen `#315C49`, selected rows `#DBE3D8`, additions `#E1EBDC`, deletions `#F0E3DC`, and empty diff cells `#EEEFE8`.

The terminal has no decorative title, repeated path strip or project-context block. The wordmark, sidebar labels, project selector, tabs and content use shared alignment guides. The status bar shows actual branch, changed-file count, project and open-terminal count. Reference file names, command output and change counts are synthetic examples; production content remains live.

At narrow window sizes, hide the repeated project path and keyboard hint before compressing primary controls. Tabs scroll horizontally and file names truncate with full-path tooltips. Files, menus, loading/error states and save-conflict recovery retain existing behavior using the same palette and typography.

The subsequently requested appearance picker adds Linen, Graphite and Midnight without changing the reference geometry. Paper stays the default. Preference writes are serialized and atomic; changing appearance preserves terminal state and existing editor buffers. Explicit terminal RGB colors and extended indexed colors are retained while the base ANSI palette adapts for readability.
