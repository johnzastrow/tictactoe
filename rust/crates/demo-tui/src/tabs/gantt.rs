//! Tab 16 — Gantt: bar labels on the left, a time axis on the right (ticks,
//! weekend shading, a today line) and `gantt.bars`. Move and resize bars by
//! key or by dragging; zoom between day, week and month scales.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::Date;
use demo_core::date::MONTH_NAMES;
use demo_core::keys::{GanttBar, MAX_BAR_DAYS};
use demo_core::safe_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::json;

use super::{h, is_focused, muted};
use crate::app::{App, Drag, Hint, Hit, hint};
use crate::widgets::draw::{fill, put, width};

pub const KEYS: &[(&str, &str)] = &[
    ("↑↓", "select a bar"),
    ("Shift+←→ < >", "move ±1 day"),
    ("[ ]", "shrink / extend"),
    ("+ -", "zoom day/week/month"),
    ("t", "scroll to today"),
    ("←→", "scroll the axis"),
    ("drag", "move; right edge resizes"),
];

const SCALES: [&str; 3] = ["day", "week", "month"];
const LABEL_W: u16 = 14;

pub struct State {
    pub sel: usize,
    /// 0 day, 1 week, 2 month.
    pub scale: usize,
    pub start: Date,
    started: bool,
    /// A bar being dragged: (id, start, days) as it would be dropped.
    pub preview: Option<(u64, Date, u32)>,
    axis: Rect,
}

impl State {
    pub fn new(today: Date) -> State {
        State {
            sel: 0,
            scale: 1,
            start: today.week_start(),
            started: false,
            preview: None,
            axis: Rect::default(),
        }
    }

    /// (columns per day, days per column) for the scale.
    fn ratio(&self) -> (i32, i32) {
        match self.scale {
            0 => (3, 1),
            1 => (1, 1),
            _ => (1, 4),
        }
    }

    /// Column offset of a date from the view start.
    pub fn col(&self, d: Date) -> i32 {
        let (cpd, dpc) = self.ratio();
        (self.start.days_until(d) * cpd).div_euclid(dpc)
    }

    /// The date at a column offset.
    pub fn date_at(&self, col: i32) -> Date {
        let (cpd, dpc) = self.ratio();
        self.start.add_days((col * dpc).div_euclid(cpd))
    }
}

pub fn bars(app: &App) -> Vec<GanttBar> {
    app.shown_get("gantt.bars").unwrap_or_default()
}

fn change(app: &mut App, f: impl FnOnce(&mut GanttBar)) {
    let mut bs = bars(app);
    let i = app.tabs.gantt.sel;
    let Some(b) = bs.get_mut(i) else { return };
    let before = b.clone();
    f(b);
    b.days = b.days.clamp(1, MAX_BAR_DAYS as u32);
    if *b != before {
        app.commit("gantt.bars", json!(bs));
    }
}

fn shift(app: &mut App, d: i32) {
    change(app, |b| {
        if let Some(s) = b.start_date() {
            b.start = s.add_days(d).iso();
        }
    });
}

fn scroll_to_today(app: &mut App) {
    let today = app.today;
    let s = &mut app.tabs.gantt;
    let (cpd, dpc) = s.ratio();
    let quarter = (s.axis.width as i32 / 4 * dpc / cpd).max(1);
    s.start = today.add_days(-quarter);
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let shift_k = k.modifiers.contains(KeyModifiers::SHIFT);
    let n = bars(app).len();
    let s = &mut app.tabs.gantt;
    let step = if s.scale == 2 { 28 } else { 7 };
    match k.code {
        KeyCode::Up => s.sel = s.sel.saturating_sub(1),
        KeyCode::Down => s.sel = (s.sel + 1).min(n.saturating_sub(1)),
        KeyCode::Left if shift_k => shift(app, -1),
        KeyCode::Right if shift_k => shift(app, 1),
        KeyCode::Char('<') => shift(app, -1),
        KeyCode::Char('>') => shift(app, 1),
        KeyCode::Char('[') => change(app, |b| b.days = b.days.saturating_sub(1).max(1)),
        KeyCode::Char(']') => change(app, |b| b.days += 1),
        KeyCode::Char('+') | KeyCode::Char('=') => {
            s.scale = s.scale.saturating_sub(1);
            app.info(format!("Scale: {}", SCALES[app.tabs.gantt.scale]));
        }
        KeyCode::Char('-') => {
            s.scale = (s.scale + 1).min(2);
            app.info(format!("Scale: {}", SCALES[app.tabs.gantt.scale]));
        }
        KeyCode::Char('t') => scroll_to_today(app),
        KeyCode::Left => s.start = s.start.add_days(-step),
        KeyCode::Right => s.start = s.start.add_days(step),
        _ => return false,
    }
    true
}

pub fn click(app: &mut App, hit: &Hit, x: u16) -> bool {
    match hit {
        Hit::Bar(id, resize) => {
            app.focus_ctrl(0);
            let bs = bars(app);
            if let Some(i) = bs.iter().position(|b| b.id == *id) {
                app.tabs.gantt.sel = i;
                if let Some(start0) = bs[i].start_date() {
                    app.drag = Some(Drag::Bar {
                        id: *id,
                        resize: *resize,
                        x0: x,
                        start0,
                        days0: bs[i].days,
                    });
                }
            }
            true
        }
        Hit::Item(0, i) => {
            app.focus_ctrl(0);
            app.tabs.gantt.sel = *i;
            true
        }
        _ => false,
    }
}

pub fn drag(app: &mut App, x: u16, end: bool) {
    let Some(Drag::Bar {
        id,
        resize,
        x0,
        start0,
        days0,
    }) = app.drag
    else {
        return;
    };
    let (cpd, dpc) = app.tabs.gantt.ratio();
    let delta = ((x as i32 - x0 as i32) * dpc).div_euclid(cpd);
    let (start, days) = if resize {
        (
            start0,
            (days0 as i32 + delta).clamp(1, MAX_BAR_DAYS as i32) as u32,
        )
    } else {
        (start0.add_days(delta), days0)
    };
    if !end {
        app.tabs.gantt.preview = Some((id, start, days));
        return;
    }
    app.tabs.gantt.preview = None;
    if delta != 0 {
        let mut bs = bars(app);
        if let Some(b) = bs.iter_mut().find(|b| b.id == id) {
            b.start = start.iso();
            b.days = days;
            app.commit("gantt.bars", json!(bs));
        }
    }
}

pub fn hints(_app: &App) -> Vec<Hint> {
    vec![
        h("↑↓", "Bar", KeyCode::Down),
        hint("⇧←→", "Move", KeyCode::Right, KeyModifiers::SHIFT),
        h("[ ]", "Size", KeyCode::Char(']')),
        h("+ -", "Zoom", KeyCode::Char('+')),
        h("t", "Today", KeyCode::Char('t')),
        h("←→", "Scroll", KeyCode::Right),
    ]
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let focused = is_focused(app, 0);
    let bs = bars(app);
    let today = app.today;
    if !app.tabs.gantt.started {
        // open on the week of the earliest bar (or today)
        let first = bs
            .iter()
            .filter_map(GanttBar::start_date)
            .min()
            .unwrap_or(today)
            .min(today);
        app.tabs.gantt.start = first.week_start();
        app.tabs.gantt.started = true;
    }
    app.hits.push((area, Hit::Ctrl(0)));
    let s = &mut app.tabs.gantt;
    s.sel = s.sel.min(bs.len().saturating_sub(1));
    let x0 = area.x + 1;
    let ax = x0 + LABEL_W + 1;
    let aw = area.right().saturating_sub(ax + 1);
    s.axis = Rect::new(ax, area.y, aw, area.height);
    let (_, dpc) = s.ratio();
    let rows_h = area.height.saturating_sub(4);
    let pitch: u16 = if bs.len() as u16 * 2 <= rows_h { 2 } else { 1 };
    let by = area.y + 2;
    let base = Style::new().bg(t.bg);
    // axis: month row and tick row
    fill(
        buf,
        Rect::new(ax, area.y, aw, 2),
        Style::new().bg(t.header_bg),
    );
    let mut last_month = 0;
    for c in 0..aw as i32 {
        let d = s.date_at(c);
        let first_col = s.col(d) == c;
        let x = ax + c as u16;
        if d.month() != last_month && first_col {
            last_month = d.month();
            // month scale has ~7 columns a month: short names, the year on January (or first)
            let label = if s.scale == 2 && d.month() != 1 && c > 2 {
                MONTH_NAMES[d.month() as usize - 1].to_string()
            } else {
                format!("{} {}", MONTH_NAMES[d.month() as usize - 1], d.year())
            };
            // never run into the next month's label
            let room = (s.col(d.end_of_month().add_days(1)) - c - 1).max(1) as usize;
            let label = if width(&label) > room {
                MONTH_NAMES[d.month() as usize - 1].to_string()
            } else {
                label
            };
            put(
                buf,
                x,
                area.y,
                &label,
                Style::new()
                    .bg(t.header_bg)
                    .fg(t.fg)
                    .add_modifier(Modifier::BOLD),
                room.min(aw as usize - c as usize),
            );
        }
        let tick = match s.scale {
            0 => first_col.then(|| format!("{}", d.day())),
            1 => (first_col && d.weekday() == 0).then(|| format!("{}", d.day())),
            _ => (d.day() <= 4 && first_col).then(String::new),
        };
        if let Some(tk) = tick {
            put(
                buf,
                x,
                area.y + 1,
                &format!("┊{tk}"),
                Style::new().bg(t.header_bg).fg(t.muted),
                aw.saturating_sub(c as u16) as usize,
            );
        }
        // weekend shading and the today line under the bars
        for r in 0..rows_h {
            let y = by + r;
            if dpc == 1 && d.is_weekend() {
                put(buf, x, y, " ", Style::new().bg(t.zebra), 1);
            }
            if d == today && first_col {
                put(
                    buf,
                    x,
                    y,
                    "│",
                    Style::new().fg(t.warn).bg(if dpc == 1 && d.is_weekend() {
                        t.zebra
                    } else {
                        t.bg
                    }),
                    1,
                );
            }
        }
    }
    put(
        buf,
        x0,
        area.y + 1,
        "Bar",
        Style::new()
            .fg(t.muted)
            .bg(t.bg)
            .add_modifier(Modifier::BOLD),
        LABEL_W as usize,
    );
    // bars
    let vis = (rows_h / pitch) as usize;
    let sel = s.sel;
    let off = if sel >= vis { sel + 1 - vis } else { 0 };
    let preview = s.preview;
    let s = &app.tabs.gantt;
    for (row, (i, b)) in bs.iter().enumerate().skip(off).take(vis).enumerate() {
        let y = by + row as u16 * pitch;
        let is_sel = i == sel;
        let ls = if is_sel && focused {
            Style::new()
                .bg(t.cur_bg)
                .fg(t.fg)
                .add_modifier(Modifier::BOLD)
        } else if is_sel {
            Style::new().bg(t.sel_bg).fg(t.fg)
        } else {
            Style::new().fg(t.fg).bg(t.bg)
        };
        fill(buf, Rect::new(x0, y, LABEL_W, 1), ls);
        put(
            buf,
            x0 + 1,
            y,
            &safe_text(&b.label),
            ls,
            LABEL_W as usize - 1,
        );
        app.hits
            .push((Rect::new(x0, y, LABEL_W, 1), Hit::Item(0, i)));
        let (start, days) = match preview {
            Some((id, st, d)) if id == b.id => (st, d),
            _ => match b.start_date() {
                Some(st) => (st, b.days),
                None => continue,
            },
        };
        let c1 = s.col(start);
        let c2 = s.col(start.add_days(days as i32)).max(c1 + 1); // exclusive
        let (v1, v2) = (c1.max(0), c2.min(aw as i32));
        if v1 >= v2 {
            let arrow = if c2 <= 0 { "◂" } else { "▸" };
            let ax2 = if c2 <= 0 { ax } else { ax + aw - 1 };
            put(buf, ax2, y, arrow, base.fg(t.muted), 1);
            continue;
        }
        let color = if is_sel { t.accent2 } else { t.info };
        let r = Rect::new(ax + v1 as u16, y, (v2 - v1) as u16, 1);
        fill(buf, r, Style::new().bg(color));
        let text = format!("{}d", days);
        if r.width as usize > width(&text) + 1 {
            put(
                buf,
                r.x + 1,
                y,
                &text,
                Style::new().bg(color).fg(t.bg).add_modifier(Modifier::BOLD),
                r.width as usize - 1,
            );
        }
        app.hits.push((r, Hit::Bar(b.id, false)));
        if c2 <= aw as i32 {
            // the last cell is the resize handle
            let hx = ax + v2 as u16 - 1;
            put(buf, hx, y, "▐", Style::new().bg(color).fg(t.fg), 1);
            app.hits
                .push((Rect::new(hx, y, 1, 1), Hit::Bar(b.id, true)));
        }
    }
    app.key_rects.push((
        "gantt.bars",
        Rect::new(x0, by, LABEL_W, (vis as u16 * pitch).min(rows_h)),
    ));
    // the selected bar's dates
    let fy = area.bottom() - 1;
    if let Some(b) = bs.get(sel) {
        let (start, days) = match preview {
            Some((id, st, d)) if id == b.id => (Some(st), d),
            _ => (b.start_date(), b.days),
        };
        if let Some(st) = start {
            let end = st.add_days(days as i32 - 1);
            let line = format!(
                "{} · {} → {} · {} day{} · scale: {}",
                safe_text(&b.label),
                st.long(),
                end.long(),
                days,
                if days == 1 { "" } else { "s" },
                SCALES[s.scale]
            );
            put(
                buf,
                x0,
                fy,
                &line,
                Style::new()
                    .fg(t.accent)
                    .bg(t.bg)
                    .add_modifier(Modifier::BOLD),
                area.width as usize - 2,
            );
        }
    } else {
        put(buf, x0, fy, "No bars.", muted(&t), 10);
    }
}
