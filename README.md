# Tic-Tac-Toe: one game, two interfaces, two languages

A learning project that compares **Rust** and **Go** on the same task: a
tic-tac-toe app that is equally playable from a **rich terminal UI** and a
**local web page**, *at the same time*, on one shared live game.

| Implementation | Status | TUI | Web |
|---|---|---|---|
| [`rust/`](rust/) | ✅ done | ratatui + crossterm | axum + WebSocket |
| `go/` | planned | Bubble Tea + Lip Gloss | net/http + WebSocket |

Both speak the same [wire protocol](docs/protocol.md), so either server works
with the same browser page.

## Architecture

```
            ┌──────────────────────────┐
            │  core: rules + minimax   │   pure, no I/O, exhaustively tested
            └────────────┬─────────────┘
            ┌────────────▼─────────────┐
            │  hub: single owner of    │   commands in (mpsc)
            │  game state              │   state out (watch / broadcast)
            └──────┬────────────┬──────┘
          in-process│            │WebSocket (127.0.0.1 only)
            ┌──────▼─────┐  ┌───▼──────────┐
            │    TUI     │  │ browser page │
            └────────────┘  └──────────────┘
```

Only the hub mutates state, so a move made in the terminal shows up in the
browser instantly (and vice versa) with no locks or races.

## Rust quick start

```sh
cd rust
cargo run --release              # TUI + web at http://127.0.0.1:8787
cargo run --release -- --no-web  # TUI only
cargo run --release -- serve     # web only (Ctrl-C to stop)
cargo run --release -- --ai off  # two humans (also: --ai x, --ai o [default])
cargo test                       # core, hub, end-to-end WebSocket, TUI render tests
```

TUI keys: arrows/`hjkl` + Enter, `1`–`9`, mouse hover/click, `n` new game,
`a` cycle opponent, `t` cycle theme, `?` help, `q` quit.

The binary is self-contained (web assets are embedded): copy
`rust/target/release/ttt` anywhere and run it.

## Security notes

This is a local, unauthenticated game server, so it is locked to the machine:

- Listens on `127.0.0.1` only and refuses to start on any other address.
- `Host` header allowlist → defeats DNS-rebinding attacks.
- WebSocket `Origin` allowlist → other websites open in your browser cannot
  connect to the game (cross-site WebSocket hijacking).
- Typed JSON parsing, 4 KiB message cap, 20 msg/s per-client limit.
- Strict CSP; the page never uses `innerHTML`.
- `unsafe_code = "forbid"` across the Rust workspace.
