//! Single-line text input (from pt-tui), with optional masking.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthChar;

use super::draw::put_clip;

#[derive(Clone, Debug, Default)]
pub struct TextInput {
    chars: Vec<char>,
    pub cursor: usize,
    pub mask: bool,
    scroll: usize,
}

impl TextInput {
    pub fn new(s: &str) -> TextInput {
        let chars: Vec<char> = s.chars().collect();
        TextInput {
            cursor: chars.len(),
            chars,
            mask: false,
            scroll: 0,
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn set(&mut self, s: &str) {
        self.chars = s.chars().collect();
        self.cursor = self.chars.len();
        self.scroll = 0;
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars().filter(|c| !c.is_control()) {
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
        }
    }

    fn word_left(&self) -> usize {
        let mut i = self.cursor;
        while i > 0 && !self.chars[i - 1].is_alphanumeric() {
            i -= 1;
        }
        while i > 0 && self.chars[i - 1].is_alphanumeric() {
            i -= 1;
        }
        i
    }

    fn word_right(&self) -> usize {
        let mut i = self.cursor;
        let n = self.chars.len();
        while i < n && !self.chars[i].is_alphanumeric() {
            i += 1;
        }
        while i < n && self.chars[i].is_alphanumeric() {
            i += 1;
        }
        i
    }

    /// Returns true if the key changed the text or cursor.
    pub fn handle(&mut self, k: &KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Char('u') if ctrl => self.clear(),
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('w') if ctrl => {
                let from = self.word_left();
                self.chars.drain(from..self.cursor);
                self.cursor = from;
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                if ctrl || alt {
                    let from = self.word_left();
                    self.chars.drain(from..self.cursor);
                    self.cursor = from;
                } else {
                    self.cursor -= 1;
                    self.chars.remove(self.cursor);
                }
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Left if ctrl || alt => self.cursor = self.word_left(),
            KeyCode::Right if ctrl || alt => self.cursor = self.word_right(),
            KeyCode::Left if self.cursor > 0 => self.cursor -= 1,
            KeyCode::Right if self.cursor < self.chars.len() => self.cursor += 1,
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            _ => return false,
        }
        true
    }

    /// Draw into one row of `r`; the cursor is shown as a reversed cell when `focused`.
    pub fn render(&mut self, buf: &mut Buffer, r: Rect, style: Style, focused: bool) {
        if r.width == 0 || r.height == 0 {
            return;
        }
        let w = r.width as usize;
        // keep the cursor in view
        let cw = |c: &char| c.width().unwrap_or(1);
        let before: usize = self.chars[self.scroll.min(self.cursor)..self.cursor]
            .iter()
            .map(cw)
            .sum();
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if before >= w {
            let mut s = self.cursor;
            let mut used = 1;
            while s > 0 && used + cw(&self.chars[s - 1]) < w {
                s -= 1;
                used += cw(&self.chars[s]);
            }
            self.scroll = s;
        }
        let visible: String = if self.mask {
            "•".repeat(self.chars.len().saturating_sub(self.scroll))
        } else {
            self.chars[self.scroll..].iter().collect()
        };
        for x in r.x..r.right() {
            buf[(x, r.y)].set_symbol(" ").set_style(style);
        }
        put_clip(buf, r.x, r.y, &visible, style, w);
        if focused {
            let off: usize = if self.mask {
                self.cursor - self.scroll
            } else {
                self.chars[self.scroll..self.cursor].iter().map(cw).sum()
            };
            let x = r.x + off.min(w - 1) as u16;
            if let Some(c) = buf.cell_mut((x, r.y)) {
                c.set_style(style.add_modifier(Modifier::REVERSED));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn editing() {
        let mut t = TextInput::new("hello");
        t.handle(&key(KeyCode::Home));
        t.handle(&key(KeyCode::Char('>')));
        t.handle(&key(KeyCode::End));
        t.handle(&key(KeyCode::Backspace));
        assert_eq!(t.text(), ">hell");
        t.handle(&KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
        assert_eq!(t.text(), ">");
        t.insert_str("a\nb");
        assert_eq!(t.text(), ">ab");
        t.handle(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(t.is_empty());
    }
}
