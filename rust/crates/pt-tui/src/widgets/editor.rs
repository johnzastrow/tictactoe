//! The field editor: one control for every editor kind in ui-spec §3 (text,
//! number, date + calendar, select, fuzzy combobox, suggest, chips, bool,
//! multi-line). Grid cells and dialog forms both host it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use pt_core::date::{Date, MONTH_NAMES, parse_natural};
use pt_core::grid::{Choice, filter_choices, fmt_hours};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::Value;

use super::draw::{fill, frame, pad, put, put_highlighted, rect_contains, shadow, width};
use super::input::{TextArea, TextInput};
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdKind {
    Text,
    Password,
    Number,
    Int,
    Date,
    Select,
    Combo,
    Suggest,
    Chips { free: bool },
    Bool,
    Multiline,
}

impl EdKind {
    pub fn is_list(self) -> bool {
        matches!(self, EdKind::Select | EdKind::Combo | EdKind::Suggest | EdKind::Chips { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Stay,
    Down,
    Right,
    Left,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EdOut {
    Consumed,
    Ignored,
    Commit(Nav),
    Cancel,
}

/// The edited value: free text (parsed by the caller per field) or a value.
#[derive(Clone, Debug, PartialEq)]
pub enum EdValue {
    Text(String),
    Value(Value),
}

#[derive(Clone, Debug)]
pub struct FieldEditor {
    pub kind: EdKind,
    pub input: TextInput,
    pub area: TextArea,
    pub choices: Vec<Choice>,
    filtered: Vec<(usize, Vec<usize>)>,
    pub sel: usize,
    scroll: usize,
    navigated: bool,
    /// Lists in forms start closed; grid editors open them immediately.
    pub list_open: bool,
    pub current: Value,
    pub chips: Vec<Choice>,
    pub flag: bool,
    pub cal: Date,
    pub today: Date,
    pub error: Option<String>,
    pub placeholder: String,
    popup: Rect,
    list_rows: Rect,
    cal_rect: Rect,
}

impl FieldEditor {
    fn base(kind: EdKind, today: Date) -> FieldEditor {
        FieldEditor {
            kind,
            input: TextInput::default(),
            area: TextArea::default(),
            choices: vec![],
            filtered: vec![],
            sel: 0,
            scroll: 0,
            navigated: false,
            list_open: false,
            current: Value::Null,
            chips: vec![],
            flag: false,
            cal: today,
            today,
            error: None,
            placeholder: String::new(),
            popup: Rect::default(),
            list_rows: Rect::default(),
            cal_rect: Rect::default(),
        }
    }

    pub fn text(kind: EdKind, s: &str, today: Date) -> FieldEditor {
        let mut e = FieldEditor::base(kind, today);
        e.input = TextInput::new(s);
        e.input.mask = kind == EdKind::Password;
        if kind == EdKind::Date {
            if let Some(d) = Date::parse_iso(s) {
                e.cal = d;
            }
        }
        if kind == EdKind::Multiline {
            e.area = TextArea::new(s);
        }
        e
    }

    /// Select / Combo / Suggest. For Suggest, `s` is the current text.
    pub fn list(kind: EdKind, choices: Vec<Choice>, current: Value, s: &str, today: Date) -> FieldEditor {
        let mut e = FieldEditor::base(kind, today);
        e.choices = choices;
        e.current = current;
        if kind == EdKind::Suggest {
            e.input = TextInput::new(s);
        }
        e.refilter();
        e.sel = e
            .filtered
            .iter()
            .position(|(i, _)| e.choices[*i].value == e.current)
            .unwrap_or(0);
        e
    }

    pub fn chips(choices: Vec<Choice>, current: &[Value], free: bool, today: Date) -> FieldEditor {
        let mut e = FieldEditor::base(EdKind::Chips { free }, today);
        e.chips = current
            .iter()
            .map(|v| {
                choices.iter().find(|c| &c.value == v).cloned().unwrap_or(Choice {
                    value: v.clone(),
                    label: v.as_str().map(String::from).unwrap_or_else(|| v.to_string()),
                    detail: String::new(),
                    style: pt_core::grid::Style::Tags,
                })
            })
            .collect();
        e.choices = choices;
        e.refilter();
        e
    }

    pub fn boolean(v: bool, today: Date) -> FieldEditor {
        let mut e = FieldEditor::base(EdKind::Bool, today);
        e.flag = v;
        e
    }

    pub fn open(mut self) -> Self {
        self.list_open = true;
        self
    }

    /// Start editing from a typed character (Excel-style).
    pub fn seed(&mut self, c: char) {
        match self.kind {
            EdKind::Bool => {
                if matches!(c, 'y' | 'Y' | '1') {
                    self.flag = true;
                } else if matches!(c, 'n' | 'N' | '0') {
                    self.flag = false;
                }
            }
            EdKind::Multiline => self.area.insert_str(&c.to_string()),
            _ => {
                self.input.set(&c.to_string());
                self.on_text_change();
            }
        }
    }

    pub fn paste(&mut self, s: &str) {
        match self.kind {
            EdKind::Multiline => self.area.insert_str(s),
            EdKind::Bool => {}
            _ => {
                self.input.insert_str(s.lines().next().unwrap_or(""));
                self.on_text_change();
            }
        }
    }

    fn refilter(&mut self) {
        let q = self.input.text();
        let pool: Vec<Choice> = self.choices.clone();
        self.filtered = filter_choices(&pool, &q)
            .into_iter()
            .filter(|(i, _)| match self.kind {
                EdKind::Chips { .. } => !self.chips.iter().any(|c| c.value == pool[*i].value),
                _ => true,
            })
            .collect();
        self.sel = 0;
        self.scroll = 0;
    }

    fn on_text_change(&mut self) {
        self.error = None;
        if self.kind.is_list() {
            self.refilter();
            self.navigated = false;
            self.list_open = true;
        }
        if self.kind == EdKind::Date {
            if let Ok(Some(d)) = parse_natural(&self.input.text(), self.today) {
                self.cal = d;
            }
        }
    }

    fn set_date(&mut self, d: Date) {
        self.cal = d;
        self.input.set(&d.iso());
        self.error = None;
    }

    fn move_sel(&mut self, delta: i32) {
        let n = self.list_len() as i32;
        if n == 0 {
            return;
        }
        self.sel = (self.sel as i32 + delta).rem_euclid(n) as usize;
        self.navigated = true;
    }

    /// The chosen item for Select/Combo after Enter.
    fn choose(&mut self) -> bool {
        if let Some((i, _)) = self.filtered.get(self.sel) {
            self.current = self.choices[*i].value.clone();
            self.input.clear();
            self.refilter();
            true
        } else {
            false
        }
    }

    /// Free chips get a trailing "✚ create" row when the typed text is new.
    fn create_row(&self) -> bool {
        let typed = self.input.text();
        let typed = typed.trim();
        matches!(self.kind, EdKind::Chips { free: true })
            && !typed.is_empty()
            && !self.choices.iter().any(|c| c.label.eq_ignore_ascii_case(typed))
            && !self.chips.iter().any(|c| c.label.eq_ignore_ascii_case(typed))
    }

    fn list_len(&self) -> usize {
        self.filtered.len() + self.create_row() as usize
    }

    fn add_chip(&mut self) -> bool {
        let typed = self.input.text().trim().to_string();
        if typed.is_empty() && !self.navigated {
            return false;
        }
        let choice = if let Some((i, _)) = self.filtered.get(self.sel) {
            Some(self.choices[*i].clone())
        } else if self.create_row() {
            Some(Choice {
                value: Value::String(typed.clone()),
                label: typed,
                detail: "new".into(),
                style: pt_core::grid::Style::Tags,
            })
        } else {
            None
        };
        match choice {
            Some(c) => {
                if !self.chips.iter().any(|x| x.value == c.value) {
                    self.chips.push(c);
                }
                self.input.clear();
                self.refilter();
                self.navigated = false;
                true
            }
            None => false,
        }
    }

    pub fn handle_key(&mut self, k: &KeyEvent) -> EdOut {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        if k.code == KeyCode::Esc {
            if self.kind.is_list() && self.list_open && self.kind != EdKind::Suggest && !self.input.is_empty() {
                self.input.clear();
                self.refilter();
                return EdOut::Consumed;
            }
            return EdOut::Cancel;
        }
        let commit_nav = match k.code {
            KeyCode::Tab => Some(Nav::Right),
            KeyCode::BackTab => Some(Nav::Left),
            _ => None,
        };
        match self.kind {
            EdKind::Multiline => {
                if let Some(n) = commit_nav {
                    return EdOut::Commit(n);
                }
                if ctrl && k.code == KeyCode::Char('s') {
                    return EdOut::Commit(Nav::Stay);
                }
                return if self.area.handle(k) { EdOut::Consumed } else { EdOut::Ignored };
            }
            EdKind::Bool => {
                return match k.code {
                    KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                        self.flag = !self.flag;
                        EdOut::Consumed
                    }
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        self.flag = true;
                        EdOut::Consumed
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.flag = false;
                        EdOut::Consumed
                    }
                    KeyCode::Enter => EdOut::Commit(Nav::Down),
                    _ => commit_nav.map_or(EdOut::Ignored, EdOut::Commit),
                };
            }
            EdKind::Select | EdKind::Combo => {
                if !self.list_open {
                    return match k.code {
                        KeyCode::Enter | KeyCode::F(4) => {
                            self.list_open = true;
                            EdOut::Consumed
                        }
                        KeyCode::Down | KeyCode::Up => {
                            // quick cycle without opening the list
                            let n = self.choices.len();
                            if n > 0 {
                                let i = self.choices.iter().position(|c| c.value == self.current).unwrap_or(0);
                                let j = if k.code == KeyCode::Down { (i + 1) % n } else { (i + n - 1) % n };
                                self.current = self.choices[j].value.clone();
                            }
                            EdOut::Consumed
                        }
                        KeyCode::Char(c) if !ctrl => {
                            self.input.set(&c.to_string());
                            self.on_text_change();
                            EdOut::Consumed
                        }
                        _ => commit_nav.map_or(EdOut::Ignored, EdOut::Commit),
                    };
                }
                match k.code {
                    KeyCode::Up => self.move_sel(-1),
                    KeyCode::Down => self.move_sel(1),
                    KeyCode::PageUp => self.move_sel(-8),
                    KeyCode::PageDown => self.move_sel(8),
                    KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => {
                        if !self.choose() && !self.input.is_empty() {
                            self.error = Some("nothing matches".into());
                            return EdOut::Consumed;
                        }
                        return EdOut::Commit(commit_nav.unwrap_or(Nav::Down));
                    }
                    _ => {
                        if self.input.handle(k) {
                            self.on_text_change();
                        } else {
                            return EdOut::Ignored;
                        }
                    }
                }
                return EdOut::Consumed;
            }
            EdKind::Suggest => {
                match k.code {
                    KeyCode::Up if self.list_open => self.move_sel(-1),
                    KeyCode::Down => {
                        self.list_open = true;
                        if self.navigated || self.filtered.len() > 1 {
                            self.move_sel(if self.navigated { 1 } else { 0 });
                        }
                        self.navigated = true;
                    }
                    KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => {
                        if self.navigated && self.list_open {
                            if let Some((i, _)) = self.filtered.get(self.sel) {
                                let label = self.choices[*i].label.clone();
                                self.input.set(&label);
                            }
                        }
                        return EdOut::Commit(commit_nav.unwrap_or(Nav::Down));
                    }
                    _ => {
                        if self.input.handle(k) {
                            self.on_text_change();
                        } else {
                            return EdOut::Ignored;
                        }
                    }
                }
                return EdOut::Consumed;
            }
            EdKind::Chips { .. } => {
                match k.code {
                    KeyCode::Up => self.move_sel(-1),
                    KeyCode::Down => {
                        self.list_open = true;
                        if self.navigated {
                            self.move_sel(1);
                        } else {
                            self.navigated = true;
                        }
                    }
                    KeyCode::Enter if ctrl => return EdOut::Commit(Nav::Stay),
                    KeyCode::Enter => {
                        if self.input.is_empty() && !self.navigated {
                            return EdOut::Commit(Nav::Down);
                        }
                        if !self.add_chip() {
                            self.error = Some("no match".into());
                        }
                    }
                    KeyCode::Tab | KeyCode::BackTab => {
                        if !self.input.is_empty() {
                            self.add_chip();
                        }
                        return EdOut::Commit(commit_nav.unwrap_or(Nav::Right));
                    }
                    KeyCode::Backspace if self.input.is_empty() => {
                        self.chips.pop();
                        self.refilter();
                    }
                    KeyCode::Char(',') => {
                        self.add_chip();
                    }
                    _ => {
                        if self.input.handle(k) {
                            self.on_text_change();
                        } else {
                            return EdOut::Ignored;
                        }
                    }
                }
                return EdOut::Consumed;
            }
            _ => {}
        }
        // single-line text kinds
        if k.code == KeyCode::Enter {
            if self.kind == EdKind::Date {
                if let Err(e) = parse_natural(&self.input.text(), self.today) {
                    self.error = Some(e);
                    return EdOut::Consumed;
                }
            }
            return EdOut::Commit(Nav::Down);
        }
        if let Some(n) = commit_nav {
            return EdOut::Commit(n);
        }
        match (self.kind, k.code) {
            (EdKind::Date, KeyCode::Up) => self.set_date(self.cal.add_days(-7)),
            (EdKind::Date, KeyCode::Down) => self.set_date(self.cal.add_days(7)),
            (EdKind::Date, KeyCode::PageUp) => self.set_date(self.cal.add_months(-1)),
            (EdKind::Date, KeyCode::PageDown) => self.set_date(self.cal.add_months(1)),
            (EdKind::Date, KeyCode::Left) if ctrl => self.set_date(self.cal.add_days(-1)),
            (EdKind::Date, KeyCode::Right) if ctrl => self.set_date(self.cal.add_days(1)),
            (EdKind::Number | EdKind::Int, KeyCode::Up | KeyCode::Down) => {
                let step = if shift && self.kind == EdKind::Number { 0.25 } else { 1.0 };
                let cur: f64 = self.input.text().trim_end_matches(['h', 'H']).trim().parse().unwrap_or(0.0);
                let n = if k.code == KeyCode::Up { cur + step } else { (cur - step).max(0.0) };
                self.input.set(&fmt_hours(n));
                self.error = None;
            }
            (EdKind::Number, KeyCode::Char(c)) if !ctrl && !(c.is_ascii_digit() || matches!(c, '.' | '-' | 'h')) => {
                self.error = Some("numbers only".into());
            }
            (EdKind::Int, KeyCode::Char(c)) if !ctrl && !(c.is_ascii_digit() || c == '-') => {
                self.error = Some("whole numbers only".into());
            }
            _ => {
                if self.input.handle(k) {
                    self.on_text_change();
                } else {
                    return EdOut::Ignored;
                }
            }
        }
        EdOut::Consumed
    }

    pub fn value(&self) -> EdValue {
        match self.kind {
            EdKind::Select | EdKind::Combo => EdValue::Value(self.current.clone()),
            EdKind::Chips { .. } => EdValue::Value(Value::Array(self.chips.iter().map(|c| c.value.clone()).collect())),
            EdKind::Bool => EdValue::Value(Value::Bool(self.flag)),
            EdKind::Multiline => EdValue::Text(self.area.text()),
            _ => EdValue::Text(self.input.text()),
        }
    }

    /// Text shown when the field is not being edited (forms).
    pub fn display(&self) -> String {
        match self.kind {
            EdKind::Select | EdKind::Combo => self
                .choices
                .iter()
                .find(|c| c.value == self.current)
                .map_or_else(|| if self.current.is_null() { String::new() } else { self.current.to_string() }, |c| c.label.clone()),
            EdKind::Chips { .. } => self.chips.iter().map(|c| c.label.as_str()).collect::<Vec<_>>().join(", "),
            EdKind::Bool => if self.flag { "☑ yes" } else { "☐ no" }.into(),
            EdKind::Password => "•".repeat(self.input.text().chars().count()),
            EdKind::Multiline => self.area.text(),
            _ => self.input.text(),
        }
    }

    pub fn current_style(&self) -> Option<&pt_core::grid::Style> {
        match self.kind {
            EdKind::Select | EdKind::Combo => self.choices.iter().find(|c| c.value == self.current).map(|c| &c.style),
            _ => None,
        }
    }

    /// Draw the editing line (or text area) inside `r`.
    pub fn render_inline(&mut self, buf: &mut Buffer, r: Rect, t: &Theme, focused: bool) {
        let st = if focused { Style::new().fg(t.fg).bg(t.sel_bg) } else { Style::new().fg(t.fg).bg(t.panel) };
        match self.kind {
            EdKind::Multiline => self.area.render(buf, r, st, focused),
            EdKind::Bool => {
                fill(buf, Rect::new(r.x, r.y, r.width, 1), st);
                let s = if self.flag { "☑ yes" } else { "☐ no" };
                put(buf, r.x, r.y, s, st.fg(if self.flag { t.ok } else { t.muted }), r.width as usize);
            }
            EdKind::Select | EdKind::Combo if !(self.list_open && focused) => {
                fill(buf, Rect::new(r.x, r.y, r.width, 1), st);
                let label = self.display();
                let cs = self.current_style().map_or(Style::new(), |s| t.cell(s));
                let glyph = self.current_style().and_then(Theme::glyph).unwrap_or("");
                put(buf, r.x, r.y, &format!("{glyph}{label}"), st.patch(cs), (r.width as usize).saturating_sub(2));
                put(buf, r.right().saturating_sub(1), r.y, "▾", st.fg(t.muted), 1);
            }
            EdKind::Chips { .. } => {
                fill(buf, Rect::new(r.x, r.y, r.width, 1), st);
                let mut x = r.x;
                let chip = Style::new().fg(t.bg).bg(t.accent2);
                let room = r.width as usize;
                // keep the input visible: drop leading chips that don't fit
                let input_w = self.input.display_width().max(6) + 1;
                let mut first = 0;
                let total: usize = self.chips.iter().map(|c| width(&c.label) + 3).sum();
                let mut over = (total + input_w).saturating_sub(room);
                while over > 0 && first < self.chips.len() {
                    over = over.saturating_sub(width(&self.chips[first].label) + 3);
                    first += 1;
                }
                if first > 0 {
                    x += put(buf, x, r.y, "…", st.fg(t.muted), 1);
                }
                for c in &self.chips[first..] {
                    let label = format!(" {} ", c.label);
                    if (x - r.x) as usize + width(&label) + 1 > room {
                        break;
                    }
                    x += put(buf, x, r.y, &label, chip, width(&label));
                    x += 1;
                }
                let rest = r.right().saturating_sub(x);
                if rest > 0 {
                    self.input.render(buf, Rect::new(x, r.y, rest, 1), st, focused);
                }
            }
            _ => {
                let line = Rect::new(r.x, r.y, r.width, 1);
                self.input.render(buf, line, st, focused);
                if self.input.is_empty() && !self.placeholder.is_empty() && !focused {
                    put(buf, r.x, r.y, &self.placeholder, st.fg(t.muted).add_modifier(Modifier::ITALIC), r.width as usize);
                }
                if self.kind.is_list() && self.input.is_empty() && focused {
                    let hint = match self.kind {
                        EdKind::Select | EdKind::Combo => self.display(),
                        _ => String::new(),
                    };
                    if !hint.is_empty() {
                        put(buf, r.x + 1, r.y, &hint, st.fg(t.muted).add_modifier(Modifier::ITALIC), (r.width as usize).saturating_sub(1));
                    }
                }
            }
        }
    }

    /// Popups (list, calendar, error) anchored under/over `anchor`, kept on `screen`.
    pub fn render_popup(&mut self, buf: &mut Buffer, anchor: Rect, screen: Rect, t: &Theme) {
        self.popup = Rect::default();
        self.list_rows = Rect::default();
        self.cal_rect = Rect::default();
        let mut below_y = anchor.bottom();
        if let Some(err) = &self.error {
            let w = (width(err) + 4).min(screen.width as usize) as u16;
            let r = Rect::new(anchor.x.min(screen.right().saturating_sub(w)), below_y.min(screen.bottom().saturating_sub(1)), w, 1);
            fill(buf, r, Style::new().bg(t.err).fg(t.bg));
            put(buf, r.x + 1, r.y, &format!("✗ {err}"), Style::new().bg(t.err).fg(t.bg).add_modifier(Modifier::BOLD), w as usize - 1);
            below_y += 1;
        }
        let create = self.create_row();
        let show_list = self.kind.is_list()
            && self.list_open
            && self.list_len() > 0
            && !(self.kind == EdKind::Suggest && self.input.is_empty() && !self.navigated && self.filtered.len() == self.choices.len() && self.choices.len() > 12);
        if show_list {
            let longest = self
                .filtered
                .iter()
                .map(|(i, _)| width(&self.choices[*i].label) + width(&self.choices[*i].detail) + 6)
                .max()
                .unwrap_or(10);
            let w = (longest.max(anchor.width as usize).max(24) as u16).min(screen.width).min(70);
            let rows = self.list_len().min(10) as u16;
            let h = rows + 2;
            let space_below = screen.bottom().saturating_sub(below_y);
            let y = if space_below >= h || space_below >= anchor.y.saturating_sub(screen.y) {
                below_y
            } else {
                anchor.y.saturating_sub(h)
            };
            let x = anchor.x.min(screen.right().saturating_sub(w));
            let r = Rect::new(x, y, w, h.min(screen.bottom().saturating_sub(y)));
            shadow(buf, r);
            let title = format!("{} of {}", self.filtered.len(), self.choices.len());
            let inner = frame(buf, r, &title, t, true);
            self.popup = r;
            self.list_rows = inner;
            let vis = inner.height as usize;
            if self.sel < self.scroll {
                self.scroll = self.sel;
            } else if self.sel >= self.scroll + vis {
                self.scroll = self.sel + 1 - vis;
            }
            for (row, (i, hits)) in self.filtered.iter().enumerate().skip(self.scroll).take(vis) {
                let c = &self.choices[*i];
                let yy = inner.y + (row - self.scroll) as u16;
                let selected = row == self.sel;
                let bg = if selected { t.cur_bg } else { t.panel };
                fill(buf, Rect::new(inner.x, yy, inner.width, 1), Style::new().bg(bg));
                let base = Style::new().bg(bg).fg(t.fg).patch(t.cell(&c.style));
                let glyph = Theme::glyph(&c.style).unwrap_or("");
                let gx = put(buf, inner.x + 1, yy, glyph, base, 2);
                let dw = width(&c.detail);
                let lw = (inner.width as usize).saturating_sub(dw + 3 + gx as usize);
                put_highlighted(buf, inner.x + 1 + gx, yy, &c.label, hits, base, Style::new().fg(t.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED), lw);
                if dw > 0 {
                    put(buf, inner.right().saturating_sub(dw as u16 + 1), yy, &c.detail, Style::new().bg(bg).fg(t.muted), dw);
                }
            }
            let crow = self.filtered.len();
            if create && crow >= self.scroll && crow < self.scroll + vis {
                let yy = inner.y + (crow - self.scroll) as u16;
                let bg = if self.sel == crow { t.cur_bg } else { t.panel };
                fill(buf, Rect::new(inner.x, yy, inner.width, 1), Style::new().bg(bg));
                let label = format!("✚ Create '{}'", self.input.text().trim());
                put(buf, inner.x + 1, yy, &label, Style::new().bg(bg).fg(t.ok), inner.width as usize - 2);
            }
        }
        if self.kind == EdKind::Date {
            let w = 24u16;
            let h = 11u16;
            let space_below = screen.bottom().saturating_sub(below_y);
            let y = if space_below >= h { below_y } else { anchor.y.saturating_sub(h) };
            let x = anchor.x.min(screen.right().saturating_sub(w));
            let r = Rect::new(x, y, w, h);
            shadow(buf, r);
            let (yy, mm, _) = self.cal.ymd();
            let inner = frame(buf, r, &format!("{} {}", MONTH_NAMES[mm as usize - 1], yy), t, true);
            self.cal_rect = inner;
            self.render_calendar(buf, inner, t);
        }
    }

    fn render_calendar(&self, buf: &mut Buffer, r: Rect, t: &Theme) {
        let head = Style::new().fg(t.muted).bg(t.panel);
        put(buf, r.x + 1, r.y, "Mo Tu We Th Fr Sa Su", head, 21);
        let first = self.cal.first_of_month();
        let start = first.week_start();
        for week in 0..6 {
            for wd in 0..7 {
                let d = start.add_days(week * 7 + wd);
                let x = r.x + 1 + (wd as u16) * 3;
                let y = r.y + 1 + week as u16;
                let mut st = Style::new().bg(t.panel).fg(if d.month() == self.cal.month() { t.fg } else { t.border });
                if wd >= 5 {
                    st = st.fg(t.muted);
                }
                if d == self.today {
                    st = st.fg(t.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
                }
                if d == self.cal {
                    st = st.bg(t.cur_bg).fg(t.fg).add_modifier(Modifier::BOLD);
                }
                put(buf, x, y, &format!("{:>2}", d.day()), st, 2);
            }
        }
        // preview of what the typed text means
        let txt = self.input.text();
        let (msg, st) = match parse_natural(&txt, self.today) {
            Ok(Some(d)) => (format!("→ {}", d.long()), Style::new().fg(t.ok)),
            Ok(None) => ("(empty)".to_string(), Style::new().fg(t.muted)),
            Err(_) => ("?".to_string(), Style::new().fg(t.err)),
        };
        put(buf, r.x + 1, r.bottom().saturating_sub(2), &pad(&msg, 21, false), st.bg(t.panel), 21);
        put(buf, r.x + 1, r.bottom().saturating_sub(1), "↑↓ wk PgUp/Dn mo", Style::new().fg(t.muted).bg(t.panel), 21);
    }

    /// Mouse click inside a popup: picks a list row or a calendar day. True if handled.
    pub fn click(&mut self, x: u16, y: u16) -> Option<EdOut> {
        if rect_contains(self.list_rows, x, y) {
            let row = self.scroll + (y - self.list_rows.y) as usize;
            if row < self.list_len() {
                self.sel = row;
                self.navigated = true;
                return Some(match self.kind {
                    EdKind::Chips { .. } => {
                        self.add_chip();
                        EdOut::Consumed
                    }
                    EdKind::Suggest => {
                        let label = self.choices[self.filtered[row].0].label.clone();
                        self.input.set(&label);
                        EdOut::Commit(Nav::Stay)
                    }
                    _ => {
                        self.choose();
                        EdOut::Commit(Nav::Stay)
                    }
                });
            }
        }
        if rect_contains(self.cal_rect, x, y) && y > self.cal_rect.y && y <= self.cal_rect.y + 6 && x > self.cal_rect.x {
            let week = (y - self.cal_rect.y - 1) as i32;
            let wd = ((x - self.cal_rect.x - 1) / 3).min(6) as i32;
            let d = self.cal.first_of_month().week_start().add_days(week * 7 + wd);
            self.set_date(d);
            return Some(EdOut::Commit(Nav::Stay));
        }
        if rect_contains(self.popup, x, y) {
            return Some(EdOut::Consumed);
        }
        None
    }

    pub fn scroll_list(&mut self, down: bool) {
        self.move_sel(if down { 3 } else { -3 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pt_core::grid::Style as S;
    use serde_json::json;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }
    fn today() -> Date {
        Date::parse_iso("2026-10-17").unwrap()
    }
    fn ch(v: Value, l: &str) -> Choice {
        Choice { value: v, label: l.into(), detail: String::new(), style: S::Plain }
    }

    #[test]
    fn select_filters_and_commits() {
        let mut e = FieldEditor::list(EdKind::Select, vec![ch(json!("todo"), "To do"), ch(json!("done"), "Done")], json!("todo"), "", today()).open();
        e.seed('d');
        assert_eq!(e.handle_key(&k(KeyCode::Enter)), EdOut::Commit(Nav::Down));
        assert_eq!(e.value(), EdValue::Value(json!("done")));
    }

    #[test]
    fn closed_select_cycles() {
        let mut e = FieldEditor::list(EdKind::Select, vec![ch(json!(1), "a"), ch(json!(2), "b")], json!(1), "", today());
        assert_eq!(e.handle_key(&k(KeyCode::Down)), EdOut::Consumed);
        assert_eq!(e.value(), EdValue::Value(json!(2)));
        assert_eq!(e.handle_key(&k(KeyCode::Enter)), EdOut::Consumed);
        assert!(e.list_open);
    }

    #[test]
    fn chips_add_and_remove() {
        let mut e = FieldEditor::chips(vec![ch(json!("retail"), "retail"), ch(json!("q4"), "q4")], &[json!("q4")], true, today());
        for c in "ret".chars() {
            e.handle_key(&k(KeyCode::Char(c)));
        }
        e.handle_key(&k(KeyCode::Enter));
        for c in "brand-new".chars() {
            e.handle_key(&k(KeyCode::Char(c)));
        }
        e.handle_key(&k(KeyCode::Enter));
        assert_eq!(e.value(), EdValue::Value(json!(["q4", "retail", "brand-new"])));
        e.handle_key(&k(KeyCode::Backspace));
        assert_eq!(e.value(), EdValue::Value(json!(["q4", "retail"])));
        assert_eq!(e.handle_key(&k(KeyCode::Enter)), EdOut::Commit(Nav::Down));
    }

    #[test]
    fn date_keys() {
        let mut e = FieldEditor::text(EdKind::Date, "2026-10-17", today());
        e.handle_key(&k(KeyCode::Down));
        assert_eq!(e.value(), EdValue::Text("2026-10-24".into()));
        e.input.set("blah");
        assert_eq!(e.handle_key(&k(KeyCode::Enter)), EdOut::Consumed);
        assert!(e.error.is_some());
        e.input.set("fri");
        assert_eq!(e.handle_key(&k(KeyCode::Enter)), EdOut::Commit(Nav::Down));
    }

    #[test]
    fn number_steps() {
        let mut e = FieldEditor::text(EdKind::Number, "2", today());
        e.handle_key(&k(KeyCode::Up));
        e.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
        assert_eq!(e.value(), EdValue::Text("3.25".into()));
    }
}
