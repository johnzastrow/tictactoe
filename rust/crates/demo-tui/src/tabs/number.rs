//! Tab 2 — Number: Hours (decimal spinner), Quantity (integer spinner) and
//! Percent (a slider that commits on every change; drags debounced 150 ms).

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::keys::{num_value, round2};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::Value;

use super::{Draft, err_style, field, h, is_focused, label, muted};
use crate::app::{App, Drag, Hint, Hit, Level};
use crate::widgets::draw::put;

pub const KEYS: &[(&str, &str)] = &[
    ("↑↓ + -", "±1"),
    ("Shift+↑↓", "±0.25"),
    ("digits .", "type"),
    ("Enter", "save"),
    ("Esc", "revert"),
    ("←→", "slider ±1"),
    ("PgUp PgDn", "slider ±10"),
    ("Home End", "0 / 100"),
];

const SPIN: [(&str, &str); 2] = [("Hours", "number.hours"), ("Quantity", "number.qty")];

#[derive(Default)]
pub struct State {
    pub spin: [Draft; 2],
    /// The slider value during a mouse drag.
    pub drag_val: Option<i64>,
}

impl State {
    pub fn drop(&mut self, key: &str) {
        if let Some(i) = SPIN.iter().position(|s| s.1 == key) {
            self.spin[i].dirty = false;
        }
    }
}

fn fmt(v: &Value) -> String {
    v.as_f64()
        .map(|f| num_value(round2(f)).to_string())
        .unwrap_or_default()
}

pub fn sync(app: &mut App) {
    for (i, (_, key)) in SPIN.iter().enumerate() {
        let s = fmt(&app.shown(key));
        let ver = app.v.version(key);
        app.tabs.number.spin[i].sync(&s, ver);
    }
}

/// Parse and range-check a spinner's text.
pub fn check(i: usize, s: &str) -> Result<f64, String> {
    let n: f64 = s.trim().parse().map_err(|_| "not a number".to_string())?;
    if !n.is_finite() {
        return Err("not a number".into());
    }
    if i == 0 {
        if !(0.0..=1000.0).contains(&n) {
            return Err("0 to 1000".into());
        }
        Ok(round2(n))
    } else {
        if n.fract() != 0.0 {
            return Err("a whole number".into());
        }
        if !(0.0..=99.0).contains(&n) {
            return Err("0 to 99".into());
        }
        Ok(n)
    }
}

fn step(app: &mut App, i: usize, by: f64) {
    let d = &mut app.tabs.number.spin[i];
    let cur: f64 = d.input.text().trim().parse().unwrap_or(0.0);
    let (lo, hi) = if i == 0 { (0.0, 1000.0) } else { (0.0, 99.0) };
    let n = round2((cur + by).clamp(lo, hi));
    d.input.set(&num_value(n).to_string());
    d.dirty = true;
}

fn commit(app: &mut App, i: usize) {
    let d = &app.tabs.number.spin[i];
    if !d.dirty {
        app.info("No changes to save");
        return;
    }
    match check(i, &d.input.text()) {
        Ok(n) => {
            let base = d.base;
            if app.commit_from(SPIN[i].1, num_value(n), Some(base)) {
                app.tabs.number.spin[i].dirty = false;
            }
        }
        Err(e) => app.set_status(format!("✗ {}: {e}", SPIN[i].0), Level::Err),
    }
}

fn percent(app: &App) -> i64 {
    app.tabs
        .number
        .drag_val
        .unwrap_or_else(|| app.shown("number.percent").as_i64().unwrap_or(0))
}

fn set_percent(app: &mut App, v: i64) {
    let v = v.clamp(0, 100);
    if v != percent(app) {
        app.commit("number.percent", Value::from(v));
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let c = app.ctrl;
    if c == 2 {
        let p = percent(app);
        match k.code {
            KeyCode::Left | KeyCode::Down | KeyCode::Char('-') => set_percent(app, p - 1),
            KeyCode::Right | KeyCode::Up | KeyCode::Char('+') => set_percent(app, p + 1),
            KeyCode::PageDown => set_percent(app, p - 10),
            KeyCode::PageUp => set_percent(app, p + 10),
            KeyCode::Home => set_percent(app, 0),
            KeyCode::End => set_percent(app, 100),
            _ => return false,
        }
        return true;
    }
    let small = if c == 0 && shift { 0.25 } else { 1.0 };
    match k.code {
        KeyCode::Up => step(app, c, small),
        KeyCode::Down => step(app, c, -small),
        KeyCode::Char('+') => step(app, c, 1.0),
        KeyCode::Char('-') => step(app, c, -1.0),
        KeyCode::Enter => commit(app, c),
        KeyCode::Esc if app.tabs.number.spin[c].dirty => {
            app.tabs.number.spin[c].dirty = false;
            sync(app);
        }
        KeyCode::Esc => return false,
        KeyCode::Char(ch) if !ctrl => {
            let ok = ch.is_ascii_digit() || (c == 0 && ch == '.');
            if ok {
                app.tabs.number.spin[c].handle(&k);
            }
        }
        _ => return app.tabs.number.spin[c].handle(&k),
    }
    true
}

pub fn click(app: &mut App, hit: &Hit, x: u16) -> bool {
    match hit {
        Hit::Btn(c @ (0 | 1), d) => {
            app.focus_ctrl(*c);
            step(app, *c, *d as f64);
            commit(app, *c);
            true
        }
        Hit::Track(2, r) => {
            app.focus_ctrl(2);
            let v = track_value(*r, x);
            app.tabs.number.drag_val = Some(v);
            app.slider_pending = Some((v, Instant::now()));
            app.drag = Some(Drag::Slider { track: *r });
            true
        }
        _ => false,
    }
}

fn track_value(r: Rect, x: u16) -> i64 {
    let w = r.width.max(2) as i64 - 1;
    ((x.saturating_sub(r.x) as i64).min(w) * 100 + w / 2) / w
}

pub fn drag(app: &mut App, x: u16, end: bool) {
    let Some(Drag::Slider { track }) = app.drag else {
        return;
    };
    let v = track_value(track, x);
    app.tabs.number.drag_val = Some(v);
    if end {
        app.slider_pending = None;
        app.tabs.number.drag_val = None;
        set_percent(app, v);
    } else {
        // committed by the tick after 150 ms without movement
        app.slider_pending = Some((v, Instant::now()));
    }
}

pub fn hints(app: &App) -> Vec<Hint> {
    if app.ctrl == 2 {
        return vec![
            h("←→", "±1", KeyCode::Right),
            h("PgUp/PgDn", "±10", KeyCode::PageUp),
            h("Home/End", "0/100", KeyCode::Home),
        ];
    }
    vec![
        h("↑↓", "±1", KeyCode::Up),
        crate::app::hint("⇧↑↓", "±0.25", KeyCode::Up, KeyModifiers::SHIFT),
        h("Enter", "Save", KeyCode::Enter),
        h("Esc", "Revert", KeyCode::Esc),
    ]
}

#[allow(clippy::needless_range_loop)]
pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let x = area.x + 1;
    let fx = x + 11;
    for i in 0..2 {
        let y = area.y + i as u16 * 2;
        let focused = is_focused(app, i);
        label(buf, x, y, SPIN[i].0, focused, &t, 10);
        let btn = Style::new()
            .fg(t.accent)
            .bg(t.panel)
            .add_modifier(Modifier::BOLD);
        put(buf, fx, y, "[−]", btn, 3);
        app.hits.push((Rect::new(fx, y, 3, 1), Hit::Btn(i, -1)));
        let d = &app.tabs.number.spin[i];
        let err = if d.dirty {
            check(i, &d.input.text()).err()
        } else {
            None
        };
        let dirty = d.dirty;
        let r = field(
            buf,
            fx + 4,
            y,
            14,
            &mut app.tabs.number.spin[i].input,
            focused,
            err.is_some(),
            &t,
        );
        app.hits.push((r, Hit::Ctrl(i)));
        app.key_rects.push((SPIN[i].1, r));
        put(buf, fx + 19, y, "[+]", btn, 3);
        app.hits.push((Rect::new(fx + 19, y, 3, 1), Hit::Btn(i, 1)));
        let note = match (&err, dirty) {
            (Some(e), _) => (format!("✗ {e}"), err_style(&t)),
            (None, true) => (
                "● unsaved · Enter saves".to_string(),
                Style::new().fg(t.warn).bg(t.bg),
            ),
            _ => (
                if i == 0 {
                    "hours, 0–1000, 2 decimals"
                } else {
                    "whole number, 0–99"
                }
                .to_string(),
                muted(&t),
            ),
        };
        put(
            buf,
            fx + 24,
            y,
            &note.0,
            note.1,
            area.right().saturating_sub(fx + 24) as usize,
        );
    }
    // the slider
    let y = area.y + 4;
    let focused = is_focused(app, 2);
    label(buf, x, y, "Percent", focused, &t, 10);
    let tw = 41.min(area.width.saturating_sub(24));
    let track = Rect::new(fx, y, tw, 1);
    let p = percent(app);
    let thumb = ((p * (tw as i64 - 1) + 50) / 100) as u16;
    let on = Style::new()
        .fg(if focused { t.accent } else { t.info })
        .bg(t.bg);
    let off = Style::new().fg(t.border).bg(t.bg);
    for i in 0..tw {
        let (s, st) = if i < thumb { ("━", on) } else { ("─", off) };
        put(buf, fx + i, y, s, st, 1);
    }
    put(buf, fx + thumb, y, "●", on.add_modifier(Modifier::BOLD), 1);
    put(
        buf,
        fx + tw + 1,
        y,
        &format!("{p:>3}%"),
        Style::new().fg(t.fg).bg(t.bg).add_modifier(Modifier::BOLD),
        4,
    );
    put(buf, fx, y + 1, "0", muted(&t), 1);
    put(buf, fx + tw - 3, y + 1, "100", muted(&t), 3);
    app.hits.push((Rect::new(x, y, 10, 1), Hit::Ctrl(2)));
    app.hits.push((track, Hit::Track(2, track)));
    app.key_rects
        .push(("number.percent", Rect::new(fx + tw + 1, y, 4, 1)));
}
