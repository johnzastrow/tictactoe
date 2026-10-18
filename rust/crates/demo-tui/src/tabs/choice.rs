//! Tab 8 — Choices: a checkbox (Notify me), a checkbox group (Channels,
//! disabled while Notify is off), radio buttons (Size) and a toggle switch
//! (Dark mode). Space toggles/selects; every change commits.

use crossterm::event::{KeyCode, KeyEvent};
use demo_core::dataset::{CHANNELS, SIZES};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::{Value, json};

use super::{h, is_focused, label, muted};
use crate::app::{App, Hint, Hit};
use crate::widgets::draw::{put, width};

pub const KEYS: &[(&str, &str)] = &[
    ("Space Enter", "toggle / select"),
    ("↑↓ ←→", "move in a group"),
    ("Tab", "next group"),
    ("click", "toggle / select"),
];

#[derive(Default)]
pub struct State {
    /// Cursor in the Channels group.
    pub ch: usize,
    /// Cursor in the Size group.
    pub size: usize,
}

pub fn enter_ctrl(app: &mut App) {
    if app.ctrl == 2 {
        let cur = app.shown_str("choice.size");
        app.tabs.choice.size = SIZES.iter().position(|s| s.id == cur).unwrap_or(0);
    }
}

fn notify(app: &App) -> bool {
    app.shown("choice.notify").as_bool().unwrap_or(false)
}

fn toggle_channel(app: &mut App, j: usize) {
    if !notify(app) {
        app.info("turn on Notify first");
        return;
    }
    let mut chans: Vec<String> = app.shown_get("choice.channels").unwrap_or_default();
    let id = CHANNELS[j].id.to_string();
    if let Some(p) = chans.iter().position(|c| *c == id) {
        chans.remove(p);
    } else {
        chans.push(id);
    }
    app.commit("choice.channels", json!(chans));
}

fn activate(app: &mut App, c: usize, j: usize) {
    match c {
        0 => {
            let on = !notify(app);
            app.commit("choice.notify", Value::from(on));
        }
        1 => toggle_channel(app, j),
        2 => {
            app.tabs.choice.size = j;
            app.commit("choice.size", Value::from(SIZES[j].id));
        }
        _ => {
            let on = !app.shown("choice.dark").as_bool().unwrap_or(false);
            app.commit("choice.dark", Value::from(on));
        }
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let c = app.ctrl;
    let dark = app.shown("choice.dark").as_bool().unwrap_or(false);
    let s = &mut app.tabs.choice;
    match (c, k.code) {
        (1, KeyCode::Down | KeyCode::Right) => s.ch = (s.ch + 1) % 4,
        (1, KeyCode::Up | KeyCode::Left) => s.ch = (s.ch + 3) % 4,
        (2, KeyCode::Right | KeyCode::Down) => s.size = (s.size + 1) % 4,
        (2, KeyCode::Left | KeyCode::Up) => s.size = (s.size + 3) % 4,
        (3, KeyCode::Left) if dark => activate(app, 3, 0),
        (3, KeyCode::Right) if !dark => activate(app, 3, 0),
        (3, KeyCode::Left | KeyCode::Right) => {}
        (_, KeyCode::Char(' ') | KeyCode::Enter) => {
            let j = match c {
                1 => s.ch,
                2 => s.size,
                _ => 0,
            };
            activate(app, c, j);
        }
        _ => return false,
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    match hit {
        Hit::Item(c, j) => {
            app.focus_ctrl(*c);
            match c {
                1 => app.tabs.choice.ch = *j,
                2 => app.tabs.choice.size = *j,
                _ => {}
            }
            activate(app, *c, *j);
            true
        }
        _ => false,
    }
}

pub fn hints(app: &App) -> Vec<Hint> {
    match app.ctrl {
        0 | 3 => vec![h("Space", "Toggle", KeyCode::Char(' '))],
        1 => vec![
            h("↑↓", "Move", KeyCode::Down),
            h("Space", "Toggle", KeyCode::Char(' ')),
        ],
        _ => vec![
            h("←→", "Move", KeyCode::Right),
            h("Space", "Select", KeyCode::Char(' ')),
        ],
    }
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let x = area.x + 1;
    let fx = x + 12;
    let on_st = |focus: bool, dim: bool| {
        let mut s = Style::new().fg(t.fg).bg(t.bg);
        if focus {
            s = s.bg(t.cur_bg).add_modifier(Modifier::BOLD);
        }
        if dim {
            s = s.fg(t.muted).add_modifier(Modifier::DIM);
        }
        s
    };
    let mark = |b: bool| if b { "[✓]" } else { "[ ]" };
    // Notify
    let f0 = is_focused(app, 0);
    let notify = notify(app);
    label(buf, x, area.y, "Notify", f0, &t, 11);
    let s = format!("{} Notify me", mark(notify));
    put(buf, fx, area.y, &s, on_st(f0, false), width(&s));
    put(
        buf,
        fx,
        area.y,
        mark(notify),
        on_st(f0, false).fg(if notify { t.ok } else { t.muted }),
        3,
    );
    app.hits
        .push((Rect::new(fx, area.y, width(&s) as u16, 1), Hit::Item(0, 0)));
    app.key_rects
        .push(("choice.notify", Rect::new(fx, area.y, 3, 1)));
    // Channels (vertical)
    let f1 = is_focused(app, 1);
    let y1 = area.y + 2;
    label(buf, x, y1, "Channels", f1, &t, 11);
    let chans: Vec<String> = app.shown_get("choice.channels").unwrap_or_default();
    for (j, ch) in CHANNELS.iter().enumerate() {
        let y = y1 + j as u16;
        let on = chans.iter().any(|c| c == ch.id);
        let cur = f1 && app.tabs.choice.ch == j;
        let s = format!("{} {:<6}", mark(on), ch.label);
        put(buf, fx, y, &s, on_st(cur, !notify), width(&s));
        app.hits
            .push((Rect::new(fx, y, width(&s) as u16, 1), Hit::Item(1, j)));
    }
    app.key_rects
        .push(("choice.channels", Rect::new(fx, y1, 3, 4)));
    if !notify {
        put(
            buf,
            fx + 12,
            y1,
            "disabled: turn on Notify first",
            muted(&t),
            area.right().saturating_sub(fx + 12) as usize,
        );
    }
    // Size (horizontal radio)
    let f2 = is_focused(app, 2);
    let y2 = y1 + 5;
    label(buf, x, y2, "Size", f2, &t, 11);
    let size = app.shown_str("choice.size");
    let mut sx = fx;
    for (j, sz) in SIZES.iter().enumerate() {
        let on = size == sz.id;
        let cur = f2 && app.tabs.choice.size == j;
        let s = format!("({}) {}", if on { "•" } else { " " }, sz.label);
        let w = width(&s) as u16;
        put(
            buf,
            sx,
            y2,
            &s,
            on_st(cur, false).fg(if on { t.accent } else { t.fg }),
            w as usize,
        );
        app.hits.push((Rect::new(sx, y2, w, 1), Hit::Item(2, j)));
        sx += w + 3;
    }
    app.key_rects
        .push(("choice.size", Rect::new(fx, y2, sx - fx, 1)));
    // Dark mode (toggle switch)
    let f3 = is_focused(app, 3);
    let y3 = y2 + 2;
    label(buf, x, y3, "Dark mode", f3, &t, 11);
    let dark = app.shown("choice.dark").as_bool().unwrap_or(false);
    let (sw, txt) = if dark {
        ("[ ━● ]", "On")
    } else {
        ("[ ●━ ]", "Off")
    };
    put(
        buf,
        fx,
        y3,
        sw,
        on_st(f3, false).fg(if dark { t.ok } else { t.muted }),
        6,
    );
    put(
        buf,
        fx + 7,
        y3,
        txt,
        on_st(false, false).add_modifier(Modifier::BOLD),
        3,
    );
    app.hits.push((Rect::new(fx, y3, 10, 1), Hit::Item(3, 0)));
    app.key_rects.push(("choice.dark", Rect::new(fx, y3, 6, 1)));
}
