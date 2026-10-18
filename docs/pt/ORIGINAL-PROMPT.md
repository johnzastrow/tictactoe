# pt: the original prompt

The request that started pt, and the design interview that followed, recorded
verbatim (typos included) from the Claude Code session of 2026-10-17. Everything
in this folder ([PLAN.md](PLAN.md), [protocol.md](protocol.md),
[schema.sql](schema.sql), [ui-spec.md](ui-spec.md)) was derived from these two
messages; the decisions table in PLAN.md is the condensed version.

## 1. Opening request

```text
we need to continue the experiment. We need to see how rich we can make a TUI app and then mirror it in a web app, using both rust and go. But we want to push each language to its maximum richness by building another demo app - wiht both web and tui versions. inside tictactoe repo, build a new app in both languages called "pt" standing for "projects and tasks". This will be backed by sqlite and allow remote instances of the front ends to connect back to a single backend. For context we'll base it on the concept of managing about 50 projects, and an ad hoc set of tasks for each (though there will also be a standard template of tasks that each project will initialize with). The objective of the app is to demonstrate UI. Both apps need to have extensive keyboard functions and rich/smart controls (like auto-completem and cascaading selections) - we want to minimize clicking without sacrificing functions. We also need to fit a lot of information on the screen at once, so editable data grids will be important. Suggest other controls. Obviously a lot can be accomplished in WebUIs, so the question is how much can we fit into a TUO. Ask me questions.
```

## 2. Design interview

Claude replied with 17 questions, each with a proposed default. The answers
were typed inline under each group (`* N ans: …`) and pasted back as one
message, reproduced here as sent.

```text
Security profile (my config asks this before any code)
  1. Is this production-quality or an experiment/learning project? Remote clients make it matter more than it did for tic-tac-toe. Default: experiment, but secure by default where it's cheap: unless told otherwise, require a token to connect, TLS when exposed, parameterized SQL.

* 1 ANS: use your default. 


  Network and users
  2. Where do the remote clients run? (a) LAN only, (b) Tailscale/VPN, (c) over the internet. Default: (a)/(b).
  3. Identity: (a) one shared token, (b) a token per named user, with the user sent on every change, (c) real accounts. Users matter if we want owner fields, presence and an activity log. Default: (b).
  4. When two people edit the same thing: (a) last write wins, (b) optimistic version checks per field, where the losing editor sees a conflict marker in the cell. Default: (b). It's better UI to demonstrate.
  5. Can the two languages talk to each other? Should the Go TUI be able to connect to the Rust server, and the reverse, through one shared protocol and one SQLite schema, as tic-tac-toe does for the browser? Default: 
  yes. Both share docs/pt-protocol.md and a single schema file.


* 2 ANS: a and b
* 3 ans: c
* 4 ans: b
* 5 ans: both to both


  Web front end. This is the biggest decision, because "push each language to the max" can mean two things.
  6. Three options:
  - (a) Vanilla JS/HTML embedded in each binary, like tic-tac-toe. The browser page is shared and the languages only differ on the server side. The least work, but it doesn't show off either language on the front end.
  - (b) A native WASM front end per language: Leptos or Dioxus for Rust, Go compiled to WASM (or templ + htmx) for Go. This really tests how far each language reaches on the web, but Go's browser-side story is much
    weaker, so we'd also be measuring that gap.
  - (c) One shared TypeScript front end (Vite plus a grid library such as AG Grid or TanStack Table). This gives the richest web UI, but it isn't Rust or Go at all.
  - My lean: (b) for Rust and templ/htmx with small JS islands for Go. Each gets its idiomatic best, and the gap between them becomes part of what we learn. (a) is the safe choice if you'd rather compare the TUIs.

* 6 ans: B. Let’s try a WASM

  Data model
  7. Project fields. I'm assuming name, code, program/client, owner, status, priority, start/due dates, % complete (calculated), tags and notes. Anything to add, such as budget, health (RAG) or links?
  8. Task fields. Should tasks have subtasks? Dependencies ("blocked by")? Time estimate vs actual? Default: one level of subtasks, plus dependencies (needed for the Gantt).
  9. Templates: (a) one standard template, or (b) several templates chosen by project type, which also gives us a cascading select. When a template changes, does that affect existing projects? Default: (b), with no 
  effect on existing projects.
  10. Custom fields defined by the user at runtime? They make the grid harder (dynamic columns) and more impressive. Default: yes, a few types (text, number, date, enum).
  11. Seed data: generate about 50 realistic projects with 10–40 tasks each? Default: yes, via a pt seed command.

* 7 ans: all the fields you show. Track budget as hours of labor
* 8 ans: no subtasks now. but yes on budget versus actual
* 9 ans: use your default
* 10 ans: your defaults
* 11 ans: your default


  Keyboard and terminal
  12. Key style: (a) vim-modal (hjkl, counts, . to repeat, visual mode), (b) conventional arrows, Ctrl shortcuts and Tab, (c) both, switchable, with remapping in a config file. Default: (c), with vim as the default.
  13. Which terminals do you use? (Ghostty, kitty, WezTerm, iTerm2, Windows Terminal.) The answer decides whether I can rely on the kitty keyboard protocol (which can tell Ctrl+I apart from Tab), OSC 52 clipboard and
  truecolor, or need fallbacks.
  14. Mouse support in the TUI: full (click, drag to resize, scroll), or keyboard-first with basic clicking? Default: full. It's cheap in both libraries.

* 12 ans: B, simple for non-tech users. Nano-style help and guidance
* 13 ans: Windows terminal, Ghostty, XFCE terminal, Mac Terminal. Use as much color as you can.
* 14 ans: default. Support as much keyboard AND mouse support as you can for each

  Stack and process
  15. SQLite drivers: rusqlite for Rust; modernc.org/sqlite for Go (pure Go, no cgo, so cross-compiling stays easy). WAL mode, with migrations embedded in the binary. Any objections?
  16. Order and layout: build the Rust version first, then reach parity in Go (as last time)? Put it in rust/crates/pt-* and go/cmd/pt + go/internal/pt/...? One binary per language with pt serve, pt tui --server URL
  and pt seed?
  17. Phasing: this is large. I'd suggest milestones: (1) schema, server, protocol and seed data; (2) TUI grid with editing and the query bar; (3) web grid at parity; (4) kanban, tree and dashboard; (5) Gantt, presence
  and undo. Do you agree, or do you want any of these in the first cut?

* 15 ans: your defaults
* 16 ans: your defaults
* 17 ans: your defaults. Develop the plan, the follow the plan. test and proceed without me. This is just demonstration, so most decisions will be unimportant as long as the app is usable.
```

## 3. Follow-up instructions during the build

- "proceed. and make sure the computer does not sleep"
- "yes, push to both remotes after updating all the docs"
- "include usage instructions" (which produced [USAGE.md](USAGE.md))
