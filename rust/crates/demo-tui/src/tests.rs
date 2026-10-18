//! App-level behaviour and render tests: the whole UI on a `TestBackend`,
//! driven through the same key/mouse/server-message paths as the real
//! thing. Outgoing messages collect in `app.outbox`; `ack_all` plays the
//! server's part (ack + changed).

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use demo_core::keys::{self, CascadePick, FileEntry, GanttBar, GridRow, KanbanCard, Profile};
use demo_core::protocol::{Ack, Entry, Peer, ServerInfo, You};
use demo_core::{ClientMsg, Date, ServerMsg};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::{Value, json};

use crate::app::{App, Focus, Phase};
use crate::net::{ConnState, ServerUrl, tls_config};
use crate::tabs::Tabs;

const W: u16 = 115;
const H: u16 = 33;

fn today() -> Date {
    Date::parse_iso("2026-10-18").unwrap()
}

fn welcome(conn: u64) -> ServerMsg {
    let values = keys::defaults()
        .into_iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                Entry {
                    value: v,
                    version: 1,
                    by: String::new(),
                    at: "2026-10-18T09:00:00Z".into(),
                },
            )
        })
        .collect();
    ServerMsg::Welcome {
        protocol: 1,
        server: ServerInfo {
            implementation: "rust".into(),
            version: "0.5.0".into(),
        },
        you: You {
            conn,
            name: "alice".into(),
            color: demo_core::dataset::peer_color(conn).into(),
        },
        values,
    }
}

fn app() -> App {
    let mut a = App::new(
        ServerUrl::parse("http://127.0.0.1:1").unwrap(),
        tls_config(None).unwrap(),
        0,
    );
    a.phase = Phase::Main;
    a.name = "alice".into();
    a.conn_state = ConnState::Online;
    a.today = today();
    a.fixed_now = Some(demo_core::date::parse_rfc3339("2026-10-18T09:05:00Z").unwrap());
    a.tabs = Tabs::new(today());
    a.on_server(welcome(1));
    a.on_server(ServerMsg::Presence {
        peers: vec![
            Peer {
                conn: 1,
                name: "alice".into(),
                color: "#e06c75".into(),
                client: "tui-rust".into(),
                tab: Some("text".into()),
            },
            Peer {
                conn: 2,
                name: "bob".into(),
                color: "#61afef".into(),
                client: "web-go".into(),
                tab: Some("date".into()),
            },
        ],
    });
    a.outbox.clear();
    a.status = None;
    a
}

fn screen_sized(a: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| crate::ui::draw(a, f)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn screen(a: &mut App) -> String {
    screen_sized(a, W, H)
}

fn press(a: &mut App, code: KeyCode, mods: KeyModifiers) {
    a.on_event(Event::Key(KeyEvent::new(code, mods)));
    // draw after every key, as the real loop does (hit rects, anchors)
    screen(a);
}

fn key(a: &mut App, code: KeyCode) {
    press(a, code, KeyModifiers::NONE);
}

fn keys(a: &mut App, codes: &[KeyCode]) {
    for c in codes {
        key(a, *c);
    }
}

fn ctrl(a: &mut App, c: char) {
    press(a, KeyCode::Char(c), KeyModifiers::CONTROL);
}

fn shift(a: &mut App, code: KeyCode) {
    press(a, code, KeyModifiers::SHIFT);
}

fn typ(a: &mut App, s: &str) {
    for c in s.chars() {
        key(a, KeyCode::Char(c));
    }
}

fn click(a: &mut App, x: u16, y: u16) {
    screen(a);
    a.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }));
    a.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }));
}

/// Go to tab `n` (1-based) from the sidebar with ↓ only, then into the body.
fn open_tab(a: &mut App, n: usize) {
    for _ in 1..n {
        key(a, KeyCode::Down);
    }
    key(a, KeyCode::Enter);
    assert_eq!(a.focus, Focus::Body);
    a.outbox.clear();
}

/// The sets in the outbox: (key, value, base).
fn sets(a: &App) -> Vec<(String, Value, u64)> {
    a.outbox
        .iter()
        .filter_map(|m| match m {
            ClientMsg::Set {
                key, value, base, ..
            } => Some((key.clone(), value.clone(), *base)),
            _ => None,
        })
        .collect()
}

#[track_caller]
fn last_set(a: &App) -> (String, Value, u64) {
    match sets(a).pop() {
        Some(s) => s,
        None => panic!("no set sent; status: {:?}", a.status),
    }
}

/// Play the server: ack every outstanding set and broadcast it.
fn ack_all(a: &mut App) {
    loop {
        let out = std::mem::take(&mut a.outbox);
        if out.is_empty() {
            break;
        }
        for m in out {
            if let ClientMsg::Set {
                req, key, value, ..
            } = m
            {
                let version = a.v.version(&key) + 1;
                a.on_server(ServerMsg::Ack(Ack {
                    req,
                    ok: true,
                    version: Some(version),
                    ..Default::default()
                }));
                a.on_server(ServerMsg::Changed {
                    key,
                    value,
                    version,
                    by: "alice".into(),
                    conn: 1,
                    at: "2026-10-18T09:05:00Z".into(),
                });
            }
        }
    }
}

fn status(a: &App) -> String {
    a.status.as_ref().map(|s| s.0.clone()).unwrap_or_default()
}

// ---- shell -------------------------------------------------------------------------

#[test]
fn starts_on_the_sidebar_with_tab_one() {
    let mut a = app();
    assert_eq!((a.tab, a.focus), (0, Focus::Sidebar));
    let s = screen(&mut a);
    assert!(
        s.contains("File") && s.contains("View") && s.contains("Help"),
        "{s}"
    );
    assert!(s.contains("demo · alice · ● rust 0.5.0"), "{s}");
    assert!(s.contains("▸ 1 Text"), "{s}");
    assert!(s.contains("16 Gantt"));
    assert!(s.contains("1 Text input") && s.contains("Single-line fields with live validation."));
    assert!(s.contains("Keys") && s.contains("Value (shared)"));
    assert!(s.contains("text.name = \"Ada Lovelace\""), "{s}");
    assert!(s.contains("v1 · default"));
    assert!(
        s.contains("↑↓ Tab")
            && s.contains("Enter Open")
            && s.contains("^K Commands")
            && s.contains("F1 Help")
            && s.contains("^Q Quit"),
        "{s}"
    );
    // presence: initials and count; bob's dot is on the Date tab
    assert!(
        s.contains(" AL ") && s.contains(" BO ") && s.contains("online: 2"),
        "{s}"
    );
    assert!(s.contains("3 Date ●"), "{s}");
}

#[test]
fn down_switches_tab_immediately_and_sends_focus() {
    let mut a = app();
    key(&mut a, KeyCode::Down);
    assert_eq!(a.tab, 1);
    assert_eq!(
        a.outbox,
        vec![ClientMsg::Focus {
            tab: "number".into()
        }]
    );
    keys(&mut a, &[KeyCode::End]);
    assert_eq!(a.tab, 15);
    key(&mut a, KeyCode::Home);
    assert_eq!(a.tab, 0);
    key(&mut a, KeyCode::Up);
    assert_eq!(a.tab, 15, "↑ wraps");
    // Enter, → and Tab all enter the body on the first control; Shift+Tab returns
    for k in [KeyCode::Enter, KeyCode::Right, KeyCode::Tab] {
        let mut a = app();
        key(&mut a, k);
        assert_eq!((a.focus, a.ctrl), (Focus::Body, 0));
        key(&mut a, KeyCode::Tab);
        assert_eq!(a.ctrl, 1);
        key(&mut a, KeyCode::BackTab);
        key(&mut a, KeyCode::BackTab);
        assert_eq!(a.focus, Focus::Sidebar);
    }
    // ^PgDn / ^PgUp and Alt+digits
    let mut a = app();
    press(&mut a, KeyCode::PageDown, KeyModifiers::CONTROL);
    assert_eq!(a.tab, 1);
    press(&mut a, KeyCode::Char('0'), KeyModifiers::ALT);
    assert_eq!(a.tab, 9);
    press(&mut a, KeyCode::PageUp, KeyModifiers::CONTROL);
    assert_eq!(a.tab, 8);
}

/// Every tab at the recording size (115×33): heading, its control and both
/// panels visible, nothing clipped off the bottom.
#[test]
fn every_tab_renders_at_recording_size() {
    let expect: [&[&str]; 16] = [
        &["Name", "Email", "Code", "Password", "[◉ show]"],
        &["Hours", "Quantity", "Percent", "[−]", "[+]", " 40%"],
        &[
            "Start",
            "Due",
            "→ Mon 2 Nov 2026",
            "November 2026 · Start",
            "Mo  Tu",
        ],
        &["Status", "Priority", "In progress", "High"],
        &["Owner", "Reviewer", "Carol Diaz", "— none —"],
        &["[frontend ×]", "[q4 ×]", "2/12 tags"],
        &["Notes", "Welcome to demo.", "Ln 1, Col 1", "words"],
        &[
            "[✓] Notify me",
            "[✓] Email",
            "[ ] SMS",
            "(•) M",
            "[ ●━ ] Off",
        ],
        &[
            "Program",
            "Northwind Retail",
            "POS refresh",
            "Build",
            "Northwind Retail ▸ POS refresh ▸ Build",
            "3 projects",
            "4 phases",
        ],
        &[
            "Item",
            "Laptop stand",
            "Headset",
            "Totals · 6 rows",
            "In progress",
        ],
        &[
            "Document",
            "Format",
            "Arrange",
            "Budget 2027.xlsx",
            "★ Roadmap.pdf",
        ],
        &["Last command:", "Go to: Text", "Command palette"],
        &[
            "[ Confirm… ]",
            "[ Form… ]",
            "[ Broadcast… ]",
            "Grace Hopper <grace@example.com>",
        ],
        &["Bars", "Sparkline", "Gauge", "Heat", "Jan", "Dec", "avg"],
        &[
            "To do (4)",
            "Doing (2)",
            "Review (1)",
            "Done (1)",
            "Write brief",
            "Launch",
        ],
        &["Discovery", "Launch", "Oct 2026", "Nov 2026", "10d"],
    ];
    let mut a = app();
    for (i, want) in expect.iter().enumerate() {
        let s = screen(&mut a);
        let tab = &demo_core::dataset::TABS[i];
        assert!(
            s.contains(&format!("{} {}", tab.number, tab.heading)),
            "tab {i}:\n{s}"
        );
        for w in *want {
            assert!(s.contains(w), "tab {} missing {w:?}:\n{s}", i + 1);
        }
        assert!(
            s.contains("─ Keys ─") && s.contains("─ Value (shared) ─"),
            "tab {} panels:\n{s}",
            i + 1
        );
        for k in tab.keys {
            assert!(s.contains(k), "tab {} value panel lacks {k}:\n{s}", i + 1);
        }
        // the panels' bottom border sits above the status line
        let lines: Vec<&str> = s.lines().collect();
        assert!(
            lines[H as usize - 3].contains("╰"),
            "tab {} bottom:\n{s}",
            i + 1
        );
        // also with the body focused (popups, calendars)
        key(&mut a, KeyCode::Enter);
        let s = screen(&mut a);
        assert!(
            s.contains("Esc Sidebar") || s.contains("Esc"),
            "tab {}:\n{s}",
            i + 1
        );
        key(&mut a, KeyCode::F(6));
        key(&mut a, KeyCode::Down);
    }
}

#[test]
fn small_terminal_and_minimum_size() {
    let mut a = app();
    let s = screen_sized(&mut a, 70, 20);
    assert!(s.contains("at least 80×24"), "{s}");
    // every tab still draws at the minimum size
    for _ in 0..16 {
        let s = screen_sized(&mut a, 80, 24);
        assert!(s.contains("Value"), "{s}");
        key(&mut a, KeyCode::Down);
    }
}

#[test]
fn help_palette_menu_and_reset() {
    let mut a = app();
    key(&mut a, KeyCode::F(1));
    let s = screen(&mut a);
    assert!(
        s.contains("This tab — 1 Text input")
            && s.contains("Getting around")
            && s.contains("Global keys"),
        "{s}"
    );
    typ(&mut a, "zoom");
    let s = screen(&mut a);
    assert!(
        s.contains("All tabs")
            && s.contains("zoom day/week/month")
            && !s.contains("Getting around"),
        "{s}"
    );
    key(&mut a, KeyCode::Esc);
    assert!(a.help.is_none());
    ctrl(&mut a, 'g');
    assert!(a.help.is_some());
    key(&mut a, KeyCode::Esc);

    // ^K palette: fuzzy, runs, records palette.last
    ctrl(&mut a, 'k');
    typ(&mut a, "gru");
    let s = screen(&mut a);
    assert!(s.contains("Theme: Gruvbox"), "{s}");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.t.name, "Gruvbox");
    assert_eq!(
        last_set(&a),
        ("palette.last".into(), json!("Theme: Gruvbox"), 1)
    );
    a.outbox.clear();

    // F10 menu: ← → between menus, View ▸ 14 Charts
    key(&mut a, KeyCode::F(10));
    let s = screen(&mut a);
    assert!(
        s.contains("Reset this tab") && s.contains("Reset all tabs…"),
        "{s}"
    );
    key(&mut a, KeyCode::Right);
    let s = screen(&mut a);
    assert!(
        s.contains("14 Charts") && s.contains("Toggle Value panel") && s.contains("Theme"),
        "{s}"
    );
    for _ in 0..13 {
        key(&mut a, KeyCode::Down);
    }
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.tab, 13);
    assert!(a.menu.is_none());
    press(&mut a, KeyCode::Char('h'), KeyModifiers::ALT);
    assert_eq!(a.menu.as_ref().map(|m| m.open), Some(2));
    key(&mut a, KeyCode::Esc);

    // ^R asks, y resets this tab's keys
    ctrl(&mut a, 'r');
    assert!(screen(&mut a).contains("Reset every control on Charts"));
    key(&mut a, KeyCode::Char('y'));
    assert_eq!(
        a.outbox.last(),
        Some(&ClientMsg::Reset {
            req: a
                .outbox
                .iter()
                .filter_map(|m| if let ClientMsg::Reset { req, .. } = m {
                    Some(*req)
                } else {
                    None
                })
                .next_back()
                .unwrap(),
            keys: vec!["charts.series".into()]
        })
    );

    // ^T cycles the theme from the sidebar
    a.focus = Focus::Sidebar;
    ctrl(&mut a, 't');
    assert_eq!(a.t.name, "Midnight");
}

#[test]
fn remote_change_conflict_and_toast() {
    let mut a = app();
    a.on_server(ServerMsg::Changed {
        key: "text.name".into(),
        value: json!("Bob B"),
        version: 2,
        by: "bob".into(),
        conn: 2,
        at: "2026-10-18T09:04:00Z".into(),
    });
    assert_eq!(status(&a), "bob changed text.name");
    assert!(a.flashes.contains_key("text.name"));
    let s = screen(&mut a);
    assert!(
        s.contains("[Bob B") && s.contains("v2 · by bob · 1m ago"),
        "{s}"
    );

    // edit Name while bob changes it: warning, then our commit conflicts
    open_tab(&mut a, 1);
    typ(&mut a, "!");
    a.on_server(ServerMsg::Changed {
        key: "text.name".into(),
        value: json!("Bobby"),
        version: 3,
        by: "bob".into(),
        conn: 2,
        at: "2026-10-18T09:04:30Z".into(),
    });
    let s = screen(&mut a);
    assert!(
        s.contains("[Bob B!") && s.contains("⚠ changed by bob"),
        "{s}"
    );
    key(&mut a, KeyCode::Enter);
    let ClientMsg::Set { req, base, .. } = a.outbox.last().cloned().unwrap() else {
        panic!()
    };
    assert_eq!(base, 2, "based on the version shown when editing began");
    let cur = Entry {
        value: json!("Bobby"),
        version: 3,
        by: "bob".into(),
        at: "2026-10-18T09:04:30Z".into(),
    };
    a.on_server(ServerMsg::Ack(Ack::conflict(req, cur)));
    assert_eq!(status(&a), "⚠ bob changed text.name first, showing theirs");
    let s = screen(&mut a);
    assert!(s.contains("[Bobby") && !s.contains("Bob B!"), "{s}");

    // a broadcast toast shows top right; safe_text strips escapes
    a.on_server(ServerMsg::Toast {
        text: "Lunch\u{1b}[2J".into(),
        by: "bob".into(),
        conn: 2,
    });
    let s = screen(&mut a);
    assert!(s.contains("» bob: Lunch�[2J"), "{s}");
}

#[test]
fn offline_edits_are_refused() {
    let mut a = app();
    a.conn_state = ConnState::Reconnecting;
    open_tab(&mut a, 8);
    key(&mut a, KeyCode::Char(' '));
    assert!(a.outbox.is_empty());
    assert!(status(&a).contains("Offline"));
}

#[test]
fn join_form() {
    let mut a = app();
    a.phase = Phase::Join;
    let s = screen(&mut a);
    assert!(
        s.contains("Server URL") && s.contains("Name") && s.contains("[ Join ]"),
        "{s}"
    );
    typ(&mut a, "1bad");
    key(&mut a, KeyCode::Enter);
    assert!(a.join.error.as_deref().unwrap_or("").starts_with("Name:"));
    assert_eq!(a.phase, Phase::Join);
    assert!(screen(&mut a).contains("✗ Name:"));
}

#[test]
fn mouse_sidebar_hints_and_controls() {
    let mut a = app();
    // sidebar row of tab 5 (rows start under the menu bar)
    click(&mut a, 3, 5);
    assert_eq!(a.tab, 4);
    // clicking a hint runs it: "Enter Open"
    let s = screen(&mut a);
    let last = s.lines().last().unwrap();
    let x = last
        .find("Enter")
        .map(|b| last[..b].chars().count())
        .unwrap() as u16;
    click(&mut a, x, H - 1);
    assert_eq!(a.focus, Focus::Body);
    // a dropdown opens on click (tab 4)
    a.focus_sidebar();
    a.switch_tab(3);
    let s = screen(&mut a);
    let (row, line) = s
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("Status") && l.contains("In progress"))
        .unwrap();
    let col = line
        .find("In progress")
        .map(|b| line[..b].chars().count())
        .unwrap() as u16;
    click(&mut a, col, row as u16);
    assert!(a.tabs.select.drop.open);
    let s = screen(&mut a);
    let (row, line) = s
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("● Review"))
        .unwrap();
    let col = line
        .find("Review")
        .map(|b| line[..b].chars().count())
        .unwrap() as u16;
    click(&mut a, col, row as u16);
    assert_eq!(last_set(&a).1, json!("review"));
}

// ---- tabs ---------------------------------------------------------------------------

#[test]
fn tab01_text() {
    let mut a = app();
    open_tab(&mut a, 1);
    ctrl(&mut a, 'u');
    typ(&mut a, "Grace");
    assert!(screen(&mut a).contains("● unsaved"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(last_set(&a), ("text.name".into(), json!("Grace"), 1));
    ack_all(&mut a);
    // Email: invalid is red and not sent
    key(&mut a, KeyCode::Tab);
    ctrl(&mut a, 'u');
    typ(&mut a, "x");
    let s = screen(&mut a);
    assert!(s.contains("✗ must look like name@example.com"), "{s}");
    key(&mut a, KeyCode::Enter);
    assert!(a.outbox.is_empty());
    // Esc reverts the field, a second Esc goes to the sidebar
    key(&mut a, KeyCode::Esc);
    assert!(screen(&mut a).contains("[ada@example.com"));
    // Code: typed lowercase shows uppercase
    key(&mut a, KeyCode::Tab);
    ctrl(&mut a, 'u');
    typ(&mut a, "abc-123");
    assert!(screen(&mut a).contains("[ABC-123"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(last_set(&a).1, json!("ABC-123"));
    // Password: masked, never sent, ^O shows it
    key(&mut a, KeyCode::Tab);
    a.outbox.clear();
    typ(&mut a, "hunter2");
    key(&mut a, KeyCode::Enter);
    assert!(a.outbox.is_empty());
    assert!(screen(&mut a).contains("[•••••••"));
    ctrl(&mut a, 'o');
    assert!(screen(&mut a).contains("[hunter2"));
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.focus, Focus::Sidebar);
}

#[test]
fn tab02_number() {
    let mut a = app();
    open_tab(&mut a, 2);
    key(&mut a, KeyCode::Up);
    shift(&mut a, KeyCode::Up);
    assert!(screen(&mut a).contains("[13.75"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(last_set(&a), ("number.hours".into(), json!(13.75), 1));
    ack_all(&mut a);
    key(&mut a, KeyCode::Tab);
    keys(&mut a, &[KeyCode::Up, KeyCode::Up, KeyCode::Enter]);
    assert_eq!(last_set(&a).1, json!(5));
    ack_all(&mut a);
    // the slider commits each change; a second change waits for the first ack
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Right);
    assert_eq!(last_set(&a), ("number.percent".into(), json!(41), 1));
    key(&mut a, KeyCode::PageUp);
    assert_eq!(sets(&a).len(), 1);
    assert!(screen(&mut a).contains(" 51%"));
    ack_all(&mut a);
    assert_eq!(a.v.i64("number.percent"), 51);
    key(&mut a, KeyCode::End);
    assert_eq!(last_set(&a).1, json!(100));
}

#[test]
fn tab03_date() {
    let mut a = app();
    open_tab(&mut a, 3);
    let s = screen(&mut a);
    assert!(s.contains("November 2026 · Start"), "{s}");
    typ(&mut a, "fri");
    assert!(screen(&mut a).contains("fri → Fri 23 Oct 2026"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(last_set(&a), ("date.start".into(), json!("2026-10-23"), 1));
    ack_all(&mut a);
    // Due: before start is refused
    key(&mut a, KeyCode::Tab);
    let s = screen(&mut a);
    assert!(s.contains("November 2026 · Due"), "{s}");
    typ(&mut a, "2026-10-01");
    assert!(screen(&mut a).contains("✗ due is before start"));
    key(&mut a, KeyCode::Enter);
    assert!(a.outbox.is_empty());
    key(&mut a, KeyCode::Esc);
    // calendar: ↓ a week from the 20th, Enter commits that day
    keys(&mut a, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(last_set(&a).1, json!("2026-11-27"));
    ack_all(&mut a);
    press(&mut a, KeyCode::Right, KeyModifiers::CONTROL);
    keys(&mut a, &[KeyCode::PageDown, KeyCode::Enter]);
    assert_eq!(last_set(&a).1, json!("2026-12-28"));
    ack_all(&mut a);
    // Del clears to none; ^T is today
    key(&mut a, KeyCode::Delete);
    assert_eq!(last_set(&a).1, Value::Null);
    ack_all(&mut a);
    key(&mut a, KeyCode::BackTab);
    ctrl(&mut a, 't');
    assert_eq!(last_set(&a), ("date.start".into(), json!("2026-10-18"), 2));
    assert_eq!(
        a.t.name, "Midnight",
        "^T in a date field is today, not the theme"
    );
}

#[test]
fn tab04_dropdown() {
    let mut a = app();
    open_tab(&mut a, 4);
    key(&mut a, KeyCode::Enter);
    assert!(a.tabs.select.drop.open);
    let s = screen(&mut a);
    assert!(s.contains("● To do") && s.contains("● Done"), "{s}");
    keys(&mut a, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(last_set(&a), ("select.status".into(), json!("blocked"), 1));
    ack_all(&mut a);
    // Space opens too; typing filters and jumps to the first match
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char(' '));
    typ(&mut a, "c");
    key(&mut a, KeyCode::Enter);
    assert_eq!(last_set(&a).1, json!("critical"));
    // ↓ opens, Esc closes without a change
    ack_all(&mut a);
    keys(&mut a, &[KeyCode::Down, KeyCode::Esc]);
    assert!(!a.tabs.select.drop.open && a.outbox.is_empty());
}

#[test]
fn tab05_combobox() {
    let mut a = app();
    open_tab(&mut a, 5);
    typ(&mut a, "gl");
    assert!(a.tabs.combo.open);
    let s = screen(&mut a);
    assert!(
        s.contains("Grace Liu") && s.contains("grace · Engineering"),
        "{s}"
    );
    key(&mut a, KeyCode::Enter);
    assert_eq!(last_set(&a), ("combo.owner".into(), json!("grace"), 1));
    ack_all(&mut a);
    // ↓ opens the full list on Reviewer; pick the second person
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Down);
    assert!(a.tabs.combo.open);
    keys(&mut a, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(last_set(&a), ("combo.reviewer".into(), json!("bob"), 1));
    ack_all(&mut a);
    key(&mut a, KeyCode::Delete);
    assert_eq!(last_set(&a).1, Value::Null);
}

#[test]
fn tab06_tags() {
    let mut a = app();
    open_tab(&mut a, 6);
    let s = screen(&mut a);
    assert!(
        s.contains("urgent") && s.contains("12 uses") && !s.contains("frontend  "),
        "{s}"
    );
    typ(&mut a, "ur");
    key(&mut a, KeyCode::Enter);
    assert_eq!(last_set(&a).1, json!(["frontend", "q4", "urgent"]));
    ack_all(&mut a);
    typ(&mut a, "zzz");
    assert!(screen(&mut a).contains("✚ Create \"zzz\""));
    key(&mut a, KeyCode::Enter);
    assert_eq!(last_set(&a).1, json!(["frontend", "q4", "urgent", "zzz"]));
    ack_all(&mut a);
    key(&mut a, KeyCode::Backspace);
    assert_eq!(last_set(&a).1, json!(["frontend", "q4", "urgent"]));
    ack_all(&mut a);
    keys(&mut a, &[KeyCode::Left, KeyCode::Left, KeyCode::Delete]);
    assert_eq!(last_set(&a).1, json!(["frontend", "urgent"]));
    ack_all(&mut a);
    typ(&mut a, "Bad Tag");
    assert!(screen(&mut a).contains("✗ a–z"));
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Enter);
    assert!(sets(&a).last().is_some_and(|s| s.0 == "tags.tags"));
}

#[test]
fn tab07_textarea() {
    let mut a = app();
    open_tab(&mut a, 7);
    press(&mut a, KeyCode::End, KeyModifiers::CONTROL);
    keys(&mut a, &[KeyCode::Enter]);
    typ(&mut a, "More");
    let s = screen(&mut a);
    assert!(s.contains("Ln 4, Col 5") && s.contains("● unsaved"), "{s}");
    // Tab leaves (to the sidebar) and keeps the draft
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.focus, Focus::Sidebar);
    assert!(a.tabs.textarea.dirty);
    assert!(screen(&mut a).contains("Notes ● unsaved"));
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 's');
    let (k, v, base) = last_set(&a);
    assert_eq!((k.as_str(), base), ("textarea.notes", 1));
    assert!(v.as_str().unwrap().ends_with("everyone connected.\nMore"));
    ack_all(&mut a);
    // Esc with edits asks first
    typ(&mut a, "x");
    key(&mut a, KeyCode::Esc);
    assert!(screen(&mut a).contains("Discard your unsaved changes"));
    key(&mut a, KeyCode::Char('y'));
    assert!(!a.tabs.textarea.dirty);
}

#[test]
fn tab08_choices() {
    let mut a = app();
    open_tab(&mut a, 8);
    key(&mut a, KeyCode::Char(' '));
    assert_eq!(last_set(&a), ("choice.notify".into(), json!(false), 1));
    ack_all(&mut a);
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char(' '));
    assert!(status(&a).contains("turn on Notify first") && a.outbox.is_empty());
    key(&mut a, KeyCode::BackTab);
    key(&mut a, KeyCode::Char(' '));
    ack_all(&mut a);
    key(&mut a, KeyCode::Tab);
    keys(&mut a, &[KeyCode::Down, KeyCode::Char(' ')]);
    assert_eq!(last_set(&a).1, json!(["email", "sms", "push"]));
    ack_all(&mut a);
    key(&mut a, KeyCode::Tab);
    keys(&mut a, &[KeyCode::Right, KeyCode::Char(' ')]);
    assert_eq!(last_set(&a).1, json!("l"));
    ack_all(&mut a);
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char(' '));
    assert_eq!(last_set(&a), ("choice.dark".into(), json!(true), 1));
    ack_all(&mut a);
    assert!(screen(&mut a).contains("[ ━● ] On"));
}

#[test]
fn tab09_cascade() {
    let mut a = app();
    open_tab(&mut a, 9);
    keys(&mut a, &[KeyCode::Enter, KeyCode::Down, KeyCode::Enter]);
    let pick: CascadePick = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(
        pick,
        CascadePick {
            program: Some("Contoso Health".into()),
            project: None,
            phase: None
        }
    );
    ack_all(&mut a);
    let s = screen(&mut a);
    assert!(
        s.contains("2 projects") && s.contains("pick above first"),
        "{s}"
    );
    key(&mut a, KeyCode::Tab);
    keys(&mut a, &[KeyCode::Enter, KeyCode::Enter]);
    ack_all(&mut a);
    key(&mut a, KeyCode::Tab);
    typ(&mut a, "te");
    key(&mut a, KeyCode::Enter);
    let pick: CascadePick = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(pick.phase.as_deref(), Some("Test"));
    ack_all(&mut a);
    assert!(screen(&mut a).contains("Contoso Health ▸ Patient portal ▸ Test"));
    // Del on Project clears it and Phase
    key(&mut a, KeyCode::BackTab);
    key(&mut a, KeyCode::Delete);
    let pick: CascadePick = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!((pick.project, pick.phase), (None, None));
}

#[test]
fn tab10_grid() {
    let mut a = app();
    open_tab(&mut a, 10);
    // the cursor starts on Item, the first editable column
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 'u');
    typ(&mut a, "Dock");
    key(&mut a, KeyCode::Enter);
    let rows: Vec<GridRow> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(rows[0].item, "Dock");
    ack_all(&mut a);
    assert_eq!(a.tabs.grid.row, 1, "Enter saves and moves down");
    // typing starts an edit: Qty of row 2
    key(&mut a, KeyCode::Right);
    typ(&mut a, "9");
    key(&mut a, KeyCode::Enter);
    let rows: Vec<GridRow> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(rows[1].qty, 9);
    ack_all(&mut a);
    // Status editor is a dropdown
    keys(
        &mut a,
        &[KeyCode::Up, KeyCode::End, KeyCode::Left, KeyCode::Enter],
    );
    assert!(screen(&mut a).contains("─ Status ─"));
    typ(&mut a, "bl");
    key(&mut a, KeyCode::Enter);
    let rows: Vec<GridRow> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(rows[1].status, "blocked");
    ack_all(&mut a);
    // Del clears Due; Ins adds a row and edits Item
    key(&mut a, KeyCode::Delete);
    let rows: Vec<GridRow> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(rows[1].due, None);
    ack_all(&mut a);
    key(&mut a, KeyCode::Insert);
    let rows: Vec<GridRow> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(
        (rows.len(), rows[6].id, rows[6].item.as_str()),
        (7, 7, "New item")
    );
    assert!(a.tabs.grid.editor.is_some());
    key(&mut a, KeyCode::Esc);
    ack_all(&mut a);
    // ^O sorts the view only; F8 deletes after confirming
    keys(&mut a, &[KeyCode::Home]);
    ctrl(&mut a, 'o');
    ctrl(&mut a, 'o');
    assert!(screen(&mut a).contains("# ▼"));
    a.outbox.clear();
    press(&mut a, KeyCode::Home, KeyModifiers::CONTROL);
    key(&mut a, KeyCode::F(8));
    assert!(screen(&mut a).contains("Delete row 7"));
    key(&mut a, KeyCode::Char('y'));
    let rows: Vec<GridRow> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(rows.len(), 6);
}

#[test]
fn tab11_menus() {
    let mut a = app();
    open_tab(&mut a, 11);
    keys(&mut a, &[KeyCode::Right, KeyCode::Enter]);
    let s = screen(&mut a);
    assert!(
        s.contains("Text") && s.contains("Paragraph") && s.contains("Show ruler"),
        "{s}"
    );
    keys(&mut a, &[KeyCode::Right]);
    assert!(screen(&mut a).contains("Strikethrough"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(status(&a), "ℹ Picked Format ▸ Text ▸ Bold");
    // the file list: Enter opens the context menu
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Enter);
    let s = screen(&mut a);
    assert!(
        s.contains("Rename…") && s.contains("Duplicate") && s.contains("Delete…"),
        "{s}"
    );
    keys(
        &mut a,
        &[KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Enter],
    );
    let files: Vec<FileEntry> = serde_json::from_value(last_set(&a).1).unwrap();
    assert!(files[0].starred);
    ack_all(&mut a);
    // Shift+F10 too; Duplicate
    shift(&mut a, KeyCode::F(10));
    keys(&mut a, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    let files: Vec<FileEntry> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(files[1].name, "Budget 2027 copy.xlsx");
    ack_all(&mut a);
    // Rename via the input dialog
    keys(&mut a, &[KeyCode::Enter, KeyCode::Down, KeyCode::Enter]);
    ctrl(&mut a, 'u');
    typ(&mut a, "Plan.xlsx");
    key(&mut a, KeyCode::Enter);
    let files: Vec<FileEntry> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(files[1].name, "Plan.xlsx");
}

#[test]
fn tab12_palette() {
    let mut a = app();
    open_tab(&mut a, 12);
    typ(&mut a, "gantt");
    let s = screen(&mut a);
    assert!(s.contains("Go to: Gantt"), "{s}");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.tab, 15);
    let s = sets(&a);
    assert_eq!(s[0], ("palette.last".into(), json!("Go to: Gantt"), 1));
}

#[test]
fn tab13_dialogs() {
    let mut a = app();
    open_tab(&mut a, 13);
    key(&mut a, KeyCode::Enter);
    assert!(screen(&mut a).contains("Delete 3 files?"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(status(&a), "ℹ Confirm: you chose Cancel");
    // Form: invalid email blocks, ^S saves
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 'u');
    typ(&mut a, "Ada");
    key(&mut a, KeyCode::Tab);
    ctrl(&mut a, 'u');
    typ(&mut a, "nope");
    ctrl(&mut a, 's');
    assert!(screen(&mut a).contains("✗ must look like name@example.com"));
    assert!(a.outbox.is_empty());
    ctrl(&mut a, 'u');
    typ(&mut a, "ADA@Example.com");
    ctrl(&mut a, 's');
    let p: Profile = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(
        (p.name.as_str(), p.email.as_str()),
        ("Ada", "ada@example.com")
    );
    assert!(a.dialogs.is_empty());
    // Message, local Toast, Broadcast
    keys(&mut a, &[KeyCode::Right, KeyCode::Enter]);
    assert!(screen(&mut a).contains("information box"));
    keys(&mut a, &[KeyCode::Enter, KeyCode::Right, KeyCode::Enter]);
    assert!(screen(&mut a).contains("A local toast"));
    keys(&mut a, &[KeyCode::Right, KeyCode::Enter]);
    typ(&mut a, "Lunch is here");
    key(&mut a, KeyCode::Enter);
    assert!(
        matches!(a.outbox.last(), Some(ClientMsg::Toast { text, .. }) if text == "Lunch is here")
    );
}

#[test]
fn tab14_charts() {
    let mut a = app();
    open_tab(&mut a, 14);
    keys(&mut a, &[KeyCode::Right, KeyCode::Up]);
    let s: Vec<i64> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(s[1], 53);
    ack_all(&mut a);
    key(&mut a, KeyCode::PageDown);
    let s: Vec<i64> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(s[1], 33);
    ack_all(&mut a);
    assert!(screen(&mut a).contains("Feb: 33"));
}

#[test]
fn tab15_kanban() {
    let mut a = app();
    open_tab(&mut a, 15);
    shift(&mut a, KeyCode::Right);
    let cards: Vec<KanbanCard> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(cards.iter().find(|c| c.id == 5).unwrap().col, "doing");
    ack_all(&mut a);
    assert_eq!(a.tabs.kanban.col, 1, "the selection follows the card");
    key(&mut a, KeyCode::Char('>'));
    let cards: Vec<KanbanCard> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(cards.iter().find(|c| c.id == 5).unwrap().col, "review");
    ack_all(&mut a);
    key(&mut a, KeyCode::Insert);
    typ(&mut a, "Ship it");
    key(&mut a, KeyCode::Enter);
    let cards: Vec<KanbanCard> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(
        cards.last().unwrap(),
        &KanbanCard {
            id: 9,
            title: "Ship it".into(),
            col: "review".into()
        }
    );
    ack_all(&mut a);
    key(&mut a, KeyCode::Enter);
    ctrl(&mut a, 'u');
    typ(&mut a, "Shipped");
    key(&mut a, KeyCode::Enter);
    let cards: Vec<KanbanCard> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(cards.last().unwrap().title, "Shipped");
    ack_all(&mut a);
    key(&mut a, KeyCode::F(8));
    key(&mut a, KeyCode::Char('y'));
    let cards: Vec<KanbanCard> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(cards.len(), 8);
}

#[test]
fn tab16_gantt() {
    let mut a = app();
    open_tab(&mut a, 16);
    shift(&mut a, KeyCode::Right);
    let bars: Vec<GanttBar> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(bars[0].start, "2026-10-20");
    ack_all(&mut a);
    keys(&mut a, &[KeyCode::Down, KeyCode::Char(']')]);
    let bars: Vec<GanttBar> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(bars[1].days, 15);
    ack_all(&mut a);
    key(&mut a, KeyCode::Char('<'));
    let bars: Vec<GanttBar> = serde_json::from_value(last_set(&a).1).unwrap();
    assert_eq!(bars[1].start, "2026-10-25");
    ack_all(&mut a);
    let s = screen(&mut a);
    assert!(
        s.contains("Design · Sun 25 Oct 2026 → Sun 8 Nov 2026 · 15 days · scale: week"),
        "{s}"
    );
    key(&mut a, KeyCode::Char('+'));
    assert!(screen(&mut a).contains("scale: day"));
    keys(&mut a, &[KeyCode::Char('-'), KeyCode::Char('-')]);
    assert!(screen(&mut a).contains("scale: month"));
    key(&mut a, KeyCode::Char('t'));
    key(&mut a, KeyCode::Right);
}

/// `cargo test -p demo-tui dump_screens -- --ignored --nocapture` prints
/// every tab at the recording size (for eyeballing layouts).
#[test]
#[ignore]
fn dump_screens() {
    let mut a = app();
    for _ in 0..16 {
        key(&mut a, KeyCode::Enter);
        println!("{}\n", screen(&mut a));
        key(&mut a, KeyCode::F(6));
        key(&mut a, KeyCode::Down);
    }
}
