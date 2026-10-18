//! Tab 13 — Dialogs: a row of buttons, each opening one kind of popup:
//! Confirm…, Form… (edits `dialog.profile`), Message, Toast (local) and
//! Broadcast… (a `toast` to everyone).

use crossterm::event::{KeyCode, KeyEvent};
use demo_core::keys::Profile;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{h, is_focused, muted};
use crate::app::{App, Hint, Hit, Level};
use crate::dialogs::{Dialog, DialogAct, ProfileForm, profile_line};
use crate::widgets::draw::{put, width};

pub const KEYS: &[(&str, &str)] = &[
    ("←→ Tab", "choose a button"),
    ("Enter Space", "open"),
    ("y / n", "confirm shortcuts"),
    ("^S", "save the form"),
    ("Esc", "cancel a dialog"),
];

const BUTTONS: [&str; 5] = ["Confirm…", "Form…", "Message", "Toast", "Broadcast…"];

fn open(app: &mut App, i: usize) {
    match i {
        0 => app.dialogs.push(Dialog::confirm("Confirm", "Delete 3 files?", "Delete", DialogAct::DemoConfirm)),
        1 => {
            let p: Profile = app.shown_get("dialog.profile").unwrap_or_default();
            let base = app.v.version("dialog.profile");
            app.dialogs.push(Dialog::Form(ProfileForm::new(&p, base)));
        }
        2 => app.dialogs.push(Dialog::message(
            "Message",
            "This is an information box.\nIt has a single OK button: Enter, Esc or a click closes it.",
        )),
        3 => app.toast("A local toast: only you see it. It goes away after 4 s.".into(), Level::Info),
        _ => app.dialogs.push(Dialog::input("Broadcast", "A message for everyone connected (1–120 chars):", "", DialogAct::Broadcast)),
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let c = app.ctrl;
    match k.code {
        KeyCode::Enter | KeyCode::Char(' ') => open(app, c),
        KeyCode::Right if c < 4 => app.focus_ctrl(c + 1),
        KeyCode::Left if c > 0 => app.focus_ctrl(c - 1),
        KeyCode::Left | KeyCode::Right => {}
        _ => return false,
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    if let Hit::Ctrl(c) = hit {
        app.focus_ctrl(*c);
        open(app, *c);
        return true;
    }
    false
}

pub fn hints(_app: &App) -> Vec<Hint> {
    vec![
        h("Enter", "Open", KeyCode::Enter),
        h("←→", "Button", KeyCode::Right),
    ]
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let x = area.x + 1;
    let mut bx = x;
    for (i, b) in BUTTONS.iter().enumerate() {
        let focused = is_focused(app, i);
        let s = format!("[ {b} ]");
        let st = if focused {
            Style::new()
                .bg(t.accent)
                .fg(t.bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().bg(t.panel).fg(t.accent)
        };
        let w = width(&s) as u16;
        put(buf, bx, area.y, &s, st, w as usize);
        app.hits.push((Rect::new(bx, area.y, w, 1), Hit::Ctrl(i)));
        bx += w + 2;
    }
    let notes = [
        "Confirm…    Cancel is the default; y / n answer directly; the result shows on the status line.",
        "Form…       edits dialog.profile: Tab between fields, ^S or [Save] commits, Esc cancels.",
        "Message     an information box with a single OK.",
        "Toast       a local toast, top right, gone after 4 s.",
        "Broadcast…  sends a toast to everyone connected (at most one per 2 s).",
    ];
    for (i, n) in notes.iter().enumerate() {
        put(
            buf,
            x,
            area.y + 2 + i as u16,
            n,
            muted(&t),
            area.width as usize - 2,
        );
    }
    let p: Profile = app.shown_get("dialog.profile").unwrap_or_default();
    let y = area.y + 8;
    put(
        buf,
        x,
        y,
        "Profile",
        Style::new().fg(t.fg).bg(t.bg).add_modifier(Modifier::BOLD),
        8,
    );
    let line = profile_line(&p);
    let w = put(
        buf,
        x + 9,
        y,
        &line,
        Style::new().fg(t.accent).bg(t.bg),
        area.width as usize - 12,
    );
    app.key_rects
        .push(("dialog.profile", Rect::new(x + 9, y, w.max(1), 1)));
}
