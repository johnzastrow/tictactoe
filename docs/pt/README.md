# pt: projects & tasks, four ways

The second Rust-vs-Go experiment in this repo. Tic-tac-toe compared the
languages on a tiny shared state; **pt** asks a harder question: *how rich
can a terminal UI get next to a browser UI built on the same data?* Each
language gets a TUI **and** a WebAssembly web client, all on one shared
backend.

|  | Rust | Go |
|---|---|---|
| TUI | ratatui 0.30 + crossterm | Bubble Tea v2 + Lip Gloss v2 |
| Web client | Leptos 0.8 (CSR) → WASM, built with trunk | `GOOS=js GOARCH=wasm` + a hand-written DOM kit (no framework) |
| Server | axum 0.8 + rusqlite | net/http + coder/websocket + modernc.org/sqlite |
| Shared logic | `pt-core` crate, compiled into server, TUI **and** WASM | `internal/pt/core` package, the same three ways |

Every client works with every server, and both servers can open the same
database file (one at a time): the same wire protocol
([protocol.md](protocol.md)), the same schema ([schema.sql](schema.sql)),
the same password hashes and the same undo records.

**New here? Read the [usage guide](USAGE.md)**: running it, every view,
the filter language, editing, the full keyboard reference and troubleshooting.

Specs: [PLAN.md](PLAN.md) (decisions and architecture) ·
[protocol.md](protocol.md) · [schema.sql](schema.sql) · [ui-spec.md](ui-spec.md)
(views, controls and keymap that all four front ends implement) ·
[ORIGINAL-PROMPT.md](ORIGINAL-PROMPT.md) (the request and design interview
that started it, verbatim).

![New project dialog in all four clients: Rust left, Go right; web top, terminal bottom](screenshots/compare/10-new-project.png)

*The New project dialog in all four clients: **Rust** on the left, **Go** on
the right; **web** on top, **terminal** below.*
**[Screenshots and recordings](screenshots/README.md)**: every form control in
all four clients, as stills and recordings, Rust vs Go and web vs terminal.

## Quick start

```sh
# Rust (needs: rustup target add wasm32-unknown-unknown; brew install trunk)
cd rust
make pt                                   # web client + release binary
./target/release/pt seed --db pt.db       # demo data; prints the password
./target/release/pt serve --db pt.db      # web client at http://127.0.0.1:8788
./target/release/pt tui --user alice      # in another terminal (or several)

# Go (needs only Go)
cd go
make pt
./bin/pt seed --db pt.db
./bin/pt serve --db pt.db
./bin/pt tui --user alice
```

Demo users: `admin` (admin), `alice` … `heidi` (members), `victor`
(read-only viewer), all with the seed password. Run several TUIs and
browser tabs as different users to see live edits, presence markers and
conflict handling.

### Serving to other machines

The server binds to `127.0.0.1` by default and refuses a non-loopback
address without TLS:

```sh
# LAN, with a certificate from a CA (e.g. mkcert; rustls rejects a self-signed
# certificate that is its own CA):
pt serve --bind 0.0.0.0:8788 --tls-cert host.pem --tls-key host-key.pem
pt tui --server https://host:8788 --ca-cert "$(mkcert -CAROOT)/rootCA.pem"

# Tailscale / VPN (already encrypted):
pt serve --bind 100.x.y.z:8788 --allow-plaintext
```

Full instructions, including LAN setup and backups, are in
[USAGE.md](USAGE.md). Other commands: `pt user add <name> --role admin|member|viewer`,
`pt user passwd <name>`, `pt user list`, `pt logout --server URL`.
Passwords come from `PT_PASSWORD` or a no-echo prompt.

## What the UI does

The same in all four front ends (details in [ui-spec.md](ui-spec.md)):

- **Nine views**: Projects (with a task grid of the selected project below),
  Tasks (grouped by project), Board (kanban), Timeline (Gantt), Dashboard,
  Activity, Templates, Fields (custom fields), Users & Programs.
- **An editable data grid**: frozen columns, multi-column sort, collapsible
  grouping with subtotals, a totals row, in-cell progress and burn bars,
  marked-row selection, copy/paste of TSV blocks, fill-down (^D), column
  resize/reorder/hide.
- **Smart editors**: a fuzzy combobox for people/programs/projects, a
  calendar date picker that reads `fri`, `+3d` or `eom`, chips for tags and
  dependencies, a phase field that suggests the project template's phases.
- **Cascading dialogs**: New Project (Program ▸ suggested next code ▸
  Template with its task count and hours ▸ dates and budget pre-filled) and
  New Task (Program ▸ Project ▸ Phase ▸ …).
- **A query bar**: `status:active @me due<+14d #q4 -health:green`, with
  autocomplete and errors underlined as you type; all filtering is local, so
  it is instant.
- **Keyboard first, never keyboard only**: conventional keys, a nano-style
  hint bar for the current context, a menu bar (F10), a command palette
  (^K) and searchable help (F1). Every action also works with the mouse,
  including dragging cards, Gantt bars and column borders.
- **Live collaboration**: changes appear everywhere instantly (changed rows
  flash), coloured presence markers show where other users are, and
  conflicting edits are flagged in the cell instead of silently lost.
  Undo/redo (^Z/^Y) works on the server, per user.
- **Four themes**; the TUIs use truecolor where the terminal supports it and
  fall back to the 256-colour palette (macOS Terminal, older VTE).

## How it works

```
 ┌──────────── pt serve (Rust or Go) ─────────────┐
 │  SQLite (WAL)  ◀── only the hub writes          │
 │  hub (actor): validates, applies, records       │
 │  activity for undo, broadcasts changes          │
 │  HTTP: /api/login /api/me /api/logout /health   │
 │  WS:   /ws  ·  static: embedded WASM client     │
 └──────────┬───────────────────────┬──────────────┘
            │ ws / wss              │ ws / wss
   ┌────────┴────────┐     ┌────────┴────────┐
   │ pt tui (Rust/Go)│     │ browser (WASM)  │   …any number, anywhere
   │ local replica   │     │ local replica   │
   └─────────────────┘     └─────────────────┘
```

- **One writer.** As in tic-tac-toe, a single hub owns the database and
  applies commands in order. There are no locks and no write races.
- **Snapshot, then deltas.** On connect a client gets the whole dataset
  (about 1 MB of JSON for 50 projects and 1,000 tasks); after that the
  server sends only changed rows. Clients filter, sort, group and compute
  metrics on their own replica, so the UI never waits on the network.
- **Per-field optimistic concurrency.** Every edit carries the value the
  user saw (`base`). If someone changed that field since, the edit is
  skipped and reported as a conflict on that cell; edits to *other* fields
  of the same row still go through.
- **Undo as data.** Each change is logged with its old and new value (or the
  whole deleted row and its tasks). Undo replays that log backwards, with the
  same conflict check, so you can't undo over someone else's later change.
- **One core per language, three targets.** The grid model (columns, cell
  styles, sorting, grouping, autocomplete choices, parsing of typed or
  pasted text) lives in the core, so the TUI and the web grid of a language
  can't disagree about a value.

## Rust vs Go: what pt showed

Measured on an Apple Silicon Mac, October 2026.

| | Rust | Go |
|---|---|---|
| Clean release build (server + TUI binary) | 60 s (LTO) | **6 s** |
| WASM build | 58 s (trunk, opt-level z) | **1.6 s** |
| Release binary (incl. embedded web client) | **13.3 MB** | 21.1 MB |
| WASM bundle on the wire (gzip) | **0.7 MB** (1.7 MB raw) | 1.5 MB (5.3 MB raw) |
| Lines, core / server / TUI / web | 3.6k / 3.1k / 8.8k / 8.5k | 4.9k / 3.7k / 9.3k / 8.3k |
| Web UI model | Leptos fine-grained signals; the framework does the DOM updates | hand-written DOM kit: pooled rows, cached cell writes, event delegation |
| WASM pitfalls hit | callbacks firing after a component was disposed | `js.Func` lifetimes released too early; one panic in a callback killed the whole program until every callback recovered |
| TUI model | immediate mode; you own the loop and widget state | Elm architecture; `View` is pure, so mouse hit-testing reuses a pure layout function |

Both languages could build everything. The gap was in the browser: Leptos
gave Rust a real component framework and a bundle under a megabyte, while
Go needed its DOM handling written by hand and ships a runtime three times
the size. On the server and in the terminal, the two were close in effort
and features. Go built 10× faster; Rust's binaries were smaller.

## TUI vs web: how far can a terminal go?

Further than expected: every view, control and dialog in the spec exists in
both TUIs, including drag-and-drop on the board, draggable Gantt bars,
column resizing with the mouse, a calendar date picker and fuzzy
autocomplete. The real limits:

| Area | Terminal | Browser |
|---|---|---|
| Layout | fixed character cells; dense grids suit it well | proportional text, smooth scrolling, hover states |
| Gantt dependencies | markers (`◂`) on blocked bars | SVG arrows between bars |
| Keys | some Ctrl combinations can't be told apart (^I = Tab, ^M = Enter); Alt needs "Option as Meta" in macOS Terminal | the browser reserves ^N, ^T, ^W, so the spec lists alternatives |
| Clipboard | OSC 52 works in Ghostty and Windows Terminal, not in macOS Terminal or XFCE | the Clipboard API needs HTTPS or localhost |
| Colour | truecolor in most terminals; a 256-colour fallback elsewhere | anything |
| Mouse | works, but hover detail is limited | everything |

## Tests

| Suite | Command |
|---|---|
| Rust core, server, TUI (unit + render) | `cd rust && cargo test` |
| Protocol conformance (17 end-to-end tests, any server) | `PT_CONFORM_URL=http://127.0.0.1:8788 PT_CONFORM_PASSWORD=<seed pw> cargo test -p pt-server --test conformance` |
| Go everything | `cd go && go vet ./... && go test -race ./...` |
| Rust web (Playwright, Chrome) | `rust/crates/pt-web/e2e/run_all.sh` |
| Go web (Playwright, Chrome) | `go/internal/pt/webapp/e2e/run_all.sh` |

The conformance suite passes against both servers, including each server
running on a database seeded by the other. Both seeders produce identical
data.

## Security notes

The profile is "demo, secure by default where cheap":

- Accounts with Argon2id password hashes (OWASP parameters); sessions are
  random 256-bit tokens of which only the SHA-256 is stored; 14-day expiry.
- Browser sessions use an `HttpOnly; SameSite=Strict` cookie (`Secure` under
  TLS); TUIs send a bearer token and can cache it in a `0600` file.
- Same-origin checks on login, logout and the WebSocket upgrade; failed-login
  throttling per user and per IP.
- Roles enforced on the server for every write: viewers are read-only,
  members edit projects and tasks, admins manage users, programs, templates
  and custom fields.
- All SQL is parameterised; table and column names only ever come from a
  fixed list in the code.
- Strict CSP (no inline script except hashed loaders, no `eval`); neither web
  client uses `innerHTML`.
- Loopback-only by default; other addresses require TLS or an explicit
  `--allow-plaintext`.
- Message size and rate limits on the WebSocket; slow clients are dropped
  rather than slowing everyone down.

Not done (out of scope for a demo): password reset by email, MFA, audit
export, and running behind a reverse proxy (the IP throttle uses the socket
address, not `X-Forwarded-For`).
