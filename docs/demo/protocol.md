# demo wire protocol (v1)

Normative for both servers (Rust, Go) and all four clients. Any client works
against any server. Values below are exact: both cores must produce the same
defaults, validate the same way and hold the same dataset.

## 1. Transport and security

| Item | Rule |
|---|---|
| Default bind | `127.0.0.1:8790`. A non-loopback bind requires TLS (`--tls-cert`/`--tls-key`) **or** `--allow-plaintext`. |
| HTTP | `GET /api/health` → `200 {"ok":true,"impl":"rust"\|"go","version":"0.5.0","protocol":1}`. `GET /*` serves the embedded web client. Nothing else. |
| WebSocket | `GET /ws?client=<id>&name=<name>`. `client` matches `[a-z0-9-]{1,24}` (`tui-rust`, `tui-go`, `web-rust`, `web-go`, `conform`). `name` (URL-encoded) matches `^[A-Za-z][A-Za-z0-9 _.-]{0,23}$` after trimming spaces. Bad either → `400`. |
| Auth | None: demo holds nothing secret. The name is a label, not an identity. |
| Origin check | If `/ws` carries an `Origin` header it must equal `http://<Host>` or `https://<Host>`, else `403`. |
| Limits | Text frames only, ≤ 256 KiB; token bucket 20 msg/s, burst 40; at most 64 open sockets (65th → `503`); `toast` at most 1 per 2 s per connection (excess dropped with an `ack` error). Exceeding the frame or rate limit closes the socket with code 1008. |
| Keepalive | Server pings every 30 s; `{"type":"ping"}` → `{"type":"pong"}`. |
| Headers | As pt: `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin`, `X-Frame-Options: DENY`, CSP `default-src 'self'; script-src 'self' 'wasm-unsafe-eval' <hashes of inline loaders>; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'`, HSTS under TLS. |

## 2. Server → client

On connect the server sends `welcome`, then `presence` (to everyone).

```jsonc
{"type":"welcome","protocol":1,"server":{"impl":"go","version":"0.5.0"},
 "you":{"conn":7,"name":"alice","color":"#e06c75"},
 "values":{"text.name":{"value":"Ada Lovelace","version":1,"by":"","at":"2026-10-18T09:00:00Z"}, …every key…}}

{"type":"changed","key":"select.status","value":"done","version":4,"by":"bob","conn":8,"at":"2026-10-18T09:03:11Z"}

{"type":"ack","req":12,"ok":true,"version":4}
{"type":"ack","req":13,"ok":false,"error":"due must be on or after start"}
{"type":"ack","req":14,"ok":false,"conflict":true,
 "current":{"value":"review","version":5,"by":"bob","at":"…"}}

{"type":"presence","peers":[{"conn":7,"name":"alice","color":"#e06c75","client":"tui-rust","tab":"date"}, …]}

{"type":"toast","text":"Lunch is here","by":"bob","conn":8}
{"type":"pong"}
{"type":"error","message":"bad message"}      // malformed JSON / unknown type; socket stays open
```

- `values` always contains **every** key in §5 (the store is initialised with
  defaults). `by` is `""` for a default never changed.
- `changed` goes to **everyone, including the sender**, after the `ack`.
- `presence` is sent to everyone on any connect, disconnect or `focus`; it
  includes the receiver.
- Colours: peer colour = `PEER_COLORS[(conn - 1) % 8]` with
  `PEER_COLORS = ["#e06c75","#61afef","#98c379","#e5c07b","#c678dd","#56b6c2","#d19a66","#f472b6"]`.
  `conn` starts at 1 per server run and increments.

## 3. Client → server

```jsonc
{"type":"set","req":12,"key":"select.status","base":3,"value":"done"}
{"type":"reset","req":15,"keys":["date.start","date.due"]}   // [] or omitted = all keys
{"type":"focus","tab":"date"}                                 // a tab id from ui-spec §4; no reply
{"type":"toast","req":16,"text":"Lunch is here"}              // 1–120 chars after trim
{"type":"ping"}
```

`set` semantics:
1. Unknown key → `ack ok:false error:"unknown key"`.
2. `base` ≠ the key's current version → `ack ok:false conflict:true current:{…}`; nothing changes.
   `"force":true` skips the check.
3. Value invalid (§5) → `ack ok:false error:"<message>"`; nothing changes.
4. Otherwise the server **normalises** the value (§5), stores it with
   `version+1`, `by` = sender's name, `at` = now (UTC, seconds, `Z`), acks with
   the new version, then broadcasts `changed`. A value equal to the current one
   still bumps the version (simple and predictable).
5. Cross-key rule: `date.due` ≥ `date.start` when both are set — checked
   against the *other* key's current value on each `set` of either.

`reset` sets each listed key (all keys when empty) to its default, bumping
versions, and broadcasts one `changed` per key; acked with `{"ok":true}`.
Unknown keys in the list → error, nothing reset. `toast` is broadcast to
everyone including the sender, then acked.

## 4. Built-in dataset (identical in both cores)

**People** (username, display name, team):

| username | name | team |
|---|---|---|
| alice | Alice Nguyen | Design |
| bob | Bob Okafor | Engineering |
| carol | Carol Diaz | Engineering |
| dave | Dave Kowalski | Product |
| erin | Erin Walsh | QA |
| frank | Frank Moreau | Operations |
| grace | Grace Liu | Engineering |
| heidi | Heidi Schmidt | Design |
| ivan | Ivan Petrov | Support |

**Programs ▸ projects ▸ phases** (order matters; it is display order):

| Program | Project | Phases |
|---|---|---|
| Northwind Retail | POS refresh | Discover, Build, Pilot, Rollout |
| Northwind Retail | Loyalty app | Design, Build, Launch |
| Northwind Retail | Store Wi-Fi | Survey, Install, Handover |
| Contoso Health | Patient portal | Discover, Design, Build, Test, Launch |
| Contoso Health | Records migration | Plan, Migrate, Verify |
| Fabrikam Energy | Smart meters | Pilot, Deploy, Monitor |
| Fabrikam Energy | Grid analytics | Data, Model, Report |
| Fabrikam Energy | Field app | Design, Build, Test |
| Tailspin Air | Crew scheduling | Define, Build, Test, Go-live |
| Tailspin Air | Baggage tracking | Pilot, Rollout |

**Tag suggestions** (tag, uses), ranked by uses then name:
urgent 12, frontend 9, backend 8, q4 7, design 6, bug 5, research 4, docs 3, security 3, perf 2.

**Statuses** (`select.status`, grid, kanban use subsets): `todo` To do `#9aa5b1`,
`in_progress` In progress `#61afef`, `blocked` Blocked `#e06c75`, `review` Review `#c678dd`, `done` Done `#98c379`.
**Priorities**: `low` Low `#7f848e`, `medium` Medium `#61afef`, `high` High `#e5c07b`, `critical` Critical `#e06c75`.

## 5. Keys, validation and defaults

Strings are trimmed of leading/trailing whitespace before validation unless
noted (textarea is not trimmed). Numbers are JSON numbers, finite. "date" means
`YYYY-MM-DD` and a real calendar date. Error messages are short English
phrases; their exact text is not normative.

| Key | Tab | Type and rule | Normalise | Default |
|---|---|---|---|---|
| `text.name` | text | string 1–80 chars | trim | `"Ada Lovelace"` |
| `text.email` | text | `""` or matches `^[^@\s]+@[^@\s]+\.[^@\s]+$`, ≤ 120 | trim, lowercase | `"ada@example.com"` |
| `text.code` | text | matches `^[A-Z]{3}-[0-9]{3}$` | trim, uppercase | `"ENG-042"` |
| `number.hours` | number | 0 ≤ n ≤ 1000 | round to 2 dp | `12.5` |
| `number.qty` | number | integer 0–99 | – | `3` |
| `number.percent` | number | integer 0–100 | – | `40` |
| `date.start` | date | date or null | – | `"2026-11-02"` |
| `date.due` | date | date or null; ≥ `date.start` if both set | – | `"2026-11-20"` |
| `select.status` | select | one of the 5 statuses | – | `"in_progress"` |
| `select.priority` | select | one of the 4 priorities | – | `"high"` |
| `combo.owner` | combo | a username from People, or null | – | `"carol"` |
| `combo.reviewer` | combo | a username from People, or null | – | `null` |
| `tags.tags` | tags | list ≤ 12, each `^[a-z0-9][a-z0-9_-]{0,23}$` | lowercase each, dedupe, sort ascending | `["frontend","q4"]` |
| `textarea.notes` | textarea | string ≤ 5000 chars (not trimmed) | `\r\n` → `\n` | see below |
| `choice.notify` | choice | bool | – | `true` |
| `choice.channels` | choice | list ⊆ `email` `sms` `push` `slack` | dedupe, order as listed | `["email","push"]` |
| `choice.size` | choice | one of `s` `m` `l` `xl` | – | `"m"` |
| `choice.dark` | choice | bool | – | `false` |
| `cascade.pick` | cascade | object `{"program":P,"project":J,"phase":H}`, each string or null; J requires P and must belong to P; H requires J and must be one of J's phases | – | `{"program":"Northwind Retail","project":"POS refresh","phase":"Build"}` |
| `grid.rows` | grid | list ≤ 50 of rows (below); ids unique | sort by nothing (order is the user's) | 6 rows below |
| `menus.files` | menus | list ≤ 30 of `{"name":S,"starred":B}`; name 1–40 chars, unique | trim names | 8 files below |
| `palette.last` | palette | string ≤ 80 | trim | `""` |
| `dialog.profile` | dialogs | `{"name":S,"email":S}`; name 1–80; email as `text.email` but required | trim; lowercase email | `{"name":"Grace Hopper","email":"grace@example.com"}` |
| `charts.series` | charts | list of exactly 12 integers 0–100 | – | `[35,48,52,61,58,70,76,72,64,55,43,38]` |
| `kanban.cards` | kanban | list ≤ 40 of `{"id":N,"title":S,"col":C}`; id ≥ 1 unique; title 1–60; col ∈ `todo` `doing` `review` `done` | trim titles | 8 cards below |
| `gantt.bars` | gantt | list ≤ 20 of `{"id":N,"label":S,"start":date,"days":N}`; label 1–40; days 1–365 | trim labels | 6 bars below |

`textarea.notes` default (with `\n` newlines):
```
Welcome to demo.
Each tab shows one control on its own.
Changes are shared live with everyone connected.
```

`grid.rows`: each row `{"id":N,"item":S,"qty":N,"price":N,"status":S,"due":date|null}`;
id ≥ 1 unique, item 1–40 chars (trim), qty integer 0–9999, price 0–1,000,000
(round 2 dp), status one of the 5 statuses. Default:
```json
[{"id":1,"item":"Laptop stand","qty":4,"price":39.5,"status":"done","due":"2026-10-30"},
 {"id":2,"item":"USB-C dock","qty":2,"price":129,"status":"review","due":"2026-11-04"},
 {"id":3,"item":"Monitor 27in","qty":3,"price":289.99,"status":"in_progress","due":"2026-11-12"},
 {"id":4,"item":"Desk lamp","qty":6,"price":24.75,"status":"todo","due":null},
 {"id":5,"item":"Keyboard","qty":5,"price":89,"status":"blocked","due":"2026-11-20"},
 {"id":6,"item":"Headset","qty":2,"price":149,"status":"todo","due":"2026-12-01"}]
```

`menus.files` default: `Budget 2027.xlsx`, `Roadmap.pdf` (starred),
`Team photo.png`, `Meeting notes.md` (starred), `Invoice 1042.pdf`,
`Logo.svg`, `Contracts.zip`, `Ideas.txt`, in that order; others not starred.

`kanban.cards` default (id, title, col):
1 Write brief / done · 2 Sketch layouts / review · 3 Pick colours / doing ·
4 Build prototype / doing · 5 User test / todo · 6 Fix feedback / todo ·
7 Write docs / todo · 8 Launch / todo.

`gantt.bars` default (id, label, start, days):
1 Discovery 2026-10-19 10 · 2 Design 2026-10-26 14 · 3 Build 2026-11-09 21 ·
4 Test 2026-11-23 12 · 5 Pilot 2026-12-07 7 · 6 Launch 2026-12-14 1.

Lists are replaced whole by `set` (the client sends the full new list with
the version it based it on). Clients assign new ids as `max(id)+1`.

## 6. Natural dates (date tab, grid due column, Gantt)

Parsed case-insensitively relative to the client's local *today*; the field
shows a live preview such as `fri → Fri 23 Oct 2026`. Accepted:
`today`/`t`, `tomorrow`/`tom`, `yesterday`, weekday names and 3-letter
abbreviations (next occurrence, today excluded), `+N`/`-N` with unit `d` `w`
`m` (`+3d`, `-2w`, `+1m`; bare `+3` = days), `eow` (Friday this week; next
week's if today is Saturday/Sunday), `eom` (last day of this month),
`YYYY-MM-DD`, `D Mon` / `Mon D` (`23 oct`, `oct 23`; this year), and empty
(= null). Anything else is invalid (red, not committed).

## 7. Fuzzy matching (combobox, palette)

Subsequence match, case-insensitive, over the candidate's search text
(People: `name username team`). Score: +10 per matched char, +8 if it
starts a word, +6 if it follows the previous match directly, −1 per
skipped char before the first match; ties broken by original order. Return
matched character positions for highlighting. Empty query = all, original order.
