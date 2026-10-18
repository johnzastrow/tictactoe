# demo: every tab in all four clients

demo has four front ends: a **web** client and a **terminal** (TUI) client in
each of two languages. Every image here says which one you are looking at:

| | Web client (browser) | Terminal client (TUI) |
|---|---|---|
| **RUST** | Leptos, compiled to WASM | ratatui + crossterm |
| **GO** | Go compiled to WASM, hand-written DOM kit (`syscall/js`) | Bubble Tea v2 + Lip Gloss v2 |

In the images the language is always named, Rust in orange and Go in blue.
Unlike [pt's gallery](../../pt/screenshots/README.md), these are **whole
screens**, not crops: each tab holds one control, so the full screen (with
its Keys panel, Value panel and hint bar) is the point.

- [All 16 tabs at a glance](#all-16-tabs-at-a-glance)
- [Rust vs Go](#rust-vs-go-all-four-clients-per-control): all four clients
  per control, as a still and a synced recording
- [Rust only](#rust-web-and-terminal) · [Go only](#go-web-and-terminal):
  web and terminal side by side, with a recording of each
- [Tours](#tours-terminal) · [Regenerating](#regenerating)

What each tab does: [usage guide](../USAGE.md#5-the-tabs-what-to-try),
[UI spec](../ui-spec.md#4-the-tabs).

## All 16 tabs at a glance

Each tab as it first appears, before anything is opened.

| | Web | Terminal |
|---|---|---|
| **RUST** | ![Rust web: all 16 tabs](compare/tabs-rust-web.png) | ![Rust terminal: all 16 tabs](compare/tabs-rust-tui.png) |
| **GO** | ![Go web: all 16 tabs](compare/tabs-go-web.png) | ![Go terminal: all 16 tabs](compare/tabs-go-tui.png) |

## Rust vs Go: all four clients, per control

Each image is a 2×2 grid: **Rust on the left, Go on the right; web on top,
terminal below**. The **▶ recording** link plays the four clients side by side
in the same layout, lined up on the moment the control opens. All four were
driven by exactly the same keys.

| Control | Rust (left) vs Go (right), web (top) vs terminal (bottom) |
|---|---|
| **Text input with live validation** (tab 1 Text) · [▶ recording](compare/01-text.gif) | ![Rust vs Go, web vs terminal: Text input with live validation](compare/01-text.png) |
| **Number spinner and slider** (tab 2 Number) · [▶ recording](compare/02-number.gif) | ![Rust vs Go, web vs terminal: Number spinner and slider](compare/02-number.png) |
| **Natural-language date with calendar** (tab 3 Date) · [▶ recording](compare/03-date.gif) | ![Rust vs Go, web vs terminal: Natural-language date with calendar](compare/03-date.png) |
| **Dropdown select** (tab 4 Dropdown) · [▶ recording](compare/04-select.gif) | ![Rust vs Go, web vs terminal: Dropdown select](compare/04-select.png) |
| **Fuzzy combobox** (tab 5 Combobox) · [▶ recording](compare/05-combo.gif) | ![Rust vs Go, web vs terminal: Fuzzy combobox](compare/05-combo.png) |
| **Tag chips with suggestions** (tab 6 Tags) · [▶ recording](compare/06-tags.gif) | ![Rust vs Go, web vs terminal: Tag chips with suggestions](compare/06-tags.png) |
| **Multi-line text area** (tab 7 Text area) · [▶ recording](compare/07-textarea.gif) | ![Rust vs Go, web vs terminal: Multi-line text area](compare/07-textarea.png) |
| **Checkboxes, radio buttons, toggle** (tab 8 Choices) · [▶ recording](compare/08-choice.gif) | ![Rust vs Go, web vs terminal: Checkboxes, radio buttons, toggle](compare/08-choice.png) |
| **Cascading selects** (tab 9 Cascade) · [▶ recording](compare/09-cascade.gif) | ![Rust vs Go, web vs terminal: Cascading selects](compare/09-cascade.png) |
| **Editable data grid** (tab 10 Grid) · [▶ recording](compare/10-grid.gif) | ![Rust vs Go, web vs terminal: Editable data grid](compare/10-grid.png) |
| **Menu bar with submenus** (tab 11 Menus) · [▶ recording](compare/11-menus.gif) | ![Rust vs Go, web vs terminal: Menu bar with submenus](compare/11-menus.png) |
| **Context menu** (tab 11 Menus) · [▶ recording](compare/12-context-menu.gif) | ![Rust vs Go, web vs terminal: Context menu](compare/12-context-menu.png) |
| **Command palette** (tab 12 Palette) · [▶ recording](compare/13-palette.gif) | ![Rust vs Go, web vs terminal: Command palette](compare/13-palette.png) |
| **Modal form dialog** (tab 13 Dialogs) · [▶ recording](compare/14-dialog.gif) | ![Rust vs Go, web vs terminal: Modal form dialog](compare/14-dialog.png) |
| **Bar chart, sparkline, gauge, heat strip** (tab 14 Charts) · [▶ recording](compare/15-charts.gif) | ![Rust vs Go, web vs terminal: Bar chart, sparkline, gauge, heat strip](compare/15-charts.png) |
| **Kanban board** (tab 15 Kanban) · [▶ recording](compare/16-kanban.gif) | ![Rust vs Go, web vs terminal: Kanban board](compare/16-kanban.png) |
| **Gantt chart** (tab 16 Gantt) · [▶ recording](compare/17-gantt.gif) | ![Rust vs Go, web vs terminal: Gantt chart](compare/17-gantt.png) |
| **Searchable help** (tab 3 Date) · [▶ recording](compare/18-help.gif) | ![Rust vs Go, web vs terminal: Searchable help](compare/18-help.png) |

## Rust: web and terminal

**RUST · Web** (Leptos → WASM) on the left, **RUST · Terminal** (ratatui) on the right.

| Control | RUST · Web vs RUST · Terminal | Recordings |
|---|---|---|
| **Text input with live validation** (tab 1 Text) | ![Rust: Text input with live validation](rust/01-text.png) | [Rust web](rust/web-01-text.gif) · [Rust terminal](rust/tui-01-text.gif) |
| **Number spinner and slider** (tab 2 Number) | ![Rust: Number spinner and slider](rust/02-number.png) | [Rust web](rust/web-02-number.gif) · [Rust terminal](rust/tui-02-number.gif) |
| **Natural-language date with calendar** (tab 3 Date) | ![Rust: Natural-language date with calendar](rust/03-date.png) | [Rust web](rust/web-03-date.gif) · [Rust terminal](rust/tui-03-date.gif) |
| **Dropdown select** (tab 4 Dropdown) | ![Rust: Dropdown select](rust/04-select.png) | [Rust web](rust/web-04-select.gif) · [Rust terminal](rust/tui-04-select.gif) |
| **Fuzzy combobox** (tab 5 Combobox) | ![Rust: Fuzzy combobox](rust/05-combo.png) | [Rust web](rust/web-05-combo.gif) · [Rust terminal](rust/tui-05-combo.gif) |
| **Tag chips with suggestions** (tab 6 Tags) | ![Rust: Tag chips with suggestions](rust/06-tags.png) | [Rust web](rust/web-06-tags.gif) · [Rust terminal](rust/tui-06-tags.gif) |
| **Multi-line text area** (tab 7 Text area) | ![Rust: Multi-line text area](rust/07-textarea.png) | [Rust web](rust/web-07-textarea.gif) · [Rust terminal](rust/tui-07-textarea.gif) |
| **Checkboxes, radio buttons, toggle** (tab 8 Choices) | ![Rust: Checkboxes, radio buttons, toggle](rust/08-choice.png) | [Rust web](rust/web-08-choice.gif) · [Rust terminal](rust/tui-08-choice.gif) |
| **Cascading selects** (tab 9 Cascade) | ![Rust: Cascading selects](rust/09-cascade.png) | [Rust web](rust/web-09-cascade.gif) · [Rust terminal](rust/tui-09-cascade.gif) |
| **Editable data grid** (tab 10 Grid) | ![Rust: Editable data grid](rust/10-grid.png) | [Rust web](rust/web-10-grid.gif) · [Rust terminal](rust/tui-10-grid.gif) |
| **Menu bar with submenus** (tab 11 Menus) | ![Rust: Menu bar with submenus](rust/11-menus.png) | [Rust web](rust/web-11-menus.gif) · [Rust terminal](rust/tui-11-menus.gif) |
| **Context menu** (tab 11 Menus) | ![Rust: Context menu](rust/12-context-menu.png) | [Rust web](rust/web-12-context-menu.gif) · [Rust terminal](rust/tui-12-context-menu.gif) |
| **Command palette** (tab 12 Palette) | ![Rust: Command palette](rust/13-palette.png) | [Rust web](rust/web-13-palette.gif) · [Rust terminal](rust/tui-13-palette.gif) |
| **Modal form dialog** (tab 13 Dialogs) | ![Rust: Modal form dialog](rust/14-dialog.png) | [Rust web](rust/web-14-dialog.gif) · [Rust terminal](rust/tui-14-dialog.gif) |
| **Bar chart, sparkline, gauge, heat strip** (tab 14 Charts) | ![Rust: Bar chart, sparkline, gauge, heat strip](rust/15-charts.png) | [Rust web](rust/web-15-charts.gif) · [Rust terminal](rust/tui-15-charts.gif) |
| **Kanban board** (tab 15 Kanban) | ![Rust: Kanban board](rust/16-kanban.png) | [Rust web](rust/web-16-kanban.gif) · [Rust terminal](rust/tui-16-kanban.gif) |
| **Gantt chart** (tab 16 Gantt) | ![Rust: Gantt chart](rust/17-gantt.png) | [Rust web](rust/web-17-gantt.gif) · [Rust terminal](rust/tui-17-gantt.gif) |
| **Searchable help** (tab 3 Date) | ![Rust: Searchable help](rust/18-help.png) | [Rust web](rust/web-18-help.gif) · [Rust terminal](rust/tui-18-help.gif) |

## Go: web and terminal

**GO · Web** (Go → WASM, `syscall/js`) on the left, **GO · Terminal** (Bubble Tea) on the right.

| Control | GO · Web vs GO · Terminal | Recordings |
|---|---|---|
| **Text input with live validation** (tab 1 Text) | ![Go: Text input with live validation](go/01-text.png) | [Go web](go/web-01-text.gif) · [Go terminal](go/tui-01-text.gif) |
| **Number spinner and slider** (tab 2 Number) | ![Go: Number spinner and slider](go/02-number.png) | [Go web](go/web-02-number.gif) · [Go terminal](go/tui-02-number.gif) |
| **Natural-language date with calendar** (tab 3 Date) | ![Go: Natural-language date with calendar](go/03-date.png) | [Go web](go/web-03-date.gif) · [Go terminal](go/tui-03-date.gif) |
| **Dropdown select** (tab 4 Dropdown) | ![Go: Dropdown select](go/04-select.png) | [Go web](go/web-04-select.gif) · [Go terminal](go/tui-04-select.gif) |
| **Fuzzy combobox** (tab 5 Combobox) | ![Go: Fuzzy combobox](go/05-combo.png) | [Go web](go/web-05-combo.gif) · [Go terminal](go/tui-05-combo.gif) |
| **Tag chips with suggestions** (tab 6 Tags) | ![Go: Tag chips with suggestions](go/06-tags.png) | [Go web](go/web-06-tags.gif) · [Go terminal](go/tui-06-tags.gif) |
| **Multi-line text area** (tab 7 Text area) | ![Go: Multi-line text area](go/07-textarea.png) | [Go web](go/web-07-textarea.gif) · [Go terminal](go/tui-07-textarea.gif) |
| **Checkboxes, radio buttons, toggle** (tab 8 Choices) | ![Go: Checkboxes, radio buttons, toggle](go/08-choice.png) | [Go web](go/web-08-choice.gif) · [Go terminal](go/tui-08-choice.gif) |
| **Cascading selects** (tab 9 Cascade) | ![Go: Cascading selects](go/09-cascade.png) | [Go web](go/web-09-cascade.gif) · [Go terminal](go/tui-09-cascade.gif) |
| **Editable data grid** (tab 10 Grid) | ![Go: Editable data grid](go/10-grid.png) | [Go web](go/web-10-grid.gif) · [Go terminal](go/tui-10-grid.gif) |
| **Menu bar with submenus** (tab 11 Menus) | ![Go: Menu bar with submenus](go/11-menus.png) | [Go web](go/web-11-menus.gif) · [Go terminal](go/tui-11-menus.gif) |
| **Context menu** (tab 11 Menus) | ![Go: Context menu](go/12-context-menu.png) | [Go web](go/web-12-context-menu.gif) · [Go terminal](go/tui-12-context-menu.gif) |
| **Command palette** (tab 12 Palette) | ![Go: Command palette](go/13-palette.png) | [Go web](go/web-13-palette.gif) · [Go terminal](go/tui-13-palette.gif) |
| **Modal form dialog** (tab 13 Dialogs) | ![Go: Modal form dialog](go/14-dialog.png) | [Go web](go/web-14-dialog.gif) · [Go terminal](go/tui-14-dialog.gif) |
| **Bar chart, sparkline, gauge, heat strip** (tab 14 Charts) | ![Go: Bar chart, sparkline, gauge, heat strip](go/15-charts.png) | [Go web](go/web-15-charts.gif) · [Go terminal](go/tui-15-charts.gif) |
| **Kanban board** (tab 15 Kanban) | ![Go: Kanban board](go/16-kanban.png) | [Go web](go/web-16-kanban.gif) · [Go terminal](go/tui-16-kanban.gif) |
| **Gantt chart** (tab 16 Gantt) | ![Go: Gantt chart](go/17-gantt.png) | [Go web](go/web-17-gantt.gif) · [Go terminal](go/tui-17-gantt.gif) |
| **Searchable help** (tab 3 Date) | ![Go: Searchable help](go/18-help.png) | [Go web](go/web-18-help.gif) · [Go terminal](go/tui-18-help.gif) |

## Tours (terminal)

The sidebar walked from tab 1 to 16 with ↓.

**RUST · Terminal (ratatui)**

![Rust terminal: tour of the tabs](rust/tui-tour.gif)

**GO · Terminal (Bubble Tea)**

![Go terminal: tour of the tabs](go/tui-tour.gif)

## Regenerating

```sh
cd rust && make demo && cd ../go && make demo && cd ..
docs/demo/screenshots/make.sh            # both languages + Rust vs Go
docs/demo/screenshots/make.sh rust       # or one language
```

Needs `vhs` (with `ttyd` and `ffmpeg`), `uv` and Google Chrome. Each pass
serves a fresh database on a spare port (18797 Rust, 18796 Go), so your own
server and data are not touched.

| File | Role |
|---|---|
| [`steps.py`](steps.py) | **the one script**: for each control, the keys to press, when it opens and when to take the still |
| [`gen_tapes.py`](gen_tapes.py) | turns `steps.py` into one VHS tape per control in [`tapes/`](tapes/), plus the tour (generated; don't edit the tapes) |
| [`capture_web.py`](capture_web.py) | replays `steps.py` in a web client with Playwright at the tapes' typing pace: 2× stills, then a recorded session |
| [`cut_web.py`](cut_web.py) | cuts the web session into one clip per control |
| [`compose.py`](compose.py) | web vs terminal stills for one language |
| [`compose4.py`](compose4.py) | Rust vs Go 2×2 stills, synced recordings and the contact sheets |
| [`make.sh`](make.sh) | runs all of the above for each language |

VHS can't send F-keys, Home/End, Shift+Tab or Shift+arrows, so the steps use
only arrows, Tab, Enter, Esc, Space, Ctrl+letter and typed characters (`>`
moves a kanban card or Gantt bar, `]` extends a bar). Items that change a
value end with ^R (reset tab) so the next item starts from the defaults.
