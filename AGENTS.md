# AGENTS.md
Rules for coding agents contributing to `pi-gui`.

## Priorities
When guidance conflicts:
1. The user's current request.
2. Safety, privacy, and preservation of user work.
3. This file.
4. `GPUI.md` and its GPUI 0.2.2 constraints.
5. Existing repository conventions.

Optimize for polished UX, then correctness, simplicity, and maintainability.
Ask before irreversible, security-sensitive, product-defining, or major scope-expanding decisions.
Do not ask about trivial details the repository already answers.
Finish the requested work and stop. Do not continue exploring, polishing, or re-editing after the request is satisfied.

## Project facts
* Rust 2024, minimum Rust 1.85
* Stable toolchain with rustfmt and Clippy
* GPUI 0.2.2
* Windows-first
* Primary font: Segoe UI

Key files:
* `src/main.rs`: minimal entry point
* `src/lib.rs`: modules and crate surface
* `src/app.rs`: app/window bootstrap
* `src/state.rs`: UI-independent state
* `src/theme.rs`: shared visual tokens
* `src/views/`: screens and large UI regions
* `GPUI.md`: authoritative project-specific GPUI guidance

GPUI is version-sensitive. Trust GPUI 0.2.2 rustdoc and code that compiles here. Verify newer Zed or GPUI examples before using them.

## Working rules
* Begin with the user's request and the smallest set of relevant code. Load only what is required for the change.
* Make the smallest coherent change that fully solves the request. When the request is done, stop. Do not re-inspect, re-edit, or expand scope.
* Preserve unrelated user work.
* Before changing shared types, actions, tokens, public items, or behavior, search the necessary call sites. Prefer targeted searches over exhaustive repo-wide scans when impact is clearly local.
* Read a complete file only when it defines a contract you must honor. Prefer reading the relevant sections; do not load large files or unrelated modules “just in case.”
* Plan only multi-file, architectural, risky, or ambiguous work. Implement straightforward local work directly and finish it in one pass.
* Avoid unrelated cleanup, broad renames, formatting sweeps, speculative abstractions, opportunistic upgrades, and any work that does not directly serve the current request.
* Do not leave `TODO`, `FIXME`, stubs, placeholders, dead alternatives, or hidden debt unless explicitly approved.
* Nearby cleanup is allowed only when it directly supports the requested change and remains easy to review.
* Do not reopen or rewrite completed work unless a real failure appears. Validate once, then hand off.

## Architecture and Rust
Keep domain logic independent of GPUI.

```text
views/components -> theme + actions + domain
views -> GPUI entities + services
services -> domain
domain -> std + domain crates
theme/domain -X-> concrete views
```

* Keep platform/window setup in `app.rs`; keep `main.rs` minimal.
* Use plain Rust types for domain state and testable behavior.
* Use `Entity<T>` only for shared state that needs GPUI observation.
* Keep temporary interaction state inside the owning view.
* Views render state and dispatch operations; substantial domain logic belongs elsewhere.
* Use `Render` for stateful views and `RenderOnce` for reusable value-like components.
* Keep I/O, long work, and side effects out of `render`.
* Introduce services only for real filesystem, network, process, or persistence boundaries.
* Never wrap GPUI entities in another ownership system such as `Rc<RefCell<_>>`.

Start concrete. Add traits, generics, macros, helpers, or reusable components only when repetition or a real boundary justifies them. Default visibility to private, then `pub(crate)`, then `pub`.

rustfmt is authoritative. Prefer clear control flow, early returns, and `let ... else`. Avoid clever APIs, needless allocation, hidden mutations, broad wildcard matches, and holding locks or borrows across `.await`. Remove unused imports instead of suppressing warnings.

Comments explain intent, invariants, tradeoffs, safety, or version-sensitive behavior—not syntax. Update documentation when behavior, setup, architecture, or public contracts change.

Text files: UTF-8, LF endings, one final newline, no trailing whitespace.

## Errors, async, privacy, and performance
Recoverable failures must not panic.
* Use `Result` and actionable errors.
* Add context at boundaries without repeating messages at every layer.
* Log diagnostic detail once; show concise recovery-oriented UI copy.
* Avoid `unwrap()` and unexplained `expect()` in production user-driven paths.
* Never expose secrets, stack traces, or raw implementation errors.
* Preserve the last valid UI state after failure when practical.

Never block the GPUI event loop with meaningful I/O, waiting, or expensive computation. Move long work off-thread, make cancellation clear, prevent stale results from overwriting newer state, use weak handles when appropriate, and handle views closing while work remains pending.

The app is private by default. No telemetry, analytics, tracking, or remote reporting without explicit approval. Never log or commit credentials, tokens, private content, or unnecessary personal data. Use synthetic fixtures.

Measure before optimizing. Do not add caches, concurrency, custom rendering, or unsafe code on a hunch. Unsafe Rust requires a demonstrated need, a small isolated boundary, documented invariants, a safe wrapper, and focused tests.

Dependencies may be added when they materially improve the solution. First check std, GPUI, and existing crates. Keep changes narrow and update `Cargo.lock` when resolution changes.

## UX and accessibility
UI work is incomplete if it only supports the ideal pointer path.

For relevant screens and controls, consider interaction states, loading/empty/error states, long text, overflow, resizing, repeated actions, interrupted work, keyboard navigation, scaling, reduced motion, and destructive-action recovery.

Use shared semantic tokens for recurring color, spacing, typography, radius, elevation, and motion. Do not scatter unexplained literals.

Every primary action needs a keyboard path. Focus must be visible and logically ordered. Meaning must not depend only on color, hover, position, or an unlabeled icon. UI copy should be concise, active, specific, and non-blaming.

## Validation
Validation is risk-based. Run only checks that provide useful evidence, and report only checks actually run.

**Tier 0 — documentation or inert metadata:** no execution when commands add no confidence.

**Tier 1 — small, low-risk changes:** format changed Rust, run the narrowest useful test if one exists, and inspect the diff.

**Tier 2 — meaningful logic, API, dependency, module, or GPUI changes:**
```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo test <target-or-filter>
```

**Tier 3 — major features, architectural changes, unsafe code, releases, or unresolved uncertainty:**
```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
```

Add deterministic tests when they meaningfully protect domain logic, regressions, parsing, validation, persistence, state transitions, error mapping, boundaries, cancellation, stale results, keyboard actions, or important conditional UI behavior.

Do not introduce warnings. Fix warnings caused by the change and small warnings in touched code without turning the task into repository-wide cleanup.

## Git and handoff
Git inspection is allowed. Without explicit instruction, do not commit, push, branch, checkout, rebase, merge, reset, revert, stash, or discard work.

Before destructive actions, confirm ownership and scope. Prefer reversible approaches. Never use destructive Git commands to make checks pass.

Edit source inputs instead of generated outputs. Do not add `target/`, PDBs, caches, temporary files, local logs, or editor state.

Before handoff, confirm the outcome is complete, the diff is focused, unrelated work is preserved, relevant UX and failure states are covered, expensive work does not block the UI, recoverable failures do not panic, documentation/tests are updated where useful, the appropriate validation tier passed or was explicitly omitted, and the final diff was reviewed.

Keep the final response result-focused: outcome first, checks in one concise line, and only real blockers or risks.
