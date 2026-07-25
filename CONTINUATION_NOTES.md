# PiDeck continuation implementation notes

## Reference implementations reviewed

- Zed GPUI: `zed-industries/zed@b1b412ddb3a15a1cd93250cf63f509fd8381e4b6`
- Pi harness: `earendil-works/pi@b711e26616a741a110f092b9dd761e1a7eb30939`

The patch uses Zed's GPUI animation and capture-phase pointer-event patterns, while keeping Pi as the authority for RPC session behavior, settings validation, and persisted transcript state.

## Delivered work

1. **Connection stability**
   - Added generation-safe automatic reconnect with bounded exponential backoff.
   - Replaces a dead worker coordinator before retrying.
   - Resumes the known Pi session file without replaying prompts.
   - Explicit stop/shutdown invalidates scheduled reconnects.
   - Added dedicated long-operation deadlines for compaction and session replacement; long shell work no longer inherits a five-minute process-killing timeout.

2. **Dead code and churn**
   - Removed the unused tool-card preview/formatting island and its obsolete tests/constants.
   - Kept the live tool presentation pipeline unchanged.

3. **Collapsed running tool animation**
   - The latest pending/running/cancelling tool row receives a 170 ms, ease-out slide/fade when shown in collapsed activity.
   - Stable animation keys avoid continuous restart during ordinary re-renders.

4. **Focus-ring cleanup**
   - Removed visible border/ring focus treatments from buttons, inputs, selectable rows, overlays, diff controls, and dialogs.
   - Preserved keyboard focus, tab order, shortcuts, and subtle surface/text feedback.

5. **Subagent view**
   - Reworked transcript layout with role-specific surfaces.
   - Reuses the main cached Markdown renderer for emitted output.
   - Renders final result and error as first-class output panels.
   - Added pointer-driven scrolling without requiring a click first.

6. **Diff panel**
   - Narrower file tree, denser rows, shallower indentation, compact headers/navigation, and clearer selected-file accent.
   - Preserved line numbers, hunk structure, horizontal scrolling, and keyboard navigation.

7. **Scroll normalization**
   - Added a shared capture-phase wheel router based on pointer bounds instead of focus.
   - Applied to model selection, thinking selection, Pi settings, subagent output, diff file tree, and diff content.
   - Supports precise trackpad deltas and horizontal diff scrolling.

8. **Markdown rendering**
   - Heading-level typography, fenced-vs-inline code treatment, task markers, footnotes, math, quote continuation, nested-list spacing, safer raw HTML handling, and bounded Unicode-aware table columns.

9. **User/agent contrast**
   - Added theme-specific, subtle user-message surfaces and edges for every bundled theme.
   - Applied consistently to the main transcript and subagent transcript.

10. **Pi settings tab**
    - Added typed controls backed by Pi's `SettingsManager`, including delivery modes, transport, compaction/retry, trust/navigation, display density, images/resources, and privacy.
    - Uses Pi's getters/setters, locking, validation, migrations, and flush behavior instead of directly editing JSON.
    - Writes global values only; trusted project-local overrides remain untouched.

11. **Notification-only command cleanup**
    - Dynamic slash/extension commands are no longer inserted as optimistic user bubbles.
    - They still appear if Pi persists an authoritative user message, so genuine prompts are not hidden.

## Validation performed here

- `node --check bridge/*.mjs`
- `node --test bridge/*.test.mjs` — **18/18 passing**
- JSON protocol schema parse check
- `git diff --check`
- Structural delimiter/string/comment scan across every changed Rust source file
- Focus-border/dead-code marker scans

## Native Rust validation still required before merge

This execution environment does not contain `cargo`, `rustc`, or `rustfmt`, and its package network is unavailable. Therefore a native Rust compile was not claimed here. Run the following in the project's normal development environment before merge:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
