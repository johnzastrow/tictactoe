# demo usage guide

How to run demo, get around it, and what to try on each tab. Everything
here works the same in all four clients (Rust or Go; terminal or browser)
unless a note says otherwise. The normative details are in
[ui-spec.md](ui-spec.md).

- [1. Start it](#1-start-it)
- [2. Join](#2-join)
- [3. The screen](#3-the-screen)
- [4. Getting around](#4-getting-around)
- [5. The tabs: what to try](#5-the-tabs-what-to-try)
- [6. Working with other people](#6-working-with-other-people)
- [7. Keyboard reference](#7-keyboard-reference)
- [8. Serving to other machines](#8-serving-to-other-machines)
- [9. Troubleshooting](#9-troubleshooting)
- [10. Command reference](#10-command-reference)

## 1. Start it

Build once, then start a server. Either language's server works with
either language's clients.

```sh
cd rust && make demo && ./target/release/demo serve     # or:
cd go   && make demo && ./bin/demo serve
```

The server listens on `http://127.0.0.1:8790` and keeps its state in
`demo.db` in the current folder (`--db` to change). Leave it running.

## 2. Join

- **Terminal:** `demo tui --name alice`. Without `--name` a small join form
  asks for the server URL and your name (Enter or ^S joins).
- **Browser:** open `http://127.0.0.1:8790`, type a name, press Join. The
  name is remembered; `http://127.0.0.1:8790/?name=alice` joins directly.
  File ▸ Leave goes back to the join page.

A name is 1–24 letters, digits, spaces or `_ . -`, starting with a letter.
It is only a label: there are no passwords, and two people may pick the
same name.

## 3. The screen

```
 File  View  Help ─────────────────────────── demo · alice · ● go 0.5.0   menu bar, connection
 sidebar: 16 tabs │ title and one-line explanation
                  │ the control(s)
                  │ ┌ Keys ───────────┐ ┌ Value (shared) ─────────────────┐
                  │ │ this tab's keys │ │ key = value · v4 · by bob · 2m  │
 status line: ✓ Saved select.status          AL BO  online: 2
 hint bar:    Enter Pick  Esc Close  ^K Commands  F1 Help  F10 Menu  ^Q Quit
```

- The **dot** at the top right is green when connected, amber while
  reconnecting, red when the server can't be reached.
- The **sidebar** shows a coloured dot next to a tab for each other person
  on it.
- The **Value** panel shows what the server holds, its version, who
  changed it and when. It flashes when someone else changes it.
- The **hint bar** always lists the keys for what has focus right now;
  click a hint to run it.

## 4. Getting around

The app starts with the **sidebar** focused on tab 1.

| Key | Does |
|---|---|
| ↑ ↓ (sidebar) | previous / next tab; the tab switches at once |
| 1–9 (sidebar, terminal) | jump to that tab |
| Enter, →, Tab (sidebar) | into the tab, on its first control |
| Tab / Shift+Tab | next / previous control in the tab |
| Esc | close a popup; with none open, back to the sidebar |
| F6 | sidebar ⇄ tab |
| ^PgDn / ^PgUp (web: Alt+↓ / Alt+↑) | next / previous tab from anywhere |
| Alt+1…9, Alt+0 | go to tab 1–10 |
| ^K or ^P | command palette: type part of any command or tab name |
| F1 or ^G | help: searchable, with this tab's keys first |
| F10, Alt+F / V / H | menu bar |
| ^R | reset this tab to its defaults (asks first; affects everyone) |
| ^T (web: View ▸ Theme) | next colour theme |
| ^Q (web: File ▸ Leave) | quit |

The mouse works everywhere: click tabs, fields, list items, buttons and
hints; drag sliders, kanban cards, Gantt bars and chart bars.

## 5. The tabs: what to try

| # | Tab | Try this |
|---|---|---|
| 1 | Text | Enter into Name, change it, Enter saves. In Code, type `ab-12`: it turns red with the rule, and nothing is sent. Esc reverts. The password field never leaves your client; ^O shows it. |
| 2 | Number | ↑/↓ step Hours by 1, Shift+↑/↓ by 0.25; Enter saves. Tab twice to the slider: ←/→, PgUp/PgDn, Home/End; every move saves. |
| 3 | Date | Type `fri`, `+2w`, `eom`, `23 oct` or `tomorrow` and watch the preview; ↑/↓ move a week in the calendar, PgUp/PgDn a month; Enter saves. A Due before Start is refused. |
| 4 | Dropdown | Enter opens the list; type to filter or press a first letter; Enter picks. |
| 5 | Combobox | Type a few letters of a name, username or team (`gl`, `eng`); matched letters are highlighted. Del clears to none. |
| 6 | Tags | Type to filter suggestions (ranked by use); Enter adds; a new word offers `✚ Create`. Backspace in the empty box removes the last chip. |
| 7 | Text area | Type freely; Enter is a new line; ^S saves; Esc asks before throwing your edit away. The footer counts lines, words and characters. |
| 8 | Choices | Space toggles; ←/→ move within a group; Tab to the next group. With Notify off, Channels is greyed out. |
| 9 | Cascade | Pick a Program, then a Project (only that program's), then a Phase (only that project's). Changing a level clears the ones below. |
| 10 | Grid | Arrows move; Enter or typing edits with the right editor (text, number, status list, date); Ins adds a row; F8 deletes; ^O sorts the view. Total and the totals row are computed. |
| 11 | Menus | Enter opens the demo menu bar: → to Format, Enter opens Text ▸, Enter picks. Tab to the file list; Enter (or Shift+F10, or right-click) opens a file's context menu: Open, Rename, Duplicate, Star, Delete. |
| 12 | Palette | The palette that ^K opens anywhere, shown inline. Type `gan` and Enter to jump to Gantt; the last command run is shared. |
| 13 | Dialogs | ←/→ pick a button, Enter opens: Confirm (y/n), Form (edit the shared profile; ^S saves), Message, Toast, Broadcast (a toast to everyone). |
| 14 | Charts | ←/→ pick a month, ↑/↓ change it by 5; the bars, sparkline, gauge and heat strip all follow. |
| 15 | Kanban | Arrows select; `>`/`<` or Shift+→/← move a card to the next column; Ins adds, Enter renames, F8 deletes. Drag cards with the mouse. |
| 16 | Gantt | ↑/↓ pick a bar; `>`/`<` move it a day; `]`/`[` lengthen or shorten it; `+`/`-` zoom; `t` shows today. Drag a bar, or its right edge. |

## 6. Working with other people

Run several clients (any mix of terminals and browsers, Rust and Go)
against one server:

- Every saved change appears in every client at once, and the status line
  says who made it.
- The status line shows everyone's initials in their colour; the sidebar
  shows who is on which tab.
- **Conflicts:** if you start editing a field and someone else saves it
  first, your field shows `⚠ changed by bob` and keeps your text. Esc takes
  theirs; Enter tries yours, which the server refuses because it was based
  on an old version, and you see their value.
- **Broadcast** (Dialogs tab) sends a toast to everyone.
- **^R** resets the current tab for everyone.

## 7. Keyboard reference

Global keys are in [§4](#4-getting-around); each tab's keys are in its
Keys panel, in help (F1) and in [ui-spec.md §4](ui-spec.md#4-the-tabs).
Terminal notes:

- macOS Terminal: to use Alt+digits and Alt+letters, turn on "Use Option
  as Meta key" (Settings ▸ Profiles ▸ Keyboard).
- Some terminals take F1 or F10 for themselves; ^G opens help and ^K
  lists every command, including "Open menu bar".
- `--colors 256` forces the 256-colour palette; `--theme gruvbox` picks a
  theme (`midnight`, `daylight`, `solarized-dark`, `gruvbox`).

## 8. Serving to other machines

The server binds to `127.0.0.1` and refuses any other address without TLS:

```sh
# LAN with a certificate (e.g. from mkcert)
demo serve --bind 0.0.0.0:8790 --tls-cert host.pem --tls-key host-key.pem
demo tui --server https://host:8790 --name alice --ca-cert "$(mkcert -CAROOT)/rootCA.pem"

# Tailscale / VPN (already encrypted)
demo serve --bind 100.x.y.z:8790 --allow-plaintext
```

Anyone who can reach the port can join and change values: demo has no
accounts.

## 9. Troubleshooting

| Problem | Fix |
|---|---|
| "make the window bigger" | The terminal needs at least 80×24. |
| Red dot / "can't reach" | Is `demo serve` running, on that URL and port? |
| Browser shows "web client not built" | Build with `make demo` (not `cargo build` / `go build` alone). |
| `demo reset` seemed to do nothing | It changes the database only; restart the server. Or use ^R / File ▸ Reset all tabs in a client. |
| Colours look wrong | Try `--colors 256`, or another `--theme`. |

## 10. Command reference

```
demo serve [--db demo.db] [--bind 127.0.0.1:8790] [--tls-cert F --tls-key F] [--allow-plaintext]
demo reset [--db demo.db]
demo tui   [--server http://127.0.0.1:8790] [--name NAME] [--ca-cert F] [--theme NAME] [--colors 256|truecolor]
```
