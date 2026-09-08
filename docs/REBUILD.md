# PiDeck native UI rebuild

## Delivery status

The Rust/GPUI source was changed against the six supplied 1440 × 960 PNG references. The implementation is not a verified 1:1 match and is not a tested Windows build. This environment had no Rust/Cargo/rustfmt or usable native Windows desktop; no native screenshot was captured. The delivered PNGs under `reference/images/` are the original design inputs, not application output.

The changes below are implemented in source. Their native rendering and live Pi behavior still require the Windows checks described below. No reference image is used as an application background, and no static demonstration answer, model, project, queue, cost or connection state is substituted for live data.

## Implemented changes

### Shell and reference geometry

- Replaced the old workspace composition with a 40-pixel custom native titlebar, 64-pixel navigation rail, 240-pixel project sidebar, shared 60-pixel session toolbar and centered 840-pixel conversation/composer measure. The default window is 1440 × 960 logical pixels. Narrow-window layout yields secondary columns while retaining the conversation.
- Added native window controls and drag handling. Closing uses the existing draft-flush guard rather than bypassing it.
- Rebuilt project selection, new conversation, session search, selected thread, section counts, project management and connection footer. The active project's sessions appear first without a second redundant active-project heading. Other project groups and their session navigation remain available.
- Rebuilt the title, Git branch and Terminal / Changes / Inspector / More controls. Branch and changes use read-only Git inspection. History, export, recovery, theme choice, sidebar and expanded-input actions remain reachable through the toolbar menu.
- Midnight uses exactly the same toolbar layout function as the other themes. No Midnight-specific toolbar displacement is reproduced. Its other palette choices remain theme-specific.

### Themes and typography

- Original, Linen, Graphite and Midnight replace the previous visual treatments, using sampled reference colors, flat surfaces, 1-pixel dividers and reference corner radii. Original's search field uses its canvas color; the other three use their panel color. The Git branch uses the sidebar color; secondary composer actions use the subtle surface.
- DM Sans is the default interface family; Instrument Serif is used for session titles and the rail mark; IBM Plex Mono regular/medium is used for technical text. The exact four design files are registered before the first text run and embedded by `build.rs` after preparation.
- Font binaries are not redistributed. `scripts/prepare-fonts.mjs` imports the original design ZIP or extracted font folder and verifies SHA-256. An optional manifest-source download is also hash-checked. Preparation rejects changed or corrupt files rather than silently substituting a font.
- Legacy default Segoe UI / Cascadia Mono preferences migrate to the design defaults. Explicitly customized families and font scaling remain available. Custom settings will intentionally differ from the references.

### Conversation and composer

- Replaced boxed timeline sections and the continuous decorative rail with a flat reading column, inset user prompt, quiet timestamp, inline activity disclosure, monospaced tool rows and tool-specific connector lines. Activity details retain their native interactive overlay.
- Expanded activity builds older rows only when needed; the compact state keeps the latest activity visible. Completed replies use the larger lead, body/list spacing, compact changes summary and actual Copy / history-backed Retry actions. Cached lead emphasis is reset when block roles change.
- Rebuilt the bottom composer with a 112-pixel normal card, quiet Attach / Model / Thinking selectors, 36-pixel delivery buttons, queue preview and usage footer. Existing IME, grapheme editing, selection, undo/redo, clipboard, attachments, file completion and slash commands remain on the existing editor path.
- Send, Steer, Queue and Stop retain distinct dispatch paths. Clearing the queue has a separate correlated runtime request: it does not abort the active run, keeps returned inputs recoverable and reconciles state after completion or failure. An in-flight clear acknowledgement remains owned if Stop is pressed immediately afterward.
- Known usage is shown without inventing zeroes for missing context or cost. Stale cost is identified; a stale context percentage is omitted. Errors, recovery controls and extension widgets remain visible when relevant rather than being hidden to force a screenshot state.
- Session search is connected to live/catalog sessions as well as commands, with matching keyboard/result limits. Session switches checkpoint the current draft. Relative date labels are derived from UTC dates rather than hardcoded demonstration copy.

### Source and checks

- Removed only the unused, unimported `src/views/composer/buffer.rs`; the active editor is `src/state/editor.rs`.
- Added font-import tests, queue-clear reducer tests, date-label tests, active-project ordering coverage, source/palette contract checks and Windows client-area capture / strict PNG comparison utilities.
- Build and validation workflows prepare the exact fonts before compiling. The original Pi version contract, dependency lockfile, bridge, process supervision, terminal, provider authentication, draft storage and extension integrations are retained rather than replaced with mocks.

## Executed checks

| Check | Result | Evidence |
|---|---|---|
| Existing bridge tests plus new font-import tests | 57 passed, 0 failed | `validation/node-tests.log` |
| Comparison utility unit tests | 4 passed, synthetic images only | `validation/comparison-tests.log` |
| Source/package and palette contract checks | Passed | `validation/source-contracts.json` |
| Import all four fonts from original design ZIP | Exact SHA-256 matches | `validation/font-import.log` |
| Rust lexical/delimiter inspection | No detected lexical or delimiter errors; not a parser or type check | `validation/rust-lexical-check.log` |
| Reference flat-color samples versus theme source | See recorded samples; not a native render | `validation/reference-palette.json` |
| Rust build, Rust tests, rustfmt, Clippy | Not run | Toolchain unavailable |
| Native workflow, keyboard, IME, DPI and screenshot tests | Not run | Native runtime unavailable |
| Live Pi / installed extension compatibility | Not run | No live SDK certification claimed |

Node tests ran under Node 22.16.0; these fixture tests do not waive the application's retained minimum Node 22.19.0 requirement. Static contrast checks cover the recorded text/background pairs, not full accessibility certification. Original's reference muted-on-selection pair is below 4.5:1 and was retained rather than changing the supplied palette.

## Reference coverage

| Supplied image | Implemented source state | Native capture / pixel result |
|---|---|---|
| 01 Original completed | Completed reply, changes summary, Copy / Retry, Send | Not captured; no similarity score |
| 02 Original ongoing compact | Collapsed earlier activity, current tool, queue and running actions | Not captured; no similarity score |
| 03 Original ongoing expanded | Expanded earlier activity and running composer | Not captured; no similarity score |
| 04 Linen ongoing expanded | Same expanded layout, Linen palette | Not captured; no similarity score |
| 05 Graphite ongoing expanded | Same expanded layout, Graphite palette | Not captured; no similarity score |
| 06 Midnight ongoing expanded | Same expanded layout, Midnight palette, shared toolbar position | Not captured; no similarity score |

The reference demonstration data is not injected into normal sessions. Real session titles, file paths, messages, timestamps, model names, counts and costs will differ unless equivalent state is deliberately prepared for comparison. Font shaping, baseline placement, wrapping and Windows rendering differences have not been measured on the native application.

## Build and native verification

From the extracted source on a configured Windows machine:

```powershell
node scripts/prepare-fonts.mjs --zip "C:\path\to\pideck-design.zip"
cargo fmt --all
.\scripts\validate.ps1
cargo run --locked
```

`cargo fmt --all` is included because rustfmt could not be executed here; the CI formatting gate remains enabled. Use the included Cargo.lock, the Pi version recorded in README and default font preferences. For reference comparison, use a 1440 × 960 client area at 100% display scaling and equivalent conversation state. Keep the native window fully visible and unobscured. This source does not fabricate that state or claim to have reproduced it.

Capture each actual state with its corresponding filename:

```powershell
.\scripts\capture-native.ps1 -ProcessId 12345 `
  -Output "visual-artifacts\native\03-original-ongoing-expanded.png"
```

The capture script reads the native client area, refuses other dimensions and refuses to overwrite an existing image. The supplied process ID must belong to the running PiDeck instance. Capture all six states, then compare:

```powershell
python -m pip install Pillow
python scripts/compare-native.py --design-zip "C:\path\to\pideck-design.zip" `
  --captures visual-artifacts/native --output visual-artifacts/comparison
```

The comparison utility performs no resizing, registration, masking, tolerance or cleanup. It emits per-screen/per-region changed-pixel counts, raw RGB error, difference bounds, file hashes, raw differences, an amplified difference view and a 50% overlay. Exit code 0 requires every pixel to match; 1 means differences; 2 means incomplete/invalid input. A comparison result alone does not prove screenshot provenance.

The original Midnight PNG has a displaced toolbar, so the intentionally corrected native toolbar must differ from that original. `--midnight-reference PATH` accepts a separately approved corrected Midnight golden; no such replacement is supplied or invented here. All other Midnight regions still require inspection against the original.
