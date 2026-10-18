//! Tab 3 — Date: Start and Due take natural dates (protocol §6) with a live
//! preview, and a month calendar beside the fields (for the focused one). Enter commits the
//! typed text, or the calendar day when nothing was typed.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::Date;
use demo_core::date::{parse_natural, preview};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::Value;

use super::{Draft, err_style, field, h, hc, is_focused, label, muted};
use crate::app::{App, Focus, Hint, Hit, Level};
use crate::widgets::draw::{fill, frame, put};

pub const KEYS: &[(&str, &str)] = &[
    ("type", "fri +3d eom 23 oct"),
    ("↑↓", "±1 week"),
    ("^←/^→", "±1 day"),
    ("PgUp PgDn", "±1 month"),
    ("Enter", "save"),
    ("^T  [t]", "today"),
    ("Del", "clear"),
    ("Esc", "revert"),
];

const FULL_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const FIELDS: [(&str, &str); 2] = [("Start", "date.start"), ("Due", "date.due")];

pub struct State {
    pub fields: [Draft; 2],
    /// The calendar cursor.
    pub cal: Date,
}

impl State {
    pub fn new(today: Date) -> State {
        State {
            fields: Default::default(),
            cal: today,
        }
    }
    pub fn drop(&mut self, key: &str) {
        if let Some(i) = FIELDS.iter().position(|f| f.1 == key) {
            self.fields[i].dirty = false;
        }
    }
}

pub fn sync(app: &mut App) {
    for (i, (_, key)) in FIELDS.iter().enumerate() {
        let s = app
            .shown(key)
            .as_str()
            .map(String::from)
            .unwrap_or_default();
        let ver = app.v.version(key);
        app.tabs.date.fields[i].sync(&demo_core::safe_text(&s), ver);
    }
}

fn shown_date(app: &App, i: usize) -> Option<Date> {
    app.shown(FIELDS[i].1).as_str().and_then(Date::parse_iso)
}

pub fn enter_ctrl(app: &mut App) {
    let c = app.ctrl.min(1);
    app.tabs.date.cal = shown_date(app, c).unwrap_or(app.today);
}

/// The cross-key rule against the other field's shown value.
fn order_error(app: &App, i: usize, d: Option<Date>) -> Option<String> {
    let other = shown_date(app, 1 - i);
    match (i, d, other) {
        (0, Some(s), Some(due)) if due < s => Some("start is after due".into()),
        (1, Some(due), Some(s)) if due < s => Some("due is before start".into()),
        _ => None,
    }
}

fn commit_date(app: &mut App, i: usize, d: Option<Date>) {
    if let Some(e) = order_error(app, i, d) {
        app.set_status(format!("✗ {}: {e}", FIELDS[i].0), Level::Err);
        return;
    }
    let base = app.tabs.date.fields[i].base;
    let v = d.map_or(Value::Null, |d| Value::from(d.iso()));
    if app.commit_from(FIELDS[i].1, v, Some(base)) {
        app.tabs.date.fields[i].dirty = false;
        if let Some(d) = d {
            app.tabs.date.cal = d;
        }
    }
}

fn commit(app: &mut App, i: usize) {
    let d = &app.tabs.date.fields[i];
    if d.dirty {
        match parse_natural(&d.input.text(), app.today) {
            Ok(v) => commit_date(app, i, v),
            Err(e) => app.set_status(format!("✗ {}: {e}", FIELDS[i].0), Level::Err),
        }
    } else {
        let day = app.tabs.date.cal;
        commit_date(app, i, Some(day));
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let i = app.ctrl.min(1);
    let s = &mut app.tabs.date;
    match k.code {
        KeyCode::Up => s.cal = s.cal.add_days(-7),
        KeyCode::Down => s.cal = s.cal.add_days(7),
        KeyCode::Left if ctrl => s.cal = s.cal.add_days(-1),
        KeyCode::Right if ctrl => s.cal = s.cal.add_days(1),
        KeyCode::PageUp => s.cal = s.cal.add_months(-1),
        KeyCode::PageDown => s.cal = s.cal.add_months(1),
        KeyCode::Char('t') if ctrl => {
            let today = app.today;
            commit_date(app, i, Some(today));
        }
        KeyCode::Enter => commit(app, i),
        KeyCode::Delete if !s.fields[i].dirty => commit_date(app, i, None),
        KeyCode::Esc if s.fields[i].dirty => {
            s.fields[i].dirty = false;
            sync(app);
            enter_ctrl(app);
        }
        KeyCode::Esc => return false,
        KeyCode::Char(_) if !ctrl && !s.fields[i].dirty => {
            // typing replaces the shown date
            s.fields[i].input.clear();
            s.fields[i].handle(&k);
        }
        _ => return s.fields[i].handle(&k),
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    match hit {
        Hit::Day(d) => {
            let i = if app.focus == Focus::Body {
                app.ctrl.min(1)
            } else {
                0
            };
            app.focus_ctrl(i);
            app.tabs.date.cal = *d;
            app.tabs.date.fields[i].dirty = false;
            commit_date(app, i, Some(*d));
            true
        }
        Hit::Btn(i, 0) => {
            app.focus_ctrl(*i);
            let today = app.today;
            commit_date(app, *i, Some(today));
            true
        }
        _ => false,
    }
}

pub fn hints(_app: &App) -> Vec<Hint> {
    vec![
        h("Enter", "Save", KeyCode::Enter),
        h("↑↓", "Week", KeyCode::Down),
        h("PgUp/PgDn", "Month", KeyCode::PageDown),
        hc("^T", "Today", 't'),
        h("Del", "Clear", KeyCode::Delete),
    ]
}

#[allow(clippy::needless_range_loop)]
pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let x = area.x + 1;
    let fx = x + 11;
    let fw = 24;
    // the calendar sits beside the fields, for the focused one (Start from the sidebar)
    let cal_for = if app.focus == Focus::Body {
        app.ctrl.min(1)
    } else {
        0
    };
    let left_w = (fw + 16) as usize;
    for i in 0..2 {
        let y = area.y + i as u16 * 4;
        let focused = is_focused(app, i);
        label(buf, x, y, FIELDS[i].0, focused, &t, 10);
        let d = &app.tabs.date.fields[i];
        let text = d.input.text();
        let (note, err) = if d.dirty {
            match preview(&text, app.today) {
                Ok(p) => (
                    p,
                    order_error(app, i, parse_natural(&text, app.today).ok().flatten()),
                ),
                Err(e) => (String::new(), Some(e)),
            }
        } else {
            match shown_date(app, i) {
                Some(d) => (format!("→ {}", d.long()), None),
                None => ("→ none".to_string(), None),
            }
        };
        let r = field(
            buf,
            fx,
            y,
            fw,
            &mut app.tabs.date.fields[i].input,
            focused,
            err.is_some(),
            &t,
        );
        app.hits.push((r, Hit::Ctrl(i)));
        app.key_rects.push((FIELDS[i].1, r));
        put(
            buf,
            fx + fw + 1,
            y,
            "[t]",
            Style::new().fg(t.accent).bg(t.bg),
            3,
        );
        app.hits
            .push((Rect::new(fx + fw + 1, y, 3, 1), Hit::Btn(i, 0)));
        put(
            buf,
            fx + 1,
            y + 1,
            &note,
            Style::new().fg(t.info).bg(t.bg),
            left_w - 12,
        );
        if let Some(e) = err {
            put(
                buf,
                fx + 1,
                y + 2,
                &format!("✗ {e}"),
                err_style(&t),
                left_w - 12,
            );
        }
    }
    let y = area.y + 8;
    if y + 1 < area.bottom() {
        put(
            buf,
            x,
            y,
            "Due before Start is refused (red)",
            muted(&t),
            left_w,
        );
        put(
            buf,
            x,
            y + 1,
            "and not sent. Empty = none.",
            muted(&t),
            left_w,
        );
    }
    let cx = fx + fw + 6;
    if cx + 30 <= area.right() {
        calendar(
            app,
            buf,
            Rect::new(cx, area.y, 30, 9.min(area.height)),
            cal_for,
        );
    }
}

fn calendar(app: &mut App, buf: &mut Buffer, r: Rect, field: usize) {
    let t = app.t.clone();
    let focused = is_focused(app, field);
    // from the sidebar the calendar shows Start's month
    let cal = if app.focus == Focus::Body {
        app.tabs.date.cal
    } else {
        shown_date(app, field).unwrap_or(app.today)
    };
    let (y, m, _) = cal.ymd();
    let title = format!(
        "{} {}{}",
        FULL_MONTHS[m as usize - 1],
        y,
        if field == 0 { " · Start" } else { " · Due" }
    );
    let inner = frame(buf, r, &title, &t, focused);
    let panel = t.panel();
    put(
        buf,
        inner.x,
        inner.y,
        " Mo  Tu  We  Th  Fr  Sa  Su",
        panel.fg(t.muted),
        inner.width as usize,
    );
    let (start, due) = (shown_date(app, 0), shown_date(app, 1));
    let mine = if field == 0 { start } else { due };
    let other = if field == 0 { due } else { start };
    for (n, d) in cal.calendar_grid().iter().enumerate() {
        let (row, col) = ((n / 7) as u16, (n % 7) as u16);
        let cx = inner.x + col * 4;
        let cy = inner.y + 1 + row;
        if cy >= inner.bottom() {
            break;
        }
        let mut st = panel;
        if d.month() != m {
            st = st.fg(t.muted).add_modifier(Modifier::DIM);
        } else if d.is_weekend() {
            st = st.fg(t.muted);
        }
        if let (Some(s), Some(e)) = (start, due) {
            if *d >= s && *d <= e {
                st = st.bg(t.sel_bg);
            }
        }
        if Some(*d) == other {
            st = st.fg(t.accent2).add_modifier(Modifier::BOLD);
        }
        if Some(*d) == mine {
            st = st.fg(t.accent).add_modifier(Modifier::BOLD);
        }
        if *d == app.today {
            st = st
                .fg(t.ok)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        }
        if *d == cal {
            st = if focused {
                st.bg(t.cur_bg).fg(t.fg)
            } else {
                st.add_modifier(Modifier::REVERSED)
            };
        }
        fill(buf, Rect::new(cx, cy, 4, 1), panel);
        put(buf, cx, cy, &format!("{:>3}", d.day()), st, 3);
        app.hits.push((Rect::new(cx, cy, 4, 1), Hit::Day(*d)));
    }
}
