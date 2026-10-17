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

### What's unusual about it

Apps that offer both a terminal UI and a web UI usually take one of two routes:

1. **Stream the terminal to the browser** (ttyd, gotty, xterm.js). Cheap, but
   the "web UI" is just a terminal in a tab.
2. **Build two separate apps** that talk to a shared backend service or
   database. Native in both places, but now you run and deploy a server.

This project does something different: **one self-contained binary runs one
game, and a native TUI and native web page are equal peers on it, live.**
Click a cell in the browser and the terminal redraws instantly; press a key in
the terminal and every open browser tab updates. There is no separate server
to deploy, no database, and no locks.

None of the individual ideas are new. It is a deliberate combination of
three well-known patterns:

| Pattern | Where it shows up here |
|---|---|
| **Ports and adapters** (hexagonal architecture) | Game rules live in a pure core with no I/O; the TUI and the web page are interchangeable adapters. |
| **Actor model** | One "hub" task owns all state and processes commands one at a time from a mailbox. |
| **Unidirectional data flow** (Elm / Redux style) | Front ends send *intents* and render *snapshots*; they never change state themselves. Here it spans an in-process client *and* a network client. |

### The layers

```
 ┌───────────────────────────────── one process (ttt) ─────────────────────────────────┐
 │                                                                                     │
 │   ┌─────────────────────────────┐                                                   │
 │   │ core                        │  rules, win/draw detection, minimax AI            │
 │   │ pure functions, no I/O      │  the ONLY place a move is validated               │
 │   └──────────────▲──────────────┘                                                   │
 │                  │ calls                                                            │
 │   ┌──────────────┴──────────────┐                                                   │
 │   │ hub  (actor)                │  owns: game, score, AI settings, client count     │
 │   │ one task / goroutine        │◀──── commands (mailbox: one queue, in order)      │
 │   │                             │────▶ StateView snapshots (latest-wins broadcast)  │
 │   └───────▲─────────────┬───────┘                                                   │
 │   intents │             │ snapshots                                                 │
 │   ┌───────┴─────────────▼───────┐        ┌─────────────────────────────┐            │
 │   │ TUI adapter (in-process)    │        │ web adapter (HTTP + WS)     │            │
 │   │ HubHandle method calls      │        │ JSON intents ⇄ JSON state   │            │
 │   └─────────────────────────────┘        └──────────────▲──────────────┘            │
 │                                                         │ 127.0.0.1 only            │
 └─────────────────────────────────────────────────────────┼───────────────────────────┘
                                                           │ WebSocket
                                              ┌────────────┴────────────┐
                                              │ browser tab(s)          │  thin client:
                                              │ embedded HTML/JS/CSS    │  render + send
                                              └─────────────────────────┘
```

### The four rules that make it work

1. **The core is pure.** `play(cell)` validates and applies a move; `best_move`
   / `choose_move` pick AI moves. Even randomness is passed in as a parameter
   (`pick(n)`), so the core has no hidden state and every test is
   deterministic. Both UIs may *grey out* illegal cells for convenience, but
   only the core decides what is legal.

2. **Exactly one owner of state.** The hub is the only code that holds a
   mutable `Game`. Every change, whether from a key press, a mouse click, a
   browser, or the AI, arrives as a command in one queue and is applied in
   order. Two people clicking at the same moment simply queue; there is
   nothing to lock and no way to race.

3. **Intents in, snapshots out.** Front ends send intents (`move`,
   `new_game`, `set_ai`, `set_difficulty`) and receive a complete `StateView`
   after every change: board, turn, status and winning line, history, score,
   AI settings, connected-browser count. Sending the *whole* state instead of
   diffs means:
   - a browser that connects mid-game, or reconnects after a drop, is correct
     the moment its first message arrives;
   - the broadcast can be **latest-wins**: a slow client skips intermediate
     states instead of making the hub wait. Rendering the newest state is
     always correct, because no state depends on the one before it.

4. **Every front end is a peer.** The TUI gets no special access. It uses the
   same intents and renders the same `StateView` as the browser. The only
   difference is the transport:

   | Intent / data | TUI (in-process) | Browser (WebSocket JSON) |
   |---|---|---|
   | Play a cell | `hub.play(4)` | `{"type":"move","cell":4}` |
   | New game | `hub.new_game()` | `{"type":"new_game"}` |
   | Change opponent | `hub.set_ai(Some(O))` | `{"type":"set_ai","player":"O"}` |
   | Change AI level | `hub.set_difficulty(Hard)` | `{"type":"set_difficulty","difficulty":"hard"}` |
   | State updates | subscribe to the hub's channel | `{"type":"state", ...}` frames |
   | Rejected move | `Err(HubError)` returned to the caller | `{"type":"error"}` sent to that socket only |

### Life of a move

What happens when you click a cell in the browser while the AI plays O:

```
 browser          web adapter            hub (actor)                core        TUI
    │  {"move",4}       │                     │                       │          │
    │──────────────────▶│ parse (typed),      │                       │          │
    │                   │ rate-limit, size cap│                       │          │
    │                   │── Move{4, reply} ──▶│ AI's turn? → reject   │          │
    │                   │                     │── play(4) ───────────▶│ validate │
    │                   │                     │◀── Ok(InProgress) ────│ + apply  │
    │                   │◀─────── Ok ─────────│ schedule AiTurn{game_no, ply}    │
    │                   │                     │ publish StateView ──────────────▶│ redraw
    │◀── state frame ───│◀────────────────────│ (every subscriber gets it)       │
    │                   │                     │                                  │
    │                   │        ~350 ms later: AiTurn arrives in the SAME queue │
    │                   │                     │ still current? → choose_move ───▶│
    │◀── state frame ───│◀────────────────────│ publish ────────────────────────▶│ redraw
```

**How the AI fits in.** The AI is not a separate thread that changes the game
on a timer. When it is the AI's turn, the hub posts a delayed `AiTurn` command
*to its own mailbox*, stamped with the current `(game_no, ply)`. When the
command comes back through the queue, the hub checks whether that stamp still
matches. If someone started a new game, switched the opponent off, or the
position otherwise moved on in the meantime, the stale command is dropped.
The AI therefore follows the same single-owner, in-order path as every human
input. The delay is cosmetic, so the reply doesn't feel instant.

### Design decisions

| Decision | Why |
|---|---|
| Full snapshots, not diffs | Late joiners and reconnects are free; no ordering or replay logic on clients. The state is tiny (<300 bytes JSON). |
| Latest-wins broadcast (`watch` channel / 1-slot channel) | A frozen browser tab can't slow the game for everyone else. |
| AI as a self-addressed, stamped message | No locks, no timer threads mutating state, and stale moves are discarded instead of corrupting a new game. |
| Validation only in the core | One source of truth. A buggy or hostile client can only send intents that get rejected. |
| TUI attaches in-process, not over the network | No extra port, auth, or serialization for the local user; it still uses the same intent/snapshot contract. |
| Bind the port *before* starting the TUI | "Port in use" becomes a clean error message, not a crash with the terminal stuck in raw mode. |
| Web assets embedded in the binary | One file to copy and run on macOS, Linux, or Windows; nothing to install. |
| Server bound to `127.0.0.1` + Host/Origin allowlists | No authentication is needed because nothing off the machine, and no other website in your browser, can reach it. |

### How each language expresses the same design

| Concept | Rust | Go |
|---|---|---|
| Hub mailbox | `tokio::sync::mpsc` of a `Command` enum | `chan` of closures (`func(*state)`) run on the hub goroutine |
| Reply to a caller | `oneshot` channel per command | closure writes the result, then closes a `finished` channel |
| Snapshot broadcast | `tokio::sync::watch` (built-in latest-wins) | per-subscriber `chan StateView` of size 1: drain, then send |
| TUI listens for state | `tokio::select!` over terminal events, `watch.changed()`, and an animation tick | a Bubble Tea `Cmd` that blocks on the channel, returns a `stateMsg`, and is re-armed after each one |
| TUI sends intents | `await hub.play(cell)` inside the event handler | returns a `Cmd` that calls `hub.Play` off the UI loop (keeps `Update` pure) |
| WebSocket | axum `ws` | `github.com/coder/websocket` |
| Shutdown | oneshot signal → axum graceful shutdown | `context.Context` cancellation |

### Adding another front end

Because the hub only knows about intents and snapshots, a new interface needs
no changes to the core or the hub. Some examples:

- **A remote TUI** (`ttt join ws://…`): the same TUI, but its `HubHandle`
  sends WebSocket JSON instead of calling the hub directly.
- **A bot** or **automated tester** that speaks `docs/protocol.md`.
- **A native GUI or mobile client** on the same protocol.
- **A different server language.** The Rust and Go servers are
  interchangeable behind the same web page, which is how this repo compares
  the two languages.

### Code map

| Concept | Rust | Go |
|---|---|---|
| Rules + AI (pure core) | `rust/crates/ttt-core/src/lib.rs` | `go/internal/core/core.go` |
| Hub (actor) | `rust/crates/ttt-server/src/hub.rs` | `go/internal/hub/hub.go` |
| Web adapter + security | `rust/crates/ttt-server/src/web.rs` | `go/internal/web/web.go` |
| Web page (shared, identical) | `rust/crates/ttt-server/web/` | `go/internal/web/assets/` |
| TUI state + input | `rust/crates/ttt/src/app.rs` | `go/internal/tui/model.go` |
| TUI rendering | `rust/crates/ttt/src/ui.rs` | `go/internal/tui/view.go` |
| Wiring / CLI | `rust/crates/ttt/src/main.rs` | `go/cmd/ttt/main.go` |
| Wire protocol | `docs/protocol.md` | `docs/protocol.md` |

## The AI

No machine learning: the opponent is classic **minimax** game-tree search.
Tic-tac-toe is small enough (~550k positions) to search completely, so on
**hard** it plays perfectly and the best you can do is draw. **Medium** plays
perfectly but makes a random move 35% of the time; **easy** plays randomly.
The core takes the random source as a parameter, so it stays pure and tests
are deterministic.

## Rust quick start

```sh
cd rust
cargo run --release              # TUI + web at http://127.0.0.1:8787
cargo run --release -- --no-web  # TUI only
cargo run --release -- serve     # web only (Ctrl-C to stop)
cargo run --release -- --ai off  # two humans (also: --ai x, --ai o [default])
cargo run --release -- -d hard   # AI level: easy, medium [default], hard
cargo test                       # core, hub, end-to-end WebSocket, TUI render tests
```

TUI keys: arrows/`hjkl` + Enter, `1`–`9`, mouse hover/click, `n` new game,
`a` cycle opponent, `d` cycle AI level, `t` cycle theme, `?` help, `q` quit.

The binary is self-contained (web assets are embedded): copy
`rust/target/release/ttt` anywhere and run it.

## Go quick start

```sh
cd go
go run ./cmd/ttt                 # TUI + web at http://127.0.0.1:8787
go run ./cmd/ttt -no-web         # TUI only
go run ./cmd/ttt serve           # web only (Ctrl-C to stop)
go run ./cmd/ttt -ai off         # two humans (also: -ai x, -ai o [default])
go run ./cmd/ttt -difficulty hard # AI level: easy, medium [default], hard
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
