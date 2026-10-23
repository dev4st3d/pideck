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
- A pane grid of one to four panes. Drag tabs between panes and drag the dividers to resize them.
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

Use the layout button at the right of the tab strip to choose one pane, two or three columns, two or three rows, or a 2 × 2 grid. A new layout opens a terminal in each empty pane. Tabs that do not fit stay open in the tab strip and keep running.

- Drag a tab or a pane title onto another pane to swap them. With a pane title focused, `Alt+Arrow` moves it.
- Drag a divider to resize the panes beside it. With a divider focused, the arrow keys nudge it and `Home` makes the panes equal. Double-clicking a divider does the same.
- With several panes, the next and previous tab shortcuts move between panes.
- Each project saves its own layout and pane sizes.

## Themes

Black, Light, Graphite, Charcoal, Flint, Stone, Slate, Moss, Kiln, and Iris. Pick one beside **New terminal**. Open files, diffs, and terminals update immediately.

## Contributing

Pull requests are not being accepted right now. There is no time to review and manage them.

MIT
