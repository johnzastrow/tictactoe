//! Tab 7 — Text area: `textarea.notes` in a word-wrapping editor with a
//! `Ln, Col · words · chars` footer. ^S commits; Esc reverts (asking first);
//! leaving with Tab keeps the draft, marked "● unsaved".

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::keys::MAX_NOTES;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use serde_json::Value;

use super::{changed_by, h, hc, is_focused, muted};
use crate::app::{App, Hint, Hit, Level};
use crate::dialogs::{Dialog, DialogAct};
use crate::widgets::draw::{frame, put, width};
use crate::widgets::textarea::TextArea;

pub const KEYS: &[(&str, &str)] = &[
    ("Enter", "new line"),
    ("arrows Home End", "move"),
    ("PgUp PgDn", "page"),
    ("^Home ^End", "start / end"),
    ("^S", "save"),
    ("Esc", "revert (asks)"),
    ("Tab", "leave, keep draft"),
];

#[derive(Default)]
pub struct State {
    pub area: TextArea,
    pub dirty: bool,
    pub base: u64,
    inner: Rect,
}

impl State {
    pub fn leave(&mut self) {}

    pub fn paste(&mut self, s: &str) {
        self.area.insert_str(s);
        self.dirty = true;
    }
}

pub fn sync(app: &mut App) {
    let shown = demo_core::safe_multiline(app.shown("textarea.notes").as_str().unwrap_or(""));
    let s = &mut app.tabs.textarea;
    if !s.dirty {
        if s.area.text() != shown {
            s.area.set(&shown);
        }
        s.base = app.v.version("textarea.notes");
    }
}

/// Esc confirmed: take the synced notes.
pub fn discard(app: &mut App) {
    app.tabs.textarea.dirty = false;
    sync(app);
    app.info("Notes reverted to the synced value");
}

fn save(app: &mut App) {
    let s = &app.tabs.textarea;
    if !s.dirty {
        app.info("No changes to save");
        return;
    }
    let text = s.area.text();
    if text.chars().count() > MAX_NOTES {
        app.set_status(
            format!("✗ notes are at most {MAX_NOTES} characters"),
            Level::Err,
        );
        return;
    }
    let base = s.base;
    if app.commit_from("textarea.notes", Value::from(text), Some(base)) {
        app.tabs.textarea.dirty = false;
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Char('s') if ctrl => save(app),
        KeyCode::Esc if app.tabs.textarea.dirty => {
            app.dialogs.push(Dialog::confirm(
                "Revert notes",
                "Discard your unsaved changes to the notes?",
                "Discard",
                DialogAct::DiscardNotes,
            ));
        }
        KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => return false,
        _ => {
            let s = &mut app.tabs.textarea;
            let before = s.area.text();
            let used = s.area.handle(&k);
            if used && before != s.area.text() {
                s.dirty = true;
            }
            return used;
        }
    }
    true
}

pub fn click(app: &mut App, hit: &Hit, x: u16, y: u16) -> bool {
    if let Hit::Ctrl(0) = hit {
        app.focus_ctrl(0);
        let r = app.tabs.textarea.inner;
        if r.width > 0 && y >= r.y && x >= r.x {
            app.tabs.textarea.area.click(x - r.x, y - r.y);
        }
        return true;
    }
    false
}

pub fn hints(_app: &App) -> Vec<Hint> {
    vec![
        hc("^S", "Save", 's'),
        h("Esc", "Revert", KeyCode::Esc),
        h("PgUp/PgDn", "Page", KeyCode::PageDown),
    ]
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let focused = is_focused(app, 0);
    let w = area.width.saturating_sub(2).min(76);
    let hgt = area.height.saturating_sub(2).clamp(4, 14);
    let r = Rect::new(area.x + 1, area.y, w, hgt);
    let title = if app.tabs.textarea.dirty {
        "Notes ● unsaved"
    } else {
        "Notes"
    };
    let inner = frame(buf, r, title, &t, focused);
    let s = &mut app.tabs.textarea;
    s.inner = inner;
    s.area
        .render(buf, inner, Style::new().bg(t.panel).fg(t.fg), focused);
    app.hits.push((r, Hit::Ctrl(0)));
    app.key_rects.push(("textarea.notes", inner));
    // footer
    let s = &app.tabs.textarea;
    let (ln, col) = s.area.position();
    let n = s.area.char_count();
    let foot = format!(
        "Ln {ln}, Col {col} · {} words · {n}/{MAX_NOTES}",
        s.area.word_count()
    );
    let fy = r.bottom();
    let over = n > MAX_NOTES;
    put(
        buf,
        r.x + 1,
        fy,
        &foot,
        if over {
            super::err_style(&t)
        } else {
            muted(&t)
        },
        w as usize,
    );
    let ver = app.v.version("textarea.notes");
    let note = if s.dirty && ver != s.base {
        Some((
            format!(
                "⚠ changed by {} · ^S overwrites (conflict), Esc takes theirs",
                changed_by(app, "textarea.notes")
            ),
            t.warn,
        ))
    } else if s.dirty {
        Some(("● unsaved · ^S saves".to_string(), t.warn))
    } else {
        None
    };
    if let Some((text, c)) = note {
        let x = r.x + 2 + width(&foot) as u16 + 2;
        put(
            buf,
            x,
            fy,
            &text,
            Style::new().fg(c).bg(t.bg),
            area.right().saturating_sub(x) as usize,
        );
    }
}
