//! Multi-line editor with word wrap (tab 7): logical lines of chars, drawn
//! as soft-wrapped visual rows. Up/Down/PgUp/PgDn move by visual rows, so
//! they use the width of the last render.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::draw::put_clip;

#[derive(Clone, Debug)]
pub struct TextArea {
    lines: Vec<Vec<char>>,
    pub row: usize,
    pub col: usize,
    top: usize,
    width: usize,
    height: usize,
}

/// One visual row: logical line, first char, end char (exclusive).
type Seg = (usize, usize, usize);

impl Default for TextArea {
    fn default() -> Self {
        TextArea::new("")
    }
}

impl TextArea {
    pub fn new(s: &str) -> TextArea {
        let mut t = TextArea {
            lines: vec![],
            row: 0,
            col: 0,
            top: 0,
            width: 60,
            height: 8,
        };
        t.set(s);
        t
    }

    /// Replace the text; the cursor goes to the start.
    pub fn set(&mut self, s: &str) {
        self.lines = s
            .split('\n')
            .map(|l| {
                l.chars()
                    .filter(|c| !c.is_control() || *c == '\t')
                    .collect()
            })
            .collect();
        if self.lines.is_empty() {
            self.lines.push(vec![]);
        }
        self.row = 0;
        self.col = 0;
        self.top = 0;
    }

    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn char_count(&self) -> usize {
        self.lines.iter().map(Vec::len).sum::<usize>() + self.lines.len() - 1
    }

    pub fn word_count(&self) -> usize {
        self.text().split_whitespace().count()
    }

    pub fn insert_str(&mut self, s: &str) {
        for ch in s.chars() {
            match ch {
                '\n' => self.newline(),
                '\r' => {}
                c if c.is_control() && c != '\t' => {}
                c => {
                    self.lines[self.row].insert(self.col, c);
                    self.col += 1;
                }
            }
        }
    }

    fn newline(&mut self) {
        let rest = self.lines[self.row].split_off(self.col);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
    }

    /// Word-wrap every line to `w` columns (one char = one column).
    fn segments(&self, w: usize) -> Vec<Seg> {
        let w = w.max(2);
        let mut out = vec![];
        for (i, l) in self.lines.iter().enumerate() {
            let mut start = 0;
            loop {
                if l.len() - start <= w {
                    out.push((i, start, l.len()));
                    break;
                }
                // break after the last space that fits, else hard-break
                let window = &l[start..start + w];
                let cut = window.iter().rposition(|c| *c == ' ').map_or(w, |p| p + 1);
                out.push((i, start, start + cut));
                start += cut;
            }
        }
        out
    }

    /// Index of the visual row holding the cursor.
    fn cursor_seg(&self, segs: &[Seg]) -> usize {
        let mut best = 0;
        for (i, &(l, s, e)) in segs.iter().enumerate() {
            if l == self.row && self.col >= s {
                best = i;
                // the cursor at a soft break belongs to the next row
                if self.col < e {
                    break;
                }
            }
        }
        best
    }

    fn move_rows(&mut self, delta: isize) {
        let segs = self.segments(self.width);
        let cur = self.cursor_seg(&segs);
        let x = self.col - segs[cur].1;
        let target = (cur as isize + delta).clamp(0, segs.len() as isize - 1) as usize;
        let (l, s, e) = segs[target];
        self.row = l;
        // stay before a soft break so the cursor doesn't jump to the next row
        let last_of_line = segs.get(target + 1).is_none_or(|n| n.0 != l);
        let max = if last_of_line {
            e
        } else {
            e.saturating_sub(1).max(s)
        };
        self.col = (s + x).min(max);
    }

    /// Returns true if the key was used.
    pub fn handle(&mut self, k: &KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.height.max(2) as isize - 1;
        match k.code {
            KeyCode::Enter => self.newline(),
            KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => {
                self.lines[self.row].insert(self.col, c);
                self.col += 1;
            }
            KeyCode::Backspace => {
                if self.col > 0 {
                    self.col -= 1;
                    self.lines[self.row].remove(self.col);
                } else if self.row > 0 {
                    let cur = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                    self.lines[self.row].extend(cur);
                }
            }
            KeyCode::Delete => {
                if self.col < self.lines[self.row].len() {
                    self.lines[self.row].remove(self.col);
                } else if self.row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.row + 1);
                    self.lines[self.row].extend(next);
                }
            }
            KeyCode::Left => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                }
            }
            KeyCode::Right => {
                if self.col < self.lines[self.row].len() {
                    self.col += 1;
                } else if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = 0;
                }
            }
            KeyCode::Up => self.move_rows(-1),
            KeyCode::Down => self.move_rows(1),
            KeyCode::PageUp => self.move_rows(-page),
            KeyCode::PageDown => self.move_rows(page),
            KeyCode::Home if ctrl => {
                self.row = 0;
                self.col = 0;
            }
            KeyCode::End if ctrl => {
                self.row = self.lines.len() - 1;
                self.col = self.lines[self.row].len();
            }
            KeyCode::Home => self.col = 0,
            KeyCode::End => self.col = self.lines[self.row].len(),
            _ => return false,
        }
        true
    }

    /// Put the cursor at a visual position (mouse click) inside the last render.
    pub fn click(&mut self, dx: u16, dy: u16) {
        let segs = self.segments(self.width);
        let i = (self.top + dy as usize).min(segs.len() - 1);
        let (l, s, e) = segs[i];
        self.row = l;
        self.col = (s + dx as usize).min(e);
    }

    /// (line, column), both 1-based, for the footer.
    pub fn position(&self) -> (usize, usize) {
        (self.row + 1, self.col + 1)
    }

    pub fn render(&mut self, buf: &mut Buffer, r: Rect, style: Style, focused: bool) {
        let h = r.height as usize;
        if h == 0 || r.width < 2 {
            return;
        }
        self.width = r.width as usize - 1; // leave the last column for the cursor
        self.height = h;
        let segs = self.segments(self.width);
        let cur = self.cursor_seg(&segs);
        if cur < self.top {
            self.top = cur;
        } else if cur >= self.top + h {
            self.top = cur + 1 - h;
        }
        self.top = self.top.min(segs.len().saturating_sub(1));
        for i in 0..h {
            let y = r.y + i as u16;
            for x in r.x..r.right() {
                buf[(x, y)].set_symbol(" ").set_style(style);
            }
            if let Some(&(l, s, e)) = segs.get(self.top + i) {
                let text: String = self.lines[l][s..e]
                    .iter()
                    .map(|c| if *c == '\t' { ' ' } else { *c })
                    .collect();
                put_clip(buf, r.x, y, &text, style, r.width as usize);
            }
        }
        // scroll marker
        if segs.len() > h {
            let pos = (self.top * (h - 1)) / (segs.len() - h).max(1);
            put_clip(
                buf,
                r.right() - 1,
                r.y + pos.min(h - 1) as u16,
                "▐",
                style.add_modifier(Modifier::DIM),
                1,
            );
        }
        if focused && cur >= self.top && cur < self.top + h {
            let x = r.x + (self.col - segs[cur].1).min(r.width as usize - 1) as u16;
            let y = r.y + (cur - self.top) as u16;
            if let Some(c) = buf.cell_mut((x, y)) {
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
    fn editing_and_counts() {
        let mut a = TextArea::new("one");
        a.handle(&key(KeyCode::End));
        a.handle(&key(KeyCode::Enter));
        a.insert_str("two words");
        assert_eq!(a.text(), "one\ntwo words");
        assert_eq!(a.position(), (2, 10));
        assert_eq!(a.word_count(), 3);
        assert_eq!(a.char_count(), 13);
        a.handle(&key(KeyCode::Home));
        a.handle(&key(KeyCode::Backspace));
        assert_eq!(a.text(), "onetwo words");
        a.handle(&KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
        assert_eq!(a.position(), (1, 13));
    }

    #[test]
    fn wraps_words() {
        let a = TextArea::new("the quick brown fox");
        let segs = a.segments(10);
        assert_eq!(segs, vec![(0, 0, 10), (0, 10, 19)]);
        assert_eq!(
            a.segments(6),
            vec![(0, 0, 4), (0, 4, 10), (0, 10, 16), (0, 16, 19)]
        );
        let mut a = TextArea::new("the quick brown fox");
        a.width = 6;
        a.handle(&key(KeyCode::Down));
        assert_eq!((a.row, a.col), (0, 4));
        a.handle(&key(KeyCode::End));
        a.handle(&key(KeyCode::Up));
        assert_eq!((a.row, a.col), (0, 13), "same x on the row above");
    }
}
