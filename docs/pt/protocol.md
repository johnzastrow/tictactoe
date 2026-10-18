# pt wire protocol (v1)

Normative for both servers (Rust, Go) and all four clients (two TUIs, two WASM
web clients). Any client works against any server.

## 1. Transport and security

| Item | Rule |
|---|---|
| Default bind | `127.0.0.1:8788`. Any non-loopback bind requires TLS (`--tls-cert`/`--tls-key`) **or** `--allow-plaintext`. |
| HTTP API | JSON bodies, `Content-Type: application/json`, max request body 4 KiB. |
| WebSocket | `GET /ws?client=<id>` where `<id>` matches `[a-z0-9-]{1,24}` (e.g. `tui-rust`, `web-go`). Text frames, one JSON object per frame. |
| Auth | Every endpoint except `/api/health`, `/api/login` and static files needs a session: either `Authorization: Bearer <token>` (TUIs) or the `pt_session` cookie (browsers). Missing/invalid/expired → `401`. |
| Origin check | If a request to `/ws`, `/api/login`, `/api/logout` carries an `Origin` header, it must equal `http://<Host>` or `https://<Host>` (same origin), else `403`. Non-browser clients send no `Origin`. |
| Inbound WS limits | Frame ≤ 512 KiB; ≤ 2000 ops per `mutate`; token bucket of 50 msg/s, burst 100. Exceeding a limit closes the socket with code 1008. |
| Keepalive | Server pings every 30 s; clients may also send `{"type":"ping"}` and get `{"type":"pong"}`. |
| Security headers | All HTTP responses: `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin` (not `no-referrer`: with it, Safari sends `Origin: null` on same-origin `POST`s, which fails the origin check), `X-Frame-Options: DENY`, a strict `Content-Security-Policy` (`default-src 'self'`, scripts `'self' 'wasm-unsafe-eval'` + hashes of any inline loader script, `frame-ancestors 'none'`), `Strict-Transport-Security` under TLS. |

### Sessions

- Token: 32 bytes from a CSPRNG, base64url without padding (43 chars).
  The DB stores only `hex(sha256(token))`.
- Lifetime: 14 days, absolute. Logout deletes the session. Deactivating a
  user or changing their password deletes all their sessions and closes their sockets.
- Cookie: `pt_session=<token>; Path=/; HttpOnly; SameSite=Strict; Max-Age=1209600`,
  plus `Secure` when serving TLS.
- Passwords: Argon2id, PHC string format, `m=19456,t=2,p=1`, 16-byte salt,
  32-byte hash. Both servers must verify each other's hashes. Minimum
  length is 8 characters, maximum 256.

## 2. HTTP endpoints

| Method + path | Request | Response |
|---|---|---|
| `GET /api/health` | – | `200 {"ok":true,"impl":"rust"\|"go","version":"0.4.0","protocol":1}` |
| `POST /api/login` | `{"username":"alice","password":"…","client":"tui-rust"}` | `200 {"token":"…","user":User}` and `Set-Cookie`. Bad credentials or inactive user: `401 {"error":"invalid credentials"}` (same message for both). Throttled: `429 {"error":"too many attempts"}` |
| `POST /api/logout` | – (session) | `204`, cookie cleared |
| `GET /api/me` | – (session) | `200 User` or `401` |
| `GET /ws` | – (session) | WebSocket upgrade |
| `GET /*` | – | Embedded web client (static, no auth) |

Login throttling: 5 failed attempts for one username in 15 minutes, or 20
failed attempts from one IP in 1 minute → `429` until the window passes.
Successful logins don't count.

## 3. Rows (JSON)

Snapshots and change events carry whole rows. `null` means empty.

```jsonc
// User (password hash never leaves the server)
{"id":3,"username":"alice","display_name":"Alice Nguyen","role":"member","active":true}

// Program
{"id":1,"name":"Northwind Retail","client":"Northwind Traders","prefix":"NWR","color":"#4f9dde"}

// Template, TemplateTask
{"id":1,"name":"Standard Project","description":"…"}
{"id":7,"template_id":1,"seq":3,"title":"Requirements sign-off","phase":"Define",
 "est_hours":6.0,"offset_days":10,"duration_days":3}

// Project
{"id":12,"code":"NWR-004","name":"POS refresh","program_id":1,"template_id":2,
 "owner_id":3,"status":"active","priority":"high","health":"amber",
 "start_date":"2026-09-01","due_date":"2026-12-15","budget_hours":320.0,
 "tags":["retail","q4"],"notes":"…","cf":{"1":"PO-7781","2":4.5},
 "created_at":"…","updated_at":"…","updated_by":3}

// Task
{"id":301,"project_id":12,"seq":5,"title":"Load test checkout","phase":"Test",
 "status":"in_progress","priority":"medium","assignee_id":4,
 "start_date":"2026-10-10","due_date":"2026-10-20","est_hours":16.0,
 "actual_hours":9.5,"deps":[299,300],"tags":["perf"],"notes":"","cf":{},
 "created_at":"…","updated_at":"…","updated_by":4}

// CustomField
{"id":2,"entity":"project","name":"Risk score","kind":"number","options":[],"seq":2}

// Activity
{"id":9001,"ts":"2026-10-17T14:03:00Z","user_id":3,"group":512,"action":"set",
 "entity":"task","entity_id":301,"field":"status","old":"todo","new":"in_progress",
 "label":"NWR-004 #5 status: todo → in_progress","state":"done"}
```

- `tags` and `deps` are sorted ascending (strings / ids).
- `cf` maps custom-field id (as a string) → value. Missing key = empty.
  `number` fields hold JSON numbers; `text`, `date` (`YYYY-MM-DD`) and `enum` hold strings.
- Hours are rounded to 2 decimals by the server.
- Computed values (% complete, est/actual totals, burn, overdue counts) are
  **not** sent; clients compute them from tasks (see §8).

## 4. Fields

Who may write: **M** = member or admin, **A** = admin only. `viewer` may
never write. **C** = settable only on create. Everything else can be set
with `set` and given on `create`.

### project
| Field | Type | Rules | Write |
|---|---|---|---|
| code | text | `^[A-Z0-9][A-Z0-9-]{1,15}$`, unique, required | M |
| name | text | 1–200 chars, required | M |
| program_id | ref program | or null | M |
| template_id | ref template | or null; on create, instantiates the template's tasks | M, C |
| owner_id | ref user | active user or null | M |
| status | enum | `planning` `active` `on_hold` `done` `cancelled` (default `planning`) | M |
| priority | enum | `low` `medium` `high` `critical` (default `medium`) | M |
| health | enum | `green` `amber` `red` (default `green`) | M |
| start_date, due_date | date | or null; if both set, due ≥ start | M |
| budget_hours | hours | 0 – 1,000,000 or null | M |
| notes | text | 0–20,000 chars | M |
| tags | list of tag | each `^[a-z0-9][a-z0-9_-]{0,31}$` (server lowercases), ≤ 20 | M |
| cf.N | custom | per field kind (§4.1) | M |

### task
| Field | Type | Rules | Write |
|---|---|---|---|
| project_id | ref project | required | M, C |
| title | text | 1–200 chars, required | M |
| phase | text | 0–60 chars | M |
| status | enum | `todo` `in_progress` `blocked` `review` `done` (default `todo`) | M |
| priority | enum | as project | M |
| assignee_id | ref user | active user or null | M |
| start_date, due_date | date | or null; due ≥ start | M |
| est_hours | hours | or null | M |
| actual_hours | hours | not null (default 0) | M |
| notes | text | 0–20,000 | M |
| tags | list of tag | as project | M |
| deps | list of task id | same project, not self, no cycles, ≤ 50 | M |
| cf.N | custom | | M |

`seq` is assigned by the server on create (`max(seq)+1` within the project) and is read-only.

### program (A)
`name` (1–100, unique, required), `client` (0–100), `prefix` (`^[A-Z]{0,5}$`), `color` (`^#[0-9a-f]{6}$`).

### template (A)
`name` (1–100, unique, required), `description` (0–2,000).

### template_task (A)
`template_id` (C, required), `seq` (int ≥ 0; default max+1), `title` (1–200, required), `phase` (0–60),
`est_hours` (hours or null), `offset_days` (int 0–3650), `duration_days` (int 1–3650).

### custom_field (A)
`entity` (C, `project`|`task`), `name` (1–40, unique per entity, required), `kind` (C, `text`|`number`|`date`|`enum`),
`options` (list of 1–40-char strings, ≤ 50, enum only), `seq` (int).

### user (A)
`username` (C, `^[a-z][a-z0-9_.-]{1,31}$`), `display_name` (1–80), `role` (`admin`|`member`|`viewer`),
`active` (bool), `password` (C, write-only). Users are never deleted; set `active=false`.
An admin cannot demote or deactivate themselves.

### 4.1 Custom field values

| Kind | Value |
|---|---|
| text | string, 0–500 chars |
| number | JSON number, finite |
| date | `YYYY-MM-DD` |
| enum | one of the field's `options` |

Setting `null` (or `""`) removes the value.

## 5. Client → server messages

`req` is a client-chosen integer echoed in the reply.

### mutate: the only write message

```json
{"type":"mutate","req":7,"ops":[
  {"op":"set","entity":"task","id":301,"field":"status","base":"todo","value":"done"},
  {"op":"set","entity":"project","id":12,"field":"cf.2","base":null,"value":7,"force":false},
  {"op":"create","entity":"task","ref":"n1","values":{"project_id":12,"title":"Retro"}},
  {"op":"delete","entity":"task","id":299}
]}
```

Semantics:

1. All ops run in **one transaction**, in order.
2. **Per-field optimistic concurrency.** For `set`, the server compares the
   field's current value with `base` (equality rules below). If they differ,
   that op is **skipped** and reported as a conflict; the other ops still
   apply. `"force":true` skips the check (used for "overwrite theirs").
3. Any validation or authorization error **rolls back the whole mutate**
   and is reported with the index of the failing op.
4. Validation of cross-field rules (due ≥ start, dependency cycles) runs
   after all ops are applied, so a batch may move both dates of a task at once.
5. A mutate that applies at least one op becomes one activity **group**
   (one undo step).

Equality for `base`: `null` = `null`; strings exact; numbers within 1e-9;
lists compared as sorted sets; a missing custom value equals `null`.

### undo / redo

```json
{"type":"undo","req":8}
{"type":"redo","req":9}
```

Undo reverses the caller's own most recent group in state `done`. Redo
re-applies their most recent `undone` group, but only if they have made
no new mutation since (a new mutation marks their `undone` groups `dead`).
Undo/redo also uses optimistic checks: if a field it would revert was changed
by someone else since, nothing is applied and the reply lists the conflicts.
Deletes of programs, templates, template tasks and custom fields are recorded
as `dead` (not undoable).

### focus (presence)

```json
{"type":"focus","view":"tasks","entity":"task","id":301,"field":"status"}
```
`entity`, `id`, `field` may be omitted or null. No reply; triggers a `presence` broadcast.

### set_password

```json
{"type":"set_password","req":10,"user_id":3,"password":"new secret","old_password":"old"}
```
Admins may set anyone's password without `old_password`; anyone may change
their own with `old_password`.

### resync, ping

`{"type":"resync"}` → server sends a fresh `snapshot`. `{"type":"ping"}` → `{"type":"pong"}`.

## 6. Server → client messages

On connect the server sends, in order: `welcome`, `snapshot`, `presence`.

### welcome
```json
{"type":"welcome","protocol":1,"server":{"impl":"go","version":"0.4.0"},"me":User,"conn":17}
```

### snapshot
```json
{"type":"snapshot","data":{
  "users":[],"programs":[],"templates":[],"template_tasks":[],
  "projects":[],"tasks":[],"custom_fields":[],"activity":[]}}
```
`activity` holds the most recent 500 entries, newest last.

### changes (broadcast to every connection, the author included)
```json
{"type":"changes","by":3,"group":512,
 "upserts":{"tasks":[Task],"projects":[Project]},
 "deletes":{"tasks":[299]},
 "activity":[Activity]}
```
`upserts` / `deletes` use the snapshot's keys; empty keys may be omitted.
Rows are complete; clients replace by `id`. The server includes every row
whose visible data changed, including side effects (deleting a task updates
the `deps` of tasks that depended on it; deleting a custom field updates
every row's `cf`). `activity` holds new entries and entries whose `state`
changed (undo/redo); clients upsert them by `id`.

### result (reply to mutate / undo / redo / set_password)
```json
{"type":"result","req":7,"ok":true,"group":512,"created":{"n1":3301},
 "conflicts":[{"index":0,"entity":"task","id":301,"field":"status",
               "current":"blocked","by":5}]}
{"type":"result","req":7,"ok":false,
 "error":{"code":"invalid","message":"due_date is before start_date","index":2}}
```
`group` is absent when nothing was applied. `by` is the user who last changed
that field, if known. Error codes: `bad_request`, `invalid`, `forbidden`,
`not_found`, `conflict` (undo/redo only), `nothing_to_undo`, `nothing_to_redo`,
`internal`.

### presence
```json
{"type":"presence","peers":[{"conn":17,"user_id":3,"client":"tui-rust","view":"tasks",
  "focus":{"entity":"task","id":301,"field":"status"}}]}
```
Sent on any connect, disconnect or focus change. Includes the receiver's own
connection (clients skip their own `conn`).

### error
`{"type":"error","code":"bad_request","message":"…"}`: for unparseable frames;
the socket stays open.

## 7. Template instantiation (project create with `template_id`)

For each template task, in `seq` order: create a task with the same `seq`, `title`,
`phase`, `est_hours`; `start_date = project.start_date (or today) + offset_days`;
`due_date = start_date + duration_days − 1`; status `todo`; assignee = project owner.
The first task of each phase depends on the last task of the previous phase.
The project's tasks are part of the create's activity bundle, so undo removes them.

## 8. Computed metrics (clients)

| Metric | Definition |
|---|---|
| tasks_total / tasks_done | count of the project's tasks / with status `done` |
| pct_done | `tasks_done / tasks_total × 100` (0 if no tasks) |
| est_total / actual_total | sum of task `est_hours` (null = 0) / `actual_hours` |
| burn_pct | `actual_total / budget_hours × 100` (null if no budget) |
| variance (task) | `actual_hours − est_hours` |
| overdue (task) | `due_date < today` and status ≠ `done` |
| overdue (project) | `due_date < today` and status ∉ {`done`,`cancelled`} |
| blocked (task) | status `blocked`, or any dep not `done` while status is `in_progress`/`review` |

## 9. Query language (client side, same in both languages)

Whitespace-separated terms, all ANDed. Case-insensitive.

| Term | Meaning |
|---|---|
| `word`, `"two words"` | free text: substring of code / name / title / phase / tags / notes |
| `field:value` | text fields: substring; enum/ref fields: exact (by name, username or display name); `field:a,b` = a OR b |
| `field>v` `>=` `<` `<=` `=` | numbers, hours and dates (dates accept the natural forms of §10) |
| `-term` | negation |
| `#tag` | shorthand for `tag:tag` |
| `@user`, `@me` | shorthand for owner (projects) / assignee (tasks) |
| `is:open` `is:done` `is:overdue` `is:blocked` `is:mine` `is:unassigned` | predicates |
| `has:notes` `has:deps` `has:due` `has:tags` | non-empty |

Fields: `code name title phase status priority health owner assignee program
project tag due start est actual budget var pct` plus custom fields by name
(lowercase, spaces → `_`). Unknown fields are reported as an error under the bar
and the term is ignored.

## 10. Natural dates (client side, same in both languages)

Relative to today. `today`/`tod`, `tomorrow`/`tom`, `yesterday`/`yest`,
weekday names or 3-letter prefixes (next occurrence, today excluded),
`+3d` `-2w` `+1m` `+1y` (and without sign = plus), `eow` (this Friday, or
today if Friday), `eom` (last day of month), `eoy`, ISO `2026-11-03`,
`11/3` and `11/3/2026` (US M/D), `nov 3`, `3 nov`, `nov 3 2026`. An empty
string means null.
