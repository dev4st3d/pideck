# Zeod

A native workspace for your project folders, files, Git, and terminals. Written in Rust with GPUI, for Windows.

![File tree and an open source file on the Stone theme](assets/screenshots/stone.png)

## Download

Grab `Zeod-win-Setup.exe` from the [latest release](https://github.com/dev4st3d/zeod/releases/latest) and run it.

The installer isn't code-signed yet, so Windows SmartScreen may warn before it starts. Choose **More info**, then **Run anyway**.

Installed builds check GitHub Releases for updates, and restart only after you confirm.

## Features

- **Several projects at once.** Each project folder keeps its own files, Git state, and terminals.
- **One tab strip.** Edit text, view images, and review diffs next to your terminals.
- **Git built in.** Stage, commit, push, and browse history, with unified and split diffs.
- **Terminals that stay put.** They start in the project directory and keep running when you switch tabs or projects.
- **Panes.** Show one to four at once and rearrange them by dragging.
- **A checklist per project** for small to-dos.
- **Ten themes** and shortcuts you can change in Settings.

Your theme, font, shortcuts, and layout stay on your machine. Zeod has no telemetry or analytics.

![Git history and a split diff on the Flint theme](assets/screenshots/flint.png)

![A project terminal on the Light theme](assets/screenshots/light.png)

## Panes

Drag a tab, or a pane's title, over the pane area and an overlay previews what releasing will do. Releasing outside the pane area cancels.

- **Middle of a pane:** swap tabs. With a pane title focused, `Alt+Arrow` moves it.
- **Near a side:** open a column or row there.
- **A layout in the snap tray** at the top of the pane area: switch to that layout.
- **A divider:** drag to resize. With one focused, the arrow keys nudge it, and `Home` or a double-click evens out the panes.

The layout button at the right of the tab strip offers the same layouts from the keyboard. Dragging never closes a terminal, and each project remembers its own layout and pane sizes.

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

## Themes

Black, Light, Graphite, Charcoal, Flint, Stone, Slate, Moss, Kiln, and Iris. Pick one beside **New terminal**, and open files, diffs, and terminals update immediately.

## Build from source

You need Windows and stable Rust 1.88 or newer.

```powershell
cargo run --locked
```

For an optimized build, use `cargo build --release --locked`.

## Contributing

Zeod is a small one-person project, and I can't review pull requests properly right now, so they're closed for the moment. Bug reports and ideas are very welcome in [Issues](https://github.com/dev4st3d/zeod/issues). Thanks for understanding!

## License

MIT. See [LICENSE](LICENSE).
