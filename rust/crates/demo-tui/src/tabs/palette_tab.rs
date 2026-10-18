//! Tab 12 — Palette: the command palette shown inline and always open, with
//! its search box focused on entering the tab. Running a command here works
//! exactly as from ^K and records it in `palette.last`.

use crossterm::event::{KeyCode, KeyEvent};
use demo_core::safe_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{h, is_focused, muted};
use crate::app::{App, Hint, Hit};
use crate::palette::{Out, PaletteState};
use crate::widgets::draw::{frame, put};

pub const KEYS: &[(&str, &str)] = &[
    ("type", "fuzzy filter"),
    ("↑↓ PgUp PgDn", "move"),
    ("Enter", "run command"),
    ("Esc", "clear / sidebar"),
    ("^K", "same palette, anywhere"),
];

#[derive(Default)]
pub struct State {
    pub p: PaletteState,
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    if k.code == KeyCode::Esc {
        if app.tabs.palette.p.input.is_empty() {
            return false;
        }
        app.tabs.palette.p.input.clear();
        app.tabs.palette.p.sel = 0;
        return true;
    }
    match app.tabs.palette.p.key(&k) {
        Out::Run(c) => {
            app.run_cmd(c, true);
            true
        }
        Out::Used => true,
        Out::Close | Out::Ignored => false,
    }
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    if let Hit::PaletteRow(i) = hit {
        app.focus_ctrl(0);
        app.tabs.palette.p.sel = *i;
        if let Some(c) = app.tabs.palette.p.command_at(*i) {
            app.run_cmd(c, true);
        }
        return true;
    }
    false
}

pub fn hints(_app: &App) -> Vec<Hint> {
    vec![
        h("type", "Filter", KeyCode::Null),
        h("↑↓", "Move", KeyCode::Down),
        h("Enter", "Run", KeyCode::Enter),
    ]
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let focused = is_focused(app, 0);
    let x = area.x + 1;
    let last = app.v.str("palette.last").to_string();
    let by = app
        .v
        .entry("palette.last")
        .map(|e| e.by.clone())
        .unwrap_or_default();
    put(buf, x, area.y, "Last command:", muted(&t), 14);
    let (text, st) = if last.is_empty() {
        (
            "— none yet —".to_string(),
            muted(&t).add_modifier(Modifier::ITALIC),
        )
    } else {
        (
            safe_text(&last),
            Style::new()
                .fg(t.accent)
                .bg(t.bg)
                .add_modifier(Modifier::BOLD),
        )
    };
    let w = put(buf, x + 14, area.y, &text, st, 40);
    if !by.is_empty() {
        put(
            buf,
            x + 15 + w,
            area.y,
            &format!("· by {}", safe_text(&by)),
            muted(&t),
            30,
        );
    }
    app.key_rects
        .push(("palette.last", Rect::new(x + 14, area.y, w.max(1), 1)));
    let r = Rect::new(
        x,
        area.y + 2,
        64.min(area.width - 2),
        area.height.saturating_sub(2).min(16),
    );
    let inner = frame(buf, r, "Command palette", &t, focused);
    app.hits.push((r, Hit::Ctrl(0)));
    let mut hits = vec![];
    app.tabs
        .palette
        .p
        .render(buf, inner, &t, focused, &mut hits);
    app.hits.extend(hits);
}
