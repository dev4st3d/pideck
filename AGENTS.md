# AGENTS.md

Working agreement for coding agents contributing to `pi-gui`.

## 1. Intent and authority

This is a small native desktop application built with Rust and GPUI. Optimize first for a polished, coherent user experience, then for correctness, simplicity, and maintainability.

Use informed judgment rather than blindly applying process. These are strong defaults, not a substitute for understanding the task. When guidance conflicts, use this order:

1. The user's current explicit request.
2. Safety, privacy, and preservation of user work.
3. This file.
4. `GPUI.md`, especially its pinned GPUI 0.2.2 constraints.
5. Existing repository conventions.

Ask often when requirements are ambiguous, especially before product-defining, irreversible, security-sensitive, or substantially scope-expanding choices. Do not ask about trivial implementation details when repository evidence makes the answer clear.

## 2. Project facts

- Crate: `pi-gui`
- Rust edition: 2024; minimum Rust: 1.85
- Toolchain: stable with rustfmt and Clippy
- UI framework: GPUI 0.2.2
- Primary desktop font: Segoe UI
- `src/main.rs`: minimal entry point
- `src/lib.rs`: module declarations and crate surface
- `src/app.rs`: GPUI/window bootstrap
- `src/state.rs`: UI-independent state
- `src/theme.rs`: shared visual tokens
- `src/views/`: entity-backed screens and large UI regions
- `GPUI.md`: detailed, source-backed engineering and interface guide

GPUI is pre-1.0 and version-sensitive. Treat GPUI 0.2.2 rustdoc and code that compiles here as authoritative. Never copy newer Zed or GPUI examples without checking the pinned API.

Choose platform scope feature by feature. The repository is currently Windows-first, but avoid needless lock-in when portability is cheap. Ask when platform support materially changes UX, dependencies, or implementation scope.

## 3. Working style and scope

- Inspect relevant code and guidance before editing.
- Keep changes compact, clean, and surgically focused.
- Prefer the smallest coherent diff that fully solves the request.
- Preserve unrelated user changes; never overwrite work you did not create.
- Search all call sites before changing a shared type, action, token, public item, or behavior.
- Read full files that establish a contract; do not infer architecture from snippets.
- Use free judgment, but make consequential tradeoffs intentional.
- Improve existing behavior freely when the improvement clearly supports the requested result and polished UX.
- Ask before materially changing product semantics, persisted data, public APIs, or task scope.
- Do not perform broad cleanup merely because a file is open.
- Plan first for multi-file, architectural, risky, or ambiguous work. Implement straightforward surgical work directly.

Nearby cleanup is in scope only when it materially clarifies or protects the requested change, remains locally reviewable, and does not hide behavior in churn. Avoid unrelated renames, formatting sweeps, speculative infrastructure, opportunistic upgrades, and personal-style rewrites.

Do not leave `TODO`, `FIXME`, stubs, placeholders, or commented-out alternatives unless the user explicitly approves the debt and its reason is documented.

## 4. Architecture

Keep domain state and behavior independent of GPUI. Framework types belong at application/view boundaries, not deep in business logic.

```text
views/components -> theme + actions + domain
views            -> GPUI entities + services
services         -> domain
domain           -> standard library + domain-focused crates
theme/domain     -X-> concrete views
```

- Put window/platform setup in `app.rs`; keep `main.rs` minimal.
- Use plain Rust types for domain state and testable behavior.
- Use `Entity<T>` for shared state that must participate in GPUI observation.
- Keep ephemeral state local to its owning view.
- Avoid a universal state object with unrelated responsibilities.
- Views render state and dispatch meaningful operations; they do not own substantial domain logic.
- Use `Render` for stateful entity-backed views and `RenderOnce` for reusable value-like components.
- Use custom `Element` implementations only for specialized rendering that built-ins cannot express or a measured performance need.
- Keep concrete types until type erasure creates a real boundary benefit.
- Never wrap GPUI entities in a second ownership/notification system such as `Rc<RefCell<_>>`.
- Keep I/O, long work, and side effects out of `render`.
- Introduce services only when a real persistence, network, process, or filesystem boundary exists.
- Create a module or directory for a clearly independent responsibility or real reuse, not aspirational structure.

## 5. Abstraction, files, and functions

Start concrete. Introduce traits, generics, macros, reusable components, or helpers after real repetition or when a boundary clearly improves ownership, testing, or substitution.

- Prefer direct code over clever generic APIs.
- Similar code is not automatically the same abstraction.
- Do not extract a helper that merely renames one simple expression.
- Prefer composition over deeply layered wrappers.
- Default visibility to private, then `pub(crate)`, then `pub` only when needed.
- Re-export only a small deliberate crate surface from `lib.rs`.

There is no hard line-count cap. Treat 600 lines as a soft warning for hand-written source. A file over 600 lines requires a cohesion review, not an automatic split. Split by ownership and change boundary; do not fragment the project into tiny files. Cohesive declarative UI, tables, generated code, and focused tests may justify longer files.

Functions have no numeric cap. Each function should have one clear job and one abstraction level. Extract when control flow is hard to scan, unrelated phases are mixed, or there are multiple reasons to change.

## 6. Rust style and file formatting

Use compact, scannable Rust. rustfmt is authoritative.

- Prefer early returns and `let ... else` over deep nesting.
- Use iterators when clearer, not for code golf.
- Avoid dense expressions that hide mutations or failures.
- Use intermediate names when they communicate domain meaning.
- Match meaningful enum variants explicitly; avoid `_` when it hides future behavior.
- Avoid needless allocation and intermediate collections.
- Use checked/saturating arithmetic where input or long-running values can overflow.
- Do not hold borrows or locks across `.await` unless explicitly designed and documented.
- Naming: modules/functions/variables `snake_case`; types/traits `UpperCamelCase`; constants `SCREAMING_SNAKE_CASE`.
- Name getters `count()`, not `get_count()`.
- Keep imports explicit. Glob imports are limited to deliberate preludes such as `gpui::prelude::*` and tightly scoped tests.
- Remove unused imports instead of suppressing warnings.

For text files created or rewritten by an agent:

- UTF-8, LF endings, and one final newline;
- no trailing whitespace;
- no hard wrapping unless the format/local convention requires it;
- no file-wide line-ending churn during a focused edit.

Use each format's native conventions. Do not force Rust formatting onto TOML, Markdown, or JSON.

## 7. Comments and documentation

Comments explain why, not what the syntax says. Document non-obvious intent, invariants, tradeoffs, safety requirements, version-sensitive GPUI behavior, and removable workarounds. Avoid line-by-line narration, comments that repeat names, and stale history.

Use rustdoc for non-obvious public contracts. Document errors, panics, side effects, units, cancellation, and thread expectations where relevant. Add `# Safety` for unsafe APIs. Keep examples minimal and compilable; promise only behavior enforced by code or tests.

When behavior, commands, architecture, setup, or public contracts change, update the relevant README, `GPUI.md`, rustdoc, and examples in the same change. Never knowingly leave guidance stale.

## 8. Errors and recovery

Normal runtime failures must recover through the UI whenever recovery is meaningful.

- Use `Result` for recoverable failures and actionable typed domain errors.
- Add context at boundaries without repeating the same message at every layer.
- Log diagnostic detail once; show concise, recovery-oriented user copy.
- Do not expose implementation errors, secrets, or stack details to users.
- Avoid `unwrap()` and unexplained `expect()` in production render, event, service, and user-driven paths.
- Panics are for broken programmer invariants, not invalid input, missing files, network failures, or ordinary platform conditions.
- Prefer safe collection access when input controls an index.
- Preserve the last valid UI state after failure where practical.
- Error copy states what happened, what remains safe, and what the user can do next.

## 9. Async and responsiveness

Never block the GPUI event loop with meaningful I/O, waiting, or expensive computation.

- Move I/O and CPU-heavy work off the UI thread; demonstrably trivial operations may stay synchronous.
- Make task ownership and cancellation visible.
- Prevent stale async results from updating newer state.
- Use weak handles when work must not keep a view alive.
- Store tasks whose lifetime matches the view; detach only deliberately.
- Prefer owned results/message passing over broad shared mutable state.
- Keep `Send`/`Sync` boundaries small and documented.
- Handle a view closing while work remains pending.
- Avoid speculative concurrency that adds complexity without responsiveness benefit.

## 10. UX and accessibility

Polished UX is the project's top product priority. UI work is incomplete if it implements only the ideal pointer path.

For each relevant component/screen, deliberately consider:

- normal, hover, focus, pressed, selected, and disabled states;
- loading, empty, error, retry, and stale-data states;
- long text, overflow, minimum window size, and resizing;
- rapid repeated actions and interrupted work;
- keyboard-only operation and logical focus movement;
- platform scaling, readable typography, and destructive-action recovery.

Not every state applies everywhere; do not add fake states mechanically.

Use shared semantic tokens for recurring color, spacing, typography, radius, elevation, and motion. Views request meaning such as surface, muted text, or danger instead of scattering unexplained literals. Add a token for a recurring design decision, not every one-off measurement.

Accessibility is a default requirement:

- every primary action has a keyboard path;
- focus is visible and logically ordered;
- interactive elements have stable IDs and complete state styling;
- meaning never depends only on color, hover, position, or an unlabeled icon;
- text/non-text contrast and target sizes remain usable;
- motion respects reduced-motion preferences;
- text scaling, high DPI, long labels, and constrained windows remain usable;
- version-sensitive accessibility APIs are verified against GPUI 0.2.2.

UI copy is concise, active, sentence case, and action-oriented. Prefer specific verbs over “Yes”/“No,” and never blame the user.

## 11. Performance

Measure before adding complexity.

- Keep rendering deterministic and side-effect free.
- Protect obvious hot paths from repeated allocation, I/O, or expensive work.
- Do not add caches, custom rendering, unsafe code, or concurrency on a hunch.
- Profile or establish a reproducible benchmark before non-obvious optimization.
- State the measured bottleneck when performance makes code less simple.
- Prefer user-perceived responsiveness over invisible micro-optimization.

## 12. Dependencies and unsafe code

Agents may add or upgrade Cargo dependencies without prior approval when they materially improve the implementation. First check whether std, GPUI, or an existing crate solves the need; then assess maintenance, license, platform/MSRV support, security history, transitive cost, compile time, binary size, and default features.

Keep additions narrow, pin GPUI deliberately, avoid unrelated upgrades, and update committed `Cargo.lock` whenever resolution changes.

Unsafe Rust is allowed only with proof:

- use it for a demonstrated platform, FFI, or measured performance boundary;
- minimize and isolate it behind a safe API;
- document every caller obligation and safety invariant;
- add focused boundary/edge-case tests;
- use suitable sanitizers or model tools when practical;
- never use unsafe merely to silence the borrow checker or chase speculative speed.

## 13. Privacy and observability

The application is private by default.

- No telemetry, analytics, tracking, or remote reporting without explicit approval.
- Never log secrets, credentials, tokens, private content, or unnecessary personal data.
- Collect, transmit, and retain only data required by the feature.
- Keep diagnostics local unless a separately approved feature requires otherwise.
- Redact sensitive values from errors, debug output, snapshots, fixtures, and examples.
- Never commit secrets or environment-specific credentials.
- Use synthetic fixtures instead of copied production data.

## 14. Testing philosophy

Testing and validation are risk-based. Do not run `cargo check` or a full suite for every changed line merely as ritual.

Add/update tests when they provide meaningful protection, especially for domain logic, state transitions, bug regressions, parsing, validation, persistence, error mapping, boundary values, async cancellation, stale results, keyboard actions, and important conditional UI behavior.

Visual-only edits do not require invented unit tests. For bugs, find the root cause, avoid symptom patches, inspect similar nearby paths, and add a focused regression test when valuable.

Tests are deterministic by default:

- no live network;
- control time and randomness;
- isolate filesystem work in temporary locations;
- avoid machine-specific state;
- remain parallel-safe and reproducible.

Prefer plain unit tests for UI-independent behavior and GPUI-aware tests for framework interactions. Do not launch a production window when a smaller test proves the behavior.

## 15. Validation tiers

Choose the lightest tier that provides credible evidence. Report only checks actually run.

### Tier 0 — no execution

For documentation, comments, or inert metadata when commands add no confidence.

### Tier 1 — focused and fast

For small, low-risk code/UI edits:

- format changed Rust;
- run the narrowest valuable test if one exists;
- inspect the diff for unintended churn;
- do not automatically run `cargo check` for trivial edits.

### Tier 2 — compile and targeted behavior

For meaningful logic/API changes, dependencies, module moves, GPUI API usage, or likely type errors:

```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo test <target-or-filter>
```

Targeted tests are enough when they establish sufficient confidence.

### Tier 3 — full project confidence

For major features, architectural refactors, toolchain/dependency changes, unsafe code, cross-cutting changes, releases, or uncertainty exposed by lower tiers:

```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
```

For UI work, compilation is the default evidence when validation is warranted. Manual launches, screenshots, and visual walkthroughs are optional unless requested or compilation cannot credibly validate the change. If a check cannot run, state why; never claim an unrun check passed.

## 16. Warnings and linting

Do not introduce warnings. Fix warnings caused by the change and small warnings in touched code, but do not turn a focused task into repository-wide cleanup.

Treat correctness and suspicious Clippy lints seriously. Use narrow `allow` attributes only when clearer or more correct than the linted alternative, and explain non-obvious allowances. Never obscure intent merely to satisfy a lint. Clippy runs according to the validation tiers, not after every edit.

## 17. Git, destructive actions, and generated files

When Git is available, agents may inspect status, history, and diffs. Without explicit instruction, never commit, amend, push, fetch with state-changing intent, branch, checkout, rebase, merge, reset, revert, stash, or discard tracked/untracked work.

Use judgment for deletion/destructive actions clearly required by the task. Inspect the affected data, distinguish agent-created artifacts from user work, prefer reversible approaches, and ask whenever ownership, scope, or data-loss risk is unclear. Never use destructive Git commands to make checks pass.

Generated/artifact policy:

- edit source inputs rather than generated outputs;
- regenerate committed outputs when the repository expects them;
- update `Cargo.lock` when resolution changes;
- never add `target/`, PDBs, caches, temporary files, local logs, or editor state;
- do not hand-edit generated files unless no source path exists and the reason is explicit;
- review generated diffs for unexpected churn.

## 18. Definition of done

Before handoff, confirm:

- the requested outcome is fully implemented;
- product-affecting ambiguity was resolved rather than silently guessed;
- the diff is focused and preserves unrelated work;
- domain logic stays independent of GPUI where practical;
- relevant UI interaction, failure, resize, and accessibility states are covered;
- I/O/expensive work does not block the UI;
- recoverable failures do not panic;
- no secrets, telemetry, hidden debt, placeholders, or transient artifacts were introduced;
- docs/examples match changed behavior;
- tests exist where they add meaningful protection;
- the appropriate validation tier passed or its omission is understood;
- no new warnings were introduced;
- the final diff was reviewed.

## 19. Handoff

Keep the final response result-focused. Lead with the outcome in one or two sentences. Mention files only when useful, state checks and results in one concise line when run, and mention only real blockers, risks, or follow-ups. Do not narrate routine investigation or repeat the diff.
