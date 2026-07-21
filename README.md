# Pi GUI

A small native desktop application built with Rust and [GPUI](https://www.gpui.rs/).

## Run

```powershell
cargo run
```

If Cargo is not on `PATH` on Windows:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run
```

## Project layout

- `src/app.rs` starts GPUI and creates the main window.
- `src/state.rs` contains UI-independent application state.
- `src/theme.rs` defines shared visual tokens.
- `src/views/root.rs` renders the root view and handles interactions.

