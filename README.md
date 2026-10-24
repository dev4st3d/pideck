# ZiDeck

A native workspace for project folders, files, Git, and terminals. Rust and GPUI, for Windows.

![File tree and an open source file on the Stone theme](assets/screenshots/stone.png)

Stone is the default. Flint and Light are below.

![Git history and a split diff on the Flint theme](assets/screenshots/flint.png)

![A project terminal with Pi open, on the Light theme](assets/screenshots/light.png)

## What you get

- Several project folders. Each keeps its own files, Git state, and terminals.
- Text editing, image viewing, and diffs in one tab strip.
- Stage, commit, push, and history, including unified and split diffs.
- Terminals start in the project directory and keep running when you switch tabs or projects.
- A pane grid of one to four panes. Rest a dragged tab toward the side of a pane to open a row or column there, or on a layout in the snap tray. Drag the dividers to resize.
- A checklist for each project. Main shortcuts can be changed in Settings.

The theme, font, shortcuts, and layout stay on your machine. ZiDeck does not send telemetry.

## Run

Windows, stable Rust (syntax floor 1.88):

```powershell
cargo run --locked
```

```powershell
cargo build --release --locked
```

**ZiDeck Setup** can check GitHub Releases. Applying an update restarts only after you confirm.

## Shortcuts

| Action | Keys |
| --- | --- |
| Add a project | `Ctrl+Shift+O` |
| New terminal | `` Ctrl+` `` |
| Close the active tab | `Ctrl+Shift+W` |
| Next / previous tab | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Save the active file | `Ctrl+S` |
| Show or hide the sidebar | `Ctrl+Shift+B` |
| Show or hide the checklist | `Ctrl+Shift+L` |
| Settings | `Ctrl+,` |
| Copy / paste in the terminal | `Ctrl+Shift+C` / `Ctrl+Shift+V` |

Settings can replace these. Explorer, diff, and checklist keys stay fixed.

## Pane grid

Drag a tab from the tab strip or a pane title over the pane area and a snap overlay appears. It previews the result before you let go, and releasing does exactly what the preview shows. Releasing outside the pane area does nothing.

- Drop a tab on the middle of a pane to swap them, or to replace the pane's tab if it was hidden. This is the default, and it follows the pointer at once. With a pane title focused, `Alt+Arrow` moves it.
- Rest a tab toward the left, right, top, or bottom of any pane to open a column or row on that side. Where the pointer leans in the pane decides it: toward the left or right is a column, toward the top or bottom is a row, and in a corner the side you are deeper in wins. Beside two panes, the gap between them is the same place from either one.
- Tabs that are already open move the same way. Drag a pane's title below another pane and they stack; the row or column it left closes up, so no terminal is added or lost.
- Rest a tab on a layout in the snap tray at the top of the pane area to switch to that layout with the tab in the cell you chose.

A side is offered only when the grid can take it: up to three across, three down, and four panes. In a full grid the whole pane is the target. The sides and the tray take over only after the pointer rests in them for a moment, so dragging across them never rearranges anything. Once one has taken over it reaches a little past its edge, so a small drift keeps your choice, and moving between tray layouts responds at once. Until then the pane under the pointer is the target.

The layout button at the right of the tab strip reaches the same layouts from the keyboard: one pane, two or three columns, two or three rows, or a 2 × 2 grid. Any layout change opens a terminal in each pane no tab fills. Tabs that do not fit stay open in the tab strip and keep running; dragging never closes a terminal.

- Drag a divider to resize the panes beside it. With a divider focused, the arrow keys nudge it and `Home` makes the panes equal. Double-clicking a divider does the same.
- With several panes, the next and previous tab shortcuts move between panes.
- Each project saves its own layout and pane sizes.

## Themes

Black, Light, Graphite, Charcoal, Flint, Stone, Slate, Moss, Kiln, and Iris. Pick one beside **New terminal**. Open files, diffs, and terminals update immediately.

## Contributing

Pull requests are not being accepted right now. There is no time to review and manage them.

MIT
