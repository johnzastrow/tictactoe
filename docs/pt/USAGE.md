# pt usage guide

How to run pt and get work done in it, in the terminal (TUI) or the
browser. The four clients (Rust TUI, Go TUI, Rust web, Go web) behave the
same, so this guide covers all of them; differences are called out.

- [1. Start it](#1-start-it)
- [2. Log in](#2-log-in)
- [3. The screen](#3-the-screen)
- [4. Getting around](#4-getting-around)
- [5. Finding things: the filter bar](#5-finding-things-the-filter-bar)
- [6. Editing in the grid](#6-editing-in-the-grid)
- [7. Creating projects and tasks](#7-creating-projects-and-tasks)
- [8. The views](#8-the-views)
- [9. Working with other people](#9-working-with-other-people)
- [10. Administration](#10-administration)
- [11. Making it yours](#11-making-it-yours)
- [12. Keyboard reference](#12-keyboard-reference)
- [13. Terminal tips](#13-terminal-tips)
- [14. Troubleshooting](#14-troubleshooting)
- [15. Command reference](#15-command-reference)

---

## 1. Start it

**Build** (once). Pick a language; both produce a single `pt` binary that
contains the server, the TUI and the web client.

```sh
# Rust: needs rustup's wasm32 target and trunk for the web client
rustup target add wasm32-unknown-unknown
brew install trunk binaryen        # or: cargo install trunk
cd rust && make pt                 # → rust/target/release/pt

# Go: needs only Go 1.26+
cd go && make pt                   # → go/bin/pt
```

**Create a database.** For a demo, fill it with sample data:

```sh
pt seed --db pt.db                 # prints a random password for all demo users
pt seed --db pt.db --password 'choose-one'   # or pick the password
```

For real use, start empty and add an admin:

```sh
pt user add jane --name "Jane Smith" --role admin --db pt.db   # prompts for a password
```

**Run the server**, then open a client:

```sh
pt serve --db pt.db                # http://127.0.0.1:8788
```

- **Browser:** open <http://127.0.0.1:8788>.
- **Terminal:** in another window, `pt tui --user jane`
  (add `--server http://host:8788` for a remote server).

You can open as many TUIs and browser tabs as you like, as different users;
they all see each other's changes live. Either language's TUI works with
either language's server.

## 2. Log in

The login form asks for the server (TUI only), username and password.
In the TUI, **Remember me** stores a session token (valid 14 days) so the
next `pt tui` skips the form; `pt logout --server URL` (or File ▸ Log out)
forgets it. The browser keeps you logged in with a cookie until you use
File ▸ Log out or the session expires.

Demo users from `pt seed`:

| User | Role | Can |
|---|---|---|
| `admin` | admin | everything, including users, programs, templates and custom fields |
| `alice` … `heidi` | member | create, edit and delete projects and tasks |
| `victor` | viewer | look, filter and export (copy), but not change anything |

After 5 wrong passwords for one user, that user is locked out for 15
minutes ("too many attempts").

## 3. The screen

```
 File  Edit  View  Data  Help                      pt · alice (member) · ● rust 0.4.0   ← menu bar, you, connection
 1 Projects │ 2 Tasks │ 3 Board │ 4 Timeline │ 5 Dashboard │ 6 Activity │ …           ← views
 Filter ▸ status:active @me due<+14d▏                         [ 12 / 50 ]  ⇅ Due ▲   ← filter bar
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ the current view: a grid, the board, the timeline or the dashboard            │
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ detail pane (Projects: that project's tasks; Tasks: the task's details)       │
 └──────────────────────────────────────────────────────────────────────────────┘
 ✓ Saved NWR-004 status                                  AN  BS  online: 2       ← messages, who's online
 ^K Commands  ^F Filter  F2 Edit  Ins New  F8 Delete  ^Z Undo  F1 Help  F10 Menu   ← keys for what you're doing
```

The bottom **hint bar changes with what you are doing**: it lists the keys
that work right now (in the grid, in an editor, in a dialog, on the board…).
Click a hint to run it. The connection dot is green when connected, amber
while reconnecting (editing is paused) and red when offline.

## 4. Getting around

You never need to memorise keys; there are four ways to find any command:

| Way | Key | What it is |
|---|---|---|
| Hint bar | – | the keys that work right now, always visible |
| Menu bar | **F10** (or Alt+F/E/V/D/H, or click) | File, Edit, View, Data, Help, with each item's key shown |
| Command palette | **^K** (or ^P) | type a few letters of any command, view, project (`nwr-4`) or task, then Enter |
| Help | **F1** (or ^G) | every key for every context; type to search it |

`^` means Ctrl: ^K is Ctrl+K.

**Switching views:** Alt+1 … Alt+9, Ctrl+PgDn / Ctrl+PgUp (TUI) or
Alt+→ / Alt+← (browser), click a tab, or pick one from the palette or the
View menu. **F6** moves between the main grid, the detail pane and the
filter bar; **Esc** closes whatever popup is open.

**Right-click** (or Shift+F10, or the Menu key) on any cell for a context
menu: edit, copy, paste, fill down, filter by this value, exclude it, sort,
group, hide the column, delete the row.

## 5. Finding things: the filter bar

Press **^F** (or F3), type, and the grid narrows as you type. Esc or ↓
goes back to the grid. Terms are combined with AND; suggestions pop up for
field names and values (Tab or Enter accepts one). A term the filter can't
understand is underlined in red with the reason in the status line, and is
ignored.

| Type | Finds |
|---|---|
| `checkout` | anything containing "checkout" (code, name/title, phase, tags, notes) |
| `"load test"` | an exact phrase |
| `status:active` | an exact status (`status:in` is enough for "In progress") |
| `status:todo,review` | either value (comma = OR) |
| `-status:done` | NOT done (a leading `-` negates any term) |
| `@me`, `@bob`, `owner:alice` | owner (projects) or assignee (tasks); `@none` = unassigned |
| `#q4`, `tag:pilot` | tagged |
| `program:gis`, `project:nwr-004` | by program (name, client or prefix) / project (code or name) |
| `due<+14d`, `due>=2026-11-01`, `start:none` | dates, with natural forms (`today`, `fri`, `eom`, `+2w`) |
| `est>16`, `actual>=40`, `var>0`, `budget<100`, `pct<50` | numbers (hours, % done) |
| `health:red,amber` | project health |
| `is:overdue`, `is:open`, `is:done`, `is:blocked`, `is:mine`, `is:unassigned` | shortcuts |
| `has:notes`, `has:deps`, `has:tags`, `has:due` | non-empty fields |
| `region:north`, `risk_score>5` | custom fields, by name (lowercase, spaces → `_`) |

Examples: `is:open @me due<eow` (my work due this week) · `#compliance
-is:done` · `program:contoso health:red` · `var>0 is:done` (finished over
estimate).

**Save a filter** with Data ▸ Save filter…; saved filters appear in the
Data menu and the command palette.

**Sort:** ^O sorts by the current column (press again: descending, then
off), or click a header (Shift+click adds a second sort key). Data ▸ Sort…
sets up to three keys. **Group:** ^R groups by the current column; group
rows show counts and subtotals and fold with Enter, ← or →.

## 6. Editing in the grid

Move with the arrows, Tab / Shift+Tab, Home / End, PgUp / PgDn and
^Home / ^End, or click a cell.

| To | Do |
|---|---|
| edit a cell | **Enter**, **F2**, double-click, or just **start typing** (it replaces the value) |
| save and move down / right | **Enter** / **Tab** (Shift+Tab: left) |
| cancel | **Esc** |
| clear a cell | **Del** |
| open the full record | **^E**: every field, notes, custom fields and the record's history |

Every cell has an editor that fits its data:

| Field type | Editor | Tips |
|---|---|---|
| Text (name, title) | text box | ^U clears, ^←/^→ jump words |
| Hours, numbers | number box | ↑/↓ add or subtract 1 (Shift: 0.25); `2.5h` works |
| Dates | text box + calendar | type `fri`, `tomorrow`, `+3d`, `-1w`, `eom`, `nov 3` or `2026-11-03` and see the date it means; ↑/↓ ±1 week, PgUp/PgDn ±1 month, ^←/^→ ±1 day, or click a day; empty = no date |
| Status, priority, health | dropdown | type to filter, first letter jumps, ↑/↓ + Enter |
| Owner, assignee, program | fuzzy search | type any part of the name or username (`bo` finds Bob Stone); matched letters are highlighted |
| Phase | text with suggestions | suggests the project template's phases; you can type a new one |
| Tags, dependencies | chips | type to search, Enter adds a chip (new tags are created), Backspace on an empty box removes the last chip, Enter on an empty box saves; deps are tasks of the same project (`#3`) |
| Notes | multi-line text (record form) | Enter = new line; ^S saves; Tab leaves the field |
| Yes/no | toggle | Space |

Changes save as soon as you leave the editor. A cell shows **dim italic**
while the server confirms the save, and a ✓ message appears in the status
line. If a value isn't valid (say, a due date before the start date), the
change is refused with the reason and nothing is saved.

**Working on many rows at once**

| To | Do |
|---|---|
| mark rows | **Space** marks/unmarks a row (● in the left gutter); **Shift+↑/↓** extends; **^A** marks all visible rows (again: none) |
| copy | **^C**: marked rows (or the current cell) as tab-separated text, ready for a spreadsheet |
| paste | **^V** or your terminal's / browser's paste: a block of cells pastes right and down from the current cell; one value pastes into every marked row. Values are understood the way you'd type them (`Done`, `bob`, `fri`, `#2 #3`) |
| fill down | **^D**: copy the first marked row's value in this column to the other marked rows (nothing marked: copy from the row above) |
| delete | **F8** (or ^Del): deletes the marked rows or the current row, after you confirm; deleting a project deletes its tasks |
| undo / redo | **^Z** / **^Y**: undoes *your* last change on the server, even a paste of 100 cells or a deleted project, as one step |

## 7. Creating projects and tasks

Press **Ins** (or ^N in the TUI, Alt+N in the browser), or use File ▸ New
project / New task. Both dialogs **cascade**: each choice narrows or fills
in the next field. Tab moves between fields, **^S** saves, Esc cancels.

**New project**
1. **Program**: pick one (fuzzy search). The **Code** fills in with the
   program's next free code (e.g. `NWR-008`) and updates if you change the
   program.
2. **Name**.
3. **Template**: shows each template's size ("12 tasks, 214 h, 63 days"),
   or choose a blank project.
4. **Owner**, **Start** (default today), **Due** (default: start plus the
   template's length), **Budget hours** (default: the template's estimate).

Saving creates the project and the template's tasks, dated from the start
date, assigned to the owner, and with each phase depending on the previous
one.

**New task**
Program ▸ Project (only that program's projects; already set if you started
from a project) ▸ Phase (that project's phases, or type a new one) ▸ Title ▸
Assignee (defaults to the project owner) ▸ Status ▸ Start ▸ Due ▸ Est hours.

On the **Board**, Ins creates a task in the column you're in; in the
**Projects** detail pane it creates a task in that project.

## 8. The views

| # | View | What it's for | Special keys |
|---|---|---|---|
| 1 | **Projects** | all projects with health, budget burn, % done and overdue counts; the detail pane (^B toggles) is an editable grid of the selected project's tasks | F6 switches between the two grids; ^R on Program groups with subtotals |
| 2 | **Tasks** | every task, grouped by project; the detail pane shows the current task's full record, its dependencies and recent changes | Enter / ← / → on a group row folds it |
| 3 | **Board** | kanban: To do · In progress · Blocked · Review · Done, with counts and hours per column | **Shift+←/→** (or `<` `>`) move the card to the next status; Enter opens it; `s` picks the scope (Program ▸ Project, cascading); ^R shows swimlanes per assignee; drag cards with the mouse |
| 4 | **Timeline** | Gantt chart: projects (expand to their tasks with → or Enter) on a time axis, with a today line, weekends shaded, bars coloured by status and filled by progress | `+`/`-` zoom (day/week/month), `t` today, `<` `>` scroll; **Shift+←/→** moves a bar a step; `[` `]` shortens or extends its due date; drag a bar, or its right edge, with the mouse |
| 5 | **Dashboard** | KPIs (active projects, overdue, blocked, hours used vs budget, due this week), status charts, hours by owner, a workload heatmap for the next 8 weeks, an activity sparkline, at-risk projects | Tab/arrows move between tiles; **Enter drills down** into the matching rows |
| 6 | **Activity** | every change, newest first, live; undone changes are struck through | Enter jumps to the changed record; filter with `@bob` or text |
| 7 | **Templates** | the task templates new projects start from, with a mini Gantt preview | admins edit; changing a template doesn't change existing projects |
| 8 | **Fields** | custom fields for projects and tasks (text, number, date or a list of choices) | admins add fields; they appear as columns for everyone immediately |
| 9 | **Users** | people, roles, open tasks and hours; programs (with their code prefix and colour) below | admins add users (Ins), change roles, deactivate, set passwords (context menu) |

Computed columns: **Est h / Actual h** sum the project's tasks; **Burn** is
actual hours ÷ budget (amber over 90 %, red over 100 %); **Done** is tasks
done ÷ total; **Late** counts overdue tasks; **Var h** is a task's actual
minus estimate (red when over).

## 9. Working with other people

- **Live updates.** Anything anyone changes appears everywhere within a
  moment; changed rows flash briefly, and a toast tells you when someone
  changes something you're looking at. Your cursor stays on the same record
  even if rows move.
- **Presence.** Coloured initials in the status line show who's online.
  A row someone else is on shows their initial in the left gutter, and the
  exact cell they're on is underlined in their colour.
- **Conflicts.** If you edit a value that someone else changed since you
  last saw it, your edit is *not* applied. The cell turns red with ⚠ and a
  message says who changed it and to what. Press Enter on the cell to edit
  again from their value, or Esc to dismiss. Edits to *other* fields of the
  same row are never blocked. In the record form, conflicting fields are
  marked and keep your value; save again with ^S to overwrite theirs.
- **Undo is personal.** ^Z undoes your own changes, newest first. If
  someone has changed the same field since, undo refuses rather than
  overwrite them. (Two sessions logged in as the same user share one undo
  history.)
- **Connection drops.** The dot turns amber, editing pauses, and the client
  reconnects by itself and reloads the data.

## 10. Administration

Admins can do the following in the app:

- **Users** (view 9): Ins adds a user (username, name, role, password).
  Change a role or untick Active in the grid; deactivating someone signs
  them out everywhere immediately. Right-click ▸ Set password resets a
  password (that user's other sessions end). Users are never deleted, so
  history keeps their name.
- **Programs** (view 9, lower grid): name, client, the **prefix** used for
  new project codes, and the colour shown next to the program everywhere.
- **Templates** (view 7): edit the template list and each template's tasks
  (title, phase, estimate, offset from the project start, duration).
- **Custom fields** (view 8): add a field to projects or tasks, choose its
  kind, and for a list, its choices (chips). Deleting a field deletes its
  values.

Anyone can change their own password: in **Users** (view 9), right-click
your own row ▸ *Set password…* (in the browser clients, also from the
record form, ^E). It asks for your current password. The Go TUI also has
File ▸ Set password.

From the command line (with the server stopped or running):

```sh
pt user add carol --name "Carol Diaz" --role member --db pt.db
pt user passwd carol --db pt.db        # also ends her sessions
pt user list --db pt.db
PT_PASSWORD='…' pt user add bot --db pt.db   # non-interactive
```

**Backups.** Everything is in the one database file. To copy it while the
server runs: `sqlite3 pt.db ".backup pt-backup.db"`. Either language's
server can open it.

### Sharing the server on a network

By default the server only accepts connections from the same machine. To
serve a team:

```sh
# Office LAN: use a certificate from a CA your machines trust. mkcert is the
# easy way; the Rust client rejects a certificate that is its own CA.
mkcert -install && mkcert pt.example.lan
pt serve --db pt.db --bind 0.0.0.0:8788 --tls-cert pt.example.lan.pem --tls-key pt.example.lan-key.pem
pt tui --server https://pt.example.lan:8788 --ca-cert "$(mkcert -CAROOT)/rootCA.pem"

# Tailscale or another VPN (traffic is already encrypted):
pt serve --db pt.db --bind 100.101.102.103:8788 --allow-plaintext
```

Browsers then use `https://pt.example.lan:8788` (or the Tailscale address).
Copying to the system clipboard from the browser needs HTTPS (or localhost).

## 11. Making it yours

- **Themes:** Midnight (dark, default), Daylight (light), Solarized Dark,
  Gruvbox. ^T cycles in the TUI (or `pt tui --theme gruvbox`); in the
  browser use View ▸ Theme (Chrome keeps Ctrl+T for itself). The browser
  also has View ▸ Density (comfortable / compact).
- **Columns:** **F4** opens the column list: Space shows/hides, Shift+↑/↓
  (or `[` `]`) reorders, `+`/`-` changes width, `r` resets. You can also drag
  a column border with the mouse. Notes are hidden by default.
- **Detail pane:** ^B shows or hides it. In a TUI at least 160 columns
  wide it sits on the right, otherwise below.
- Saved filters and layouts are kept per computer: in the browser's local
  storage, or in the TUI's config folder (see below).

## 12. Keyboard reference

The same in all four clients unless marked.

### Everywhere

| Key | Action |
|---|---|
| F1, ^G | help |
| F10, Alt+F/E/V/D/H | menu bar |
| ^K, ^P | command palette |
| ^F, F3 | filter bar |
| Alt+1 … Alt+9 | go to view 1–9 |
| ^PgDn / ^PgUp (TUI) · Alt+→ / Alt+← (browser) | next / previous view |
| F6 / Shift+F6 | next / previous pane |
| ^Z / ^Y | undo / redo |
| Shift+F10, Menu key, right-click | context menu |
| Esc | close popup, cancel |
| ^T (TUI) | next theme |
| ^L (TUI) | redraw the screen |
| ^Q (TUI) | quit (browser: File ▸ Log out) |

### Grids

| Key | Action |
|---|---|
| arrows, Tab, Shift+Tab, Home, End, PgUp, PgDn, ^Home, ^End | move |
| Enter, F2, typing | edit cell |
| Del | clear cell |
| Ins, ^N (TUI) / Alt+N (browser) | new row |
| ^E | record form |
| F8, ^Del | delete row(s) |
| Space, Shift+↑/↓, ^A | mark rows |
| ^C / ^V | copy / paste (TSV) |
| ^D | fill down |
| ^O | sort by this column |
| ^R | group by this column |
| ^B | detail pane |
| F4 | columns |

### Editors

| Key | Action |
|---|---|
| Enter / Tab / Shift+Tab | save and move down / right / left |
| Esc | cancel |
| ↑ ↓ | choose (lists); ±1 (numbers); ±1 week (dates) |
| PgUp / PgDn | ±1 month (dates) |
| ^← / ^→ | ±1 day (dates); word jump (text) |
| ^U | clear (text) |
| Backspace on empty | remove last chip |

### Board and Timeline

| View | Key | Action |
|---|---|---|
| Board | Shift+←/→, `<` `>` | move card to previous/next status |
| Board | `s` | scope: Program ▸ Project |
| Board | ^R | swimlanes by assignee |
| Board | Enter, ^E | open card |
| Timeline | `+` `-` | zoom |
| Timeline | `t` | today |
| Timeline | `<` `>` | scroll time |
| Timeline | Shift+←/→ | move bar |
| Timeline | `[` `]` | shorten / extend due date |
| Timeline | → / Enter, ← | expand / collapse a project |
| Dashboard | Enter | drill down |

### Dialogs

Tab / Shift+Tab between fields · **^S** saves (Ctrl+Shift+S in the browser
record form forces your value over a conflict) · Esc cancels · in a
confirmation, Cancel is the default and `y` / `n` answer.

## 13. Terminal tips

- **Size:** at least 80×24; wider is better (the Projects detail pane moves
  to the right from 160 columns).
- **Colours:** truecolor is used when the terminal says it supports it
  (Ghostty, Windows Terminal, iTerm2, kitty, WezTerm…); otherwise colours
  map to the 256-colour palette (macOS Terminal, older XFCE Terminal). Force
  it with `pt tui --colors 256` or `--colors truecolor`.
- **macOS Terminal:** enable *Settings ▸ Profiles ▸ Keyboard ▸ Use Option as
  Meta key* for the Alt shortcuts, and *fn* for F-keys if your keyboard
  needs it. Everything is also in the F10 menu and the ^K palette.
- **Clipboard:** ^C sends copied cells to the system clipboard using
  OSC 52, which works in Ghostty, Windows Terminal, kitty and iTerm2 but
  not in macOS Terminal or XFCE Terminal. Copy/paste inside pt always
  works, and your terminal's own paste (Cmd+V, Ctrl+Shift+V) pastes into
  pt.
- **tmux:** works; `set -g mouse on` passes the mouse through, and tmux
  must allow OSC 52 (`set -g set-clipboard on`) for the system clipboard.
- **Files:** sessions and saved filters live in your config folder:
  `~/Library/Application Support/pt/` (macOS), `~/.config/pt/` (Linux),
  `%AppData%\pt\` (Windows). The Rust TUI uses `sessions.json` and
  `filters.json`; the Go TUI uses `sessions.json` and `tui-go.json`. All are
  readable only by you. Set `PT_CONFIG_DIR` to use another folder (handy
  for a second login on one machine, or scripted demos).

## 14. Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `refusing to serve 0.0.0.0:8788 without TLS` | non-local addresses need `--tls-cert`/`--tls-key`, or `--allow-plaintext` on a VPN such as Tailscale |
| `the database has no users` (server log) | run `pt seed` or `pt user add <name> --role admin` |
| "invalid credentials" | wrong username or password (the message is the same for both on purpose) |
| "too many attempts" | 5 failed logins for that user (wait 15 minutes) or 20 from your address (wait a minute) |
| TUI can't connect to `https://…`: `CaUsedAsEndEntity` or unknown issuer | the certificate isn't from a CA the client trusts: use mkcert and pass `--ca-cert "$(mkcert -CAROOT)/rootCA.pem"` |
| browser shows "The web client has not been built" | the binary was built without the web client: run `make pt` (not just `cargo build` / `go build`) |
| amber dot, "reconnecting" | the server is unreachable; the client keeps retrying and reloads when it's back |
| your edit shows ⚠ instead of saving | someone changed that value first; see [Conflicts](#9-working-with-other-people) |
| "viewers cannot make changes" | your role is viewer; ask an admin |
| Alt+number doesn't switch views (macOS Terminal) | enable Option as Meta (above) or use the tabs, ^K or F10 |
| `address already in use` | another server is on that port: `--bind 127.0.0.1:8789` |

## 15. Command reference

```
pt serve  [--db pt.db] [--bind 127.0.0.1:8788] [--tls-cert FILE --tls-key FILE] [--allow-plaintext]
pt seed   [--db pt.db] [--password PW] [--force]         # --force wipes existing data first
pt user add <username> [--name "Full Name"] [--role admin|member|viewer] [--db pt.db]
pt user passwd <username> [--db pt.db]
pt user list [--db pt.db]
pt tui    [--server http://127.0.0.1:8788] [--user NAME] [--ca-cert FILE]
          [--theme midnight|daylight|solarized-dark|gruvbox] [--colors 256|truecolor]
pt logout [--server URL]
```

Passwords for `user add` / `user passwd` come from `PT_PASSWORD` or a prompt
that doesn't echo; `seed` also reads `PT_SEED_PASSWORD`. Set `RUST_LOG=debug`
for more server logging (Rust).
