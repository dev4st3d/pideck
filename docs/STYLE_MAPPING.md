# Reference styling applied to the manager

Visual authority: [final Paper references](../design/paper/README.md), including the user's terminal and Git screenshots. These supersede the historical Graphite conversation design. The manager keeps its project selector, Projects / Files / Git panel tabs, editable files, diffs, and terminals.

The reference uses a 256px sidebar with 16px insets and a 24px main inset. The project selector, tab strip and content share the main x=280 guide. The shared header has a 30px titlebar, 52px toolbar and 28px serif Pideck wordmark. The primary action is 30px high with 4px corners. Bottom navigation is 42px high, with an underline for the selected tab; the global status bar is 30px high.

Static Geist regular/medium, Newsreader 16pt regular/medium and JetBrains Mono regular/medium faces are embedded from their official upstream sources. Font licenses and hashes are recorded alongside the assets. Legacy font preferences do not override the reference's monospace face. The saved settings file is not modified.

File rows use a 28px rhythm and Geist labels; Git rows have a 40px surface and 8px separation. Both retain virtualization, quiet selection and explicit focus feedback. Mixed document tabs are flat with a 2px active underline, reserve their close-button area, and retain full titles in tooltips. A fresh sidebar does not imply that its first entry is selected. Terminal content remains a real PTY with no decorative heading or repeated path/status block. Git diffs align old and new line numbers with explicit addition/deletion markers as well as color.

GPUI 0.2.2 SVG elements require their own `text_color`; the SVG paint path does not infer it from a parent label. Every workbench SVG now has an explicit tint. The asset source also includes the reused editor's search-control icons.

The Paper quota was exhausted during implementation, so geometry and colors come from the authored styles preserved in the design conversation and the user's final screenshots. Screenshots specify appearance, not fixed terminal output or fabricated Git state. Native text rasterization, window scaling and live content can differ from the Paper export; final validation results are reported with the implementation handoff.
