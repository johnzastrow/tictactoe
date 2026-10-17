# pt — Projects & Tasks: build plan

`pt` is the second experiment in this repo. Tic-tac-toe compared Rust and Go on
a *tiny* shared state; `pt` pushes both languages as far as they go on a
**data-dense, keyboard-first business app** with a TUI and a web UI that mirror
each other.

## Goals

1. **Show how rich a TUI can get** next to a browser UI built on the same data:
   editable data grids, autocomplete, cascading selects, popups, menus, charts,
   a Gantt chart, drag-and-drop, live presence.
2. **Minimise clicking without losing function**: every action has a keyboard
   path, discoverable through nano-style hint bars, a menu bar, a command
   palette and a help screen. The mouse is fully supported, never required.
3. **Fit a lot on screen**: dense grids, frozen columns, grouping with
   subtotals, master-detail panes, in-cell bars.
4. **One backend, many remote front ends**: one server owns one SQLite file;
   TUIs and browsers anywhere on the LAN or tailnet connect to it, live.
5. **Cross-language interchangeability**: the Rust TUI works against the Go
   server and vice versa, and both servers can open the same database file
   (one at a time).

## Decisions (from the design interview, 2026-10-17)

| Topic | Decision |
|---|---|
| Profile | Experiment / demo, secure by default where cheap: loopback bind by default, auth required, TLS when exposed, parameterized SQL only |
| Network | LAN and Tailscale/VPN |
| Identity | Real accounts: username + password (Argon2id), roles `admin` / `member` / `viewer` |
| Concurrent edits | Optimistic, **per field**: each edit carries the value the client last saw (`base`); a mismatch is a conflict marker in the cell |
| Interop | Rust ⇄ Go in every direction; one protocol (`docs/pt/protocol.md`), one schema (`docs/pt/schema.sql`) |
| Web front end | **WASM in both languages**: Leptos (CSR, built with trunk) for Rust; Go `GOOS=js GOARCH=wasm` with a small hand-written DOM kit (Go has no mature WASM UI framework, which is part of what we are measuring) |
| Project fields | code, name, program/client, owner, status, priority, health (RAG), start/due, budget (labor hours), % complete (computed), tags, notes |
| Task fields | No subtasks. Phase, status, priority, assignee, start/due, **estimate vs actual hours**, dependencies, tags, notes |
| Templates | Several templates; a new project picks one (cascading select). Editing a template does not touch existing projects |
| Custom fields | Yes, defined at runtime: text, number, date, enum |
| Seed data | `pt seed`: ~50 projects, 10–40 tasks each, 9 users, 8 programs, 5 templates |
| Keys | **Conventional** (arrows, Tab, Enter, Ctrl/F-keys), nano-style hint bar, Turbo-Vision-style menu bar. Not vim |
| Terminals | Windows Terminal, Ghostty, XFCE Terminal, macOS Terminal. Truecolor where supported, automatic 256-colour fallback |
| Mouse | Everything clickable; drag to resize columns, drag cards and Gantt bars, wheel scroll, right-click context menus |
| SQLite | `rusqlite` (bundled) for Rust; `modernc.org/sqlite` (pure Go, no cgo) for Go. WAL mode, schema embedded in the binary |

## Architecture

```
 ┌──────────── pt serve (Rust or Go) ─────────────┐
 │  store (SQLite, WAL)  ◀── only the hub touches │
 │        ▲                                       │
 │  hub (actor): validates, applies, versions,    │
 │  records activity (undo), broadcasts changes   │
 │        ▲ commands          │ changes / presence│
 │  HTTP: /api/login /api/me /api/logout /health  │
 │  WS:   /ws  (JSON frames, docs/pt/protocol.md) │
 │  static: embedded WASM web client              │
 └──────────┬──────────────────────┬──────────────┘
            │ wss/ws               │ wss/ws
   ┌────────┴────────┐    ┌────────┴────────┐
   │ pt tui (Rust/Go)│    │ browser (WASM)  │   …any number, anywhere
   │ local replica   │    │ local replica   │
   └─────────────────┘    └─────────────────┘
```

Same four rules as tic-tac-toe, adapted for a network:

1. **Pure core, shared by every tier of a language.** `pt-core` (Rust) and
   `internal/pt/core` (Go) hold the model types, field metadata and
   validation, the query language, the natural-language date parser, fuzzy
   matching, computed metrics, and the client-side **replica** that applies
   snapshots and change events. The server, the TUI and the WASM web client
   all compile the *same* core: one set of rules per language.
2. **Exactly one owner of state.** The server hub is the only writer to SQLite.
3. **Intents in, changes out.** Clients send `mutate` / `undo` / `redo` /
   `focus`. The server replies to the sender and broadcasts `changes` (full
   updated rows) to everyone. On connect or reconnect a client gets a full
   `snapshot` (about 1–2 MB of JSON for the seed data; trivial on a LAN).
4. **Every front end is a peer.** TUI and browser use the same protocol and
   the same core logic; neither gets special access.

Clients filter, sort, group and compute metrics **locally** from the
replica, so typing in the query bar or an autocomplete box never waits on the
network.

## Repository layout

```
docs/pt/PLAN.md          this file
docs/pt/protocol.md      wire protocol (normative)
docs/pt/schema.sql       SQLite schema (normative; both servers embed it)
docs/pt/ui-spec.md       views, controls, keymap (normative for all 4 UIs)

rust/crates/pt-core      model, validation, query, dates, fuzzy, replica (no I/O, wasm-safe)
rust/crates/pt-server    store + hub + axum HTTP/WS + auth + embedded web assets
rust/crates/pt-tui       ratatui front end + WebSocket client
rust/crates/pt-web       Leptos CSR front end (wasm32, built by trunk)
rust/crates/pt           the `pt` binary: serve / seed / user / tui / logout
rust/Makefile            `make web` (trunk → pt-server/web-dist), `make pt`, `make test`
go/Makefile              `make web` (wasm → internal/pt/web/dist), `make pt`, `make test`

go/internal/pt/core      same responsibilities as pt-core (also wasm-safe)
go/internal/pt/store     SQLite store (embeds a drift-checked copy of schema.sql)
go/internal/pt/auth      Argon2id PHC hashes (compatible with Rust), tokens
go/internal/pt/seed      demo data, identical to the Rust seeder
go/internal/pt/server    hub + net/http + coder/websocket
go/internal/pt/client    HTTP/WebSocket client and session cache used by the TUI
go/internal/pt/tui       Bubble Tea v2 front end
go/internal/pt/webapp    GOOS=js wasm front end + DOM kit (source, static files, e2e)
go/internal/pt/web       embed package: serves dist/, built by `make web`
go/cmd/pt                the `pt` binary
```

## CLI (identical in both languages)

```
pt serve  [--db pt.db] [--bind 127.0.0.1:8788] [--tls-cert F --tls-key F] [--allow-plaintext]
pt seed   [--db pt.db] [--password PW] [--force]
pt user   add <username> [--name "Full Name"] [--role admin|member|viewer]
pt user   passwd <username>
pt user   list
pt tui    [--server http://127.0.0.1:8788] [--user NAME] [--ca-cert F] [--theme NAME] [--colors 256|truecolor]
pt logout [--server URL]
```

Passwords come from `PT_PASSWORD` or a no-echo prompt, never from argv
(except `seed --password` for demos). Binding a non-loopback address
without TLS requires `--allow-plaintext` (meant for Tailscale, which
already encrypts).

## Status (2026-10-17)

All milestones M1–M5 are done in both languages; see [README.md](README.md)
for results and [USAGE.md](USAGE.md) for the user guide. The build ran in
the order below, with the two Rust front ends and the Go server built in
parallel, then the two Go front ends. Deviations from the UI spec are listed
in [ui-spec.md §10](ui-spec.md#10-implementation-notes).

## Milestones (each language)

| # | Milestone | Done when |
|---|---|---|
| M1 | Core, schema, store, hub, HTTP/WS, auth, seed, CLI | Conformance test passes against the server |
| M2 | TUI: login, shell (menu, tabs, hint bar, palette, help), Projects + Tasks grids with editing, query bar, master-detail | Edit a cell in TUI A, see it in TUI B |
| M3 | Web: same shell and grids | Edit in browser, see it in TUI |
| M4 | Board (kanban), Dashboard, Activity, Templates, Fields, Users views; record form; cascading New dialogs | All views reachable by keyboard |
| M5 | Timeline (Gantt), presence markers, conflict markers, undo/redo, copy/paste, fill-down | Cross-language matrix passes |

## Build order and parallelism

1. Shared contracts (this folder).
2. Rust `pt-core` + `pt-server` + `pt` CLI + a protocol conformance test client.
3. In parallel: Rust TUI · Rust web · Go core + server.
4. In parallel: Go TUI · Go web.
5. Cross-language matrix (each TUI against each server, each browser client
   against its server, both servers on one DB), README, CHANGELOG.

## Testing

- Core: unit tests (query parser, dates, fuzzy, validation, replica).
- Server: store/hub tests plus an end-to-end WebSocket conformance suite
  (login, snapshot, set, conflict, batch, undo/redo, authz, rate limits).
  The Rust suite (`pt-conform`) can target **any** URL, so it also checks the Go
  server.
- TUI: render tests on a fixed-size buffer, plus scripted runs in `tmux`.
- Web: Playwright (headless Chrome) scripts that log in, edit, and check
  that changes from another client arrive live.
