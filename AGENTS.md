# AGENTS.md

Repository-wide instructions for `pi-gui`; they apply to this directory and its descendants. A deeper `AGENTS.md` applies only to its subtree and overrides this file there. User instructions take precedence.

## Priorities

1. Follow the user's request.
2. Preserve user work and protect privacy.
3. Prefer polished UX, then correctness, simplicity, and maintainability.

Ask before irreversible, security-sensitive, product-defining, or major scope-expanding work. Do not ask about facts the repository already answers. Stop when the requested work is complete.

## Project map

- Rust 2024; minimum Rust 1.85; stable toolchain.
- GPUI 0.2.2; Windows-first; primary font is Segoe UI.
- `src/main.rs`: entry point; `src/app.rs`: app/window setup.
- `src/lib.rs`: crate surface; `src/state.rs`: UI-independent state.
- `src/theme.rs`: shared visual tokens; `src/views/`: screens and large UI regions.
- `GPUI.md` is authoritative for version-sensitive GPUI behavior. Prefer its guidance and code that compiles here over newer examples.

## Working rules

- Read only relevant files and targeted call sites before changing shared/public types or behavior. Preserve unrelated edits.
- Do not broaden scope, perform opportunistic cleanup, or rewrite completed work.
- Keep domain logic in plain Rust. Views render state and dispatch operations; use `Render` for stateful views and `RenderOnce` for value-like components. Use services for real filesystem, network, process, or persistence boundaries.
- Keep platform/window setup in `app.rs`; keep `main.rs` minimal.
- Use `Entity<T>` only for shared state that needs GPUI observation. Keep temporary interaction state in its owning view; never wrap GPUI entities in `Rc<RefCell<_>>`.
- Keep I/O, long work, and side effects out of `render`; move meaningful work off the event loop, handle cancellation and stale results, tolerate closed views, and preserve the last valid UI state when practical.
- Start concrete. Add traits, generics, macros, or helpers only for justified repetition or a real boundary. Default visibility to private, then `pub(crate)`, then `pub`.
- Follow existing Rust patterns. `rustfmt` is authoritative; prefer clear control flow, early returns, `let ... else`, and explicit matches. Avoid needless allocation, hidden mutation, broad wildcard matches, and locks or borrows held across `.await`.
- Comments explain intent, invariants, trade-offs, or version-sensitive behavior—not syntax. Update docs when behavior, setup, architecture, or public contracts change. Keep text UTF-8 with LF endings, one final newline, and no trailing whitespace.
- Do not introduce TODOs, FIXMEs, stubs, placeholders, or dead alternatives in changed code.

## Reliability and privacy

- Use `Result` with actionable context at boundaries. Recoverable user-driven failures must not panic; avoid unexplained `unwrap()` or `expect()` in those paths.
- Log diagnostic detail once and show concise recovery-oriented UI copy. Never expose secrets, tokens, stack traces, raw implementation errors, or unnecessary personal data.
- The app is private by default: no telemetry, analytics, tracking, or remote reporting without explicit approval. Use synthetic fixtures.
- Measure before optimizing. Do not add caches, concurrency, custom rendering, or unsafe code on a hunch. Unsafe code needs a documented invariant, a small isolated boundary, and a safe wrapper.
- Add dependencies only when they materially improve the solution; use existing std/GPUI/crates first and update `Cargo.lock` when resolution changes.

## UX and accessibility

For affected screens and controls, cover applicable loading, empty, error, overflow/resize, repeated or interrupted actions, keyboard, scaling, reduced-motion, and destructive states.

Use shared semantic tokens for recurring color, spacing, typography, radius, elevation, and motion. Every primary action needs a keyboard path and visible logical focus. Do not make meaning depend only on color, hover, position, or an unlabeled icon. Use concise, active, non-blaming copy.

## Validation

Do not run `cargo fmt`, `cargo check`, `cargo test`, `clippy`, or `doc` while exploring or between edits. Finish implementation first, then validate once in the final phase. For documentation or inert metadata only, skip cargo validation and inspect the diff. Do not rerun checks merely to reconfirm unchanged code.

For code changes, run the smallest relevant final checks: `cargo fmt --all -- --check` for changed Rust, `cargo check --all-targets` for code or build changes, and `cargo test <target-or-filter>` for changed behavior. Broaden to `cargo test --all-targets` for shared or cross-module changes; use `cargo clippy --all-targets --all-features -- -D warnings` or `cargo doc --no-deps` only when warranted. Add deterministic tests for meaningful behavior or state transitions. If a final check fails, fix the cause and rerun only the check needed to verify that fix. Report commands run, skipped checks, and pre-existing failures.

## Git and handoff

- Do not commit, push, rewrite history, or discard work unless explicitly asked. Edit source inputs, not generated outputs; never add `target/`, PDBs, caches, local logs, or editor state. Never use destructive Git commands to make checks pass.
- Commit subjects follow this repository's existing Conventional Commits style: `<type>: <imperative summary>`; use specific types such as `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, or `chore`. Keep the subject short and do not invent another format.
- Before handoff, review the focused diff, preserve unrelated changes, and report touched paths, validation or skips, and real blockers.
