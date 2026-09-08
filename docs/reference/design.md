# Pideck — desktop Pi harness design specification

> Handoff inventory, 2026-09-06: this package contains SIX original 1440 x 960 PNGs, indexed in README.md. The seven Figma screens described below are historical authoring context; Porcelain has no supplied PNG. The former design/source/ SVGs and token JSON have intentionally been removed from this design handoff. PNGs take precedence over conflicting historical prose. See README.md for the Midnight toolbar export defect and the visual acceptance rule.


Version: 2026-09-06. This document describes the final aligned Figma designs, including the revised icon toolbar and the four theme explorations. Measurements below come from the final editable SVG design sources. They describe the designs, not a claim that the application already implements them.

## 1. Product and design direction

Pideck is a native desktop GUI for the Pi AI harness. Its primary surface is a conversation document with agent activity embedded in the conversation. It is not an analytics dashboard or a separate tool-execution console.

Preserve these decisions:

- A narrow dark navigation rail and a fuller project/session sidebar, retaining the original first screen's visual direction.
- A dominant, centered conversation column with a bottom-anchored composer.
- Quiet editorial typography: a serif session title, a readable sans-serif interface, and monospaced tool activity.
- Compact tool calls inside the conversation, including completed conversations.
- The same window, sidebar, toolbar, reading measure, and composer geometry in every state and theme.
- Flat surfaces, restrained rounded corners, subtle separators, and a limited accent color. No gradients, decorative dashboard cards, large metric tiles, or unrelated imagery.

## 2. What exists: appearances, states, and screens

There are **7 final screens in Figma**, organized across **4 pages**. They represent **3 conversation states** and **5 appearances**: the original light appearance plus two additional light themes and two dark themes.

| Page / screen | Appearance | Conversation state | Figma |
|---|---|---|---|
| 01 — Conversation completed | Original | Completed; earlier steps collapsed | [Completed](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv?node-id=21-175) |
| 02 — Ongoing conversation | Original | Running; earlier steps collapsed | [Ongoing](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv?node-id=21-348) |
| 03 — Conversation · Inline tools | Original | Same running conversation; earlier steps expanded | [Inline history](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv?node-id=21-523) |
| 04 — Linen · Light | Linen | Running; earlier steps expanded | [Linen](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv?node-id=23-2027) |
| 04 — Porcelain · Light | Porcelain | Running; earlier steps expanded | [Porcelain](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv?node-id=23-2214) |
| 04 — Graphite · Dark | Graphite | Running; earlier steps expanded | [Graphite](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv?node-id=23-2401) |
| 04 — Midnight · Dark | Midnight | Running; earlier steps expanded | [Midnight](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv?node-id=23-2588) |

[Open the complete Figma file](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv). [Open the four-theme comparison](https://www.figma.com/design/WnJKe9Vnc5A1VxjrpZeCUv?node-id=15-1744).

Five appearances multiplied by three states gives **15 possible combinations**. That is a combination count, not the number of screens authored in Figma. The four theme explorations use the same expanded conversation so their colors can be compared directly.

### The three conversation variants

| Variant | Earlier activity | Latest tool | Final Pi answer | Composer |
|---|---|---|---|---|
| Completed | Collapsed under “6 earlier steps” | `bash`, status `done`, command still visible | Visible below the activity | Empty message field and Send |
| Ongoing, compact | Collapsed under “4 earlier steps” | `edit`, status `running`, file path visible | Not shown while the run is ongoing | Draft, queued follow-up, Stop, Queue, Steer |
| Ongoing, expanded | Same earlier activity expanded inline | Same running `edit` | Not shown | Identical to the compact ongoing variant |

The third variant is a disclosure state of the ongoing conversation. It is not another dashboard, a different app layout, or a separate tool-call destination.

## 3. Reference geometry and coordinate system

Every frame is **1440 × 960 design pixels**. Coordinates are measured from the top-left corner of the frame. In a native implementation, treat these as logical layout units; physical pixels depend on display scaling.

For text, the SVG `y` coordinate is a **baseline**, not a top margin. Do not convert a text baseline directly into container padding.

| Region | X | Y | Width | Height | Notes |
|---|---:|---:|---:|---:|---|
| Window | 0 | 0 | 1440 | 960 | Fixed reference frame |
| Native titlebar | 0 | 0 | 1440 | 40 | Full-width window chrome |
| Navigation rail | 0 | 40 | 64 | 920 | Dark in every appearance |
| Project/session sidebar | 64 | 40 | 240 | 920 | 24 px horizontal content insets |
| Main workspace | 304 | 40 | 1136 | 920 | Remaining width |
| Conversation toolbar | 304 | 40 | 1136 | 60 | Bottom separator at Y=99 |
| Reading column | 452 | 100 onward | 840 | Content-dependent | Centered in the main workspace |
| User message surface | 452 | 155 | 840 | 95 | Radius 7 |
| Queued-follow-up separator | 452 | 758 | 840 | 1 | Ongoing variants only |
| Composer | 452 | 805 | 840 | 112 | Radius 11; bottom at 917 |
| Composer footer | 452–1292 | Baseline 942 | 840 | Text-dependent | Hint left, usage right |

The main workspace has **148 px on either side of the 840 px reading column** at the reference width: `(1136 − 840) / 2 = 148`. The user surface, activity area, Pi reply, diff summary, queue separator, composer, and footer must use this same column.

## 4. Typography — exact design fonts

### Font families

| Family | Weights used | Role |
|---|---|---|
| **DM Sans** | 400 Regular, 500 Medium, 600 Semibold | Interface labels, conversation prose, controls, metadata, sidebar wordmark |
| **Instrument Serif** | 400 Regular | Session title, rail `p.` mark, native-titlebar π mark |
| **IBM Plex Mono** | 400 Regular, 500 Medium | Tool names, file paths, commands, branch name, activity summaries, small technical labels |

No italic style is used in the final seven screens. No additional letter-spacing is specified in the source SVGs. Keep normal font spacing; do not introduce wide tracking to ordinary labels.

These are the **design fonts**. The current Rust app's default font preferences are Segoe UI for main/sans text and Cascadia Mono for monospace. Do not mistake those runtime defaults for the typefaces used in these Figma concepts. Implementing this design requires an explicit font-loading decision and visual verification with the design fonts.

### Type roles

| Element | Family | Size | Weight |
|---|---|---:|---:|
| Sidebar Pideck wordmark | DM Sans | 25 | 600 |
| Sidebar project name | DM Sans | 16 | 600 |
| Session title in toolbar | Instrument Serif | 24 | 400 |
| Rail `p.` mark | Instrument Serif | 28 | 400 |
| Native titlebar π | Instrument Serif | 22 | 400 |
| Native app name | DM Sans | 12 | 500 |
| Native project label | DM Sans | 12 | 400 |
| Native demo-session label | DM Sans | 11 | 400 |
| Rail captions | DM Sans | 9 | 400 |
| Sidebar section label | DM Sans | 10 | 500 |
| Sidebar session titles | DM Sans | 13 | 500 |
| Sidebar session metadata | DM Sans | 11 | 400 |
| New conversation label | DM Sans | 13 | 500 |
| Search label | DM Sans | 11 | 400 |
| Search shortcut / technical eyebrow | IBM Plex Mono | 10 | 400 |
| Toolbar action labels | DM Sans | 13 | 500 |
| Changes count | DM Sans | 11 | 600 |
| Branch name | IBM Plex Mono | 12 | 400 |
| You / Pi labels | DM Sans | 12 | 600 |
| User prompt and composer text | DM Sans | 16 | 400 |
| Lead sentence of final answer | DM Sans | 17 | 500 |
| Answer body / activity commentary | DM Sans | 15 | 400 |
| Earlier-steps disclosure | DM Sans | 12 | 400 |
| Activity summary | IBM Plex Mono | 10 | 400 |
| Tool header | IBM Plex Mono | 12 | 500 |
| Tool file/command rows | IBM Plex Mono | 12 | 400 |
| Tool status | IBM Plex Mono | 11 | 400 |
| Details label | DM Sans | 11 | 400 |
| Working label | DM Sans | 12 | 400 |
| Queue label | DM Sans | 11 | 500 |
| Queue message | DM Sans | 12 | 400 |
| Attach / model / thinking labels | DM Sans | 12 | 400 |
| Send / Stop / Queue / Steer | DM Sans | 13 | 500 |
| Footer hints and usage | DM Sans | 11 | 400 |

### Complete size/weight inventory

- DM Sans: `9/400`, `10/500`, `11/400`, `11/500`, `11/600`, `12/400`, `12/500`, `12/600`, `13/500`, `15/400`, `16/400`, `16/600`, `17/500`, `25/600`.
- Instrument Serif: `22/400`, `24/400`, `28/400`.
- IBM Plex Mono: `10/400`, `11/400`, `12/400`, `12/500`.

### Line spacing and font rendering

The source uses positioned text lines rather than a global line-height token. Measured examples:

- User prompt baselines: Y=211 and Y=235, a **24 px** interval for 16 px text.
- Expanded file rows: Y=350, 372, 394, a **22 px** interval for 12 px monospace.
- Completed bullet baselines: Y=520, 550, 580, a **30 px** interval including paragraph spacing, not a 30 px line-height requirement.
- Tool header to its first path/command: **26 px**.

For implementation, start with 24 px line height for 16 px prose, 22–24 px for 15 px prose, and 20–22 px for 12 px tool text. These are implementation recommendations, not extra values encoded in the static SVG. Preserve measured baseline positions when reproducing the reference frame.

The SVG does not lock DM Sans optical-size axes or font-engine hinting. Resolve the actual font face and weight explicitly and compare a rendered result. A monospace fallback for DM Sans or a sans-serif fallback for Instrument Serif is a failed reproduction.

Font sources: [DM Sans](https://github.com/google/fonts/tree/main/ofl/dmsans), [Instrument Serif](https://github.com/google/fonts/tree/main/ofl/instrumentserif), [IBM Plex Mono](https://github.com/google/fonts/tree/main/ofl/ibmplexmono). Preserve the accompanying font license files when distributing the fonts.

## 5. Color system

Use semantic roles rather than replacing every identical hex value globally. In the original appearance, `#24272C` is both the rail background and the main text color; these roles must diverge in a dark theme.

### Colors rendered in the original appearance

| Role | Value | Use |
|---|---|---|
| Canvas | `#FAF9F6` | Conversation background and quiet toolbar controls |
| Surface | `#FFFFFF` | Composer and text/icons on primary blue controls |
| Sidebar / chrome | `#F0EFEB` | Sidebar, titlebar, branch surface, user prompt, secondary actions |
| Ink / rail | `#24272C` | Main text; dark rail background as a separate semantic role |
| Muted | `#6A6D73` | Metadata, inactive controls, secondary tool text |
| Border | `#DDDED9` | Hairlines and composer/toolbar borders |
| Tool branch | `#B7BAB6` | Compact file-tree connectors |
| Rail muted | `#D4D7DD` | Inactive rail icons and captions |
| Accent | `#304CDC` | Primary actions, selection accents, running state |
| Selection tint | `#E8EDFC` | Selected session and Changes toolbar control |
| Success | `#326C52` | `done`, connection status, positive diff counts |

### Four theme palettes

| Token | Linen · light | Porcelain · light | Graphite · dark | Midnight · dark |
|---|---|---|---|---|
| Canvas | `#F7F4ED` | `#F8FAFD` | `#20211F` | `#141D2B` |
| Surface | `#FFFEFA` | `#FFFFFF` | `#282A27` | `#1A2638` |
| Sidebar / chrome | `#EFEBE2` | `#EEF2F8` | `#1C1E1B` | `#111A28` |
| Rail | `#202521` | `#19263B` | `#141613` | `#0B121D` |
| Main text | `#242823` | `#202C40` | `#EEEFE8` | `#EAF0FA` |
| Muted text | `#62675E` | `#5F6C81` | `#ABB0A4` | `#A6B5CC` |
| Border | `#D8D8CE` | `#D6DFEB` | `#41453D` | `#34465F` |
| Accent | `#355E4B` | `#365BC0` | `#C3D6A3` | `#ADC6FF` |
| Selection tint | `#DDE7DC` | `#E4EBFC` | `#36402D` | `#263F64` |
| Success | `#2F664A` | `#306748` | `#ABD4AD` | `#9CD7C2` |
| Subtle surface | `#F0F0E8` | `#F0F4FA` | `#30332C` | `#23334A` |
| Rail muted | `#B2BAAE` | `#AEBDD3` | `#ABB5A4` | `#A6B5CC` |
| Working | `#795817` | `#79561D` | `#DDC28D` | `#E7C69B` |
| Foreground on accent | `#FFFFFF` | `#FFFFFF` | `#202521` | `#152033` |

Linen pairs warm paper with forest green. Porcelain pairs cool pale surfaces with blue. Graphite uses warm charcoal and pale sage. Midnight uses navy and pale cornflower blue.

### Token application rules

- Canvas fills use `canvas`; the composer uses `surface`.
- The user prompt and Stop/Queue surfaces use `subtle` in the four explorations.
- The sidebar and native chrome use `sidebar`; the rail background uses `rail`.
- Primary actions and the selected rail control use `accent`. Their text and vectors use `onAccent`.
- The selected session and Changes toolbar surface use `selection`; their accent text remains `accent`.
- Tool connectors use `muted` in the four explorations, replacing the original dedicated branch gray.
- Literal `running` and `Working…` text use `working` in the four explorations; completed tool labels use `success`.
- The working-session subtitle in the selected sidebar remains accent-colored in the rendered explorations. Do not assume every working-related label was mapped to the amber working token.
- In the dark explorations, pale primary buttons use dark foregrounds. Never leave white text on the pale sage or blue accent.

### Additional palette tokens, not demonstrated in these screens

The palette definitions also contain the following values. They are reserved values; the current four rendered explorations do not demonstrate a separate `railText` treatment, `railSelected` surface, error state, or data state.

| Reserved token | Linen | Porcelain | Graphite | Midnight |
|---|---|---|---|---|
| Rail text | `#EEF2E9` | `#F0F5FE` | `#EEF2E9` | `#EAF0FA` |
| Rail selected | `#3B483B` | `#344963` | `#34422E` | `#293F60` |
| Data | `#5F557F` | `#62518E` | `#C3BDE1` | `#C6B8E8` |
| Error | `#9A3F37` | `#A23C3D` | `#EDAAA1` | `#F0A8B1` |

## 6. Spacing, padding, radius, and stroke rules

The design uses a recurring 4/8/12/16/20/24/32 px spacing vocabulary, with optical adjustments where text baselines or exact control geometry require them. It is not a claim that every coordinate falls on an 8 px grid.

| Item | Exact reference value |
|---|---|
| Sidebar horizontal inset | 24 px |
| Standard sidebar content width | 192 px |
| Selected session inner horizontal inset | 12 px |
| User message horizontal text inset | 18 px |
| Composer horizontal content inset | 20 px |
| Toolbar title inset from main workspace | 32 px |
| Toolbar control top/bottom inset | 12 px top; 12 px to the 100 px boundary |
| Gap between Terminal / Changes / Inspector / More | 8 px |
| Gap from branch control to Terminal | 12 px |
| Composer action gap | 8 px |
| Composer actions to right edge | 20 px |
| Composer action bottom inset | 11 px |
| Typical action label left inset | 12 px |
| Typical action icon right inset | 11 px |
| Inline tool type inset from reading column | 4 px |
| File/command text inset from reading column | 28 px |
| Tool status end to Details text start | 32 px |

| Shape | Radius |
|---|---:|
| Rail mark, selected rail item | 6 px |
| Sidebar buttons, search, selection | 6 px |
| Branch selector and toolbar controls | 6 px |
| Changes count badge | 4 px |
| User prompt surface | 7 px |
| Composer | 11 px |
| Composer action buttons | 6 px |

- Primary separators and surface outlines are 1 px.
- Small custom icon strokes are generally 1.4 px, with round caps and joins.
- The branch-selector icon uses 1.5 px strokes.
- Window-control strokes are 1.2 px.
- Repository toolbar icons keep their own source stroke weights and are scaled from a 16 px viewBox to 20 px; their effective stroke widths scale with the vectors.
- There are no drop shadows, backdrop blurs, textures, or gradients in the final screen artwork. Shadows around Figma's editor UI are not part of Pideck.

## 7. Native titlebar and revised conversation toolbar

### Native titlebar

The titlebar is 40 px high. The app mark and name sit on the left, followed by a short divider and the project name. The demo-session label is centered at X=720, baseline Y=25. The centered demo label identifies the fixture; it is not required production copy.

| Window control | X | Width | Height | Icon |
|---|---:|---:|---:|---|
| Minimize | 1302 | 46 | 40 | Centered 10 px horizontal line |
| Maximize | 1348 | 46 | 40 | Centered 10 × 10 outline |
| Close | 1394 | 46 | 40 | Centered 10 × 10 diagonal cross |

### Conversation toolbar

The session title is Instrument Serif 24 Regular at X=336, baseline Y=78. The control row starts at Y=52, has a height of 36 px, and uses Y=75 text baselines. The bottom separator is at Y=99.

| Control | X | Y | Width | Height | Treatment |
|---|---:|---:|---:|---:|---|
| Branch selector | 912 | 52 | 84 | 36 | Branch vector, `main`, down chevron |
| Terminal | 1008 | 52 | 108 | 36 | 20 px terminal icon + label, subtle outline |
| Changes | 1124 | 52 | 128 | 36 | 20 px diff icon + label + count, selection tint |
| Inspector | 1260 | 52 | 108 | 36 | 20 px inspector icon + label, subtle outline |
| More | 1376 | 52 | 36 | 36 | Three centered vector dots, subtle outline |

The Changes count badge is at X=1227, Y=60, size 18 × 20, radius 4. Its text is centered at X=1236, baseline Y=74. Preserve the roughly 12 px optical gap between “Changes” and the badge; do not compress them into one crowded label.

Terminal, Changes, and Inspector remain functional harness controls. The Changes tint is the visual emphasis shown in these reference frames; it is not evidence of a separate dashboard tab. Their resulting panels are not drawn in this set.

## 8. Navigation rail and project/session sidebar

### Rail

All rail icons and captions center on **X=32**. Vector icons use a consistent 16 × 16 logical slot, typically starting at X=24.

- The `p.` mark sits in a 40 × 40 accent square at X=12, Y=52, radius 6.
- Projects uses a folder vector at X=24, Y=124; caption baseline Y=158.
- Sessions uses a 48 × 54 selected surface at X=8, Y=176. Its vector begins at X=24, Y=187; caption baseline Y=220.
- Settings sits near the bottom: vector at X=24, Y=886; caption baseline Y=920.
- Captions are DM Sans 9 Regular, centered. Do not align these by manually guessing a different text X for each word.

### Sidebar

The sidebar starts at X=64. Content begins at X=88 and ends at X=280.

| Element | Reference geometry / baselines |
|---|---|
| Pideck wordmark | X=88, baseline 87 |
| Project name | X=88, baseline 138 |
| Project chevron | 16 px slot at X=256, Y=122 |
| Local workspace eyebrow | X=88, baseline 160 |
| New conversation | X=88, Y=191, 192 × 40 |
| New conversation icon | 16 px slot at X=101, Y=203 |
| New conversation label | X=126, baseline 217 |
| Search | X=88, Y=249, 192 × 36 |
| Search icon | 16 px slot at X=99, Y=259 |
| Search label | X=124, baseline 272; “Search sessions” |
| Search shortcut | Right-aligned to X=269, baseline 272; “Ctrl K” |
| Today / count | Baseline 334, label X=88, count right-aligned X=280 |
| Selected session | X=88, Y=352, 192 × 90 |
| Selection eyebrow / title / metadata | X=100; baselines 376 / 399 / 421 |
| First recent session | X=100; title 486, metadata 510 |
| Second recent session | X=100; title 564, metadata 588 |
| Add project | Plus icon at X=88, Y=801; label X=112, baseline 814 |
| Settings & providers | X=88, baseline 877 |
| Pi connection | Dot plus label; label X=103, baseline 920 |

Use the available width to truncate long session names with an ellipsis in implementation. Preserve the 12 px selection padding and reserve shortcut width in search. Do not let the search label run beneath “Ctrl K”.

## 9. Conversation and inline activity

### Turn structure

The structure is **You prompt → activity/tools → final Pi response**. Completion must not remove the activity or move it into a separate screen. The latest activity remains visible even when earlier steps are collapsed.

“Earlier steps” expands previous activity within the same turn. A tool's **Details** action is different: it opens the existing detailed activity surface. Do not replace the history disclosure with a large terminal dump or make every compact tool card an output console.

### Compact tool styling

- Flat, transparent tool rows with compact monospaced names and paths.
- Tool names start at X=456; file and command text start at X=480.
- Status text is right-aligned to X=1160.
- Details starts at X=1192 and has a separate 16 px external-link vector at X=1241.
- Connected tree strokes line up around X=464, with path labels at X=480. Multi-file groups retain connected branches for every row.
- File/command strings are secondary text; the tool name is the stronger line.
- Use explicit status words such as `done` and `running`; color alone does not express state.
- Consecutive same-tool file operations can group, for example `read (3 files)`. Bash calls remain distinct.

### Exact vertical state examples

| Content | Completed | Ongoing compact | Ongoing expanded |
|---|---:|---:|---:|
| Disclosure baseline | 287 | 287 | 287 |
| First visible tool header | 326: bash | 326: edit | 324: read group |
| First visible tool path | 352 | 352 | 350 |
| Additional read paths | Hidden | Hidden | 372, 394 |
| Prior commentary | Hidden | Hidden | 437 |
| Current edit header/path | Not applicable | 326 / 352 | 488 / 514 |
| Working label | Not shown | 399 | 560 |
| Final Pi label | 406 | Not shown | Not shown |
| Final answer lead sentence | 442 | Not shown | Not shown |

The completed screen shows final body text at baseline 474, bullets at 520/550/580, verification copy at 622, a diff-summary separator at 656, summary baseline 684, and response actions at 726.

Model names, paths, commands, timings, counts, token usage, costs, and test results shown in these screens are synthetic demonstration content. They are not measurements or claims about an actual run.

## 10. Composer, queue, and action semantics

The composer is fixed to the bottom of the conversation surface while the transcript scrolls independently. Its reference rectangle is X=452, Y=805, width 840, height 112, radius 11, with a 1 px outline.

- Input starts at X=472, baseline Y=839: 20 px horizontal inset.
- Attachment vector: 16 px slot at X=472, Y=879; “Attach” starts X=495, baseline Y=892.
- Model label starts X=566, baseline Y=892; down-chevron slot X=645, Y=878.
- Thinking label starts X=694, baseline Y=892; down-chevron slot X=806, Y=878.
- Treat selector icons as separate vectors, not Unicode chevrons appended to text.

| Action | State | X | Y | Width | Height |
|---|---|---:|---:|---:|---:|
| Send | Idle/completed | 1180 | 870 | 92 | 36 |
| Stop | Ongoing | 982 | 870 | 84 | 36 |
| Queue | Ongoing | 1074 | 870 | 94 | 36 |
| Steer | Ongoing | 1176 | 870 | 96 | 36 |

Action labels share baseline Y=893. Trailing icons are 16 px and centered vertically within the 36 px control. Stop uses a **square**, not the X used to remove an item. Send and Steer use upward arrows; Queue uses a return/queue arrow.

Semantic behavior to preserve:

- **Send:** submit a new prompt when idle.
- **Steer:** direct the currently running agent.
- **Queue:** retain a follow-up for delivery after the current response.
- **Stop:** stop the current run. Do not imply a retry or a destructive session deletion.
- A draft stays editable during a run and belongs to its session.

The queue preview is directly above the composer. Its separator is Y=758. “Queued”, message, and Remove share baseline Y=783. Remove has its own close vector; it must not be confused with Stop.

The footer uses baseline Y=942. Keyboard instructions align left to X=452; usage aligns right to X=1292. Keep usage secondary and do not convert it into a dashboard card.

## 11. Icons and alignment requirements

Use editable vector paths with deliberate bounds. Repository assets for the main toolbar include `assets/icons/terminal.svg`, `assets/icons/diff.svg`, and `assets/icons/inspector.svg`; their 16 px source viewBoxes are scaled to 20 px in the toolbar.

Other icons in the final source include folder, diamond/session, settings, plus, search, disclosure chevrons, attachment, external-link, stop square, queue arrow, send/steer arrow, branch, overflow dots, and native window controls.

Alignment rules:

1. Center rail vectors and labels on X=32.
2. Use a common control rectangle before aligning its icon and text.
3. Keep 16 px small-control icons and 20 px toolbar icons consistent within their respective families.
4. Use optical text baselines; an icon's top is not a text baseline.
5. Keep trailing action icons 11 px from the control's right edge.
6. Reserve space for counts, keyboard hints, and disclosure indicators.
7. Use a square for Stop, a cross for Close/Remove, and a proper branch symbol for the branch selector.
8. Preserve the final toolbar's count spacing and shared Y=52 control row.

The π and `p.` marks are intentional typographic marks. They are exceptions to the vector-icon rule, not examples for rendering control icons with arbitrary glyphs.

## 12. Interaction and accessibility requirements

The Figma screens are static concept frames. They show resting, selected, completed, running, and expanded-history appearances. They do not prove implemented keyboard interaction, responsive layout, animation, or runtime behavior.

Implementation requirements:

- Every button and disclosure must have a keyboard path and visible focus.
- Enter/Space activates focused disclosure and tool-detail controls.
- Preserve the shown composer hints: Enter to send or steer, Shift+Enter for newline, Alt+Enter to queue, and Escape to stop.
- “Ctrl K” is the shortcut drawn in the session-search design. Confirm or implement that binding before shipping the label; the mock alone does not establish an existing runtime shortcut.
- Icon-only controls need accessible names and tooltips: Projects, Sessions, Settings, More, window controls, and Remove.
- Pair color with text for run and tool states.
- Keep a useful focus target after opening/closing details or switching sessions.
- Do not clear a newer draft because an older operation finishes.
- Keep failed, cancelled, pending, and unknown outcomes distinguishable when implemented. Those complete screens are not included in this design set.

Theme calculations during the design pass reported primary-button foreground contrast of at least 6.15:1 and muted-text contrast of at least 4.73:1 across the checked theme surfaces. These checks do not establish accessibility for every possible control state or future layout; recheck actual foreground/background pairs after implementation.

The 9–11 px captions are deliberate desktop reference sizes. Support display/text scaling and test legibility at actual application scale. Do not use these caption sizes for main conversation prose.

## 13. Resizing, overflow, and motion

Only the 1440 × 960 reference size was drawn. The following are implementation rules, not additional completed Figma variants:

- Keep the reading/composer width at a maximum of 840 logical pixels.
- Center that column in the available main workspace. On narrower windows, reduce the column width while retaining practical side insets, rather than scaling all text down.
- Collapse or hide navigation/optional panels before allowing the conversation to become unusably narrow.
- Keep the composer anchored, and allow the conversation and session list to scroll independently.
- Truncate long session titles, branch names, and single-line tool paths; preserve a route to their full content.
- Allow multiline prose and composer input to wrap naturally. The line breaks in the reference are fixtures, not a universal wrapping algorithm.
- Do not proportionally scale fixed X coordinates to implement responsiveness; translate the measured geometry into layout constraints.

No animation durations or easing curves are encoded in these static sources. For implementation, use subtle feedback for running tools and short disclosure transitions, preserve scroll position during expansion, and honor reduced-motion preferences. Avoid decorative looping motion, pulsing whole panels, and motion that shifts the composer.

## 14. Editable structure and handoff boundaries

The final imported frames contain editable vector/text groups. The source structure includes:

- `Native-titlebar` and `Window-*` controls.
- `Navigation-rail` and icon groups.
- `Project-sidebar`, `New-conversation`, and `Search-conversations`.
- `Conversation-toolbar`, `Branch-selector`, and `Toolbar-*` controls.
- `User-message` and `Inline-activity`, including `Tool-*` groups.
- `Pi-final-response` for the completed variant.
- `Queued-message` for the ongoing variants.
- `Message-composer`, `Button-*`, and `Icon-*` groups.

These grouped SVG imports are editable artwork. They are not a finished Figma component library with comprehensive variants, auto-layout constraints, variable bindings, or a wired prototype. Preserve semantic grouping when rebuilding them as native components.

Recommended reusable implementation units are the window titlebar, rail item, project selector, session row, toolbar action, conversation turn, earlier-steps disclosure, compact tool group, tool-details trigger, queue preview, model/thinking selector, composer, and composer action. Layout and theme values should come from shared semantic tokens rather than repeated literals.

The historical specification was measured from editable SVG snapshots. Those sources are intentionally excluded from this image-led handoff. Use the six PNGs in images/ as the supplied visual references, with the inventory and exceptions in README.md. Figma links are provenance, not a prerequisite for using this package.

## 15. Implementation acceptance checklist

- [ ] All three original screens share the same shell, titlebar, toolbar, reading column, and composer alignment.
- [ ] The original dark rail and fuller sidebar remain intact.
- [ ] Completed history is collapsed, with the latest tool still visible above the Pi reply.
- [ ] Ongoing compact and expanded variants represent the same conversation and draft.
- [ ] Expanded tool history stays within the turn; Details remains a separate affordance.
- [ ] The three font families and their actual weights resolve correctly without fallback.
- [ ] Sidebar names, search shortcut, toolbar labels/count, and composer actions do not overlap or clip.
- [ ] Rail icons are centered; toolbar icons use 20 px bounds; smaller controls use 16 px bounds.
- [ ] Stop is a square, Close/Remove is a cross, and native window controls occupy equal slots.
- [ ] Changes has a readable gap before its count badge.
- [ ] Dark-theme primary foregrounds remain dark on pale accents.
- [ ] Color changes preserve geometry and typography.
- [ ] No standalone dashboard or decorative metrics surface has replaced the conversation.
- [ ] Runtime keyboard, scaling, focus, overflow, and reduced-motion behavior are verified separately from the static artwork.
