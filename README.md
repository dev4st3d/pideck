# Pi GUI

Native desktop shell for the pi coding harness. Rust + GPUI 0.2.2.

## Run

```powershell
cargo run
```

If Cargo is not on `PATH` on Windows:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run
```

## Concept UI

The current build is an interactive three-region shell backed by placeholder data. Tabs, selection, run status, and the demo send action work locally; harness RPC and composer text editing are not connected yet.

| Region | Contents |
|---|---|
| Title bar | Session, model, thinking level, cost, and run status |
| Sidebar | Sessions / Skills / Extensions tabs |
| Conversation | Message stream and tool calls |
| Inspector | Context, tasks, subagents, and queue |
| Composer | Input preview + demo send action |

The interface uses Switzer for body text, Tanker for the wordmark, and Cascadia Mono for data.

## Layout

- `src/app.rs` - window bootstrap and font registration
- `src/fonts.rs` - embedded Tanker and Switzer registration
- `src/theme.rs` - shared visual tokens
- `src/state.rs` - UI-independent placeholder domain
- `src/views/root.rs` - shell composition
- `src/views/pier/` - conversation, sidebar, and inspector presentation
