//! App-level behaviour and render tests: the whole UI on a `TestBackend`,
//! driven through the same key/mouse/server-message paths as the real thing.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use pt_core::model::*;
use pt_core::protocol::{Conflict, Op, ResultMsg, ServerMsg};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;
use tokio::sync::mpsc;

use crate::app::{App, Phase};
use crate::dialogs::Dialog;
use crate::net::{ConnState, ServerUrl, tls_config};
use crate::testdata::sample;
use crate::views::{self, Views};

fn app() -> App {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut a = App::new(ServerUrl::parse("http://127.0.0.1:1").unwrap(), tls_config(None).unwrap(), 0, tx);
    a.r = sample();
    a.views = Views::new(&a.r);
    a.phase = Phase::Main;
    a.conn_state = ConnState::Online;
    a
}

fn screen(a: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| crate::ui::draw(a, f)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
}

fn key(a: &mut App, code: KeyCode) {
    a.on_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn ctrl(a: &mut App, c: char) {
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)));
}

fn alt(a: &mut App, c: char) {
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)));
}

fn typ(a: &mut App, s: &str) {
    for c in s.chars() {
        key(a, KeyCode::Char(c));
    }
}

#[test]
fn projects_view_with_detail_pane() {
    let mut a = app();
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("1 Projects"), "{s}");
    assert!(s.contains("GIS-001") && s.contains("NWR-004"));
    assert!(s.contains("Tasks of GIS-001 Parcel fabric migration"), "{s}");
    assert!(s.contains("Field survey"));
    assert!(s.contains("[ 2 / 2 ]"));
    assert!(s.contains("^K Commands"));
    // F6 moves focus to the detail grid
    key(&mut a, KeyCode::F(6));
    assert_eq!(a.views.projects.pane, 1);
}

#[test]
fn small_terminal_message() {
    let mut a = app();
    let s = screen(&mut a, 70, 20);
    assert!(s.contains("needs at least 80×24"));
}

#[test]
fn every_view_renders() {
    let mut a = app();
    let expect = [
        (views::TASKS, "Load test checkout"),
        (views::BOARD, "In progress"),
        (views::TIMELINE, "NWR-004 POS refresh"),
        (views::DASHBOARD, "Active projects"),
        (views::ACTIVITY, "status: To do → In progress"),
        (views::TEMPLATES, "Preview: 2 tasks"),
        (views::FIELDS, "Billable"),
        (views::USERS, "Victor Viewer"),
    ];
    for (v, text) in expect {
        alt(&mut a, char::from(b'1' + v as u8));
        assert_eq!(a.view, v);
        let s = screen(&mut a, 160, 45);
        assert!(s.contains(text), "view {v} lacks {text:?}:\n{s}");
    }
    // and at the minimum size every view still draws its chrome
    for v in 0..9u8 {
        alt(&mut a, char::from(b'1' + v));
        let s = screen(&mut a, 80, 24);
        assert!(s.contains("9 Users"), "{s}");
    }
}

#[test]
fn tasks_card_follows_cursor() {
    let mut a = app();
    alt(&mut a, '2');
    screen(&mut a, 160, 40);
    // first row is the GIS-001 group; then its first task
    key(&mut a, KeyCode::Down);
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("GIS-001 #1"), "{s}");
    assert!(s.contains("Define area of interest"));
    assert!(s.contains("Parcel fabric migration · Municipal GIS"));
}

#[test]
fn status_dropdown_edit_sends_mutate() {
    let mut a = app();
    screen(&mut a, 160, 40);
    // cursor on GIS-001; Status is the 5th column
    for _ in 0..4 {
        key(&mut a, KeyCode::Right);
    }
    key(&mut a, KeyCode::Enter);
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("On hold") && s.contains("Cancelled"), "{s}");
    typ(&mut a, "act");
    key(&mut a, KeyCode::Enter);
    assert!(a.editor.is_none());
    let p = a.pending.values().next().expect("a pending mutate");
    assert_eq!(p.ops, vec![Op::set(Entity::Project, 11, "status", json!("planning"), json!("active"))]);
    assert!(a.pending_cells.contains(&(Entity::Project, 11, "status".into())));
}

#[test]
fn typing_starts_text_edit_and_parses_dates() {
    let mut a = app();
    alt(&mut a, '2');
    screen(&mut a, 160, 40);
    key(&mut a, KeyCode::Down);
    a.views.tasks.select_col_key("due_date");
    typ(&mut a, "fri");
    assert!(a.editor.is_some());
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("→ Fri 23 Oct 2026"), "calendar preview: {s}");
    key(&mut a, KeyCode::Enter);
    let p = a.pending.values().next().unwrap();
    assert_eq!(p.ops, vec![Op::set(Entity::Task, 110, "due_date", json!("2026-11-03"), json!("2026-10-23"))]);
}

#[test]
fn invalid_input_keeps_editor_open() {
    let mut a = app();
    alt(&mut a, '2');
    screen(&mut a, 160, 40);
    key(&mut a, KeyCode::Down);
    a.views.tasks.select_col_key("est_hours");
    typ(&mut a, "4");
    a.editor.as_mut().unwrap().ed.input.set("abc");
    key(&mut a, KeyCode::Enter);
    assert!(a.editor.as_ref().is_some_and(|e| e.ed.error.is_some()));
    assert!(a.pending.is_empty());
}

#[test]
fn tsv_paste_is_one_mutate() {
    let mut a = app();
    alt(&mut a, '2');
    a.views.tasks.group = None;
    a.views.tasks.touch();
    screen(&mut a, 160, 40);
    a.views.tasks.select_col_key("est_hours");
    a.on_event(Event::Paste("1\r2\r3".into()));
    assert_eq!(a.pending.len(), 1);
    let ops = &a.pending.values().next().unwrap().ops;
    assert_eq!(ops.len(), 3);
    assert!(matches!(&ops[0], Op::Set { field, value, .. } if field == "est_hours" && value == &json!(1.0)));
}

#[test]
fn copy_then_fill_down() {
    let mut a = app();
    alt(&mut a, '2');
    a.views.tasks.group = None;
    a.views.tasks.touch();
    screen(&mut a, 160, 40);
    a.views.tasks.select_col_key("phase");
    ctrl(&mut a, 'c');
    assert_eq!(a.clipboard, "Scope");
    // mark rows 1-3 and fill the first value down
    key(&mut a, KeyCode::Char(' '));
    key(&mut a, KeyCode::Char(' '));
    key(&mut a, KeyCode::Char(' '));
    ctrl(&mut a, 'd');
    let ops = &a.pending.values().next().unwrap().ops;
    assert_eq!(ops.len(), 2);
    assert!(ops.iter().all(|o| matches!(o, Op::Set { value, .. } if value == &json!("Scope"))));
}

#[test]
fn conflicts_mark_cells_and_explain() {
    let mut a = app();
    screen(&mut a, 160, 40);
    for _ in 0..4 {
        key(&mut a, KeyCode::Right);
    }
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "act");
    key(&mut a, KeyCode::Enter);
    let req = *a.pending.keys().next().unwrap();
    a.on_net(crate::net::NetEvent::Msg(Box::new(ServerMsg::Result(ResultMsg {
        req,
        ok: true,
        conflicts: vec![Conflict { index: 0, entity: Entity::Project, id: 11, field: "status".into(), current: json!("on_hold"), by: Some(3) }],
        ..Default::default()
    }))));
    assert!(a.conflicts.contains_key(&(Entity::Project, 11, "status".into())));
    key(&mut a, KeyCode::Up);
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("⚠"), "{s}");
    assert!(s.contains("Bob Stone set this to “On hold”"), "{s}");
    // Esc dismisses
    key(&mut a, KeyCode::Esc);
    assert!(a.conflicts.is_empty());
}

#[test]
fn remote_changes_flash_and_toast() {
    let mut a = app();
    let mut t = a.r.tasks[&101].clone();
    t.status = TaskStatus::Review;
    let act = Activity {
        id: 2,
        ts: "2026-10-17T10:00:00Z".into(),
        user_id: Some(3),
        group: 2,
        action: Action::Set,
        entity: "task".into(),
        entity_id: 101,
        field: "status".into(),
        old: json!("in_progress"),
        new: json!("review"),
        label: "NWR-004 #2 status: In progress → Review".into(),
        state: ActState::Done,
    };
    a.on_net(crate::net::NetEvent::Msg(Box::new(ServerMsg::Changes(pt_core::protocol::Changes {
        by: Some(3),
        group: Some(2),
        upserts: Tables { tasks: vec![t], ..Default::default() },
        deletes: Default::default(),
        activity: vec![act],
    }))));
    assert!(a.flashes.contains_key(&(Entity::Task, 101)));
    assert!(a.toasts.iter().any(|t| t.text.contains("bob: NWR-004 #2 status")));
    assert_eq!(a.r.tasks[&101].status, TaskStatus::Review);
}

#[test]
fn query_bar_filters_and_completes() {
    let mut a = app();
    alt(&mut a, '2');
    ctrl(&mut a, 'f');
    assert!(a.focus_query);
    typ(&mut a, "sta");
    assert_eq!(a.queries[views::TASKS].sugg, vec!["status:", "start:"]);
    key(&mut a, KeyCode::Tab);
    typ(&mut a, "blo");
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.queries[views::TASKS].input.text(), "status:blocked ");
    key(&mut a, KeyCode::Enter);
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("[ 1 / 5 ]"), "{s}");
    assert!(s.contains("Field survey"));
    // a bad term is reported
    ctrl(&mut a, 'f');
    typ(&mut a, "frob:1");
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("unknown field 'frob'"), "{s}");
}

#[test]
fn new_project_dialog_cascades() {
    let mut a = app();
    screen(&mut a, 160, 40);
    key(&mut a, KeyCode::Insert);
    let s = screen(&mut a, 160, 40);
    // the current row is GIS-001 → program Municipal GIS → next code GIS-002
    assert!(s.contains("New project") && s.contains("GIS-002"), "{s}");
    assert!(s.contains("2 tasks, 44 h, 21 days will be created"), "{s}");
    let Some(Dialog::Form(f)) = a.dialogs.last_mut() else { panic!() };
    // switch program to Northwind: the code follows
    f.focus = 0;
    key(&mut a, KeyCode::Enter);
    typ(&mut a, "north");
    key(&mut a, KeyCode::Enter);
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("NWR-005"), "{s}");
    // name is required
    ctrl(&mut a, 's');
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("Please fill in the required fields"), "{s}");
}

#[test]
fn new_task_dialog_filters_projects_by_program() {
    let mut a = app();
    alt(&mut a, '2');
    key(&mut a, KeyCode::Insert);
    let Some(Dialog::Form(f)) = a.dialogs.last() else { panic!() };
    assert_eq!(f.fields[1].ed.choices.len(), 2, "no program → all projects");
    a.dialogs.pop();
    // from a GIS task the program and project are preset
    screen(&mut a, 160, 40);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Insert);
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("1 project(s) in Municipal GIS"), "{s}");
    assert!(s.contains("phases of GIS-001"));
}

#[test]
fn record_form_saves_changed_fields_only() {
    let mut a = app();
    alt(&mut a, '2');
    screen(&mut a, 160, 40);
    key(&mut a, KeyCode::Down);
    ctrl(&mut a, 'e');
    let s = screen(&mut a, 160, 45);
    assert!(s.contains("Task GIS-001 #1 — Define area of interest"), "{s}");
    let Some(Dialog::Form(f)) = a.dialogs.last_mut() else { panic!() };
    f.fields[0].ed.input.set("Define AOI");
    ctrl(&mut a, 's');
    let p = a.pending.values().next().unwrap();
    assert_eq!(p.ops, vec![Op::set(Entity::Task, 110, "title", json!("Define area of interest"), json!("Define AOI"))]);
}

#[test]
fn palette_help_menu_and_context() {
    let mut a = app();
    screen(&mut a, 160, 40);
    ctrl(&mut a, 'k');
    typ(&mut a, "nwr-004");
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("NWR-004  POS refresh"), "{s}");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.views.projects.main.current_id(), Some(10));
    key(&mut a, KeyCode::F(1));
    typ(&mut a, "fill");
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("Fill down") && !s.contains("Timeline"), "{s}");
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::F(10));
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("New project…"), "{s}");
    key(&mut a, KeyCode::Right);
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("Fill down") && s.contains("Undo"), "{s}");
    key(&mut a, KeyCode::Esc);
    a.on_event(Event::Key(KeyEvent::new(KeyCode::F(10), KeyModifiers::SHIFT)));
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("Filter by this value"), "{s}");
}

#[test]
fn board_keys_move_cards() {
    let mut a = app();
    alt(&mut a, '3');
    screen(&mut a, 160, 40);
    // most urgent To do card first; move it right
    let id = a.views.board.current.unwrap();
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT)));
    let p = a.pending.values().next().unwrap();
    assert!(matches!(&p.ops[0], Op::Set { id: i, field, value, .. } if *i == id && field == "status" && value == &json!("in_progress")));
    ctrl(&mut a, 'r');
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("── Alice Nguyen"), "{s}");
}

#[test]
fn timeline_expand_and_shift() {
    let mut a = app();
    alt(&mut a, '4');
    screen(&mut a, 160, 40);
    // rows sorted by start: NWR-004 (Sep) then GIS-001 (Nov)
    key(&mut a, KeyCode::Right);
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("#2 Load test checkout"), "{s}");
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT)));
    let p = a.pending.values().next().unwrap();
    assert_eq!(p.ops.len(), 2, "start and due move together");
    key(&mut a, KeyCode::Char('+'));
    assert_eq!(a.views.timeline.zoom, crate::views::timeline::Zoom::Day);
}

#[test]
fn dashboard_drills_down() {
    let mut a = app();
    alt(&mut a, '5');
    screen(&mut a, 160, 45);
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, views::TASKS);
    assert_eq!(a.queries[views::TASKS].input.text(), "is:overdue");
    let s = screen(&mut a, 160, 40);
    assert!(s.contains("[ 1 / 5 ]"), "{s}");
}

#[test]
fn viewer_cannot_edit() {
    let mut a = app();
    a.r.me = a.r.users.get(&4).cloned();
    screen(&mut a, 160, 40);
    key(&mut a, KeyCode::Char('x'));
    assert!(a.editor.is_none());
    assert!(a.toasts.iter().any(|t| t.text.contains("read-only")));
}

#[test]
fn mouse_clicks_tabs_and_sorts_headers() {
    let mut a = app();
    screen(&mut a, 160, 40);
    let (r, v) = a.hits.tabs[2];
    a.on_event(Event::Mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x + 1, row: r.y, modifiers: KeyModifiers::NONE }));
    assert_eq!(a.view, v);
    alt(&mut a, '1');
    screen(&mut a, 160, 40);
    let (_, x, _) = a.views.projects.main.layout.cols[1];
    let y = a.views.projects.main.layout.header_y;
    a.on_event(Event::Mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x + 1, row: y, modifiers: KeyModifiers::NONE }));
    assert_eq!(a.views.projects.main.sort[0].key, "name");
}

#[test]
fn login_screen() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut a = App::new(ServerUrl::parse("http://127.0.0.1:1").unwrap(), tls_config(None).unwrap(), 1, tx);
    a.show_login(Some("Logged out.".into()));
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("Log in to pt") && s.contains("http://127.0.0.1:1") && s.contains("Logged out."), "{s}");
}
