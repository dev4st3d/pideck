# Zeod

A native workspace for your project folders, files, Git, and terminals. Written in Rust with GPUI, for Windows.

![File tree and an open source file on the Stone theme](assets/screenshots/stone.png)

## Download

Grab `Zeod-win-Setup.exe` from the [latest release](https://github.com/dev4st3d/zeod/releases/latest) and run it.

The installer isn't code-signed yet, so Windows SmartScreen may warn before it starts. Choose **More info**, then **Run anyway**.

Installed builds check GitHub Releases for updates. Zeod restarts to apply one only after you confirm.

## What you get

- **Several projects at once.** Each project folder keeps its own files, Git state, and terminals.
- **One tab strip for everything.** Edit text, view images, and review diffs next to your terminals.
- **Git without leaving the app.** Stage, commit, push, and browse history, with unified and split diffs.
- **Terminals that stay put.** They start in the project directory and keep running when you switch tabs or projects.
- **A pane grid.** Show one to four panes, drag tabs to rearrange them, and drag the dividers to resize.
- **A checklist per project.** Keep small to-dos next to the work they belong to.
- **Ten themes** and shortcuts you can change in Settings.

Your theme, font, shortcuts, and layout stay on your machine. Zeod has no telemetry or analytics.

![Git history and a split diff on the Flint theme](assets/screenshots/flint.png)

![A project terminal on the Light theme](assets/screenshots/light.png)

## Build from source

You need Windows and stable Rust 1.88 or newer.

```powershell
cargo run --locked
```

For an optimized build:

```powershell
cargo build --release --locked
```

## Shortcuts

| Action | Keys |
| --- | --- |
| Add a project | `Ctrl+Shift+O` |
| New terminal | `Ctrl+Shift+T` or `` Ctrl+` `` |
| Close the active tab | `Ctrl+Shift+W` |
| Next / previous tab | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Save the active file | `Ctrl+S` |
| Show or hide the sidebar | `Ctrl+Shift+B` |
| Show or hide the checklist | `Ctrl+Shift+L` |
| Settings | `Ctrl+,` |
| Copy / paste in the terminal | `Ctrl+Shift+C` / `Ctrl+Shift+V` |

Settings can replace the main shortcuts. Explorer, diff, and checklist keys stay fixed.

## Pane grid

Drag a tab from the tab strip, or a pane's title, over the pane area. A snap overlay previews the result, and releasing does exactly what it shows. Releasing outside the pane area does nothing.

- **Middle of a pane:** swap the two tabs, or replace the pane's tab if it was hidden. This is the default. With a pane title focused, `Alt+Arrow` moves it.
- **Near a side:** open a column (left or right) or a row (top or bottom) there. In a corner, the side you are deeper in wins. Between two panes, the gap is the same target from either one.
- **Snap tray:** rest the tab on a layout at the top of the pane area to switch to it, with the tab in the cell you chose.
- **Tabs already open** move the same way. Drag a pane's title below another pane and they stack; the row or column it left closes up, so no terminal is added or lost.

The grid takes up to three columns, three rows, and four panes. A side is offered only when the grid can take it, and in a full grid the whole pane is the target. Sides and the tray take over only after the pointer rests in them for a moment, so dragging across them never rearranges anything. Once one is active it reaches a little past its edge, so a small drift keeps your choice.

The layout button at the right of the tab strip offers the same layouts from the keyboard: one pane, two or three columns, two or three rows, or a 2 × 2 grid. A layout change opens a terminal in each pane no tab fills. Tabs that do not fit stay open in the tab strip and keep running. Dragging never closes a terminal.

- Drag a divider to resize the panes beside it. With a divider focused, the arrow keys nudge it and `Home` makes the panes equal. Double-clicking a divider does the same.
- With several panes, the next and previous tab shortcuts move between panes.
- Each project saves its own layout and pane sizes.

## Themes

Black, Light, Graphite, Charcoal, Flint, Stone, Slate, Moss, Kiln, and Iris. Pick one beside **New terminal**. Open files, diffs, and terminals update immediately.

## Contributing

Zeod is a small one-person project, and I can't review pull requests properly right now, so they're closed for the moment. Bug reports and ideas are very welcome in [Issues](https://github.com/dev4st3d/zeod/issues). Thanks for understanding!

## License

MIT. See [LICENSE](LICENSE).
