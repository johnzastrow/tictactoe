//! Tab 6 — Tags: chips then an input. Typing filters the suggestions
//! (excluding tags present); Enter adds the selected suggestion or creates
//! the typed tag; Backspace in the empty input removes the last chip.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::dataset::suggest_tags;
use demo_core::keys::valid_tag;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::json;

use super::{err_style, field, h, is_focused, label, muted};
use crate::app::{App, Hint, Hit, Level};
use crate::widgets::draw::{put, width};
use crate::widgets::input::TextInput;
use crate::widgets::list::{self, Row};

pub const KEYS: &[(&str, &str)] = &[
    ("type", "filter / new tag"),
    ("↑↓", "pick a suggestion"),
    ("Enter", "add"),
    ("Bksp", "remove last chip"),
    ("←→", "select a chip"),
    ("Del", "remove selected chip"),
    ("click ×", "remove"),
    ("Esc", "clear input"),
];

#[derive(Default)]
pub struct State {
    pub input: TextInput,
    /// Selected suggestion row.
    pub sel: Option<usize>,
    /// Selected chip (←/→ in the empty input).
    pub chip: Option<usize>,
    anchor: Rect,
}

impl State {
    pub fn leave(&mut self) {
        self.sel = None;
        self.chip = None;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Suggest(&'static str, u32),
    Create(String),
}

fn present(app: &App) -> Vec<String> {
    app.shown_get::<Vec<String>>("tags.tags")
        .unwrap_or_default()
}

/// The suggestion list for the current input.
pub fn entries(app: &App) -> Vec<Entry> {
    let tags = present(app);
    let q = app.tabs.tags.input.text().trim().to_lowercase();
    let sugg = suggest_tags(&q, &tags);
    let mut v: Vec<Entry> = sugg.iter().map(|(t, n)| Entry::Suggest(t, *n)).collect();
    if !q.is_empty() && !tags.contains(&q) && !sugg.iter().any(|(t, _)| *t == q) {
        v.push(Entry::Create(q));
    }
    v
}

fn add(app: &mut App, tag: &str) {
    let tag = tag.trim().to_lowercase();
    if !valid_tag(&tag) {
        app.set_status(
            "✗ a tag is a–z, 0–9, _ or -, starting with a letter or digit, ≤ 24".into(),
            Level::Err,
        );
        return;
    }
    let mut tags = present(app);
    if tags.contains(&tag) {
        app.info(format!("{tag} is already there"));
        return;
    }
    if tags.len() >= 12 {
        app.set_status("✗ at most 12 tags".into(), Level::Err);
        return;
    }
    tags.push(tag);
    if app.commit("tags.tags", json!(tags)) {
        let s = &mut app.tabs.tags;
        s.input.clear();
        s.sel = None;
    }
}

fn remove(app: &mut App, i: usize) {
    let mut tags = present(app);
    if i < tags.len() {
        tags.remove(i);
        app.commit("tags.tags", json!(tags));
        let n = tags.len();
        let s = &mut app.tabs.tags;
        s.chip = s.chip.filter(|_| n > 0).map(|c| c.min(n - 1));
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let n_chips = present(app).len();
    let ents = entries(app);
    let s = &mut app.tabs.tags;
    let empty = s.input.is_empty();
    let n = ents.len();
    match k.code {
        KeyCode::Down if n > 0 => s.sel = Some(s.sel.map_or(0, |i| (i + 1) % n)),
        KeyCode::Up if n > 0 => s.sel = Some(s.sel.map_or(n - 1, |i| (i + n - 1) % n)),
        KeyCode::Enter => match s.sel.and_then(|i| ents.get(i)).cloned() {
            Some(Entry::Suggest(t, _)) => add(app, t),
            Some(Entry::Create(t)) => add(app, &t),
            None if !empty => {
                let t = s.input.text();
                add(app, &t);
            }
            None => app.info("Type a tag, or ↓ to pick a suggestion"),
        },
        KeyCode::Backspace if empty => {
            if n_chips > 0 {
                let i = s.chip.unwrap_or(n_chips - 1);
                remove(app, i);
            }
        }
        KeyCode::Left if empty && n_chips > 0 => {
            s.chip = Some(s.chip.map_or(n_chips - 1, |c| c.saturating_sub(1)))
        }
        KeyCode::Right if empty && s.chip.is_some() => {
            s.chip = s.chip.and_then(|c| (c + 1 < n_chips).then_some(c + 1))
        }
        KeyCode::Delete if empty && s.chip.is_some() => {
            if let Some(c) = s.chip {
                remove(app, c);
            }
        }
        KeyCode::Esc if !empty || s.chip.is_some() || s.sel.is_some() => {
            s.input.clear();
            s.chip = None;
            s.sel = None;
        }
        KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => return false,
        _ => {
            let before = s.input.text();
            let used = s.input.handle(&k);
            if s.input.text() != before {
                s.chip = None;
                s.sel = if s.input.is_empty() { None } else { Some(0) };
            }
            return used || !k.modifiers.contains(KeyModifiers::CONTROL);
        }
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    match hit {
        Hit::ChipX(i) => {
            app.focus_ctrl(0);
            remove(app, *i);
            true
        }
        Hit::Item(0, i) => {
            app.focus_ctrl(0);
            app.tabs.tags.chip = Some(*i);
            true
        }
        Hit::Popup(0, j) => {
            match entries(app).get(*j).cloned() {
                Some(Entry::Suggest(t, _)) => add(app, t),
                Some(Entry::Create(t)) => add(app, &t),
                None => {}
            }
            true
        }
        _ => false,
    }
}

pub fn hints(app: &App) -> Vec<Hint> {
    let mut v = vec![
        h("type", "Filter", KeyCode::Null),
        h("↑↓", "Suggestion", KeyCode::Down),
        h("Enter", "Add", KeyCode::Enter),
    ];
    if app.tabs.tags.input.is_empty() {
        v.push(h("Bksp", "Remove last", KeyCode::Backspace));
        v.push(h("←→", "Chip", KeyCode::Left));
        if app.tabs.tags.chip.is_some() {
            v.push(h("Del", "Remove chip", KeyCode::Delete));
        }
    }
    v
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let focused = is_focused(app, 0);
    let x = area.x + 1;
    label(buf, x, area.y, "Tags", focused, &t, 10);
    let tags = present(app);
    let mut cx = x + 11;
    let mut y = area.y;
    let chip_sel = if focused { app.tabs.tags.chip } else { None };
    for (i, tag) in tags.iter().enumerate() {
        let tag = demo_core::safe_text(tag);
        let w = width(&tag) as u16 + 4;
        if cx + w > area.right() {
            cx = x + 11;
            y += 1;
        }
        let st = if chip_sel == Some(i) {
            Style::new()
                .bg(t.accent2)
                .fg(t.bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new()
                .bg(t.panel)
                .fg(t.accent2)
                .add_modifier(Modifier::BOLD)
        };
        put(buf, cx, y, &format!("[{tag} "), st, w as usize);
        put(
            buf,
            cx + w - 2,
            y,
            "×]",
            st.fg(if chip_sel == Some(i) { t.bg } else { t.err }),
            2,
        );
        app.hits.push((Rect::new(cx, y, w - 2, 1), Hit::Item(0, i)));
        app.hits
            .push((Rect::new(cx + w - 2, y, 1, 1), Hit::ChipX(i)));
        cx += w + 1;
    }
    app.key_rects.push((
        "tags.tags",
        Rect::new(
            x + 11,
            area.y,
            cx.saturating_sub(x + 11).max(1),
            y - area.y + 1,
        ),
    ));
    // the input on its own row
    let iy = y + 2;
    let text = app.tabs.tags.input.text();
    let bad = !text.trim().is_empty() && !valid_tag(&text.trim().to_lowercase());
    let r = field(
        buf,
        x + 11,
        iy,
        26,
        &mut app.tabs.tags.input,
        focused,
        bad,
        &t,
    );
    app.hits
        .push((Rect::new(x + 11, iy, 26, 1), crate::app::Hit::Ctrl(0)));
    if text.is_empty() && !focused {
        put(
            buf,
            r.x + 1,
            iy,
            "add a tag…",
            Style::new()
                .fg(t.muted)
                .bg(t.panel)
                .add_modifier(Modifier::ITALIC),
            12,
        );
    }
    let note = if bad {
        ("✗ a–z, 0–9, _ or - (≤ 24)".to_string(), err_style(&t))
    } else {
        (
            format!(
                "{}/12 tags · sorted and lower-cased by the server",
                tags.len()
            ),
            muted(&t),
        )
    };
    put(
        buf,
        x + 39,
        iy,
        &note.0,
        note.1,
        area.right().saturating_sub(x + 39) as usize,
    );
    app.tabs.tags.anchor = Rect::new(x + 11, iy, 26, 1);
}

pub fn popup(app: &mut App, buf: &mut Buffer, _area: Rect) {
    if !is_focused(app, 0) {
        return;
    }
    let t = app.t.clone();
    let rows: Vec<Row> = entries(app)
        .into_iter()
        .map(|e| match e {
            Entry::Suggest(tag, n) => Row {
                label: tag.to_string(),
                dim: format!("{n} uses"),
                ..Default::default()
            },
            Entry::Create(s) => Row {
                label: format!("✚ Create \"{s}\""),
                ..Default::default()
            },
        })
        .collect();
    let a = app.tabs.tags.anchor;
    let r = Rect::new(a.x, a.y + 1, 32, (rows.len() as u16 + 2).clamp(3, 12));
    let sel = app.tabs.tags.sel;
    for (rect, j) in list::render(buf, r, "Suggestions", &rows, sel, &t) {
        app.hits.push((rect, Hit::Popup(0, j)));
    }
}
