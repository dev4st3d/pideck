# Pideck UI icons

The 44 SVGs in this directory are the app's custom, monochrome control icons.
`src/assets.rs` embeds the complete set under `icons/<name>.svg`.

## Drawing rules

- Use a **16 × 16** viewBox and explicit 16px intrinsic dimensions. Draw for the
  native 16px toolbar first; inspect at 12, 20, 24, and 32px as well.
- Use **1.5px outlines**, round caps and round joins. Keep at least 1px of clear
  space between painted geometry and the viewBox edge, including the stroke.
- Use quarter-pixel adjustments for optical balance. Panel and document frames
  use 1.25px corner radii; paired chevrons and side panels share mirrored geometry.
- Use `currentColor` and transparent interiors. GPUI paints SVGs as alpha masks
  tinted by the view's explicit `text_color`; backgrounds, translucent washes,
  gradients and embedded colors do not belong in these assets.
- Solid shapes are reserved for the stop control, ellipsis dots, small information
  marks and agent cores. They retain their meaning at small sizes.
- Windows caption controls deliberately use **1px square-ended strokes** and
  9px centerline spans at 16px, matching compact native chrome.
- Keep each file self-contained: geometric primitives only, with no fonts,
  external resources, CSS, scripts or SVG filters.

The user-selected Catppuccin explorer theme is a separately licensed upstream
bundle; its source and version are documented in `../catppuccin/README.md`.
`info/assets/icons` contains the older reference artwork, not runtime assets.

## Review and validation

Open the [icon specimen](../../design/icons.html) through a local HTTP server to
compare every asset at 12, 16, 20, 24 and 32px on Paper and Graphite surfaces:

```powershell
python -m http.server 8765 --bind 127.0.0.1
# Open http://127.0.0.1:8765/design/icons.html
```

The specimen reads the embedded inventory from `src/assets.rs` and masks the
actual SVG files, so it previews the same artwork and tinting model as GPUI.

`cargo test assets::tests` checks inventory completeness, asset resolution and
GPUI rasterization at the supported sizes, including nonempty alpha masks and
clear image edges. Native window/DPI inspection remains a separate visual check.
