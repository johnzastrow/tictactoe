//! Tab 4 — Dropdown: Status and Priority as closed dropdowns showing a
//! coloured chip; the open list filters as you type and jumps to the first
//! label starting with the typed text.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::dataset::{Choice, PRIORITIES, STATUSES};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::Value;

use super::{h, is_focused, label, muted};
use crate::app::{App, Hint, Hit};
use crate::theme::Theme;
use crate::widgets::draw::{put, width};
use crate::widgets::list::{self, Drop, DropOut, Row};

pub const KEYS: &[(&str, &str)] = &[
    ("Enter Space ↓", "open"),
    ("↑↓", "move"),
    ("type", "filter / jump"),
    ("Enter", "pick & save"),
    ("Esc", "close"),
    ("Tab", "next dropdown"),
];

const FIELDS: [(&str, &str, &[Choice]); 2] = [
    ("Status", "select.status", &STATUSES),
    ("Priority", "select.priority", &PRIORITIES),
];

#[derive(Default)]
pub struct State {
    pub drop: Drop,
    /// Where the open list hangs (set while drawing).
    anchor: Rect,
}

pub fn labels(list: &[Choice]) -> Vec<&'static str> {
    list.iter().map(|c| c.label).collect()
}

/// Rows of a choice list after the dropdown's filter, with colours.
pub fn rows(list: &[Choice], d: &Drop, t: &Theme) -> Vec<Row> {
    let ls = labels(list);
    d.visible(&ls)
        .into_iter()
        .map(|i| {
            let c = &list[i];
            let f = d.filter.to_lowercase();
            let hits = match c.label.to_lowercase().find(&f) {
                Some(b) if !f.is_empty() => {
                    let start = c.label[..b].chars().count();
                    (start..start + f.chars().count()).collect()
                }
                _ => vec![],
            };
            Row {
                label: c.label.to_string(),
                hits,
                color: (!c.color.is_empty()).then(|| t.hex(c.color, t.fg)),
                dim: String::new(),
            }
        })
        .collect()
}

/// A closed dropdown box `[ ● In progress   ▾ ]`; returns its rect.
#[allow(clippy::too_many_arguments)]
pub fn closed(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    c: Option<&Choice>,
    placeholder: &str,
    focused: bool,
    enabled: bool,
    t: &Theme,
) -> Rect {
    let bg = if focused { t.sel_bg } else { t.panel };
    let br = Style::new()
        .fg(if focused { t.accent } else { t.border })
        .bg(t.bg);
    put(buf, x, y, "[", br, 1);
    put(buf, x + w - 1, y, "]", br, 1);
    let inner = Rect::new(x + 1, y, w - 2, 1);
    crate::widgets::draw::fill(buf, inner, Style::new().bg(bg));
    match c {
        Some(c) if enabled => {
            let col = if c.color.is_empty() {
                t.fg
            } else {
                t.hex(c.color, t.fg)
            };
            put(buf, x + 2, y, "●", Style::new().fg(col).bg(bg), 1);
            put(
                buf,
                x + 4,
                y,
                c.label,
                Style::new().fg(t.fg).bg(bg).add_modifier(Modifier::BOLD),
                (w - 7) as usize,
            );
        }
        _ => {
            put(
                buf,
                x + 2,
                y,
                placeholder,
                Style::new()
                    .fg(t.muted)
                    .bg(bg)
                    .add_modifier(Modifier::ITALIC),
                (w - 5) as usize,
            );
        }
    }
    put(
        buf,
        x + w - 3,
        y,
        "▾",
        Style::new()
            .fg(if enabled { t.accent } else { t.muted })
            .bg(bg),
        1,
    );
    inner
}

/// Draw an open dropdown list under `anchor`.
pub fn open_list(
    app: &mut App,
    buf: &mut Buffer,
    anchor: Rect,
    list: &[Choice],
    c: usize,
    title_hint: &str,
) {
    let t = app.t.clone();
    let d = match app.tab {
        8 => &app.tabs.cascade.drop,
        _ => &app.tabs.select.drop,
    };
    let rows = rows(list, d, &t);
    let title = if d.filter.is_empty() {
        title_hint.to_string()
    } else {
        format!("filter: {}", d.filter)
    };
    let w = rows
        .iter()
        .map(|r| width(&r.label) + 6)
        .max()
        .unwrap_or(10)
        .max(width(&title) + 6)
        .max(anchor.width as usize) as u16;
    let r = Rect::new(
        anchor.x,
        anchor.y + 1,
        w,
        (rows.len() as u16 + 2).clamp(3, 12),
    );
    let sel = d.sel;
    for (rect, j) in list::render(buf, r, &title, &rows, Some(sel), &t) {
        app.hits.push((rect, Hit::Popup(c, j)));
    }
}

fn current(app: &App, c: usize) -> Option<usize> {
    let v = app.shown(FIELDS[c].1);
    FIELDS[c].2.iter().position(|ch| Some(ch.id) == v.as_str())
}

fn pick(app: &mut App, c: usize, i: usize) {
    let list = FIELDS[c].2;
    app.commit(FIELDS[c].1, Value::from(list[i].id));
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let c = app.ctrl;
    let list = FIELDS[c].2;
    let ls = labels(list);
    if app.tabs.select.drop.open {
        return match app.tabs.select.drop.key(&ls, &k) {
            DropOut::Pick(i) => {
                pick(app, c, i);
                true
            }
            DropOut::Used | DropOut::Closed => true,
            DropOut::Ignored => false,
        };
    }
    let cur = current(app, c);
    match k.code {
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Down => app.tabs.select.drop.open_at(cur),
        KeyCode::Char(ch)
            if !k
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.tabs.select.drop.open_at(cur);
            app.tabs
                .select
                .drop
                .key(&ls, &KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        KeyCode::Up if c > 0 => app.next_ctrl(true),
        _ => return false,
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    match hit {
        Hit::Ctrl(c) => {
            let was = app.ctrl == *c && app.tabs.select.drop.open;
            app.focus_ctrl(*c);
            if was {
                app.tabs.select.drop.close();
            } else {
                let cur = current(app, *c);
                app.tabs.select.drop.open_at(cur);
            }
            true
        }
        Hit::Popup(c, j) => {
            app.tabs.select.drop.close();
            pick(app, *c, *j);
            true
        }
        _ => false,
    }
}

/// Hints for a dropdown, open or closed.
pub fn drop_hints(open: bool) -> Vec<Hint> {
    if open {
        vec![
            h("↑↓", "Move", KeyCode::Down),
            h("type", "Filter", KeyCode::Null),
            h("Enter", "Pick", KeyCode::Enter),
            h("Esc", "Close", KeyCode::Esc),
        ]
    } else {
        vec![
            h("Enter", "Open", KeyCode::Enter),
            h("Space", "Open", KeyCode::Char(' ')),
            h("type", "Jump", KeyCode::Null),
        ]
    }
}

pub fn hints(app: &App) -> Vec<Hint> {
    drop_hints(app.tabs.select.drop.open)
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let x = area.x + 1;
    for (c, (name, key, list)) in FIELDS.iter().enumerate() {
        let y = area.y + c as u16 * 2;
        let focused = is_focused(app, c);
        label(buf, x, y, name, focused, &t, 10);
        let cur = current(app, c).map(|i| &list[i]);
        let r = closed(buf, x + 11, y, 24, cur, "choose…", focused, true, &t);
        app.hits.push((Rect::new(x + 11, y, 24, 1), Hit::Ctrl(c)));
        app.key_rects.push((key, r));
        if focused {
            app.tabs.select.anchor = Rect::new(x + 11, y, 24, 1);
        }
        let ids: Vec<&str> = list.iter().map(|c| c.id).collect();
        put(
            buf,
            x + 37,
            y,
            &format!("one of: {}", ids.join(" · ")),
            muted(&t),
            area.right().saturating_sub(x + 37) as usize,
        );
    }
}

pub fn popup(app: &mut App, buf: &mut Buffer, _area: Rect) {
    if app.tabs.select.drop.open {
        let c = app.ctrl;
        let a = app.tabs.select.anchor;
        open_list(app, buf, a, FIELDS[c].2, c, "type to filter");
    }
}
