# demo: the original prompt

The request that started demo, recorded verbatim (typos included) from the
Claude Code session of 2026-10-18.

## 1. Request

```text
the pt app is too busy to properly test the controls, widgets and screen concepts. create a new app called demo, in both languages as before and same architecture as before, that puts an example of each control type on its own tab or otherwise isolates it so that it is cleaner to demonstrate and use. Then document demo with the same screenshots and videos as pt
```

Added while the plan was being written:

```text
th3e screens should have the same rich onscreen help options as well
```

## 2. Design questions

Claude proposed 16 tabs (Text, Number, Date, Dropdown, Combobox, Tags, Text
area, Choices, Cascade, Grid, Menus, Palette, Dialogs, Charts, Kanban, Gantt)
and asked three questions; the recommended option was chosen for each:

| Question | Answer |
|---|---|
| Backend | Own small server per language: hub + WebSocket + SQLite holding each control's value, synced live; small built-in dataset; Rust ⇄ Go interop; independent of pt |
| Login | Name only, no passwords; loopback by default, TLS or `--allow-plaintext` to expose |
| Controls | The 16 tabs as proposed |

The decisions table in [PLAN.md](PLAN.md) is the condensed version.
