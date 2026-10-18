//! The 16 tabs (ui-spec §4). Each module owns its control state and exposes
//! the same small set of functions, dispatched here by tab index:
//! `render` (controls), `popup` (anything drawn over the panels), `key`
//! (true if used), `click`, `hints`, and the lifecycle hooks `enter_ctrl` /
//! `leave_ctrl` / `sync` / `drop_drafts`.

pub mod cascade;
pub mod charts;
pub mod choice;
pub mod combo;
pub mod date;
pub mod dialogs_tab;
pub mod gantt;
pub mod grid;
pub mod kanban;
pub mod menus;
pub mod number;
pub mod palette_tab;
pub mod select;
pub mod tags;
pub mod text;
pub mod textarea;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::Date;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, Focus, Hint, Hit, hint};
use crate::theme::Theme;
use crate::widgets::draw::put;
use crate::widgets::input::TextInput;

pub struct Tabs {
    pub text: text::State,
    pub number: number::State,
    pub date: date::State,
    pub select: select::State,
    pub combo: combo::State,
    pub tags: tags::State,
    pub textarea: textarea::State,
    pub choice: choice::State,
    pub cascade: cascade::State,
    pub grid: grid::State,
    pub menus: menus::State,
    pub palette: palette_tab::State,
    pub charts: charts::State,
    pub kanban: kanban::State,
    pub gantt: gantt::State,
}

impl Tabs {
    pub fn new(today: Date) -> Tabs {
        Tabs {
            text: Default::default(),
            number: Default::default(),
            date: date::State::new(today),
            select: Default::default(),
            combo: Default::default(),
            tags: Default::default(),
            textarea: Default::default(),
            choice: Default::default(),
            cascade: Default::default(),
            grid: Default::default(),
            menus: Default::default(),
            palette: Default::default(),
            charts: Default::default(),
            kanban: Default::default(),
            gantt: gantt::State::new(today),
        }
    }
}

/// How many focusable controls each tab has (Tab order = the spec's order).
pub fn controls(tab: usize) -> usize {
    [4, 3, 2, 2, 2, 1, 1, 4, 3, 1, 2, 1, 5, 1, 1, 1][tab]
}

/// The Keys panel (and help) entries per tab.
pub fn keys_panel(tab: usize) -> &'static [(&'static str, &'static str)] {
    match tab {
        0 => text::KEYS,
        1 => number::KEYS,
        2 => date::KEYS,
        3 => select::KEYS,
        4 => combo::KEYS,
        5 => tags::KEYS,
        6 => textarea::KEYS,
        7 => choice::KEYS,
        8 => cascade::KEYS,
        9 => grid::KEYS,
        10 => menus::KEYS,
        11 => palette_tab::KEYS,
        12 => dialogs_tab::KEYS,
        13 => charts::KEYS,
        14 => kanban::KEYS,
        _ => gantt::KEYS,
    }
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    match app.tab {
        0 => text::render(app, buf, area),
        1 => number::render(app, buf, area),
        2 => date::render(app, buf, area),
        3 => select::render(app, buf, area),
        4 => combo::render(app, buf, area),
        5 => tags::render(app, buf, area),
        6 => textarea::render(app, buf, area),
        7 => choice::render(app, buf, area),
        8 => cascade::render(app, buf, area),
        9 => grid::render(app, buf, area),
        10 => menus::render(app, buf, area),
        11 => palette_tab::render(app, buf, area),
        12 => dialogs_tab::render(app, buf, area),
        13 => charts::render(app, buf, area),
        14 => kanban::render(app, buf, area),
        _ => gantt::render(app, buf, area),
    }
}

/// Popups that open over the Keys/Value panels.
pub fn popup(app: &mut App, buf: &mut Buffer, area: Rect) {
    match app.tab {
        3 => select::popup(app, buf, area),
        4 => combo::popup(app, buf, area),
        5 => tags::popup(app, buf, area),
        8 => cascade::popup(app, buf, area),
        9 => grid::popup(app, buf, area),
        10 => menus::popup(app, buf, area),
        _ => {}
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    match app.tab {
        0 => text::key(app, k),
        1 => number::key(app, k),
        2 => date::key(app, k),
        3 => select::key(app, k),
        4 => combo::key(app, k),
        5 => tags::key(app, k),
        6 => textarea::key(app, k),
        7 => choice::key(app, k),
        8 => cascade::key(app, k),
        9 => grid::key(app, k),
        10 => menus::key(app, k),
        11 => palette_tab::key(app, k),
        12 => dialogs_tab::key(app, k),
        13 => charts::key(app, k),
        14 => kanban::key(app, k),
        _ => gantt::key(app, k),
    }
}

/// A click on something the current tab drew.
pub fn click(app: &mut App, hit: Hit, x: u16, y: u16, right: bool) {
    let used = match app.tab {
        0 => text::click(app, &hit),
        1 => number::click(app, &hit, x),
        2 => date::click(app, &hit),
        3 => select::click(app, &hit),
        4 => combo::click(app, &hit),
        5 => tags::click(app, &hit),
        6 => textarea::click(app, &hit, x, y),
        7 => choice::click(app, &hit),
        8 => cascade::click(app, &hit),
        9 => grid::click(app, &hit, x, y),
        10 => menus::click(app, &hit, right),
        11 => palette_tab::click(app, &hit),
        12 => dialogs_tab::click(app, &hit),
        13 => charts::click(app, &hit),
        14 => kanban::click(app, &hit),
        _ => gantt::click(app, &hit, x),
    };
    if !used {
        if let Hit::Ctrl(c) = hit {
            app.focus_ctrl(c);
        }
    }
}

pub fn drag(app: &mut App, x: u16, y: u16, end: bool) {
    match app.tab {
        1 => number::drag(app, x, end),
        14 => kanban::drag(app, x, y, end),
        15 => gantt::drag(app, x, end),
        _ => {}
    }
}

pub fn scroll(app: &mut App, down: bool) {
    let code = if down { KeyCode::Down } else { KeyCode::Up };
    if app.focus == Focus::Sidebar {
        let n = demo_core::dataset::TABS.len();
        app.switch_tab(if down {
            (app.tab + 1) % n
        } else {
            (app.tab + n - 1) % n
        });
    } else if matches!(app.tab, 6 | 9 | 10 | 11 | 15) {
        key(app, KeyEvent::new(code, KeyModifiers::NONE));
    }
}

pub fn paste(app: &mut App, s: &str) {
    let line: String = s.chars().filter(|c| !c.is_control()).collect();
    match app.tab {
        0 => text::paste(app, &line),
        6 => app.tabs.textarea.paste(s),
        11 => {
            app.tabs.palette.p.input.insert_str(&line);
            app.tabs.palette.p.sel = 0;
        }
        _ => {}
    }
}

/// Hints for the focused control (the hint bar adds the global ones).
pub fn hints(app: &App) -> Vec<Hint> {
    let mut v = match app.tab {
        0 => text::hints(app),
        1 => number::hints(app),
        2 => date::hints(app),
        3 => select::hints(app),
        4 => combo::hints(app),
        5 => tags::hints(app),
        6 => textarea::hints(app),
        7 => choice::hints(app),
        8 => cascade::hints(app),
        9 => grid::hints(app),
        10 => menus::hints(app),
        11 => palette_tab::hints(app),
        12 => dialogs_tab::hints(app),
        13 => charts::hints(app),
        14 => kanban::hints(app),
        _ => gantt::hints(app),
    };
    if !popup_open(app) {
        if controls(app.tab) > 1 || matches!(app.tab, 5 | 6) {
            v.push(hint("Tab", "Next", KeyCode::Tab, KeyModifiers::NONE));
        }
        if !v.iter().any(|h| h.key == "Esc") {
            v.push(hint("Esc", "Sidebar", KeyCode::Esc, KeyModifiers::NONE));
        }
    }
    v
}

/// True when the current control has a popup open (Esc closes it, not the tab).
pub fn popup_open(app: &App) -> bool {
    match app.tab {
        3 => app.tabs.select.drop.open,
        4 => app.tabs.combo.open,
        8 => app.tabs.cascade.drop.open,
        9 => app.tabs.grid.editor.is_some(),
        10 => app.tabs.menus.menu.is_some() || app.tabs.menus.ctx.is_some(),
        _ => false,
    }
}

/// Focus entered the body: the first control.
pub fn enter(app: &mut App) {
    enter_ctrl(app);
}

pub fn enter_ctrl(app: &mut App) {
    match app.tab {
        2 => date::enter_ctrl(app),
        7 => choice::enter_ctrl(app),
        11 => app.tabs.palette.p.sel = 0,
        _ => {}
    }
}

/// Focus is leaving the current control: close its popups.
pub fn leave_ctrl(app: &mut App) {
    match app.tab {
        3 => app.tabs.select.drop.close(),
        4 => app.tabs.combo.close(),
        5 => app.tabs.tags.leave(),
        6 => app.tabs.textarea.leave(),
        8 => app.tabs.cascade.drop.close(),
        9 => app.tabs.grid.editor = None,
        10 => {
            app.tabs.menus.menu = None;
            app.tabs.menus.ctx = None;
        }
        _ => {}
    }
}

/// Leaving the tab (or the body).
pub fn leave(app: &mut App) {
    if app.focus == Focus::Body {
        leave_ctrl(app);
    }
}

/// Refresh unedited field buffers from the shown values (before drawing).
pub fn sync(app: &mut App) {
    text::sync(app);
    number::sync(app);
    date::sync(app);
    textarea::sync(app);
}

/// Forget local drafts of `key` (after a conflict, rejection or reset):
/// the control shows the synced value again.
pub fn drop_drafts(app: &mut App, key: &str) {
    match key {
        "text.name" | "text.email" | "text.code" => app.tabs.text.drop(key),
        "number.hours" | "number.qty" => app.tabs.number.drop(key),
        "date.start" | "date.due" => app.tabs.date.drop(key),
        "textarea.notes" => app.tabs.textarea.dirty = false,
        "grid.rows" => app.tabs.grid.editor = None,
        _ => {}
    }
    sync(app);
}

/// After `welcome`: every draft is dropped.
pub fn reset_all(app: &mut App) {
    for k in demo_core::KEYS.iter() {
        drop_drafts(app, k.key);
    }
}

// ---- shared drawing helpers --------------------------------------------------------

pub fn is_focused(app: &App, c: usize) -> bool {
    app.focus == Focus::Body && app.ctrl == c
}

/// A field label, accent when its control is focused.
pub fn label(buf: &mut Buffer, x: u16, y: u16, s: &str, focused: bool, t: &Theme, w: usize) {
    let st = if focused {
        Style::new()
            .fg(t.accent)
            .bg(t.bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(t.fg).bg(t.bg)
    };
    put(buf, x, y, if focused { "▸" } else { " " }, st, 1);
    put(buf, x + 1, y, s, st, w.saturating_sub(1));
}

/// A bracketed single-line input `[text▏    ]`; returns the inner rect.
#[allow(clippy::too_many_arguments)]
pub fn field(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    input: &mut TextInput,
    focused: bool,
    bad: bool,
    t: &Theme,
) -> Rect {
    let br = Style::new()
        .fg(if focused { t.accent } else { t.border })
        .bg(t.bg);
    put(buf, x, y, "[", br, 1);
    put(buf, x + w - 1, y, "]", br, 1);
    let inner = Rect::new(x + 1, y, w.saturating_sub(2), 1);
    let st = Style::new()
        .bg(if focused { t.sel_bg } else { t.panel })
        .fg(if bad { t.err } else { t.fg });
    input.render(buf, inner, st, focused);
    inner
}

/// Status message colour helpers for inline errors / info.
pub fn err_style(t: &Theme) -> Style {
    Style::new().fg(t.err).bg(t.bg)
}
pub fn muted(t: &Theme) -> Style {
    Style::new().fg(t.muted).bg(t.bg)
}

/// Shorthand for a hint with no modifiers.
pub fn h(key: &str, label: &str, code: KeyCode) -> Hint {
    hint(key, label, code, KeyModifiers::NONE)
}

/// Shorthand for a Ctrl hint.
pub fn hc(key: &str, label: &str, c: char) -> Hint {
    hint(key, label, KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// `who` of an entry for "⚠ changed by bob".
pub fn changed_by(app: &App, key: &str) -> String {
    app.v
        .entry(key)
        .map(|e| demo_core::safe_text(&e.by))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "someone".into())
}

/// A single-line field bound to a key: the draft text, whether the user
/// edited it, and the version shown when editing began (the commit's base).
#[derive(Default)]
pub struct Draft {
    pub input: TextInput,
    pub dirty: bool,
    pub base: u64,
}

impl Draft {
    /// Show `shown` unless the user is editing.
    pub fn sync(&mut self, shown: &str, version: u64) {
        if !self.dirty {
            if self.input.text() != shown {
                self.input.set(shown);
            }
            self.base = version;
        }
    }

    /// Feed a key to the input; marks the draft edited if the text changed.
    pub fn handle(&mut self, k: &KeyEvent) -> bool {
        let before = self.input.text();
        let used = self.input.handle(k);
        if self.input.text() != before {
            self.dirty = true;
        }
        used
    }

    /// Someone else changed the key while we were editing it.
    pub fn changed_meanwhile(&self, version: u64) -> bool {
        self.dirty && version != self.base
    }
}
