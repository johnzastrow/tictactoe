//! Tab 5 — Combobox: Owner and Reviewer. Typing opens a fuzzy list over
//! People (protocol §7) with matched letters highlighted; Enter picks and
//! commits, Del clears to null ("— none —").

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::dataset::{Person, person, search_people};
use demo_core::fuzzy::Match;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::Value;

use super::{h, is_focused, label, muted};
use crate::app::{App, Hint, Hit};
use crate::widgets::draw::{fill, put};
use crate::widgets::input::TextInput;
use crate::widgets::list::{self, Row};

pub const KEYS: &[(&str, &str)] = &[
    ("type", "fuzzy search"),
    ("↓", "open the list"),
    ("↑↓", "move"),
    ("Enter", "pick & save"),
    ("Esc", "close"),
    ("Del", "clear to none"),
];

const FIELDS: [(&str, &str); 2] = [("Owner", "combo.owner"), ("Reviewer", "combo.reviewer")];

#[derive(Default)]
pub struct State {
    pub input: TextInput,
    /// The user typed a query (the box shows it instead of the value).
    pub typing: bool,
    pub open: bool,
    pub sel: usize,
    anchor: Rect,
}

impl State {
    pub fn close(&mut self) {
        self.open = false;
        self.typing = false;
        self.input.clear();
    }

    pub fn matches(&self) -> Vec<(&'static Person, Match)> {
        let q = if self.typing {
            self.input.text()
        } else {
            String::new()
        };
        search_people(&q)
    }
}

fn current(app: &App, c: usize) -> Option<&'static Person> {
    app.shown(FIELDS[c].1).as_str().and_then(person)
}

fn pick(app: &mut App, c: usize, p: Option<&'static Person>) {
    app.tabs.combo.close();
    app.commit(
        FIELDS[c].1,
        p.map_or(Value::Null, |p| Value::from(p.username)),
    );
}

fn open(app: &mut App, c: usize) {
    let cur = current(app, c);
    let s = &mut app.tabs.combo;
    s.open = true;
    s.sel = if s.typing {
        0
    } else {
        cur.and_then(|p| {
            s.matches()
                .iter()
                .position(|(q, _)| q.username == p.username)
        })
        .unwrap_or(0)
    };
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let c = app.ctrl;
    let ctrl = k
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let s = &mut app.tabs.combo;
    let n = s.matches().len();
    match k.code {
        KeyCode::Down if !s.open => open(app, c),
        KeyCode::Down => s.sel = if n == 0 { 0 } else { (s.sel + 1) % n },
        KeyCode::Up if s.open => s.sel = if n == 0 { 0 } else { (s.sel + n - 1) % n },
        KeyCode::Up if c > 0 => app.next_ctrl(true),
        KeyCode::Enter if s.open => {
            let p = s.matches().get(s.sel).map(|(p, _)| *p);
            match p {
                Some(p) => pick(app, c, Some(p)),
                None => app.info("No one matches; Esc closes"),
            }
        }
        KeyCode::Enter => open(app, c),
        KeyCode::Esc if s.open || s.typing => s.close(),
        KeyCode::Esc => return false,
        KeyCode::Delete if !s.typing => pick(app, c, None),
        KeyCode::Tab | KeyCode::BackTab => {
            s.close();
            return false;
        }
        KeyCode::Char(_) if !ctrl && !s.typing => {
            s.typing = true;
            s.input.clear();
            s.input.handle(&k);
            open(app, c);
        }
        _ if s.typing => {
            if s.input.handle(&k) {
                s.sel = 0;
                s.open = true;
            }
        }
        _ => return false,
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    match hit {
        Hit::Ctrl(c) => {
            let was = app.ctrl == *c && app.tabs.combo.open;
            app.focus_ctrl(*c);
            if was {
                app.tabs.combo.close();
            } else {
                open(app, *c);
            }
            true
        }
        Hit::Popup(c, j) => {
            let p = app.tabs.combo.matches().get(*j).map(|(p, _)| *p);
            if p.is_some() {
                pick(app, *c, p);
            }
            true
        }
        Hit::Btn(c, 0) => {
            app.focus_ctrl(*c);
            pick(app, *c, None);
            true
        }
        _ => false,
    }
}

pub fn hints(app: &App) -> Vec<Hint> {
    if app.tabs.combo.open {
        vec![
            h("type", "Search", KeyCode::Null),
            h("↑↓", "Move", KeyCode::Down),
            h("Enter", "Pick", KeyCode::Enter),
            h("Esc", "Close", KeyCode::Esc),
        ]
    } else {
        vec![
            h("type", "Search", KeyCode::Null),
            h("↓", "List", KeyCode::Down),
            h("Del", "None", KeyCode::Delete),
        ]
    }
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let x = area.x + 1;
    let fw = 30;
    for (c, (name, key)) in FIELDS.iter().enumerate() {
        let y = area.y + c as u16 * 2;
        let focused = is_focused(app, c);
        label(buf, x, y, name, focused, &t, 10);
        let fx = x + 11;
        let br = Style::new()
            .fg(if focused { t.accent } else { t.border })
            .bg(t.bg);
        put(buf, fx, y, "[", br, 1);
        put(buf, fx + fw - 1, y, "]", br, 1);
        let inner = Rect::new(fx + 1, y, fw - 2, 1);
        let bg = if focused { t.sel_bg } else { t.panel };
        let cur = current(app, c);
        if focused && app.tabs.combo.typing {
            app.tabs.combo.input.render(
                buf,
                Rect::new(inner.x, y, inner.width - 2, 1),
                Style::new().bg(bg).fg(t.fg),
                true,
            );
        } else {
            fill(buf, inner, Style::new().bg(bg));
            match cur {
                Some(p) => put(
                    buf,
                    inner.x + 1,
                    y,
                    p.name,
                    Style::new().fg(t.fg).bg(bg).add_modifier(Modifier::BOLD),
                    inner.width as usize - 3,
                ),
                None => put(
                    buf,
                    inner.x + 1,
                    y,
                    "— none —",
                    Style::new()
                        .fg(t.muted)
                        .bg(bg)
                        .add_modifier(Modifier::ITALIC),
                    12,
                ),
            };
        }
        put(
            buf,
            fx + fw - 3,
            y,
            "▾",
            Style::new().fg(t.accent).bg(bg),
            1,
        );
        app.hits.push((Rect::new(fx, y, fw, 1), Hit::Ctrl(c)));
        app.key_rects.push((key, inner));
        let mut mx = fx + fw + 1;
        if cur.is_some() {
            put(buf, mx, y, "[×]", Style::new().fg(t.accent).bg(t.bg), 3);
            app.hits.push((Rect::new(mx, y, 3, 1), Hit::Btn(c, 0)));
            mx += 4;
        }
        let info = cur
            .map(|p| format!("{} · {}", p.username, p.team))
            .unwrap_or_default();
        put(
            buf,
            mx,
            y,
            &info,
            muted(&t),
            area.right().saturating_sub(mx) as usize,
        );
        if focused {
            app.tabs.combo.anchor = Rect::new(fx, y, fw, 1);
        }
    }
}

pub fn popup(app: &mut App, buf: &mut Buffer, _area: Rect) {
    let s = &app.tabs.combo;
    if !s.open {
        return;
    }
    let t = app.t.clone();
    let rows: Vec<Row> = s
        .matches()
        .into_iter()
        .map(|(p, m)| {
            let n = p.name.chars().count();
            Row {
                label: p.name.to_string(),
                hits: m.indices.into_iter().filter(|i| *i < n).collect(),
                color: None,
                dim: format!("{} · {}", p.username, p.team),
            }
        })
        .collect();
    let a = s.anchor;
    let r = Rect::new(a.x, a.y + 1, 46, (rows.len() as u16 + 2).clamp(3, 11));
    let title = if s.typing {
        format!("“{}”", s.input.text())
    } else {
        "People".into()
    };
    let sel = s.sel;
    let c = app.ctrl;
    for (rect, j) in list::render(buf, r, &title, &rows, Some(sel), &t) {
        app.hits.push((rect, Hit::Popup(c, j)));
    }
}
