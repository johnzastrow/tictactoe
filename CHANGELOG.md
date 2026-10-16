# Changelog

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
