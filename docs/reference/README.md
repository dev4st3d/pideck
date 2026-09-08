> Archived design handoff, not a source-package inventory. The images here are original references, not native captures. Font binaries are not included; use `../../scripts/prepare-fonts.mjs` with the original design ZIP.

# PiDeck image references

These six supplied PNGs are the visual targets for the native PiDeck rework. Every image is 1440 x 960 pixels. They were inspected, extracted or renamed without re-encoding. Duplicate exports are omitted.

| Image | Appearance and state |
|---|---|
| images/01-original-completed.png | Original light; completed response, earlier tools collapsed, Send |
| images/02-original-ongoing-compact.png | Original light; running edit, earlier tools collapsed, queued follow-up, Stop/Queue/Steer |
| images/03-original-ongoing-expanded.png | Original light; same running conversation with earlier tools expanded inline |
| images/04-linen-light-ongoing-expanded.png | Linen light; expanded running conversation |
| images/05-graphite-dark-ongoing-expanded.png | Graphite dark; expanded running conversation |
| images/06-midnight-dark-ongoing-expanded.png | Midnight dark; expanded running conversation; toolbar export defect described below |

## Authority and one export defect

Use the images as the primary visual authority and design.md for measurements, typography, palettes and interaction interpretation. The spec describes seven historical Figma frames; only six PNG references were supplied. Porcelain is specified in prose and palette tables but has no supplied image. Do not claim a seventh image was inspected or reproduced. The former source/ directory is intentionally excluded; no SVG source or token JSON is required to consume this handoff.

The Midnight PNG places its conversation toolbar roughly 100 px too low and shifts it right, behind the user message. The other five references and the shared geometry in design.md put the toolbar at x=304, y=40, height=60, with title x=336 and controls starting y=52. Default implementation direction: preserve Midnight's colors, content and other geometry, but align this toolbar to that shared shell. Record this narrow correction explicitly in visual validation; do not call its corrected toolbar a literal pixel match to the defective export. The original PNG remains untouched.

Differences elsewhere must be resolved from the supplied images, not by inventing a new layout. Historical design prose sometimes differs from the raster exports (including status colors); inspect the actual PNG before choosing a token value.

## Fonts

fonts/ contains DM Sans variable, Instrument Serif Regular, and IBM Plex Mono Regular/Medium, plus their OFL licenses and provenance/hash records in sources.json. Bundle/load these faces in the application as needed to match the references. Existing Segoe UI/system-only presentation defaults are superseded by the requested design. Verify actual weight selection and fallback at runtime.

## Package use

Extract pideck-source.zip, then extract pideck-design.zip beside it. The archives yield sibling pideck/ and design/ directories. The implementation may move fonts and any necessary assets into pideck/assets/ and update build/runtime asset loading. The source ZIP contains the existing Rust application and Node bridge, not a browser frontend.

Local originals, duplicate PNGs, input ZIPs and former source/ files are preserved outside this handoff in temp/design-originals-20260906/. They are not additional design targets.
