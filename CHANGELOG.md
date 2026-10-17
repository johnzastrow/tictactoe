# Changelog

## [0.4.0] - 2026-10-17

### Added
- **pt (projects & tasks)**, a second experiment: one SQLite-backed server
  that any number of TUI and browser clients connect to, in both languages.
  - Shared specs in `docs/pt/`: plan, wire protocol, schema, UI spec.
  - Rust: `pt-core` (shared by server, TUI and WASM), `pt-server` (axum,
    rusqlite, Argon2id sessions, per-field optimistic concurrency, undo/redo),
    `pt-tui` (ratatui), `pt-web` (Leptos → WASM, embedded), and the `pt` CLI
    (`serve`, `seed`, `user`, `tui`, `logout`).
  - Go: the same pieces with net/http, modernc.org/sqlite, Bubble Tea v2,
    and a `syscall/js` WASM client with a hand-written DOM kit.
  - Nine views (projects, tasks, kanban board, Gantt timeline, dashboard,
    activity, templates, custom fields, users), an editable data grid,
    fuzzy comboboxes, a natural-language date picker, cascading dialogs, a
    query language, live presence and conflict markers.
  - Protocol conformance suite that runs against either server; Playwright
    suites for both web clients.
- Both servers send precompressed (`.gz`) web assets to browsers that accept them.


## [0.3.1] - 2026-10-16

### Changed
- README: full architecture write-up: what is unusual about the design, the four
  rules (pure core, single state owner, intents in / snapshots out, peer front ends),
  life of a move, design decisions, Rust vs Go mapping, extension points, code map.

## [0.3.0] - 2026-10-16

### Added
- AI difficulty levels (easy / medium / hard) in both implementations: `set_difficulty`
  protocol message, `difficulty` field in state, `d` key in both TUIs, level selector on
  the web page, and `--difficulty` / `-difficulty` CLI flags. Default is medium.

### Changed
- Go module path is now `git.wharf.fluidgrid.org/jcz/tictactoe/go`.

## [0.2.0] - 2026-10-16

### Added
- Go implementation (`go/`): core rules + minimax, single-goroutine hub, net/http +
  coder/websocket server with the same Host/Origin/size/rate defences, and a
  Bubble Tea v2 / Lip Gloss v2 TUI with feature parity (mouse, themes, win flash,
  score bar, history, layered help popup).
- README comparison of the two implementations.

## [0.1.0] - 2026-10-16

### Added
- Rust implementation (`rust/`): pure core with unbeatable minimax AI, single-owner
  game hub, localhost web server with WebSocket live sync, and a ratatui TUI with
  mouse support, three colour themes, animated win highlight, score bar and move history.
- Shared wire protocol spec (`docs/protocol.md`).
