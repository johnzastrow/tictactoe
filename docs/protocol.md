# Tic-tac-toe wire protocol (v1)

Both the Rust and Go implementations speak exactly this protocol, so the same
web page works against either server.

## Transport

- HTTP server bound to **127.0.0.1 only** (default port `8787`).
- WebSocket endpoint: `GET /ws`, text frames carrying one JSON object each.
- Max inbound message size: 4 KiB. Clients sending more than 20 messages per
  second are disconnected.
- Requests whose `Host` header is not `127.0.0.1:<port>` or `localhost:<port>`
  are rejected (DNS-rebinding defense).
- WebSocket upgrades with an `Origin` header other than
  `http://127.0.0.1:<port>` / `http://localhost:<port>` are rejected with 403
  (cross-site WebSocket hijacking defense). A missing `Origin` (non-browser
  local tools) is allowed.

## Cells

Cells are indexed `0`–`8`, row-major:

```
 0 | 1 | 2
 3 | 4 | 5
 6 | 7 | 8
```

## Client → server

| Message | Meaning |
|---|---|
| `{"type":"move","cell":4}` | Play the current player's mark in cell 4. |
| `{"type":"new_game"}` | Abandon the current game and start a new one. |
| `{"type":"set_ai","player":"O"}` | AI plays O. `"X"` = AI plays X, `null` = two humans. |
| `{"type":"set_difficulty","difficulty":"easy"}` | AI level: `"easy"`, `"medium"` or `"hard"`. Applies from the AI's next move. |

Unknown or malformed messages produce an `error` reply; the connection stays open.

## Server → client

### `state` (broadcast to every client after any change, and on connect)

```json
{
  "type": "state",
  "board": ["X", null, null, null, "O", null, null, null, null],
  "turn": "X",
  "status": {"kind": "in_progress"},
  "history": [0, 4],
  "score": {"x": 2, "o": 1, "draws": 3},
  "ai": "O",
  "difficulty": "medium",
  "web_clients": 1,
  "game_no": 7
}
```

`status` is one of:

- `{"kind":"in_progress"}`
- `{"kind":"won","winner":"X","line":[0,4,8]}`
- `{"kind":"draw"}`

### `error` (sent only to the client that caused it)

```json
{"type": "error", "message": "cell 4 is already taken"}
```

## Rules enforced by the server

- X always moves first; `game_no` increments on every new game.
- When `ai` is set and it is the AI's turn, human moves are rejected and the
  AI replies after a short delay (~350 ms).
- Score is updated once when a game ends.
- AI levels: **easy** plays uniformly random legal moves; **medium** plays
  perfectly except for a random move 35% of the time; **hard** is full
  minimax and cannot be beaten. Default: medium.
