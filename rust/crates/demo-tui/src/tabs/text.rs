//! Tab 1 — Text: Name, Email, Code (uppercase as typed) and a masked,
//! local-only Password with a show toggle. Enter commits, Esc reverts.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::keys::{valid_code, valid_email};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use serde_json::Value;

use super::{Draft, changed_by, err_style, field, h, hc, is_focused, label, muted};
use crate::app::{App, Hint, Hit, Level};
use crate::widgets::draw::put;
use crate::widgets::input::TextInput;

pub const KEYS: &[(&str, &str)] = &[
    ("←→ Home End", "move"),
    ("^←/^→", "word jump"),
    ("Bksp Del", "delete"),
    ("^U", "clear"),
    ("Enter", "save"),
    ("Esc", "revert"),
    ("Tab ↑↓", "next field"),
    ("^O", "show password"),
];

const FIELDS: [(&str, &str); 3] = [
    ("Name", "text.name"),
    ("Email", "text.email"),
    ("Code", "text.code"),
];

pub struct State {
    pub fields: [Draft; 3],
    pub password: TextInput,
    pub show: bool,
}

impl Default for State {
    fn default() -> Self {
        let mut password = TextInput::new("");
        password.mask = true;
        State {
            fields: Default::default(),
            password,
            show: false,
        }
    }
}

impl State {
    pub fn drop(&mut self, key: &str) {
        if let Some(i) = FIELDS.iter().position(|f| f.1 == key) {
            self.fields[i].dirty = false;
        }
    }
}

/// Live validation of a field's text (None = fine).
pub fn check(i: usize, s: &str) -> Option<String> {
    let t = s.trim();
    match i {
        0 if t.is_empty() => Some("a name is required".into()),
        0 if t.chars().count() > 80 => Some("at most 80 characters".into()),
        1 if !t.is_empty() && (!valid_email(&t.to_lowercase()) || t.chars().count() > 120) => {
            Some("must look like name@example.com".into())
        }
        2 if !valid_code(&t.to_uppercase()) => Some("must look like ABC-123".into()),
        _ => None,
    }
}

pub fn sync(app: &mut App) {
    for (i, (_, key)) in FIELDS.iter().enumerate() {
        let shown = app.shown_str(key);
        let ver = app.v.version(key);
        app.tabs.text.fields[i].sync(&demo_core::safe_text(&shown), ver);
    }
}

pub fn paste(app: &mut App, s: &str) {
    match app.ctrl {
        3 => app.tabs.text.password.insert_str(s),
        i => {
            let d = &mut app.tabs.text.fields[i];
            d.input.insert_str(&if i == 2 {
                s.to_uppercase()
            } else {
                s.to_string()
            });
            d.dirty = true;
        }
    }
}

fn commit(app: &mut App, i: usize) {
    let d = &app.tabs.text.fields[i];
    if !d.dirty {
        app.info("No changes to save");
        return;
    }
    let text = d.input.text();
    if let Some(e) = check(i, &text) {
        app.set_status(format!("✗ {}: {e}", FIELDS[i].0), Level::Err);
        return;
    }
    let base = d.base;
    if app.commit_from(FIELDS[i].1, Value::from(text), Some(base)) {
        app.tabs.text.fields[i].dirty = false;
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let c = app.ctrl;
    match k.code {
        KeyCode::Char('o') if ctrl => {
            let s = &mut app.tabs.text;
            s.show = !s.show;
            s.password.mask = !s.show;
        }
        KeyCode::Down if c < 3 => {
            app.next_ctrl(false);
        }
        KeyCode::Up if c > 0 => app.next_ctrl(true),
        KeyCode::Up | KeyCode::Down => {}
        KeyCode::Enter if c == 3 => app.info("The password is local only: it is never sent"),
        KeyCode::Enter => commit(app, c),
        KeyCode::Esc if c < 3 && app.tabs.text.fields[c].dirty => {
            app.tabs.text.fields[c].dirty = false;
            sync(app);
            app.info(format!("{} reverted to the synced value", FIELDS[c].0));
        }
        KeyCode::Esc => return false,
        _ if c == 3 => return app.tabs.text.password.handle(&k),
        KeyCode::Char(ch) if c == 2 && !ctrl => {
            // Code is shown uppercase as typed
            let up = KeyEvent::new(KeyCode::Char(ch.to_ascii_uppercase()), k.modifiers);
            return app.tabs.text.fields[2].handle(&up);
        }
        _ => return app.tabs.text.fields[c].handle(&k),
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    if let Hit::Btn(3, _) = hit {
        let s = &mut app.tabs.text;
        s.show = !s.show;
        s.password.mask = !s.show;
        return true;
    }
    false
}

pub fn hints(app: &App) -> Vec<Hint> {
    if app.ctrl == 3 {
        return vec![
            hc("^O", if app.tabs.text.show { "Hide" } else { "Show" }, 'o'),
            hc("^U", "Clear", 'u'),
        ];
    }
    vec![
        h("Enter", "Save", KeyCode::Enter),
        h("Esc", "Revert", KeyCode::Esc),
        hc("^U", "Clear", 'u'),
        h("↑↓", "Field", KeyCode::Down),
    ]
}

#[allow(clippy::needless_range_loop)]
pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let fw = 42.min(area.width.saturating_sub(12));
    let x = area.x + 1;
    for i in 0..4 {
        let y = area.y + i as u16 * 2;
        if y >= area.bottom() {
            break;
        }
        let focused = is_focused(app, i);
        let name = if i == 3 { "Password" } else { FIELDS[i].0 };
        label(buf, x, y, name, focused, &t, 10);
        let fx = x + 11;
        if i == 3 {
            let r = field(
                buf,
                fx,
                y,
                fw,
                &mut app.tabs.text.password,
                focused,
                false,
                &t,
            );
            app.hits.push((Rect::new(fx, y, fw, 1), Hit::Ctrl(3)));
            let _ = r;
            let btn = if app.tabs.text.show {
                "[◉ hide]"
            } else {
                "[◉ show]"
            };
            put(
                buf,
                fx + fw + 1,
                y,
                btn,
                Style::new().fg(t.accent).bg(t.bg),
                8,
            );
            app.hits
                .push((Rect::new(fx + fw + 1, y, 8, 1), Hit::Btn(3, 0)));
            put(
                buf,
                fx + fw + 10,
                y,
                "local only · never sent",
                muted(&t),
                area.right().saturating_sub(fx + fw + 10) as usize,
            );
            continue;
        }
        let key = FIELDS[i].1;
        let ver = app.v.version(key);
        let d = &app.tabs.text.fields[i];
        let err = if d.dirty {
            check(i, &d.input.text())
        } else {
            None
        };
        let meanwhile = d.changed_meanwhile(ver);
        let unsaved = d.dirty;
        let r = field(
            buf,
            fx,
            y,
            fw,
            &mut app.tabs.text.fields[i].input,
            focused,
            err.is_some(),
            &t,
        );
        app.hits.push((Rect::new(fx, y, fw, 1), Hit::Ctrl(i)));
        app.key_rects.push((key, r));
        let mx = fx + fw + 1;
        let room = area.right().saturating_sub(mx) as usize;
        if let Some(e) = err {
            put(buf, mx, y, &format!("✗ {e}"), err_style(&t), room);
        } else if meanwhile {
            let who = changed_by(app, key);
            put(
                buf,
                mx,
                y,
                &format!("⚠ changed by {who} · Esc takes theirs"),
                Style::new().fg(t.warn).bg(t.bg),
                room,
            );
        } else if unsaved {
            put(
                buf,
                mx,
                y,
                "● unsaved · Enter saves",
                Style::new().fg(t.warn).bg(t.bg),
                room,
            );
        }
    }
}
