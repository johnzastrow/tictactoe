//! The command palette (ui-spec §3): every action with its key, fuzzy
//! filtered (protocol §7). The same state and drawing serve the ^K overlay
//! and tab 12, where it is shown inline and always open.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::dataset::TABS;
use demo_core::fuzzy::{self, Match};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::Hit;
use crate::theme::{PALETTES, Theme};
use crate::widgets::draw::{fill, put, put_highlighted, width};
use crate::widgets::input::TextInput;

/// Everything the menus, palette and hints can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    GoTo(usize),
    Next,
    Prev,
    ResetTab,
    ResetAll,
    Theme(usize),
    ToggleKeys,
    ToggleValue,
    Help,
    About,
    MenuBar,
    Quit,
}

impl Cmd {
    pub fn label(self) -> String {
        match self {
            Cmd::GoTo(i) => format!("Go to: {}", TABS[i].title),
            Cmd::Next => "Next tab".into(),
            Cmd::Prev => "Previous tab".into(),
            Cmd::ResetTab => "Reset this tab".into(),
            Cmd::ResetAll => "Reset all tabs".into(),
            Cmd::Theme(i) => format!("Theme: {}", PALETTES[i].name),
            Cmd::ToggleKeys => "Toggle Keys panel".into(),
            Cmd::ToggleValue => "Toggle Value panel".into(),
            Cmd::Help => "Help".into(),
            Cmd::About => "About".into(),
            Cmd::MenuBar => "Open menu bar".into(),
            Cmd::Quit => "Quit".into(),
        }
    }

    pub fn key(self) -> String {
        match self {
            Cmd::GoTo(i) if i < 9 => format!("Alt+{}", i + 1),
            Cmd::GoTo(9) => "Alt+0".into(),
            Cmd::GoTo(_) => String::new(),
            Cmd::Next => "^PgDn".into(),
            Cmd::Prev => "^PgUp".into(),
            Cmd::ResetTab => "^R".into(),
            Cmd::Theme(_) => "^T cycles".into(),
            Cmd::Help => "F1".into(),
            Cmd::MenuBar => "F10".into(),
            Cmd::Quit => "^Q".into(),
            _ => String::new(),
        }
    }
}

/// The palette's commands, in display order (ui-spec §3).
pub fn commands() -> Vec<Cmd> {
    let mut v: Vec<Cmd> = (0..TABS.len()).map(Cmd::GoTo).collect();
    v.extend([Cmd::ResetTab, Cmd::ResetAll]);
    v.extend((0..PALETTES.len()).map(Cmd::Theme));
    v.extend([Cmd::Help, Cmd::About, Cmd::MenuBar, Cmd::Quit]);
    v
}

#[derive(Default)]
pub struct PaletteState {
    pub input: TextInput,
    pub sel: usize,
}

pub enum Out {
    Used,
    Ignored,
    Run(Cmd),
    Close,
}

impl PaletteState {
    /// Matching commands, best first: (command, match on its label).
    pub fn matches(&self) -> Vec<(Cmd, Match)> {
        let cmds = commands();
        let labels: Vec<String> = cmds.iter().map(|c| c.label()).collect();
        fuzzy::rank(&self.input.text(), labels.iter().map(String::as_str))
            .into_iter()
            .map(|(i, m)| (cmds[i], m))
            .collect()
    }

    pub fn command_at(&self, row: usize) -> Option<Cmd> {
        self.matches().get(row).map(|(c, _)| *c)
    }

    pub fn key(&mut self, k: &KeyEvent) -> Out {
        let n = self.matches().len();
        match k.code {
            KeyCode::Esc => return Out::Close,
            KeyCode::Up => self.sel = if n == 0 { 0 } else { (self.sel + n - 1) % n },
            KeyCode::Down => self.sel = if n == 0 { 0 } else { (self.sel + 1) % n },
            KeyCode::PageUp => self.sel = self.sel.saturating_sub(8),
            KeyCode::PageDown => self.sel = (self.sel + 8).min(n.saturating_sub(1)),
            KeyCode::Enter => {
                return match self.command_at(self.sel) {
                    Some(c) => Out::Run(c),
                    None => Out::Used,
                };
            }
            KeyCode::Tab | KeyCode::BackTab => return Out::Ignored,
            KeyCode::Char('k' | 'p') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                return Out::Close;
            }
            _ => {
                if self.input.handle(k) {
                    self.sel = 0;
                } else {
                    return Out::Ignored;
                }
            }
        }
        Out::Used
    }

    /// Draw the search box at `r.y` and the list below it inside `r`.
    pub fn render(
        &mut self,
        buf: &mut Buffer,
        r: Rect,
        t: &Theme,
        focused: bool,
        hits: &mut Vec<(Rect, Hit)>,
    ) {
        let bg = Style::new().bg(t.panel).fg(t.fg);
        fill(buf, r, bg);
        put(
            buf,
            r.x + 1,
            r.y,
            "›",
            bg.fg(t.accent).add_modifier(Modifier::BOLD),
            1,
        );
        let ir = Rect::new(r.x + 3, r.y, r.width.saturating_sub(4), 1);
        let empty = self.input.is_empty();
        self.input
            .render(buf, ir, Style::new().bg(t.sel_bg).fg(t.fg), focused);
        if empty {
            put(
                buf,
                ir.x + 1,
                r.y,
                "type to search commands",
                Style::new()
                    .bg(t.sel_bg)
                    .fg(t.muted)
                    .add_modifier(Modifier::ITALIC),
                ir.width as usize - 1,
            );
        }
        let list = self.matches();
        self.sel = self.sel.min(list.len().saturating_sub(1));
        let vis = r.height.saturating_sub(2) as usize;
        let off = if self.sel >= vis {
            self.sel + 1 - vis
        } else {
            0
        };
        if list.is_empty() {
            put(
                buf,
                r.x + 3,
                r.y + 2,
                "no matching command",
                bg.fg(t.muted),
                r.width as usize - 4,
            );
        }
        for (row, (i, (c, m))) in list.iter().enumerate().skip(off).take(vis).enumerate() {
            let y = r.y + 2 + row as u16;
            let sel = i == self.sel;
            let st = if sel {
                Style::new().bg(t.cur_bg).fg(t.fg)
            } else {
                bg
            };
            fill(buf, Rect::new(r.x, y, r.width, 1), st);
            let key = c.key();
            let kw = width(&key) as u16;
            put_highlighted(
                buf,
                r.x + 3,
                y,
                &c.label(),
                &m.indices,
                st,
                Style::new()
                    .fg(t.accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                (r.width.saturating_sub(kw + 6)) as usize,
            );
            put(
                buf,
                r.right().saturating_sub(kw + 2),
                y,
                &key,
                st.fg(t.muted),
                kw as usize,
            );
            hits.push((Rect::new(r.x, y, r.width, 1), Hit::PaletteRow(i)));
        }
        if list.len() > vis && vis > 0 {
            let more = format!(" {}/{} ", self.sel + 1, list.len());
            put(
                buf,
                r.right().saturating_sub(width(&more) as u16 + 1),
                r.y + 1,
                &more,
                bg.fg(t.muted),
                12,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_and_run() {
        assert_eq!(commands().len(), 16 + 2 + 4 + 4);
        let mut p = PaletteState::default();
        for c in "gant".chars() {
            p.key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let Out::Run(c) = p.key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) else {
            panic!()
        };
        assert_eq!(c, Cmd::GoTo(15));
        assert_eq!(c.label(), "Go to: Gantt");
    }
}
