# Terminal pivot removal manifest

The user approved this exact cleanup, and all 96 listed deletions have been applied. The active application starts `TerminalManager` from `src/app.rs`; a read-only dependency scan found no references from its live module tree to the legacy modules listed below. The removed Windows process helper was referenced only by the obsolete `git_diff` service.

Automatic approval review initially rejected removal of the module declarations because it considered the public API and legacy test dependencies a potential compilation or behavior risk. The user then explicitly approved the listed cleanup. Module and dependency pruning is complete; Cargo lockfile refresh and final validation remain part of the parent task.

## Subsequent updater restoration

This manifest records the historical terminal-pivot cleanup, not the current module inventory. The app updater has since been restored in `src/services/app_update.rs` and integrated with `TerminalManager`, without restoring the legacy Pi runtime or update panel. Installed Windows builds check on startup and expose manual check/update controls in the footer. Downloading is separate from scheduling replacement: file-save decisions, terminal-close consent, and workspace persistence finish before Velopack is asked to apply and relaunch. See [App updates](../README.md#app-updates) for current behavior.

## Exact deletions

Removed only the following 96 tracked files. All 10 integration test and fixture files concern the removed Pi runtime; terminal engine, terminal worker, workspace, path, accessibility, and rendering tests remain in their live source modules.

### Legacy Rust source (64 files)

- `src/actions.rs`
- `src/attachments.rs`
- `src/command_catalog.rs`
- `src/controller.rs`
- `src/file_completion.rs`
- `src/model_runtime.rs`
- `src/orchestration.rs`
- `src/resource_center.rs`
- `src/services/app_update.rs`
- `src/services/draft_store.rs`
- `src/services/git_diff.rs`
- `src/services/path_actions.rs`
- `src/services/pi_process/diagnostics.rs`
- `src/services/pi_process/discovery.rs`
- `src/services/pi_process/gui_extensions.rs`
- `src/services/pi_process/mod.rs`
- `src/services/pi_process/platform.rs`
- `src/services/projects.rs`
- `src/services/rpc/client.rs`
- `src/services/rpc/codec.rs`
- `src/services/rpc/mod.rs`
- `src/services/rpc/protocol/command.rs`
- `src/services/rpc/protocol/common.rs`
- `src/services/rpc/protocol/event.rs`
- `src/services/rpc/protocol/extension.rs`
- `src/services/rpc/protocol/ids.rs`
- `src/services/rpc/protocol/mod.rs`
- `src/services/rpc/protocol/response.rs`
- `src/services/rpc/protocol/session.rs`
- `src/services/rpc/runtime_adapter.rs`
- `src/services/runtime_worker.rs`
- `src/services/sdk_bridge.rs`
- `src/services/session_catalog.rs`
- `src/state.rs`
- `src/state/drafts.rs`
- `src/state/editor.rs`
- `src/state/history.rs`
- `src/state/reducer.rs`
- `src/state/runtime.rs`
- `src/state/workspace_layout.rs`
- `src/views/composer/buffer.rs`
- `src/views/composer/element.rs`
- `src/views/composer/mod.rs`
- `src/views/composer/render.rs`
- `src/views/composer/tests.rs`
- `src/views/controls.rs`
- `src/views/conversation.rs`
- `src/views/conversation/list.rs`
- `src/views/conversation/scroll.rs`
- `src/views/diff_summary.rs`
- `src/views/markdown/mod.rs`
- `src/views/markdown/render.rs`
- `src/views/root.rs`
- `src/views/root/composer_bar.rs`
- `src/views/root/drafts.rs`
- `src/views/root/inspector.rs`
- `src/views/root/model_panels.rs`
- `src/views/root/overlays.rs`
- `src/views/root/render.rs`
- `src/views/root/shared.rs`
- `src/views/root/shell.rs`
- `src/views/time_labels.rs`
- `src/views/tool_card.rs`
- `src/views/tool_card/data.rs`

### Legacy Pi integration tests and fixtures (10 files)

- `tests/fixtures/fake_pi.rs`
- `tests/fixtures/pi_0_80_10_inbound.jsonl`
- `tests/fixtures/pi_0_82_inbound.jsonl`
- `tests/fixtures/stock_extension_ui_demo.ts`
- `tests/pi_process_supervisor.rs`
- `tests/rpc_client.rs`
- `tests/rpc_protocol.rs`
- `tests/runtime_controller.rs`
- `tests/runtime_reducer.rs`
- `tests/sdk_bridge.rs`

### Legacy Pi bridge and its tests (22 files)

- `bridge/README.md`
- `bridge/jsonl-writer.test.mjs`
- `bridge/jsonl.mjs`
- `bridge/jsonl.test.mjs`
- `bridge/orchestration-adapter.mjs`
- `bridge/orchestration-adapter.test.mjs`
- `bridge/orchestration-core.mjs`
- `bridge/orchestration-core.test.mjs`
- `bridge/pi-bridge-lifecycle.test.mjs`
- `bridge/pi-bridge-model-settings.test.mjs`
- `bridge/pi-bridge-resources.test.mjs`
- `bridge/pi-bridge.mjs`
- `bridge/pi-bridge.test.mjs`
- `bridge/pi-contract.mjs`
- `bridge/pi-contract.test.mjs`
- `bridge/pi-settings.mjs`
- `bridge/pi-settings.test.mjs`
- `bridge/protocol.schema.json`
- `bridge/protocol.ts`
- `bridge/resource-index.mjs`
- `bridge/resource-index.test.mjs`
- `bridge/test-support/harness.mjs`

## Retained module declarations

`src/lib.rs` retains:

```rust
pub mod app;
mod assets;
mod fonts;
mod services;
mod theme;
mod views;
```

`src/views/mod.rs` retains:

```rust
mod terminal;
pub(crate) mod terminal_manager;
```

`src/services/mod.rs` retains:

```rust
pub(crate) mod accessibility;
pub mod atomic_file;
pub(crate) mod paths;
pub mod terminal;
pub(crate) mod terminal_engine;
pub(crate) mod terminal_workspace;
```

Removed the `RootView` re-export and the unused `suppress_console_window` helper. The terminal worker uses `portable-pty` and did not call that helper. Only the application entry point remains public at the crate root; implementation modules are private.

The extracted `src/services/paths.rs` retains `without_windows_verbatim_prefix` and its ordinary-path, Windows drive-path, and UNC-path tests. The terminal manager and workspace now import that helper instead of the legacy session catalog.

Retain the live terminal submodules (`src/views/terminal/input.rs` and `src/views/terminal/element.rs`), `src/views/terminal_manager.rs`, `src/theme/terminal_manager.rs`, `src/services/terminal_engine.rs`, `src/services/terminal_workspace.rs`, and `src/services/paths.rs`, including files not yet tracked by Git.

## Cargo dependency cleanup

Removed these unused direct dependencies from `Cargo.toml`:

- `base64`
- `image`
- `pulldown-cmark`
- `unicode-segmentation`
- `unicode-width`

Changed the `serde` feature list from `["derive", "rc"]` to `["derive"]`. Removed the `[profile.dev.package."pulldown-cmark"]` and `[profile.dev.package.image]` overrides and updated their now-obsolete profile comment.

Keep `alacritty_terminal`, `async-channel`, `gpui`, `portable-pty`, `serde`, `serde_json`, and `velopack`. Keep the `embed-resource` build dependency and GPUI test support. The executable still calls the Velopack bootstrap, so deleting its old update-panel service does not authorize removal of that packaging dependency.

After deleting the old Windows-specific process/filesystem services and helper, retained the direct `windows-sys` `Win32_UI_WindowsAndMessaging` feature for the existing accessibility preference query. Removed the unused direct features `Win32_Foundation`, `Win32_Security`, `Win32_System_JobObjects`, `Win32_System_Pipes`, `Win32_System_Threading`, and `Win32_UI_Shell`; transitive crates retain their own required Windows features.

Refresh `Cargo.lock` through Cargo dependency resolution after changing the manifest. A dependency may legitimately remain in the lockfile as a transitive dependency.

## Retained material and scope

This manifest does not delete design references, reference images, embedded fonts or icons, app resources, packaging configuration, `AGENTS.md`, `GPUI.md`, `README.md`, local settings, saved projects, terminal layouts, shell history, or other user data. Parent-task work on scripts, CI, and documentation is separate from this exact source-removal manifest.

## Final validation

After implementation is complete, inspect the focused diff and run `cargo fmt --all -- --check`, `cargo check --all-targets`, and `cargo test --all-targets`. Confirm the real PTY integration tests receive rendered command output through `TerminalEngine` and route its protocol replies back to the corresponding PTY. Do not restore the removed fake cursor-query responder to make tests pass.
