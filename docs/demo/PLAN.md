# demo — a control gallery: build plan

`demo` is the third app in this repo. [pt](../pt/README.md) showed how rich a
TUI can get, but its screens are busy: a control is always buried in a grid
with fifty other things going on, which makes each control hard to test,
demonstrate or record. **demo puts each control on its own tab**, alone,
with its own explanation, its own key list and a readout of its current
value.

It keeps pt's architecture and its onscreen help (menu bar, nano-style hint
bar, searchable help, command palette), in both languages.

## Decisions (2026-10-18)

| Topic | Decision |
|---|---|
| Profile | Demo, secure by default where cheap (as pt): loopback bind by default, TLS or `--allow-plaintext` for other addresses, same-origin checks, size and rate limits, parameterised SQL, strict CSP |
| Backend | Own small server per language: one hub owns a SQLite file holding the **current value of every control**; every client sees every change live |
| Identity | **Name only**, no passwords. A display name (for presence and "changed by") chosen at join. Nothing in demo is secret |
| Concurrency | Optimistic **per key**: each `set` carries the version the client last saw; a mismatch is reported as a conflict and the client shows the newer value |
| Interop | Rust ⇄ Go in every direction: one protocol ([protocol.md](protocol.md)), one schema ([schema.sql](schema.sql)), one built-in dataset |
| Front ends | Same four as pt: ratatui TUI, Bubble Tea v2 TUI, Leptos CSR web (WASM), Go `syscall/js` web (WASM) |
| Tabs | 16, one control each ([ui-spec.md](ui-spec.md) §4) |
| Help | Menu bar, hint bar, F1/^G searchable help, ^K palette, a per-tab "Keys" panel |

## Architecture

```
 ┌──────────── demo serve (Rust or Go) ───────────┐
 │  SQLite (WAL): one row per control key          │
 │  hub (actor): validates, versions, broadcasts   │
 │  HTTP: /api/health   WS: /ws   static web client│
 └──────────┬───────────────────────┬─────────────┘
            │ ws / wss              │ ws / wss
   ┌────────┴─────────┐    ┌────────┴────────┐
   │ demo tui (Rust/Go)│    │ browser (WASM)  │   …any number
   └──────────────────┘    └─────────────────┘
```

The same rules as pt:

1. **Pure core per language**, compiled into server, TUI and WASM client:
   key catalogue and validation, defaults, the built-in dataset (people,
   programs ▸ projects ▸ phases, tag suggestions), natural-date parsing,
   fuzzy matching, the client-side value store.
2. **One owner of state**: the hub is the only writer to SQLite.
3. **Intents in, changes out**: clients send `set` / `reset` / `focus` /
   `toast`; the server broadcasts `changed` to everyone.
4. **Every front end is a peer.**

## Repository layout

```
docs/demo/                    PLAN.md, protocol.md, schema.sql, ui-spec.md, README.md, USAGE.md, screenshots/

rust/crates/demo-core         keys, validation, defaults, dataset, dates, fuzzy, store (no I/O, wasm-safe)
rust/crates/demo-server       SQLite store + hub + axum HTTP/WS + embedded web assets (web-dist/)
rust/crates/demo-tui          ratatui front end + WebSocket client
rust/crates/demo-web          Leptos CSR front end (wasm32, trunk)
rust/crates/demo              the `demo` binary: serve / reset / tui

go/internal/demo/core         same as demo-core (wasm-safe)
go/internal/demo/store        SQLite store (embeds a drift-checked copy of schema.sql)
go/internal/demo/server       hub + net/http + coder/websocket
go/internal/demo/client       WebSocket client used by the TUI
go/internal/demo/tui          Bubble Tea v2 front end
go/internal/demo/webapp       GOOS=js wasm front end (source, static/, e2e/)
go/internal/demo/web          embed package serving dist/
go/cmd/demo                   the `demo` binary
```

Build: `cd rust && make demo` → `rust/target/release/demo`;
`cd go && make demo` → `go/bin/demo`.

## CLI (identical in both languages)

```
demo serve [--db demo.db] [--bind 127.0.0.1:8790] [--tls-cert F --tls-key F] [--allow-plaintext]
demo reset [--db demo.db]                       restore every control to its default
demo tui   [--server http://127.0.0.1:8790] [--name NAME] [--ca-cert F] [--theme NAME] [--colors 256|truecolor]
```

`demo tui` without `--name` shows a join form (server URL, name).

## Milestones (each language)

| # | Milestone | Done when |
|---|---|---|
| D1 | Core, schema, store, hub, HTTP/WS, CLI | Conformance suite passes against the server (both servers) |
| D2 | TUI: shell (menu, sidebar, hint bar, status line, palette, help) and all 16 tabs | Change a value in TUI A, see it in TUI B; render tests per tab |
| D3 | Web: same shell and tabs | Change in browser, see it in a TUI; Playwright suite |
| D4 | Cross-language matrix, docs, screenshots and recordings like pt's | Every client against every server; gallery generated |

## Testing

- Core: unit tests (validation of every key, dates, fuzzy, cascade rules).
- Server: store/hub tests and a Rust end-to-end conformance suite
  (`demo-server/tests/conformance.rs`, `DEMO_CONFORM_URL=…`) that also runs
  against the Go server.
- TUI: render tests of every tab on a fixed buffer, plus scripted runs.
- Web: Playwright (Chrome) scripts: join, use each tab, see a change from a
  second page arrive live.
