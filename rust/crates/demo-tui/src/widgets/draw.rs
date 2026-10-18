//! Low-level drawing helpers on a ratatui `Buffer`: width-aware text with
//! ellipsis, fills, framed boxes with drop shadows, and key/label hints.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate to `max` columns, ending in `…` when cut.
pub fn fit(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Pad (or cut) to exactly `w` columns; `right` aligns right.
pub fn pad(s: &str, w: usize, right: bool) -> String {
    let t = fit(s, w);
    let n = w.saturating_sub(width(&t));
    if right {
        format!("{}{}", " ".repeat(n), t)
    } else {
        format!("{}{}", t, " ".repeat(n))
    }
}

/// Write `s` at (x, y), clipped to `max` columns and the buffer. Returns columns used.
pub fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style, max: usize) -> u16 {
    let area = buf.area;
    if y < area.y || y >= area.bottom() || x >= area.right() {
        return 0;
    }
    let max = max.min((area.right() - x) as usize);
    let t = fit(s, max);
    let (nx, _) = buf.set_stringn(x, y, &t, max, style);
    nx.saturating_sub(x)
}

/// Like `put` but without the ellipsis (hard clip).
pub fn put_clip(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style, max: usize) -> u16 {
    let area = buf.area;
    if y < area.y || y >= area.bottom() || x >= area.right() {
        return 0;
    }
    let max = max.min((area.right() - x) as usize);
    let (nx, _) = buf.set_stringn(x, y, s, max, style);
    nx.saturating_sub(x)
}

pub fn fill(buf: &mut Buffer, r: Rect, style: Style) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(" ");
                c.set_style(style);
            }
        }
    }
}

/// Apply a style over a region without changing the text.
pub fn restyle(buf: &mut Buffer, r: Rect, style: Style) {
    buf.set_style(r.intersection(buf.area), style);
}

/// Darken the cells right of / below `r` (a drop shadow).
pub fn shadow(buf: &mut Buffer, r: Rect) {
    let sh = Style::new().bg(Color::Black).fg(Color::DarkGray);
    let right = Rect::new(r.right(), r.y + 1, 2, r.height);
    let below = Rect::new(r.x + 2, r.bottom(), r.width, 1);
    for part in [right, below] {
        let p = part.intersection(buf.area);
        for y in p.top()..p.bottom() {
            for x in p.left()..p.right() {
                if let Some(c) = buf.cell_mut((x, y)) {
                    c.set_style(sh);
                }
            }
        }
    }
}

/// A framed panel with a title; returns the inner area.
pub fn frame(buf: &mut Buffer, r: Rect, title: &str, t: &Theme, focused: bool) -> Rect {
    let r = r.intersection(buf.area);
    if r.width < 2 || r.height < 2 {
        return Rect::default();
    }
    fill(buf, r, t.panel());
    let b = Style::new()
        .fg(if focused { t.accent } else { t.border })
        .bg(t.panel);
    let (l, rt, tp, bt) = (r.left(), r.right() - 1, r.top(), r.bottom() - 1);
    for x in l..=rt {
        buf[(x, tp)].set_symbol("─").set_style(b);
        buf[(x, bt)].set_symbol("─").set_style(b);
    }
    for y in tp..=bt {
        buf[(l, y)].set_symbol("│").set_style(b);
        buf[(rt, y)].set_symbol("│").set_style(b);
    }
    buf[(l, tp)].set_symbol("╭");
    buf[(rt, tp)].set_symbol("╮");
    buf[(l, bt)].set_symbol("╰");
    buf[(rt, bt)].set_symbol("╯");
    if !title.is_empty() {
        let ts = Style::new()
            .fg(if focused { t.accent } else { t.fg })
            .bg(t.panel)
            .add_modifier(Modifier::BOLD);
        put(
            buf,
            l + 2,
            tp,
            &format!(" {title} "),
            ts,
            (r.width as usize).saturating_sub(4),
        );
    }
    Rect::new(l + 1, tp + 1, r.width - 2, r.height - 2)
}

/// Modal box: shadow + frame.
pub fn modal(buf: &mut Buffer, r: Rect, title: &str, t: &Theme) -> Rect {
    shadow(buf, r);
    frame(buf, r, title, t, true)
}

/// Centered rect of at most w×h inside `outer`.
pub fn centered(outer: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(outer.width);
    let h = h.min(outer.height);
    Rect::new(
        outer.x + (outer.width - w) / 2,
        outer.y + (outer.height - h) / 2,
        w,
        h,
    )
}

/// Text with some char indices highlighted (fuzzy matches).
#[allow(clippy::too_many_arguments)]
pub fn put_highlighted(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    s: &str,
    hits: &[usize],
    style: Style,
    hl: Style,
    max: usize,
) {
    let mut cx = x;
    let mut used = 0usize;
    let total = width(s);
    for (i, c) in s.chars().enumerate() {
        let cw = c.width().unwrap_or(0);
        if used + cw > max || (total > max && used + cw + 1 > max) {
            if total > max && used < max {
                put(buf, cx, y, "…", style, 1);
            }
            break;
        }
        let st = if hits.contains(&i) {
            style.patch(hl)
        } else {
            style
        };
        let mut tmp = [0u8; 4];
        put(buf, cx, y, c.encode_utf8(&mut tmp), st, cw.max(1));
        cx += cw as u16;
        used += cw;
    }
}

/// Eighth-block progress bar, `pct` of `w` cells.
pub fn bar(buf: &mut Buffer, x: u16, y: u16, w: usize, pct: f64, fg: Color, bg: Color) {
    const PARTS: [&str; 8] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let eighths = ((pct.clamp(0.0, 100.0) / 100.0) * (w * 8) as f64).round() as usize;
    for i in 0..w {
        let n = eighths.saturating_sub(i * 8).min(8);
        let sym = if n == 8 { "█" } else { PARTS[n] };
        if let Some(c) = buf.cell_mut((x + i as u16, y)) {
            c.set_symbol(sym).set_style(Style::new().fg(fg).bg(bg));
        }
    }
}

pub fn rect_contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting() {
        assert_eq!(fit("hello", 10), "hello");
        assert_eq!(fit("hello world", 6), "hello…");
        assert_eq!(pad("ab", 4, true), "  ab");
        assert_eq!(pad("abcdef", 4, false), "abc…");
    }
}
