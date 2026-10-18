//! Full-screen, searchable help (ui-spec §3): This tab, Getting around,
//! Global keys, All tabs, How demo works. The search box filters every line
//! live; a section heading stays while any of its lines match.

use crossterm::event::{KeyCode, KeyEvent};
use demo_core::dataset::TABS;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, Hint, hint};
use crate::tabs;
use crate::widgets::draw::{fill, put, width};
use crate::widgets::input::TextInput;

#[derive(Default)]
pub struct HelpState {
    pub input: TextInput,
    pub scroll: usize,
    /// Lines that fit on screen at the last draw (for PgUp/PgDn).
    page: usize,
}

type Line = (Kind, String);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Section,
    Sub,
    Key,
    Text,
}

pub const GETTING_AROUND: [(&str, &str); 8] = [
    ("↑ ↓", "sidebar: previous / next tab (switches immediately)"),
    ("Home End", "sidebar: first / last tab"),
    (
        "Enter → Tab",
        "sidebar: move into the tab body (first control)",
    ),
    ("Tab", "body: next control in the tab"),
    (
        "Shift+Tab",
        "body: previous control; from the first, back to the sidebar",
    ),
    (
        "Esc",
        "body: close a popup or undo an edit; else back to the sidebar",
    ),
    ("F6", "toggle sidebar ⇄ tab body"),
    (
        "click",
        "tabs, fields, list items, buttons; drag cards, bars and sliders",
    ),
];

pub const GLOBAL_KEYS: [(&str, &str); 11] = [
    ("F1 ^G", "this help"),
    ("F10 Alt+F/V/H", "menu bar"),
    ("^K ^P", "command palette"),
    ("^PgDn ^PgUp", "next / previous tab"),
    ("Alt+1…9 Alt+0", "go to tab 1–9, 10"),
    ("^R", "reset this tab to defaults (asks first)"),
    ("^T", "cycle the theme"),
    ("^L", "redraw the screen"),
    ("^Q", "quit"),
    ("Shift+F10", "context menu (tab 11 files), also right-click"),
    ("Esc", "close any popup, menu, palette or this help"),
];

const HOW: &str = "One demo server holds the current value of every control in a SQLite file. Every client — Rust or Go, terminal or browser — connects over a WebSocket and sees every change live. Each key has its own version: a change is sent with the version you started from, and if someone else changed it first the server says so (a conflict) and every client shows the newer value. The Value panel on each tab shows the key, its value, its version and who changed it last.";

/// Every help line for the current tab, unfiltered.
pub fn lines(app: &App) -> Vec<(Kind, String)> {
    let mut out = vec![];
    let tab = app.cur_tab();
    out.push((
        Kind::Section,
        format!("This tab — {} {}", tab.number, tab.heading),
    ));
    out.push((Kind::Text, tab.blurb.to_string()));
    for (k, l) in tabs::keys_panel(app.tab) {
        out.push((Kind::Key, format!("{k:<16}{l}")));
    }
    out.push((Kind::Section, "Getting around".into()));
    for (k, l) in GETTING_AROUND {
        out.push((Kind::Key, format!("{k:<16}{l}")));
    }
    out.push((Kind::Section, "Global keys".into()));
    for (k, l) in GLOBAL_KEYS {
        out.push((Kind::Key, format!("{k:<16}{l}")));
    }
    out.push((Kind::Section, "All tabs".into()));
    for (i, t) in TABS.iter().enumerate() {
        out.push((
            Kind::Sub,
            format!("{} {} — {}", t.number, t.heading, t.blurb),
        ));
        for (k, l) in tabs::keys_panel(i) {
            out.push((Kind::Key, format!("{k:<16}{l}")));
        }
    }
    out.push((Kind::Section, "How demo works".into()));
    // wrapped when drawn
    out.push((Kind::Text, HOW.to_string()));
    out
}

/// Lines after the search filter: matching lines, plus the headings of
/// sections (and tab sub-headings) that have a match.
pub fn filtered(all: Vec<(Kind, String)>, q: &str) -> Vec<(Kind, String)> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return all;
    }
    let mut out: Vec<(Kind, String)> = vec![];
    let (mut section, mut sub): (Option<Line>, Option<Line>) = (None, None);
    for (k, s) in all {
        match k {
            Kind::Section => {
                section = Some((k, s));
                sub = None;
            }
            Kind::Sub if !s.to_lowercase().contains(&q) => sub = Some((k, s)),
            _ if s.to_lowercase().contains(&q) => {
                if let Some(h) = section.take() {
                    out.push(h);
                }
                if let Some(h) = sub.take() {
                    out.push(h);
                }
                out.push((k, s));
            }
            _ => {}
        }
    }
    out
}

impl HelpState {
    /// Returns true when help should close.
    pub fn key(&mut self, k: &KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc | KeyCode::F(1) => return true,
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-(self.page.max(2) as i32 - 1)),
            KeyCode::PageDown => self.scroll_by(self.page.max(2) as i32 - 1),
            KeyCode::Home if self.input.is_empty() => self.scroll = 0,
            _ => {
                if self.input.handle(k) {
                    self.scroll = 0;
                }
            }
        }
        false
    }

    pub fn scroll_by(&mut self, d: i32) {
        self.scroll = (self.scroll as i32 + d).max(0) as usize;
    }

    pub fn hints() -> Vec<Hint> {
        use crossterm::event::KeyModifiers as M;
        vec![
            hint("type", "Search", KeyCode::Null, M::NONE),
            hint("↑↓", "Scroll", KeyCode::Down, M::NONE),
            hint("PgUp/PgDn", "Page", KeyCode::PageDown, M::NONE),
            hint("^U", "Clear search", KeyCode::Char('u'), M::CONTROL),
            hint("Esc", "Close", KeyCode::Esc, M::NONE),
        ]
    }
}

/// Word-wrap `s` to `w` columns.
pub fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out = vec![];
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && width(&line) + 1 + width(word) > w {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let all = lines(app);
    let Some(h) = app.help.as_mut() else { return };
    fill(buf, area, t.base());
    let head = Style::new().bg(t.header_bg).fg(t.fg);
    fill(buf, Rect::new(area.x, area.y, area.width, 1), head);
    put(
        buf,
        area.x + 1,
        area.y,
        "Help — demo keyboard guide",
        head.add_modifier(Modifier::BOLD),
        30,
    );
    put(buf, area.x + 32, area.y, "Search:", head.fg(t.muted), 8);
    let ir = Rect::new(
        area.x + 40,
        area.y,
        area.width.saturating_sub(42).min(40),
        1,
    );
    let empty = h.input.is_empty();
    h.input
        .render(buf, ir, Style::new().bg(t.sel_bg).fg(t.fg), true);
    if empty {
        put(
            buf,
            ir.x + 1,
            ir.y,
            "type to filter",
            Style::new()
                .bg(t.sel_bg)
                .fg(t.muted)
                .add_modifier(Modifier::ITALIC),
            ir.width as usize - 1,
        );
    }
    let body = Rect::new(
        area.x + 2,
        area.y + 2,
        area.width.saturating_sub(4),
        area.height.saturating_sub(3),
    );
    // flatten with wrapping
    let mut rows: Vec<(Kind, String)> = vec![];
    for (k, s) in filtered(all, &h.input.text()) {
        match k {
            Kind::Text => rows.extend(
                wrap(&s, body.width as usize - 2)
                    .into_iter()
                    .map(|l| (Kind::Text, l)),
            ),
            Kind::Section => {
                if !rows.is_empty() {
                    rows.push((Kind::Text, String::new()));
                }
                rows.push((k, s));
            }
            _ => rows.push((k, s)),
        }
    }
    if rows.is_empty() {
        rows.push((Kind::Text, format!("Nothing matches “{}”.", h.input.text())));
    }
    h.page = body.height as usize;
    h.scroll = h
        .scroll
        .min(rows.len().saturating_sub(body.height as usize));
    for (i, (k, s)) in rows
        .iter()
        .skip(h.scroll)
        .take(body.height as usize)
        .enumerate()
    {
        let y = body.y + i as u16;
        let (x, st) = match k {
            Kind::Section => (
                body.x,
                Style::new()
                    .fg(t.accent)
                    .bg(t.bg)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
            Kind::Sub => (
                body.x + 1,
                Style::new()
                    .fg(t.accent2)
                    .bg(t.bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Kind::Key => (body.x + 3, Style::new().fg(t.fg).bg(t.bg)),
            Kind::Text => (body.x + 1, Style::new().fg(t.fg).bg(t.bg)),
        };
        put(buf, x, y, s, st, body.right().saturating_sub(x) as usize);
        if *k == Kind::Key {
            // the key column in the key colour
            let key: String = s.chars().take(16).collect();
            put(
                buf,
                x,
                y,
                key.trim_end(),
                Style::new()
                    .fg(t.warn)
                    .bg(t.bg)
                    .add_modifier(Modifier::BOLD),
                16,
            );
        }
    }
    if rows.len() > body.height as usize {
        let pos = format!(
            " {}–{} of {} ",
            h.scroll + 1,
            (h.scroll + body.height as usize).min(rows.len()),
            rows.len()
        );
        put(
            buf,
            area.right().saturating_sub(width(&pos) as u16 + 1),
            area.y,
            &pos,
            head.fg(t.muted),
            24,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_keeps_headings() {
        let all = vec![
            (Kind::Section, "A".to_string()),
            (Kind::Key, "x   one".to_string()),
            (Kind::Sub, "sub".to_string()),
            (Kind::Key, "y   two".to_string()),
            (Kind::Section, "B".to_string()),
            (Kind::Key, "z   three".to_string()),
        ];
        let f = filtered(all, "two");
        assert_eq!(
            f.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(),
            ["A", "sub", "y   two"]
        );
        assert_eq!(wrap("aa bb cc", 5), vec!["aa bb", "cc"]);
    }
}
