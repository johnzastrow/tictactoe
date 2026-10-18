//! Popup lists: the open part of a dropdown (tabs 4, 9, the grid's status
//! editor), the combobox's fuzzy list and the tag suggestions. [`Drop`] is a
//! dropdown's open/filter/selection state; [`render`] draws any list of rows.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::draw::{fill, frame, put, put_highlighted, shadow, width};
use crate::theme::Theme;

/// One row of a popup list.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub label: String,
    /// Char indices of `label` to highlight (fuzzy matches).
    pub hits: Vec<usize>,
    /// A coloured `●` before the label.
    pub color: Option<Color>,
    /// Dimmed text after the label.
    pub dim: String,
}

impl Row {
    pub fn new(label: impl Into<String>) -> Row {
        Row {
            label: label.into(),
            ..Default::default()
        }
    }
}

/// A dropdown's open state: typed filter and selection (an index into the
/// *filtered* list).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drop {
    pub open: bool,
    pub filter: String,
    pub sel: usize,
}

pub enum DropOut {
    /// Key used, nothing picked.
    Used,
    /// Picked this index of the original list.
    Pick(usize),
    /// Closed without picking.
    Closed,
    /// Not a list key.
    Ignored,
}

impl Drop {
    /// Open with the selection on `current` (an index of the original list).
    pub fn open_at(&mut self, current: Option<usize>) {
        self.open = true;
        self.filter.clear();
        self.sel = current.unwrap_or(0);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.filter.clear();
    }

    /// Indices of the original `labels` that match the filter (substring,
    /// case-insensitive), in order.
    pub fn visible(&self, labels: &[&str]) -> Vec<usize> {
        let f = self.filter.to_lowercase();
        labels
            .iter()
            .enumerate()
            .filter(|(_, l)| l.to_lowercase().contains(&f))
            .map(|(i, _)| i)
            .collect()
    }

    /// Keys of an open dropdown: ↑/↓ move, typing filters (and jumps to the
    /// first label starting with the text), Enter picks, Esc closes.
    pub fn key(&mut self, labels: &[&str], k: &KeyEvent) -> DropOut {
        let vis = self.visible(labels);
        let n = vis.len();
        match k.code {
            KeyCode::Esc => {
                self.close();
                return DropOut::Closed;
            }
            KeyCode::Up => self.sel = if n == 0 { 0 } else { (self.sel + n - 1) % n },
            KeyCode::Down => self.sel = if n == 0 { 0 } else { (self.sel + 1) % n },
            KeyCode::Home | KeyCode::PageUp => self.sel = 0,
            KeyCode::End | KeyCode::PageDown => self.sel = n.saturating_sub(1),
            KeyCode::Enter | KeyCode::Char(' ')
                if self.filter.is_empty() || k.code == KeyCode::Enter =>
            {
                if let Some(&i) = vis.get(self.sel) {
                    self.close();
                    return DropOut::Pick(i);
                }
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.sel = 0;
            }
            KeyCode::Char(c)
                if !k
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.filter.push(c);
                let f = self.filter.to_lowercase();
                let vis = self.visible(labels);
                self.sel = vis
                    .iter()
                    .position(|&i| labels[i].to_lowercase().starts_with(&f))
                    .unwrap_or(0);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.close();
                return DropOut::Ignored;
            }
            _ => return DropOut::Ignored,
        }
        DropOut::Used
    }
}

/// Draw a framed list at `r` (outer size; it scrolls to keep `sel` in view).
/// Returns each visible row's rectangle with its index into `rows`.
pub fn render(
    buf: &mut Buffer,
    r: Rect,
    title: &str,
    rows: &[Row],
    sel: Option<usize>,
    t: &Theme,
) -> Vec<(Rect, usize)> {
    let r = r.intersection(buf.area);
    if r.height < 3 || r.width < 6 {
        return vec![];
    }
    shadow(buf, r);
    let inner = frame(buf, r, title, t, true);
    let vis = inner.height as usize;
    let off = match sel {
        Some(s) if s >= vis => s + 1 - vis,
        _ => 0,
    };
    let mut hits = vec![];
    if rows.is_empty() {
        put(
            buf,
            inner.x + 1,
            inner.y,
            "no matches",
            Style::new().fg(t.muted).bg(t.panel),
            inner.width as usize - 1,
        );
    }
    for (row, (i, item)) in rows.iter().enumerate().skip(off).take(vis).enumerate() {
        let y = inner.y + row as u16;
        let selected = sel == Some(i);
        let st = if selected {
            Style::new().bg(t.cur_bg).fg(t.fg)
        } else {
            t.panel()
        };
        fill(buf, Rect::new(inner.x, y, inner.width, 1), st);
        let mut x = inner.x + 1;
        if let Some(c) = item.color {
            put(buf, x, y, "●", st.fg(c), 1);
            x += 2;
        }
        let room = inner.right().saturating_sub(x + 1) as usize;
        let lw = width(&item.label).min(room);
        put_highlighted(
            buf,
            x,
            y,
            &item.label,
            &item.hits,
            st,
            Style::new()
                .fg(t.accent)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            room,
        );
        if !item.dim.is_empty() && lw + 2 < room {
            put(
                buf,
                x + lw as u16 + 2,
                y,
                &item.dim,
                st.fg(t.muted),
                room - lw - 2,
            );
        }
        hits.push((Rect::new(inner.x, y, inner.width, 1), i));
    }
    if rows.len() > vis {
        let more = format!(" {}/{} ", sel.map_or(0, |s| s + 1), rows.len());
        put(
            buf,
            r.right().saturating_sub(width(&more) as u16 + 2),
            r.bottom() - 1,
            &more,
            Style::new().fg(t.muted).bg(t.panel),
            12,
        );
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn filter_jump_pick() {
        let labels = ["To do", "In progress", "Blocked", "Review", "Done"];
        let mut d = Drop::default();
        d.open_at(Some(1));
        assert!(matches!(d.key(&labels, &k(KeyCode::Down)), DropOut::Used));
        assert_eq!(d.sel, 2);
        d.key(&labels, &k(KeyCode::Char('d')));
        // "d" matches To do, Blocked, Done; jumps to Done (starts with d)
        assert_eq!(d.visible(&labels), vec![0, 2, 4]);
        assert_eq!(d.sel, 2);
        let DropOut::Pick(i) = d.key(&labels, &k(KeyCode::Enter)) else {
            panic!()
        };
        assert_eq!(i, 4);
        assert!(!d.open);
    }
}
