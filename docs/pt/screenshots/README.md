# pt: form controls, web and terminal side by side

Each control is shown open in the **web client** (left or top) and the
**TUI** (right or bottom) of the same language, cropped to the control. The
**recording** link under each pair is the TUI being driven from the keyboard
by a [VHS](https://github.com/charmbracelet/vhs) tape.

- [Rust](#rust): Leptos → WASM in the browser, ratatui in the terminal
- [Go](#go): Go → WASM (`syscall/js`) in the browser, Bubble Tea in the terminal
- [Regenerating](#regenerating)

What each control does is in the [usage guide](../USAGE.md#6-editing-in-the-grid) and
the [UI spec](../ui-spec.md#cell-editors-controls).

## Rust

**Tour of the nine views** (switched from the command palette):

![Rust TUI: tour of the views](rust/views.gif)

| Control | Web · Leptos  \|  Terminal · ratatui |
|---|---|
| **Fuzzy combobox**: type in the Owner cell; matched letters highlighted, with username and role · [recording](rust/01-combobox.gif) | ![](rust/01-combobox.png) |
| **Dropdown select**: Status values in their colours · [recording](rust/02-dropdown.gif) | ![](rust/02-dropdown.png) |
| **Date editor**: `fri`, `+3w`, `eom` resolve as you type, with a calendar · [recording](rust/03-date.gif) | ![](rust/03-date.png) |
| **Tag chips**: existing tags ranked by use, or create one · [recording](rust/04-tags.gif) | ![](rust/04-tags.png) |
| **Query bar**: field and value completion · [recording](rust/05-query.gif) | ![](rust/05-query.png) |
| **Command palette** (Ctrl+K): actions, views, projects and tasks · [recording](rust/06-palette.gif) | ![](rust/06-palette.png) |
| **Context menu** (Shift+F10 or right-click) · [recording](rust/07-context-menu.gif) | ![](rust/07-context-menu.png) |
| **Column chooser** (F4) · [recording](rust/08-columns.gif) | ![](rust/08-columns.png) |
| **Record form** (Ctrl+E) · [recording](rust/09-record.gif) | ![](rust/09-record.png) |
| **New project, cascading**: program → next free code; template → task count, hours, dates, budget · [recording](rust/10-new-project.gif) | ![](rust/10-new-project.png) |
| **New task, cascading**: program → project → that template's phases · [recording](rust/11-new-task.gif) | ![](rust/11-new-task.png) |
| **Dependency chips**: tasks of the same project by number or title · [recording](rust/12-deps.gif) | ![](rust/12-deps.png) |

## Go

**Tour of the nine views:**

![Go TUI: tour of the views](go/views.gif)

| Control | Web · Go WASM  \|  Terminal · Bubble Tea |
|---|---|
| **Fuzzy combobox** · [recording](go/01-combobox.gif) | ![](go/01-combobox.png) |
| **Dropdown select** · [recording](go/02-dropdown.gif) | ![](go/02-dropdown.png) |
| **Date editor** · [recording](go/03-date.gif) | ![](go/03-date.png) |
| **Tag chips** · [recording](go/04-tags.gif) | ![](go/04-tags.png) |
| **Query bar** · [recording](go/05-query.gif) | ![](go/05-query.png) |
| **Command palette** · [recording](go/06-palette.gif) | ![](go/06-palette.png) |
| **Context menu** · [recording](go/07-context-menu.gif) | ![](go/07-context-menu.png) |
| **Column chooser** · [recording](go/08-columns.gif) | ![](go/08-columns.png) |
| **Record form** · [recording](go/09-record.gif) | ![](go/09-record.png) |
| **New project, cascading** · [recording](go/10-new-project.gif) | ![](go/10-new-project.png) |
| **New task, cascading** · [recording](go/11-new-task.gif) | ![](go/11-new-task.png) |
| **Dependency chips** · [recording](go/12-deps.gif) | ![](go/12-deps.png) |

Differences worth spotting: the Go web palette lists projects before tasks
while the Go TUI palette ranks tasks first; the Go web record form is a
two-column grid, the TUIs a single column.

## Regenerating

```sh
cd rust && make pt && cd ../go && make pt && cd ..
docs/pt/screenshots/make.sh            # both languages (about 25 minutes)
docs/pt/screenshots/make.sh rust       # or one
```

Needs `vhs` (with `ttyd` and `ffmpeg`), `uv` and Google Chrome. Each run
seeds a throwaway database with a demo password on a spare port (8799 Rust,
8798 Go), so your own server and data are not touched.

| File | Role |
|---|---|
| [`make.sh`](make.sh) | seeds, serves, runs the steps below, shrinks the GIFs |
| [`capture_web.py`](capture_web.py) | Playwright: opens each control in the web client and clips a 2× screenshot to it |
| [`tapes/`](tapes/) | one VHS tape per control (shared by both TUIs, which have the same keys), plus `_setup.tape` (terminal size, login) and `views.tape` |
| [`compose.py`](compose.py) | crops the TUI frames and builds the side-by-side images |

VHS can't send F-keys or Alt+digit, so the tapes reach the context menu,
column chooser and views through the command palette, which lists every
action.
