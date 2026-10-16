# Tic-Tac-Toe: one game, two interfaces, two languages

A learning project that compares **Rust** and **Go** on the same task: a
tic-tac-toe app that is equally playable from a **rich terminal UI** and a
**local web page**, *at the same time*, on one shared live game.

| Implementation | Status | TUI | Web |
|---|---|---|---|
| [`rust/`](rust/) | ✅ done | ratatui + crossterm | axum + WebSocket |
| [`go/`](go/) | ✅ done | Bubble Tea v2 + Lip Gloss v2 | net/http + coder/websocket |

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

## Go quick start

```sh
cd go
go run ./cmd/ttt                 # TUI + web at http://127.0.0.1:8787
go run ./cmd/ttt -no-web         # TUI only
go run ./cmd/ttt serve           # web only (Ctrl-C to stop)
go run ./cmd/ttt -ai off         # two humans (also: -ai x, -ai o [default])
go test -race ./...              # core, hub, end-to-end WebSocket, TUI render tests

# Cross-compile from any OS, no extra toolchains needed:
GOOS=windows GOARCH=amd64 go build -o bin/ttt.exe ./cmd/ttt
```

Same keys, themes, and web page as the Rust version.

## Rust vs Go: what building this showed

Measured on an Apple Silicon Mac (October 2026).

| | Rust | Go |
|---|---|---|
| TUI model | ratatui: immediate mode, you own the event loop and widget state | Bubble Tea: Elm architecture (`Init`/`Update`/`View`), framework owns the loop |
| Concurrency | tokio tasks + `mpsc`/`watch`/`oneshot` channels | goroutines + channels; hub commands are closures |
| Release binary | **2.4 MB** (LTO, stripped) | 7.4 MB (stripped) |
| Clean release build | ~26 s | **~3.5 s** |
| Cross-compiling | needs `rustup target add`, plus a linker/SDK per target (or `cross` / `cargo-zigbuild`) | **`GOOS=… go build`, nothing else** |
| Source lines (excluding tests) | ~1,850 | ~1,530 |
| Safety net | compiler rejects data races; `unsafe` forbidden | `go test -race` catches races at test time |
| Mouse hit-testing | renderer records cell rects as it draws | pure `layoutFor(w,h)` shared by `View` and `Update` (Elm: `View` can't mutate) |
| Overlays | `Clear` widget + render on top | Lip Gloss v2 layer compositor |

Both share `docs/protocol.md` and a byte-identical web page (a Go test fails
if the two copies drift).

## Security notes

This is a local, unauthenticated game server, so it is locked to the machine:

- Listens on `127.0.0.1` only and refuses to start on any other address.
- `Host` header allowlist → defeats DNS-rebinding attacks.
- WebSocket `Origin` allowlist → other websites open in your browser cannot
  connect to the game (cross-site WebSocket hijacking).
- Typed JSON parsing, 4 KiB message cap, 20 msg/s per-client limit.
- Strict CSP; the page never uses `innerHTML`.
- `unsafe_code = "forbid"` across the Rust workspace; Go tests run with `-race`.
