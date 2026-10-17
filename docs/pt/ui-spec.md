# pt UI specification

Normative for all four front ends: Rust TUI (ratatui), Go TUI (Bubble Tea),
Rust web (Leptos/WASM), Go web (Go/WASM). The web clients mirror the TUIs:
the same views, columns, controls and keys, rendered with the extra
freedom of a browser (proportional fonts, smooth scrolling, hover).

The audience is **non-technical users**: conventional keys, with the keys
for the current context always shown on screen (nano-style), a menu bar,
a command palette and a help screen. Every action has a keyboard path and a
mouse path.

## 1. Screen layout

```
┌ File  Edit  View  Data  Help ─────────────────── pt · alice (member) · ● rust 0.4.0 ┐  menu bar
│ 1 Projects │ 2 Tasks │ 3 Board │ 4 Timeline │ 5 Dashboard │ 6 Activity │ 7 Templates │ …  tabs
│ Filter ▸ status:active @me due<+14d▏                  [ 12 / 50 ]  ⇅ Due ▲  ⊞ Program  │  query bar
├──────────────────────────────────────────────────────────────────────────────────────┤
│ grid / board / timeline / dashboard …                                                │  main area
│                                                                                      │
├───────────────────────── detail pane (optional, F6 to focus) ────────────────────────┤
│ …                                                                                    │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ ✓ Saved NWR-004 status · ⚠ 1 conflict            AB  CD  EF  online: 3   ⟳ 0 pending │  status line
│ ^K Commands  ^F Filter  F2 Edit  Ins New  F8 Delete  ^Z Undo  F1 Help  F10 Menu  ^Q Quit│  hint bar(s)
└──────────────────────────────────────────────────────────────────────────────────────┘
```

- **Menu bar** (top): `File Edit View Data Help`; right side: app name, user + role,
  connection dot (green connected / amber reconnecting / red offline), server impl + version.
- **Tab bar**: the views, numbered; active tab highlighted; click to switch.
- **Query bar**: filter text with live validation (bad terms underlined red, message
  in status line), row count `[shown / total]`, sort and group indicators.
  Shown on grid, board and timeline views.
- **Status line**: last message (✓ success, ⚠ conflict, ✗ error, ℹ info), presence avatars
  (2-letter initials in each user's colour), online count, pending-request count.
- **Hint bar** (nano style): 1–2 lines of `KEY Label` pairs for the **current context**
  (grid, editor, dropdown, dialog, board…). Keys rendered inverse/accent, labels normal.
  Clicking a hint runs it.
- Minimum TUI size 80×24; a narrower terminal shows a "make the window bigger" notice.
  Layout adapts: detail pane goes to the right when width ≥ 160, else bottom.

## 2. Keymap (global)

Conventional keys. Where a browser reserves a key, the web column shows the alternative.

| Action | TUI | Web |
|---|---|---|
| Help (searchable) | F1, ^G | F1, ^G |
| Menu bar | F10, Alt+letter (F/E/V/D/H) | F10, Alt+letter |
| Command palette | ^K, ^P | ^K, ^P |
| Focus filter bar | ^F, F3 | ^F, F3 |
| Next / previous view | ^PgDn / ^PgUp | Alt+→ / Alt+← |
| Go to view n | Alt+1…9 | Alt+1…9 |
| Next pane (grid ⇄ detail ⇄ filter) | F6 / Shift+F6 | F6 / Shift+F6 |
| Undo / Redo (server-side, your own changes) | ^Z / ^Y | ^Z / ^Y |
| New row / record | Ins, ^N | Ins, Alt+N |
| Edit record (full form) | ^E | ^E |
| Delete row(s) (confirm) | F8, ^Del | F8, ^Del |
| Toggle detail pane | ^B | ^B |
| Columns (show/hide/reorder/width) | F4 | F4 |
| Sort by current column (cycle ▲ ▼ off) | ^O | ^O |
| Group by current column (toggle) | ^R | ^R |
| Theme (cycle) | ^T | ^T (alt: View menu) |
| Redraw | ^L | – |
| Quit / log out | ^Q | (File ▸ Log out) |
| Close popup / cancel | Esc | Esc |
| Context menu | Shift+F10, Menu key, right-click | same |

## 3. The data grid (the centrepiece)

One reusable grid component per front end, used by Projects, Tasks, the
Projects detail pane, Templates, Fields, Users and Activity.

### Rendering
- Header row with column titles, sort arrows (`▲1 ▼2` for multi-sort), filter hint.
- **Frozen columns**: the first N columns (per view) stay put during horizontal scroll.
- Column widths in characters; numbers right-aligned; text ellipsised `…`.
- Zebra striping; current row highlighted; **current cell** strongly highlighted.
- **Selection gutter** (1–2 chars) at the left: `●` marked rows; a coloured
  presence initial when another user's focus is on that row.
- Cell decorations:
  - enum values coloured (status, priority, health chips; `◆ critical`, `● green` …)
  - overdue dates red; due within 7 days amber
  - progress cells as bars: `███████░░░ 70%` (eighth-block resolution in TUI, CSS bar on web)
  - burn % bar turns amber > 90 %, red > 100 %
  - hours variance red when over estimate
  - **conflict cell**: red background + `⚠`; **pending cell** (sent, not yet confirmed): dim italic
  - **another user editing this cell**: underline in their colour
- **Group rows** (when grouped): `▾ Northwind Retail (7)` with subtotals in numeric columns
  (sum of hours, average %), collapsible.
- Footer/total row for numeric columns over the visible rows.
- Virtualised: only visible rows are rendered (web too).

### Navigation
| Key | Action |
|---|---|
| ↑ ↓ ← → | move current cell |
| Tab / Shift+Tab | next / previous cell, wrapping to next/previous row |
| Home / End | first / last column |
| ^Home / ^End | first / last row |
| PgUp / PgDn | page |
| Enter on a group row, ← / → on a group row | collapse / expand |
| Mouse: click cell, wheel, click header | select; scroll; sort (Shift+click adds a sort key) |
| Mouse: drag header border | resize column |

### Editing (Excel-like)
| Key | Action |
|---|---|
| Enter, F2, double-click | open the cell's editor |
| type a printable character | open the editor seeded with that character (text/number/date) or open the dropdown filtered by it (enum/ref/tags) |
| Del | clear the cell (if the field allows empty) |
| *in editor* Enter | commit, move down |
| *in editor* Tab / Shift+Tab | commit, move right / left |
| *in editor* Esc | cancel |
| Space | mark/unmark row (gutter ●) |
| Shift+↑ / Shift+↓ | extend marked range |
| ^A | mark all visible rows (again: unmark all) |
| ^C | copy: marked rows (visible columns) or current cell, as TSV; also sent to the system clipboard (OSC 52 in TUIs) |
| ^V / terminal paste / browser paste | paste TSV starting at the current cell, filling right and down; values parsed per column; one undo step |
| ^D | fill down: copy the current column's value from the first marked row into the other marked rows (no marks: copy from the row above) |
| ^E | open the record form for the current row |

Every committed edit is a `mutate` with `base` = the value the client
showed when editing began. Multi-cell actions (paste, fill-down, bulk set)
send one `mutate`.

### Cell editors (controls)
| Control | Used for | Behaviour |
|---|---|---|
| **Text input** | name, title, phase, code, text custom fields | cursor, Home/End, ^←/^→ word jump, ^U clear, paste |
| **Number input** | hours, numeric custom fields | accepts digits, `.`, `-`; ↑/↓ add or subtract 1 (Shift: 0.25); invalid input → red, no commit |
| **Date input + calendar popup** | dates | text box accepting natural dates (protocol §10) with a live preview (`fri → Fri 23 Oct 2026`); a month calendar below; ↑/↓ ±1 week, PgUp/PgDn ±1 month, ^←/^→ ±1 day; click a day to pick; `t` button = today |
| **Dropdown select** | status, priority, health, enums | list with coloured values; type to filter; ↑/↓, Enter; first letter jumps |
| **Autocomplete combobox** | owner, assignee, program, project | fuzzy matching over name/username/code with matched characters highlighted, ranked; shows secondary text (role, client); "✚ Create …" entry where creation is allowed (programs, admin) |
| **Chip / tag input** | tags, deps, enum options | chips `[retail ×]`; typing filters suggestions (existing tags by frequency / tasks of the same project as `#5 Title`); Enter adds (or creates a new tag); Backspace on empty removes the last chip; ^Enter / Tab commits |
| **Multiline text area** | notes | in the record form; Enter = newline; Tab leaves the field |
| **Checkbox** | user active | Space toggles |

## 4. Views

`n` = tab number. Default sort shown in brackets.

### 1 Projects
Grid of projects [code ▲], frozen: `Code`, `Name`.
Columns: Code · Name · Program · Owner · Status · Priority · Health · Start · Due ·
Budget h · Est h · Actual h · Burn (bar) · Done (bar, `tasks_done/total`) · Overdue (#) · Tags ·
custom project fields.
**Detail pane** (^B toggles, on by default): a **task grid of the selected project**
(same columns as the Tasks view minus Project), so editing projects and their
tasks is one screen. F6 moves focus between the two grids.
Default group: none. ^R on Program groups by program with subtotals.

### 2 Tasks
All tasks [project code ▲, seq ▲], frozen: `Project`, `#`, `Title`.
Columns: Project · # · Title · Phase · Status · Priority · Assignee · Start · Due ·
Est h · Actual h · Var h · Deps · Tags · custom task fields.
Default group: Project (collapsible tree: `▾ NWR-004 POS refresh (12) ███░ 40%`).
Detail pane: read-only **record card** of the current task (all fields, notes, deps
with their status, recent activity on this task).

### 3 Board (kanban)
Columns = task statuses (To do · In progress · Blocked · Review · Done) with counts
and hour sums. Cards: `#seq Title`, project code, assignee initials, due (coloured),
priority marker, est/actual bar.
**Scope selector** at the top: cascading **Program ▸ Project** comboboxes (`All` allowed);
the query bar also filters cards.
Keys: arrows move between cards and columns; **Shift+← / Shift+→** (also `<` `>`)
move the card to the adjacent status; Enter = record form; Ins = new task in this
column; ^R toggles **swimlanes by assignee**.
Mouse: drag a card to another column (TUI too). Click opens it, wheel scrolls a column.

### 4 Timeline (Gantt)
Left: tree of projects (expand to tasks with → / Enter, collapse with ←), frozen.
Right: time axis with month/week labels, weekend shading, a **today** line, bars
coloured by status (projects: by health), progress fill inside bars, ◆ milestones
(tasks with duration 1 day and est 0), dependency markers (`◂` at the start of a
blocked bar; on web, SVG arrows).
Zoom: `+` / `-` (day ▸ week ▸ month). `t` jumps to today. ←/→ on the right pane
scroll time when not on a bar.
Edit: **Shift+← / Shift+→** move the selected bar ±1 unit (start and due together, one
mutate); **[ / ]** shrink or extend the due date; mouse drag moves a bar,
dragging its right edge resizes it.

### 5 Dashboard
Tiles in a responsive grid, Tab/arrows move focus between tiles, **Enter drills
down** (switches to the Tasks or Projects view with a matching filter):
- KPI row: Active projects · Overdue tasks · Blocked tasks · Hours used / budget (gauge) · Due this week
- Projects by status (horizontal bars) · Tasks by status (stacked bar)
- Hours by owner: budget vs actual (paired bars)
- Workload heatmap: assignee × next 8 weeks (open estimated hours per week), shaded cells
- Activity sparkline (changes per day, last 30 days) and the 8 latest changes
- Top 10 at-risk projects (red/amber health or burn > 90 %) as a mini table

### 6 Activity
Read-only grid of activity [newest first]: Time (relative, e.g. "3m ago") · User · Action ·
Entity · Summary · State (undone entries struck through). Live updates.
Enter jumps to the record. Filter bar works (`@alice`, free text). ^Z / ^Y as everywhere.

### 7 Templates
Master grid of templates (left/top) + detail grid of the selected template's tasks
(Seq · Title · Phase · Est h · Offset d · Duration d). Admins edit; others read only.
A mini Gantt preview of the template under the detail grid (TUI: block chars).

### 8 Fields
Grid of custom fields: Entity · Name · Kind · Options (chip input) · Seq. Admin edits.
Adding a field immediately adds a column to the Projects/Tasks grids of every client.

### 9 Users
Grid: Username · Name · Role · Active · Open tasks · Projects owned. Admin edits; Ins = new
user dialog (username, name, role, password with confirmation); context
menu ▸ Set password. Programs are managed here too (second grid below, F6).

## 5. Dialogs and popups

All dialogs: Tab / Shift+Tab move between fields, **^S or Enter on the [Save] button
saves**, Esc cancels, invalid fields are marked with the message under them, mouse works.
Dialogs are centred overlays with a drop shadow (TUI: shaded cells).

| Dialog | Content |
|---|---|
| **New project** (cascading) | Program (combobox) ▸ Code (pre-filled with the next free code for that program's prefix, e.g. `NWR-008`, updates when the program changes) ▸ Name ▸ Template (select; shows "12 tasks, 214 h, 63 days") ▸ Owner (combobox) ▸ Start (date, default today) ▸ Due (date, default start + template span) ▸ Budget h (default template est total) |
| **New task** (cascading) | Program ▸ Project (filtered by program; preset from context) ▸ Phase (dropdown of this project's phases + its template's phases, or type a new one) ▸ Title ▸ Assignee (default project owner) ▸ Status ▸ Start ▸ Due ▸ Est h |
| **Record form** (^E) | Every field in two columns, custom fields, notes text area, tags/deps chips, plus the record's recent activity. Saving sends one mutate with per-field `base`; conflicting fields are marked and keep your value for retry (Enter on the field = keep theirs, ^S = force mine) |
| **Confirm** | Delete etc. Default button is *Cancel*; `y`/`n` shortcuts |
| **Columns** (F4) | Checklist of columns: Space show/hide, Shift+↑/↓ (or `[`/`]`) move, `+`/`-` width, `r` reset |
| **Sort** (Data ▸ Sort…) | Up to 3 keys, each column + direction |
| **Command palette** (^K) | Fuzzy list of: all actions (with their key), views, "Go to project NWR-004 …", "Go to task NWR-004 #5 …", themes, saved filters. Matched chars highlighted; Enter runs |
| **Help** (F1) | Full-screen, sections per context, a search box filtering the keymap, a short "how pt works" intro |
| **Context menu** | Right-click / Shift+F10 on a cell: Edit · Record form · Copy · Paste · Fill down · Clear · Filter by this value · Exclude this value · Sort ▲/▼ · Group by this column · Hide column · Delete row |
| **Login** (TUI) | Server URL, username, password (masked), "remember me" checkbox; errors inline. Web login is a page with the same fields |
| **Toasts** | Top-right, auto-dismiss after 4 s, stack up to 3: other users' changes affecting what you see ("bob moved #5 to Done"), conflicts, errors |

**Saved filters**: Data ▸ Save filter… stores `{name, view, query, sort, group}` locally
(TUI: a config file; web: localStorage). They appear in the palette and the Data menu.

## 6. Menus

| Menu | Items |
|---|---|
| File | New project · New task · Log out · Quit |
| Edit | Undo · Redo · Copy · Paste · Fill down · Clear cell · Edit record · Delete row(s) |
| View | the 9 views · Toggle detail pane · Columns… · Theme ▸ · Density (comfortable/compact; web) |
| Data | Filter… · Clear filter · Sort by column · Sort… · Group by column · Clear grouping · Save filter… · saved filters |
| Help | Keyboard help · About (server, protocol, client impl, versions) |

Menus: ←/→ between menus, ↑/↓ items, Enter, Esc, underlined letter accelerators,
key shown right-aligned.

## 7. Live collaboration

- **Presence**: users online appear in the status line (initials in their colour). Each
  front end sends `focus` when the current cell changes (debounced 150 ms). Rows that other
  users focus show their initial in the gutter; the exact cell is underlined in their colour.
- **Live updates**: `changes` events update the replica; the grid keeps the cursor on the
  same row id (not the same index). Rows changed by others flash briefly (1 s, accent background).
- **Conflicts**: a `result` with conflicts marks those cells (⚠, red) and toasts
  "bob changed Status to Blocked; your edit wasn't applied". On a conflict cell: Enter =
  edit again from their value; ^S in the editor = force mine; Esc = dismiss.
- **Reconnect**: on disconnect the connection dot turns amber, edits are disabled with a
  message, the client retries with backoff (0.5 s → 5 s), and the snapshot on reconnect
  replaces the replica.

## 8. Colour and themes

Themes: **Midnight** (default dark), **Daylight** (light), **Solarized Dark**, **Gruvbox**.
Each is a palette of named roles: `bg, panel, fg, muted, accent, accent2, border, sel_bg,
cur_bg, ok, warn, err, info` plus status/priority/health colours and 8 user colours.
TUI: truecolor when `COLORTERM` is `truecolor`/`24bit` or the terminal is known to support
it (Windows Terminal: `WT_SESSION` set); otherwise map every colour to the nearest xterm-256
colour (macOS Terminal.app, XFCE with an old VTE). Web themes are CSS custom properties.

| Value | Colour role |
|---|---|
| status: planning / active / on_hold / done / cancelled | info / ok / warn / muted / muted-strikethrough |
| task: todo / in_progress / blocked / review / done | muted / info / err / accent2 / ok |
| priority: low / medium / high / critical | muted / fg / warn / err (bold, `◆`) |
| health: green / amber / red | ok `●` / warn `●` / err `●` |

## 9. Client implementation notes

- Each front end holds a **replica** (from pt-core / internal/pt/core) and derives every view from it.
- `focus`, `mutate`, `undo`, `redo` are the only messages a UI needs; everything else comes
  from `snapshot` / `changes` / `presence`.
- TUI session tokens are cached per server URL in the user config directory
  (`pt/sessions.json`, mode 0600) when "remember me" is checked; `pt logout` deletes it.
- TUIs enable: alternate screen, mouse capture (all motion, so hover works), bracketed paste,
  focus events, and the kitty keyboard protocol where supported (disambiguates ^I/Tab, etc.).

## 10. Implementation notes

Where the four clients differ from this spec (as built, 2026-10-17):

| Area | Rust TUI | Go TUI | Rust web | Go web |
|---|---|---|---|---|
| Theme key | ^T | ^T | View ▸ Theme (Chrome reserves ^T) | View ▸ Theme |
| Board scope | `s` opens Program ▸ Project; Esc clears | same | scope comboboxes above the board | same |
| Timeline dependencies | `◂` marker on waiting bars | same | SVG arrows | SVG arrows |
| Timeline extras | `e` expands/collapses all; `<` `>` scroll | `<` `>` scroll | buttons for zoom/today | same |
| Record form conflicts | Enter = keep theirs, ^S = force mine | same | "Keep theirs" button; Ctrl+Shift+S forces | save again forces |
| Record form layout | one scrolling column | one column, scrolls | two columns | two columns |
| Board column cap | none | none | 120 cards + "N more" | 120 cards + "N more" |
| Saved filters store | query text | query text | query, sort, group | query text |
| Grid hover | – | – | yes | yes |
| Set own password | Users ▸ context menu | File menu, Users ▸ context menu | Users ▸ context menu | record form |
| Colour depth | detect, `--colors`, `PT_COLOR` | detect, `--colors` | – | – |
| Group subtotal for % columns | sums only | sums only | sums only | sums only |

Both TUIs share one key table (identical bindings). Browser clients use the
spec's Web column. The protocol-level behaviour is identical everywhere
(checked by the conformance suite and the cross-language runs).
