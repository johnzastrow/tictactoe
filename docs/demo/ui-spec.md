# demo UI specification

Normative for all four front ends: Rust TUI (ratatui), Go TUI (Bubble Tea v2),
Rust web (Leptos/WASM), Go web (Go/WASM). The web clients mirror the TUIs:
same tabs, controls, keys and help, with a browser's extra freedom
(proportional fonts, hover, smooth drag).

**Purpose:** each control alone on its own tab, so it can be tested,
demonstrated and recorded without distraction. Each tab shows: a title, a
one-sentence explanation, the control(s), a **Keys** panel listing that
tab's keys, and a **Value** panel showing the synced value (`key = value ·
v4 · by bob · 2m ago`).

Audience and style are pt's (see [pt ui-spec](../pt/ui-spec.md)):
conventional keys, keys for the current context always on screen, a menu
bar, a command palette and searchable help. Every action has a keyboard path
and a mouse path. Reuse pt's look: its four themes (Midnight, Daylight,
Solarized Dark, Gruvbox), truecolor with a 256-colour fallback in the TUIs,
drop-shadowed popups.

## 1. Screen layout

```
┌ File  View  Help ──────────────────────────── demo · alice · ● go 0.5.0 ┐  menu bar
│  1 Text      │ Text input                                                │
│  2 Number    │ Single-line fields with live validation.                  │
│  3 Date      │                                                           │
│▸ 4 Dropdown  │  Name   [Ada Lovelace▏                  ]                 │
│  5 Combobox  │  Email  [ada@example.com                ]                 │
│  6 Tags      │  Code   [ENG-042 ]  ✗ must look like ABC-123              │
│  7 Text area │                                                           │
│  8 Choices   │ ┌ Keys ──────────────────┐ ┌ Value (shared) ────────────┐ │
│  9 Cascade   │ │ Tab  next field  …     │ │ text.name = "Ada Lovelace" │ │
│ 10 Grid      │ │ Enter save  Esc undo   │ │ v3 · by bob · 2m ago       │ │
│ 11 Menus     │ └────────────────────────┘ └────────────────────────────┘ │
│ 12 Palette   │                                                           │
│ 13 Dialogs   │                                                           │
│ 14 Charts    │                                                           │
│ 15 Kanban    │                                                           │
│ 16 Gantt     │                                                           │
├──────────────┴───────────────────────────────────────────────────────────┤
│ ✓ Saved text.name               AL BO  online: 2                          │  status line
│ ↑↓ Tab  Enter Open  ^K Commands  F1 Help  F10 Menu  ^Q Quit               │  hint bar
└──────────────────────────────────────────────────────────────────────────┘
```

- **Menu bar**: `File View Help`; right side: `demo · <name> · ● <impl> <version>`
  (dot green connected, amber reconnecting, red offline).
- **Sidebar** (left, ~16 cols): the 16 tabs, numbered. Current tab marked `▸`
  and highlighted. A coloured dot after a tab name for each *other* peer on it.
- **Tab body**: title, explanation, control area, then the Keys and Value
  panels (side by side when wide, stacked when narrow).
- **Status line**: last message (✓ success, ⚠ conflict, ✗ error, ℹ info, or
  "bob changed select.status"), presence initials in each peer's colour,
  online count.
- **Hint bar** (nano style): 1–2 lines of `KEY Label` for the current focus
  (sidebar, a field, an open popup, a dialog). Clicking a hint runs it.
- Minimum TUI size 80×24; smaller shows a "make the window bigger" notice.
- **Toasts**: top-right, auto-dismiss after 4 s, stack up to 3.
- Changed values flash briefly (the Value panel and the control) when a
  `changed` from another peer arrives.

## 2. Focus and navigation

Two focus areas: the **sidebar** and the **tab body**. The app starts with
the sidebar focused on tab 1.

| Key | Sidebar | Tab body |
|---|---|---|
| ↑ / ↓ | previous / next tab (switches immediately) | control-specific |
| Home / End | first / last tab | control-specific |
| Enter, →, Tab | move focus into the tab body (first control) | control-specific |
| Tab / Shift+Tab | – | next / previous control in the tab; Shift+Tab from the first control returns to the sidebar |
| Esc | – | close a popup; with none open, return to the sidebar |
| F6 | toggle sidebar ⇄ tab body | same |

These keys are deliberately enough to reach any tab and control with only
arrows, Tab, Enter and Esc (the recording tool can't send F-keys or Alt).

## 3. Global keys

| Action | TUI | Web |
|---|---|---|
| Help (full-screen, searchable) | F1, ^G | F1, ^G |
| Menu bar | F10, Alt+F / Alt+V / Alt+H | same |
| Command palette | ^K, ^P | ^K, ^P |
| Next / previous tab | ^PgDn / ^PgUp | Alt+↓ / Alt+↑ |
| Go to tab 1–9, 10 | Alt+1…9, Alt+0 | same |
| Reset this tab to defaults | ^R (confirm dialog) | same |
| Theme (cycle) | ^T | View menu (browser owns ^T) |
| Redraw | ^L | – |
| Quit | ^Q | – (File ▸ Leave) |
| Context menu | Shift+F10, Menu key, right-click | same |

### Menus

| Menu | Items |
|---|---|
| File | Reset this tab (^R) · Reset all tabs… · Quit / Leave (^Q) |
| View | the 16 tabs (with Alt+n) · Next tab · Previous tab · Theme ▸ (4) · Toggle Keys panel · Toggle Value panel |
| Help | Keyboard help (F1) · About (server impl/version, client impl/version, protocol) |

←/→ between menus, ↑/↓ items, Enter runs, Esc closes, underlined accelerator
letters, keys right-aligned.

### Command palette (^K)

Fuzzy list (protocol §7) of every action with its key: "Go to: Text" … "Go
to: Gantt", "Reset this tab", "Reset all tabs", "Theme: <name>", "Help",
"About", "Open menu bar", "Quit". Matched characters highlighted; Enter runs;
running one also sets `palette.last` (see tab 12).

### Help (F1 / ^G)

Full screen. A search box at the top filters every line live. Sections:
**This tab** (the current tab's explanation and keys, first), **Getting
around**, **Global keys**, **All tabs** (each tab's keys), **How demo works**
(one paragraph: one server, every client live, per-key versions and
conflicts). ↑/↓/PgUp/PgDn scroll, Esc closes.

## 4. The tabs

Tab ids (used in `focus` messages) in **bold**. "Commit" means sending a
`set` with `base` = the version shown when editing began.

### 1 Text — **text**
Three single-line inputs: **Name** (`text.name`), **Email** (`text.email`),
**Code** (`text.code`, shown uppercase as typed), plus a **Password** field
that is masked (`•`), local only, never sent, with a "show" toggle (^O or
click 👁) — it demonstrates masking.
Keys: ←/→, Home/End, ^←/^→ word jump, Backspace/Del, ^U clear, paste;
**Enter** commits; **Esc** reverts the field to its synced value. Invalid
input turns red with the message under/next to the field and is not sent.
A field whose synced value changes while you are editing it keeps your text
and shows `⚠ changed by bob` (Esc takes theirs).

### 2 Number — **number**
**Hours** (`number.hours`, spinner): digits, `.`, `-`; ↑/↓ ±1, Shift+↑/↓
±0.25 (TUI may also accept `+`/`-`); Enter commits; ▲▼ buttons on web, `[−] [+]`
clickable in the TUI. **Quantity** (`number.qty`, integer spinner, ↑/↓ ±1).
**Percent** (`number.percent`) as a **slider**: ←/→ ±1, PgUp/PgDn ±10,
Home/End 0/100, drawn as a track with a thumb and the value; mouse click /
drag on the track sets it. Slider commits on every change (debounced 150 ms
for drags).

### 3 Date — **date**
**Start** (`date.start`) and **Due** (`date.due`). Each is a text box taking
natural dates (protocol §6) with a live preview (`fri → Fri 23 Oct 2026`),
and a month **calendar** below the focused field: ↑/↓ ±1 week, ^←/^→ ±1
day, PgUp/PgDn ±1 month, today highlighted, the other date of the pair
marked, the range between them shaded. Enter commits (typed text if any,
else the calendar day); `t` button / ^T in the field = today; Del clears.
Due before start → red error, not sent.

### 4 Dropdown — **select**
**Status** (`select.status`) and **Priority** (`select.priority`) as
closed dropdowns showing the value as a coloured chip. Enter/Space/↓ or
click opens a list with coloured values; ↑/↓ move, typing filters, the
first letter jumps, Enter picks and commits, Esc closes.

### 5 Combobox — **combo**
**Owner** (`combo.owner`) and **Reviewer** (`combo.reviewer`). Typing opens
a fuzzy list over People: display name with matched letters highlighted,
then username and team dimmed; ↑/↓, Enter picks and commits, Esc closes,
Del clears to null (shown "— none —"). The list also opens on ↓ or click.

### 6 Tags — **tags**
`tags.tags` as chips `[frontend ×] [q4 ×]` then an input. Typing filters
the suggestions (tag + use count, ranked as protocol §4, excluding present
ones); ↑/↓ select; Enter adds the selected suggestion, or the typed text as
a new tag (`✚ Create "xyz"` entry); invalid text red. Backspace in the empty
input removes the last chip; ←/→ in the empty input select a chip, Del
removes it; clicking × removes. Every add/remove commits.

### 7 Text area — **textarea**
`textarea.notes` in a multi-line box: Enter = newline, arrows, Home/End,
PgUp/PgDn, ^Home/^End, word wrap, scroll. A footer shows `Ln 2, Col 14 · 23
words · 141/5000`. **^S** commits; Esc reverts (asks first if edited);
leaving with Tab keeps the draft and marks the field "● unsaved".

### 8 Choices — **choice**
**Notify me** checkbox (`choice.notify`); **Channels** checkbox group
(`choice.channels`: Email, SMS, Push, Slack); **Size** radio group
(`choice.size`: S M L XL) shown horizontally; **Dark mode** toggle switch
(`choice.dark`, `[ ●━ ] Off` / `[ ━● ] On`). Space toggles/selects, ↑/↓ or
←/→ move within a group, Tab moves between groups; clicking works. Every
change commits. When Notify is off, Channels is shown disabled (dim) but
still focusable, and Space there shows "ℹ turn on Notify first".

### 9 Cascade — **cascade**
Three dependent dropdowns for `cascade.pick`: **Program** ▸ **Project**
(only that program's projects; disabled until a program is picked) ▸
**Phase** (only that project's phases). Changing Program clears Project and
Phase; changing Project clears Phase. Each pick commits the whole object.
Below: a breadcrumb `Northwind Retail ▸ POS refresh ▸ Build` and the count
of choices at each level ("3 projects", "4 phases"). Lists behave as tab 4's
(type to filter, Enter picks).

### 10 Grid — **grid**
An editable grid of `grid.rows`: columns # (id, read-only), Item, Qty,
Price, Total (qty × price, computed, read-only), Status, Due. Header shows
sort arrows; a totals row (sum of Qty and Total). Current cell highlighted,
zebra rows.
Keys: arrows, Tab/Shift+Tab, Home/End; **Enter/F2** or typing edits the
cell with the right editor (text, number, status dropdown, natural date);
Enter commits and moves down, Esc cancels; **Del** clears Due; **Ins**
appends a row (`New item`, qty 1, price 0, todo, no due) and edits Item;
**F8** / **^Del** deletes the row (confirm dialog); **^O** cycles the sort on
the current column (▲ ▼ off — sorting is a *view*, it does not change the
stored order). Mouse: click a cell, double-click edits, click a header sorts.

### 11 Menus — **menus**
Two things: (a) a **demo menu bar** inside the tab (`Document  Format
Arrange`) with submenus (Format ▸ Text ▸ Bold/Italic…; items with keys,
separators, a disabled item, a checkable item ✓); Enter or ↓ opens it when
focused, ←/→ between menus, → opens a submenu; picking an item only shows
"ℹ Picked Format ▸ Text ▸ Bold". (b) a **file list** (`menus.files`, name
with ★ for starred). **Enter** on a file (also Shift+F10, Menu key,
right-click) opens its **context menu**: Open · Rename… · Duplicate · Star /
Unstar · Delete… (the last confirmed). Rename opens a small input dialog.
Changes commit `menus.files`.

### 12 Palette — **palette**
The command palette (^K) is global; this tab shows it **inline** and always
open: the same component and list, with a search box focused on entering
the tab. Typing filters, matched letters highlighted, Enter runs the command
exactly as from ^K. Above it: "Last command: <palette.last> · by bob"
(every run, from here or ^K, sets `palette.last` to the command's label).

### 13 Dialogs — **dialogs**
A row of buttons, each opening one kind of popup (Enter/Space or click):
- **Confirm…**: "Delete 3 files?" with Cancel (default) / Delete; `y`/`n`
  shortcuts. Result shown as ℹ status message.
- **Form…**: edit `dialog.profile` (Name, Email) in a modal: Tab between
  fields, invalid fields marked with the message under them, **^S** or Enter
  on [Save] commits, Esc cancels.
- **Message**: an information box with a single OK.
- **Toast**: shows a local toast.
- **Broadcast…**: a one-line input; Enter sends a `toast` to everyone.
The tab also shows the current `dialog.profile`.

### 14 Charts — **charts**
`charts.series` (12 months, Jan–Dec) drawn four ways: a **vertical bar
chart** (TUI: eighth-block bars, value labels; web: SVG or CSS bars), a
**sparkline**, a **gauge** of the average (0–100, coloured green < 60 ≤ amber
< 80 ≤ red), and a **heat strip** (12 shaded cells). ←/→ select a month
(highlighted in all four), ↑/↓ ±5, PgUp/PgDn ±20 (clamped 0–100), each
change commits. Mouse: click a bar to select; web may drag the bar top.

### 15 Kanban — **kanban**
Four columns To do · Doing · Review · Done with counts; cards from
`kanban.cards`. ←/→ move between columns, ↑/↓ between cards;
**Shift+←/→** (also `<` `>`) moves the card to the adjacent column; **Ins**
new card in the current column (title prompt); **Enter** renames; **F8**/**^Del**
deletes (confirm). Mouse: drag a card to another column (TUI too), click
selects. Each change commits the whole list.

### 16 Gantt — **gantt**
Left: bar labels; right: a time axis (week ticks with dates, weekend
shading, a **today** line) and bars from `gantt.bars`, the selected one
highlighted with its dates shown in the status of the tab. ↑/↓ select a bar;
**Shift+←/→** move the bar ±1 day (also `<` `>`); **[ / ]** shrink /
extend by 1 day; **+ / -** zoom (day ▸ week ▸ month scale); **t** scrolls to
today; ←/→ scroll the time axis. Mouse: drag a bar to move it, drag its right
edge to resize. Each change commits the whole list (drags commit on release).

## 5. Join

- **TUI**: `demo tui --name alice` connects directly. Without `--name`, a
  join form (Server URL, Name; Enter or ^S joins) is shown; errors inline.
- **Web**: a join page (Name, Join button); the name is remembered in
  `localStorage`; `?name=alice` in the URL joins directly. File ▸ Leave
  returns to the join page.
- Reconnect automatically with backoff (amber dot); on reconnect the
  `welcome` replaces all values.

## 6. Conflicts

If a commit gets `conflict`, the control shows the server's value, the
status line says `⚠ bob changed <key> first, showing theirs`, and the Value
panel flashes. (Lists: the user's edit is lost; that is acceptable for a
demo and is the visible behaviour being demonstrated.)
