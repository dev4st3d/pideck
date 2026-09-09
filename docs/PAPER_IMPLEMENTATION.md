# Paper redesign implementation

The native workbench now follows the user's final Paper references in `design/paper/`. The shared header, compact tabs, Explorer, Git sidebar, status bar and side-by-side diff use the authored geometry and bundled Geist, Newsreader and JetBrains Mono fonts. Terminal content and Git counts remain real data.

The appearance picker adds Paper, Linen, Graphite and Midnight. Preferences are saved atomically in a separate local `appearance.json`; repeated choices are serialized. Theme changes preserve open terminal sessions, history, selection, modes and file buffers. Corrupt preferences are preserved until a user chooses a replacement theme.

## Changed areas

| Area | Paths |
| --- | --- |
| Window and shared layout | `src/app.rs`, `src/views/terminal_manager.rs`, `src/theme/terminal_manager.rs` |
| Palettes and persistence | `src/theme.rs`, `src/services/appearance.rs`, `src/services/mod.rs` |
| Fonts and icons | `src/fonts.rs`, `src/assets.rs`, `design/fonts/` |
| Files and Git sidebars | `src/views/project_panels.rs`, `src/services/project_git.rs` |
| Terminal and diff presentation | `src/views/terminal.rs`, `src/views/terminal/element.rs`, `src/views/terminal/diff.rs` |
| Editor theme and save notifications | `src/views/file_editor.rs` |
| Reference and handoff | `design/paper/`, `README.md`, `docs/STYLE_MAPPING.md`, this document |

Existing unrelated work was preserved. No commits, pushes, package changes or dependency additions were made by this redesign.

## Final validation

- `cargo fmt --all -- --check`: passed.
- `cargo check --all-targets`: passed.
- `cargo test --all-targets`: 77 passed, 0 failed, 1 ignored manual benchmark.
- `cargo rustc --bin pi-gui -- -o target/debug/pideck-design.exe`: passed; the separate output avoids replacing the user's running executable.
- Focused whitespace review: passed. Font source hashes were verified.

The tests cover palette contrast, preference corruption/round trips, repeated theme choices, terminal state preservation, explicit RGB/extended color preservation, save notifications, real ConPTY input/output, Git paths/statistics, configured Git patch formatting, aligned diffs, conflicts, Unicode and EOF newline changes.

Clippy and documentation builds were not run. The existing manual performance benchmark stayed ignored. Cargo reports an upstream future-compatibility warning for `proc-macro-error2 2.0.1`; the custom executable output also produces expected `-o`/output-directory warnings. Neither blocked the build.

## Native inspection boundary

An isolated Paper preview was opened with a synthetic project and separate session settings. Its shared window appearance was inspected, and the initially oversized window was corrected to fit the display. The user stopped Computer Use with Escape before the Files/Git interaction and final theme/resize checks could be completed. No further computer interaction was performed. Full pixel-for-pixel fidelity and the final themes have therefore not been visually certified.

`target/debug/pideck-design.exe` is the final build including themes. The earlier `pideck-paper.exe` preview does not include them. Generated executables, logs and the synthetic project remain in ignored `target/` and `temp/` directories.
