//! Tab 14 — Charts: `charts.series` (12 months) drawn as eighth-block bars
//! with value labels, a sparkline, a gauge of the average and a heat strip.
//! ←/→ select a month (highlighted in all four); ↑/↓ and PgUp/PgDn change
//! it, each change committing the series.

use crossterm::event::{KeyCode, KeyEvent};
use demo_core::dataset::CHART_MONTHS;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use serde_json::json;

use super::{h, is_focused, muted};
use crate::app::{App, Hint, Hit};
use crate::theme::{Theme, rgb};
use crate::widgets::draw::{bar, put};

pub const KEYS: &[(&str, &str)] = &[
    ("←→ Home End", "select a month"),
    ("↑↓", "±5"),
    ("PgUp PgDn", "±20"),
    ("click", "select a bar"),
];

const EIGHTHS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

#[derive(Default)]
pub struct State {
    pub month: usize,
}

pub fn series(app: &App) -> Vec<i64> {
    let mut s: Vec<i64> = app.shown_get("charts.series").unwrap_or_default();
    s.resize(12, 0);
    s
}

fn change(app: &mut App, d: i64) {
    let mut s = series(app);
    let m = app.tabs.charts.month;
    let n = (s[m] + d).clamp(0, 100);
    if n != s[m] {
        s[m] = n;
        app.commit("charts.series", json!(s));
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let m = &mut app.tabs.charts.month;
    match k.code {
        KeyCode::Left => *m = (*m + 11) % 12,
        KeyCode::Right => *m = (*m + 1) % 12,
        KeyCode::Home => *m = 0,
        KeyCode::End => *m = 11,
        KeyCode::Up => change(app, 5),
        KeyCode::Down => change(app, -5),
        KeyCode::PageUp => change(app, 20),
        KeyCode::PageDown => change(app, -20),
        _ => return false,
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    if let Hit::Month(m) = hit {
        app.focus_ctrl(0);
        app.tabs.charts.month = *m;
        return true;
    }
    false
}

pub fn hints(_app: &App) -> Vec<Hint> {
    vec![
        h("←→", "Month", KeyCode::Right),
        h("↑↓", "±5", KeyCode::Up),
        h("PgUp/PgDn", "±20", KeyCode::PageUp),
    ]
}

/// Green < 60 ≤ amber < 80 ≤ red.
pub fn level_color(v: f64, t: &Theme) -> Color {
    if v < 60.0 {
        t.ok
    } else if v < 80.0 {
        t.warn
    } else {
        t.err
    }
}

/// Heat colour: cool blue (0) → amber (50) → red (100).
fn heat(v: i64) -> Color {
    let lerp = |a: u32, b: u32, f: f64| -> u32 {
        let ch = |s: u32| ((a >> s) & 0xff) as f64 * (1.0 - f) + ((b >> s) & 0xff) as f64 * f;
        ((ch(16) as u32) << 16) | ((ch(8) as u32) << 8) | ch(0) as u32
    };
    let v = v.clamp(0, 100) as f64 / 100.0;
    let hex = if v < 0.5 {
        lerp(0x1f3b5c, 0xd9a43b, v * 2.0)
    } else {
        lerp(0xd9a43b, 0xd8404a, (v - 0.5) * 2.0)
    };
    rgb(hex)
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let focused = is_focused(app, 0);
    let s = series(app);
    let sel = app.tabs.charts.month;
    let x = area.x + 1;
    let lx = x + 11; // charts start after a label column
    let bh = area.height.saturating_sub(9).clamp(3, 8);
    let base = Style::new().bg(t.bg);
    let label = |buf: &mut Buffer, y: u16, s: &str| put(buf, x, y, s, muted(&t), 10);
    // bars: 3 wide, 2 apart; the top row holds the value labels
    label(buf, area.y + 1, "Bars");
    let bottom = area.y + bh; // last bar row
    for (m, v) in s.iter().enumerate() {
        let bx = lx + m as u16 * 5;
        let is_sel = m == sel;
        let col = if is_sel {
            if focused { t.accent2 } else { t.accent }
        } else {
            t.info
        };
        let eighths = (*v as f64 / 100.0 * (bh as f64) * 8.0).round() as i64;
        for row in 0..bh {
            let y = bottom - row;
            let n = (eighths - row as i64 * 8).clamp(0, 8) as usize;
            for dx in 0..3 {
                put(buf, bx + dx, y, EIGHTHS[n], base.fg(col), 1);
            }
        }
        let top = bottom.saturating_sub((eighths as u16).div_ceil(8));
        let vs = Style::new()
            .fg(if is_sel { t.fg } else { t.muted })
            .bg(t.bg)
            .add_modifier(if is_sel {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        put(buf, bx, top.max(area.y), &format!("{v:>3}"), vs, 3);
        let ms = if is_sel {
            Style::new().fg(t.bg).bg(col).add_modifier(Modifier::BOLD)
        } else {
            muted(&t)
        };
        put(buf, bx, bottom + 1, CHART_MONTHS[m], ms, 3);
        app.hits
            .push((Rect::new(bx, area.y, 4, bh + 2), Hit::Month(m)));
    }
    app.key_rects
        .push(("charts.series", Rect::new(lx, bottom + 1, 60, 1)));
    // sparkline
    let y = bottom + 3;
    label(buf, y, "Sparkline");
    for (m, v) in s.iter().enumerate() {
        let n = ((*v as f64 / 100.0) * 7.0).round() as usize + 1;
        let st = if m == sel {
            base.fg(t.accent2).add_modifier(Modifier::BOLD)
        } else {
            base.fg(t.info)
        };
        put(
            buf,
            lx + m as u16 * 2,
            y,
            &EIGHTHS[n.min(8)].repeat(2),
            st,
            2,
        );
        app.hits
            .push((Rect::new(lx + m as u16 * 2, y, 2, 1), Hit::Month(m)));
    }
    put(
        buf,
        lx + 26,
        y,
        &format!(
            "min {}  max {}",
            s.iter().min().unwrap_or(&0),
            s.iter().max().unwrap_or(&0)
        ),
        muted(&t),
        20,
    );
    // gauge of the average
    let y = y + 2;
    label(buf, y, "Gauge");
    let avg = s.iter().sum::<i64>() as f64 / 12.0;
    let gc = level_color(avg, &t);
    bar(buf, lx, y, 40, avg, gc, t.panel);
    put(
        buf,
        lx + 41,
        y,
        &format!("avg {avg:.1}"),
        base.fg(gc).add_modifier(Modifier::BOLD),
        10,
    );
    put(
        buf,
        lx + 52,
        y,
        "green < 60 ≤ amber < 80 ≤ red",
        muted(&t),
        area.right().saturating_sub(lx + 52) as usize,
    );
    // heat strip
    let y = y + 2;
    if y < area.bottom() {
        label(buf, y, "Heat");
        for (m, v) in s.iter().enumerate() {
            let cx = lx + m as u16 * 5;
            let st =
                Style::new()
                    .bg(heat(*v))
                    .fg(if *v >= 50 { Color::Black } else { Color::White });
            let text = format!("{}{v:>3} ", if m == sel { '▸' } else { ' ' });
            put(
                buf,
                cx,
                y,
                &text,
                if m == sel {
                    st.add_modifier(Modifier::BOLD)
                } else {
                    st
                },
                5,
            );
            app.hits.push((Rect::new(cx, y, 5, 1), Hit::Month(m)));
        }
    }
    let y = y + 1;
    if y < area.bottom() {
        put(
            buf,
            lx,
            y,
            &format!("{}: {}", CHART_MONTHS[sel], s[sel]),
            base.fg(t.fg).add_modifier(Modifier::BOLD),
            12,
        );
    }
    app.hits
        .push((Rect::new(x, area.y, 10, area.height), Hit::Ctrl(0)));
}
