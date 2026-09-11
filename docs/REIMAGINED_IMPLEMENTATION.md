# Pideck reimagined implementation

Implemented from all 14 exports and the editable Pen master linked in
[the handoff](../design/pideck-reimagined/README.md).

## Workbench

- Black, Light, Graphite, and Charcoal use the master's exact neutral and semantic colors. Existing theme preferences migrate to the corresponding neutral appearance.
- Instrument Serif wordmark, DM Sans controls, IBM Plex Mono code and terminal text.
- 40 px titlebar, 52 px workspace toolbar, 42 px tabs, 28 px status bar; 288 px default sidebar, 336 px Git sidebar, and 256 px compact sidebar.
- The divider supports dragging and keyboard resizing, with its chosen width persisted locally.
- Files, Projects, and Git navigation sits at the top. Project actions and update commands use their respective menus.

## Interactions

Files and Git filters preserve ancestor context and restore the previous browsing state when cleared. Directory work stays on background workers and rejects stale generations. Failed creation and rename retain the entered name and focus. Refresh preserves populated listings and shows folder-local Retry controls.

Git uses a nested, virtualized tree with section and descendant counts, separate branch/filter rows, fixed status/statistic columns, collapse-all, and keyboard navigation. Collapsing an ancestor moves selection to its visible folder without closing the diff. Unified and split views include file-specific totals, previous/next changes, copy, refresh, and Open file. Loading immediately clears a different file's old diff. Late results do not reactivate a tab the user left or reopen a closed review.

Terminal sessions, editor buffers, file-operation checks, branch-switch checks, and save-before-close flows remain owned by the existing project views and services.

## Windows font resolution

The downloaded static DM Sans faces contain `DM Sans 9pt` in the SFNT name table, but Windows DirectWrite registers them as `DM Sans 9pt 14pt` after applying the STAT optical-size naming. The Windows UI must request that registered family. Merely inspecting `resolve_font`'s returned descriptor does not prove that the correct face was used: an unknown family can retain the requested descriptor while shaping Segoe UI.

A native DirectWrite probe confirmed the registered family and measured the 12 px sample `Files Projects New terminal`: DM Sans widths are 150.120, 153.072, and 156.756 px at weights 400, 500, and 600. The incorrect family and Segoe UI both measured 142.189 px at weight 400. Startup validation now checks the registered collection itself. Static font provenance, hashes, and the Windows family name are recorded in `design/fonts/sources.json`.

## Validation boundary

Final checks: `cargo fmt --all -- --check`, `cargo check --all-targets`, and `cargo build --bin pi-gui` passed. `cargo test --all-targets` passed with 103 tests, no failures, and one ignored manual benchmark. The existing dependency warning for `proc-macro-error2` remains. Clippy and rustdoc were not run; this change does not require those broader checks.

GPUI's headless tests use `NoopTextSystem`; they validate state and layout logic, not native font rendering. The separate native font probe validates DirectWrite family resolution and shaping.

Computer Use was stopped by the user before the final visual walkthrough. The final corrected font build has not received a complete native comparison across all four themes, Git review, and 125/150/200% scaling. Static references contain synthetic terminal output; the application renders real shell output.

Changed source areas: `src/fonts.rs`, `src/assets.rs`, `src/theme.rs`, `src/theme/terminal_manager.rs`, `src/views/terminal_manager.rs`, `src/views/project_panels.rs`, `src/views/project_panels/file_actions.rs`, `src/views/terminal.rs`, `src/views/terminal/diff.rs`, and the appearance, project-files, project-Git, and terminal-workspace services. Added static font faces and neutral icons; updated their provenance and the README. The supplied design exports and editable master were preserved.
