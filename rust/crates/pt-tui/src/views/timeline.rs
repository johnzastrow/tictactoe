//! Timeline / Gantt (ui-spec §4.4): a project ▸ task tree on the left, a
//! zoomable time axis on the right with weekend shading, a today line,
//! status/health-coloured bars with progress fill, milestones and
//! dependency markers. Bars move and resize by keyboard or mouse drag.

use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use pt_core::date::{Date, MONTH_NAMES};
use pt_core::model::{Entity, TaskStatus};
use pt_core::protocol::Op;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use serde_json::Value;

use crate::app::{App, Drag, PendKind};
use crate::keymap::Action;
use crate::views;
use crate::widgets::draw::{fill, put, rect_contains};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    Day,
    Week,
    Month,
}

impl Zoom {
    /// (columns, days): how many terminal columns represent how many days.
    fn scale(self) -> (i32, i32) {
        match self {
            Zoom::Day => (3, 1),
            Zoom::Week => (1, 1),
            Zoom::Month => (1, 4),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Zoom::Day => "day",
            Zoom::Week => "week",
            Zoom::Month => "month",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlRow {
    pub entity: Entity,
    pub id: i64,
}

pub struct TimelineView {
    pub expanded: HashSet<i64>,
    pub cur: usize,
    pub top: usize,
    pub zoom: Zoom,
    pub origin: Date,
    pub rows: Vec<TlRow>,
    pub shown: usize,
    left_w: u16,
    axis: Rect,
    body_y: u16,
    bars: Vec<(Rect, TlRow)>,
    /// live drag preview: (row, start delta days, due delta days)
    pub preview: Option<(TlRow, i32, i32)>,
}

impl TimelineView {
    pub fn new(today: Date) -> TimelineView {
        TimelineView {
            expanded: HashSet::new(),
            cur: 0,
            top: 0,
            zoom: Zoom::Week,
            origin: today.add_days(-14),
            rows: vec![],
            shown: 0,
            left_w: 36,
            axis: Rect::default(),
            body_y: 0,
            bars: vec![],
            preview: None,
        }
    }

    pub fn goto_today(&mut self, today: Date) {
        let (c, d) = self.zoom.scale();
        // today about a fifth of the way in
        let cols = self.axis.width.max(100) as i32;
        self.origin = today.add_days(-(cols / 5) * d / c);
    }

    pub fn current_entity(&self) -> Option<(Entity, i64)> {
        self.rows.get(self.cur).map(|r| (r.entity, r.id))
    }

    fn x_of(&self, d: Date) -> i32 {
        let (c, days) = self.zoom.scale();
        self.axis.x as i32 + (d.days() - self.origin.days()) * c / days
    }

    fn date_at(&self, x: u16) -> Date {
        let (c, days) = self.zoom.scale();
        self.origin.add_days((x as i32 - self.axis.x as i32) * days / c)
    }
}

fn build_rows(app: &App) -> Vec<TlRow> {
    let r = &app.r;
    let q = &app.queries[views::TIMELINE].query;
    let mut projects: Vec<&pt_core::model::Project> = r.projects.values().filter(|p| q.matches_project(p, r)).collect();
    projects.sort_by(|a, b| a.start_date.cmp(&b.start_date).then(a.code.cmp(&b.code)));
    let mut rows = vec![];
    for p in projects {
        rows.push(TlRow { entity: Entity::Project, id: p.id });
        if app.views.timeline.expanded.contains(&p.id) {
            rows.extend(r.project_tasks(p.id).iter().map(|id| TlRow { entity: Entity::Task, id: *id }));
        }
    }
    rows
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let rows = build_rows(app);
    let presence = app.presence_map();
    let tl = &mut app.views.timeline;
    tl.shown = rows.iter().filter(|r| r.entity == Entity::Project).count();
    // keep the cursor on the same row
    let cur_row = tl.rows.get(tl.cur).copied();
    tl.rows = rows;
    if let Some(cr) = cur_row {
        if let Some(i) = tl.rows.iter().position(|r| *r == cr) {
            tl.cur = i;
        }
    }
    tl.cur = tl.cur.min(tl.rows.len().saturating_sub(1));
    fill(buf, area, t.base());
    let left_w = 36.min(area.width / 3);
    tl.left_w = left_w;
    let axis = Rect::new(area.x + left_w + 1, area.y, area.width.saturating_sub(left_w + 1), 2);
    tl.axis = axis;
    let body_y = area.y + 2;
    tl.body_y = body_y;
    let body_h = area.height.saturating_sub(2) as usize;
    if tl.cur < tl.top {
        tl.top = tl.cur;
    } else if tl.cur >= tl.top + body_h {
        tl.top = tl.cur + 1 - body_h;
    }
    let (cols, days) = tl.zoom.scale();
    let r = &app.r;
    let today = r.today;
    // axis header
    let hs = Style::new().bg(t.header_bg).fg(t.fg);
    fill(buf, Rect::new(area.x, area.y, area.width, 2), hs);
    put(buf, area.x + 1, area.y, &format!("Zoom: {} (+/-)", tl.zoom.label()), hs.fg(t.muted), left_w as usize - 1);
    put(buf, area.x + 1, area.y + 1, "Project / task", hs.add_modifier(Modifier::BOLD), left_w as usize - 1);
    let ncols = axis.width as i32;
    let mut last_month = 0;
    let mut label_end = 0u16;
    for cx in 0..ncols {
        let d = tl.origin.add_days(cx * days / cols);
        let x = axis.x + cx as u16;
        let first_col_of_day = (cx * days) % cols == 0 || cols == 1;
        if d.month() != last_month && first_col_of_day {
            last_month = d.month();
            let label = format!("{} {}", MONTH_NAMES[d.month() as usize - 1], d.year());
            // the leading partial month only gets a label if the next month starts far enough away
            let room = (d.end_of_month().days() - d.days() + 1) * cols / days;
            if x >= label_end && (cx > 0 || room as usize > label.len() + 1) {
                put(buf, x, area.y, &format!("▏{label}"), hs.add_modifier(Modifier::BOLD), (axis.right() - x) as usize);
                label_end = x + label.len() as u16 + 2;
            }
        }
        let show_day = match tl.zoom {
            Zoom::Day => first_col_of_day,
            Zoom::Week => d.weekday() == 0,
            Zoom::Month => d.weekday() < 4 && (cx % 7 == 0),
        };
        if show_day && (cx * days) % cols == 0 {
            let s = if tl.zoom == Zoom::Day { format!("{:>2}", d.day()) } else { d.day().to_string() };
            let st = if d == today { hs.fg(t.accent).add_modifier(Modifier::BOLD) } else { hs.fg(t.muted) };
            put(buf, x, area.y + 1, &s, st, (axis.right() - x) as usize);
        }
    }
    // background columns: weekends and today
    let today_x = tl.x_of(today);
    for (vis, ri) in (tl.top..tl.rows.len()).take(body_h).enumerate() {
        let y = body_y + vis as u16;
        let row = tl.rows[ri];
        let selected = ri == tl.cur;
        let row_bg = if selected { t.sel_bg } else if vis % 2 == 1 { t.zebra } else { t.bg };
        fill(buf, Rect::new(area.x, y, area.width, 1), Style::new().bg(row_bg));
        if tl.zoom != Zoom::Month {
            for cx in 0..ncols {
                let d = tl.origin.add_days(cx * days / cols);
                if d.weekday() >= 5 && !selected {
                    if let Some(c) = buf.cell_mut((axis.x + cx as u16, y)) {
                        c.set_bg(t.panel);
                    }
                }
            }
        }
        if today_x >= axis.x as i32 && today_x < axis.right() as i32 {
            put(buf, today_x as u16, y, "│", Style::new().fg(t.accent).bg(row_bg), 1);
        }
        // left tree
        let peers = presence.get(&(row.entity, row.id));
        if let Some((uid, init, _)) = peers.and_then(|p| p.first()) {
            put(buf, area.x, y, init, Style::new().bg(t.user(*uid)).fg(t.bg).add_modifier(Modifier::BOLD), 1);
        }
        let ls = Style::new().bg(row_bg).fg(t.fg);
        let label = match row.entity {
            Entity::Project => {
                let p = &r.projects[&row.id];
                let arrow = if tl.expanded.contains(&row.id) { "▾" } else { "▸" };
                format!("{arrow} {} {}", p.code, p.name)
            }
            _ => {
                let tk = &r.tasks[&row.id];
                format!("    #{} {}", tk.seq, tk.title)
            }
        };
        let st = if row.entity == Entity::Project { ls.add_modifier(Modifier::BOLD) } else { ls };
        put(buf, area.x + 1, y, &label, st, left_w as usize - 1);
        put(buf, area.x + left_w, y, "│", Style::new().fg(t.border).bg(row_bg), 1);
    }
    // bars
    tl.bars.clear();
    for (vis, ri) in (tl.top..tl.rows.len()).take(body_h).enumerate() {
        let y = body_y + vis as u16;
        let row = tl.rows[ri];
        let (s, d) = {
            let tlr: &TimelineView = tl;
            let (mut s, mut d) = dates_of_r(r, row);
            if let Some((pr, ds, dd)) = tlr.preview {
                if pr == row {
                    s = s.map(|x| x.add_days(ds));
                    d = d.map(|x| x.add_days(dd));
                }
            }
            (s, d)
        };
        let (Some(s), Some(d)) = (s.or(d), d.or(s)) else { continue };
        let x0 = tl.x_of(s);
        let x1 = tl.x_of(d.add_days(1)).max(x0 + 1);
        let (cx0, cx1) = (x0.max(axis.x as i32), x1.min(axis.right() as i32));
        let selected = ri == tl.cur;
        let (color, pct, text, milestone, waiting) = match row.entity {
            Entity::Project => {
                let p = &r.projects[&row.id];
                let m = r.metrics(p.id);
                (t.health_color(p.health), m.pct_done(), format!("{} {}%", p.name, m.pct_done().round()), false, false)
            }
            _ => {
                let tk = &r.tasks[&row.id];
                let est = tk.est_hours.unwrap_or(0.0);
                let pct = if tk.status == TaskStatus::Done { 100.0 } else if est > 0.0 { (tk.actual_hours * 100.0 / est).min(99.0) } else { 0.0 };
                let blocked = r.task_blocked(tk);
                let ms = s == d && est == 0.0;
                (t.task_status_color(if blocked { TaskStatus::Blocked } else { tk.status }), pct, tk.title.clone(), ms, tk.deps.iter().any(|x| r.task_status(*x).is_some_and(|s| s != TaskStatus::Done)))
            }
        };
        if waiting && x0 > axis.x as i32 && x0 - 1 < axis.right() as i32 {
            put(buf, (x0 - 1) as u16, y, "◂", Style::new().fg(t.err), 1);
        }
        if cx0 >= cx1 {
            // off screen: an arrow at the edge
            let (ex, sym) = if x1 <= axis.x as i32 { (axis.x, "◀") } else { (axis.right() - 1, "▶") };
            put(buf, ex, y, sym, Style::new().fg(color), 1);
            continue;
        }
        if milestone {
            put(buf, cx0 as u16, y, "◆", Style::new().fg(color).add_modifier(Modifier::BOLD), 1);
            tl.bars.push((Rect::new(cx0 as u16, y, 1, 1), row));
            continue;
        }
        let total = (x1 - x0).max(1);
        let filled = ((pct / 100.0) * total as f64).round() as i32;
        for x in cx0..cx1 {
            let done = x - x0 < filled;
            let st = if done { Style::new().bg(color).fg(Color::Black) } else { Style::new().bg(dim(color, &t)).fg(t.fg) };
            if let Some(c) = buf.cell_mut((x as u16, y)) {
                c.set_symbol(" ").set_style(st);
            }
        }
        // label inside the bar, or right after it
        let inside = (cx1 - cx0) as usize;
        if inside > text.chars().count() + 1 {
            for (i, ch) in text.chars().enumerate() {
                let x = cx0 + 1 + i as i32;
                if let Some(c) = buf.cell_mut((x as u16, y)) {
                    c.set_symbol(&ch.to_string());
                    if x - x0 < filled {
                        c.set_fg(Color::Black);
                    }
                }
            }
        } else if cx1 + 1 < axis.right() as i32 {
            put(buf, (cx1 + 1) as u16, y, &text, Style::new().fg(t.muted), (axis.right() as i32 - cx1 - 1) as usize);
        }
        if selected {
            let edge = Style::new().fg(t.accent).add_modifier(Modifier::BOLD);
            if cx0 == x0 && cx0 > axis.x as i32 && !waiting {
                put(buf, (cx0 - 1) as u16, y, "▕", edge, 1);
            }
            if cx1 == x1 && cx1 < axis.right() as i32 {
                put(buf, cx1 as u16, y, "▏", edge, 1);
            }
        }
        tl.bars.push((Rect::new(cx0 as u16, y, (cx1 - cx0) as u16, 1), row));
    }
    if tl.rows.is_empty() {
        put(buf, area.x + 2, body_y + 1, "No projects match the filter.", t.muted(), 40);
    }
}

fn dates_of_r(r: &pt_core::replica::Replica, row: TlRow) -> (Option<Date>, Option<Date>) {
    let (s, d) = match row.entity {
        Entity::Project => r.projects.get(&row.id).map(|p| (p.start_date.clone(), p.due_date.clone())).unwrap_or_default(),
        _ => r.tasks.get(&row.id).map(|t| (t.start_date.clone(), t.due_date.clone())).unwrap_or_default(),
    };
    (s.as_deref().and_then(Date::parse_iso), d.as_deref().and_then(Date::parse_iso))
}

/// A muted version of a bar colour for the not-yet-done part.
fn dim(c: Color, t: &crate::theme::Theme) -> Color {
    match c {
        Color::Rgb(r, g, b) => {
            let (br, bg, bb) = match t.bg {
                Color::Rgb(r, g, b) => (r as u16, g as u16, b as u16),
                _ => (0, 0, 0),
            };
            Color::Rgb(((r as u16 + br * 2) / 3) as u8, ((g as u16 + bg * 2) / 3) as u8, ((b as u16 + bb * 2) / 3) as u8)
        }
        _ => t.border,
    }
}

/// Mini Gantt of the selected template (Templates view).
pub fn render_template_preview(app: &App, buf: &mut Buffer, area: Rect) {
    let t = &app.t;
    let r = &app.r;
    let Some(tid) = app.views.templates.main.current_id() else { return };
    let tts = r.template_tasks_of(tid);
    fill(buf, area, t.panel());
    let (n, hours, span) = r.template_stats(tid);
    put(buf, area.x + 1, area.y, &format!("Preview: {n} tasks · {} h · {span} days", pt_core::grid::fmt_hours(hours)), Style::new().bg(t.panel).fg(t.accent).add_modifier(Modifier::BOLD), area.width as usize - 2);
    let label_w = 24u16;
    let gx = area.x + label_w + 1;
    let gw = area.width.saturating_sub(label_w + 2) as i64;
    let span = span.max(1);
    let rows = (area.height.saturating_sub(1)) as usize;
    // phases get one colour each
    let mut phases: Vec<&str> = vec![];
    for tt in &tts {
        if !phases.contains(&tt.phase.as_str()) {
            phases.push(&tt.phase);
        }
    }
    let palette = [t.info, t.accent2, t.ok, t.warn, t.accent, t.err];
    for (i, tt) in tts.iter().take(rows).enumerate() {
        let y = area.y + 1 + i as u16;
        put(buf, area.x + 1, y, &format!("{:>2} {}", tt.seq, tt.title), Style::new().bg(t.panel).fg(t.fg), label_w as usize - 1);
        let x0 = gx as i64 + tt.offset_days * gw / span;
        let x1 = (gx as i64 + (tt.offset_days + tt.duration_days) * gw / span).max(x0 + 1);
        let c = palette[phases.iter().position(|p| *p == tt.phase).unwrap_or(0) % palette.len()];
        for x in x0..x1 {
            put(buf, x as u16, y, "█", Style::new().fg(c).bg(t.panel), 1);
        }
    }
    if tts.len() > rows {
        put(buf, area.right().saturating_sub(12), area.bottom() - 1, &format!("+{} more", tts.len() - rows), t.muted().bg(t.panel), 11);
    }
}

impl App {
    pub fn timeline_key(&mut self, k: KeyEvent) {
        let tl = &mut self.views.timeline;
        let n = tl.rows.len();
        let cur = tl.rows.get(tl.cur).copied();
        match k.code {
            KeyCode::Up => tl.cur = tl.cur.saturating_sub(1),
            KeyCode::Down => tl.cur = (tl.cur + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => tl.cur = tl.cur.saturating_sub(10),
            KeyCode::PageDown => tl.cur = (tl.cur + 10).min(n.saturating_sub(1)),
            KeyCode::Home => tl.cur = 0,
            KeyCode::End => tl.cur = n.saturating_sub(1),
            KeyCode::Right => {
                if let Some(TlRow { entity: Entity::Project, id }) = cur {
                    tl.expanded.insert(id);
                }
            }
            KeyCode::Left => match cur {
                Some(TlRow { entity: Entity::Project, id }) => {
                    tl.expanded.remove(&id);
                }
                Some(TlRow { entity: Entity::Task, id }) => {
                    if let Some(pid) = self.r.tasks.get(&id).map(|t| t.project_id) {
                        if let Some(i) = tl.rows.iter().position(|r| r.entity == Entity::Project && r.id == pid) {
                            tl.cur = i;
                        }
                    }
                }
                _ => {}
            },
            KeyCode::Enter | KeyCode::Char(' ') => match cur {
                Some(TlRow { entity: Entity::Project, id }) => {
                    if !tl.expanded.remove(&id) {
                        tl.expanded.insert(id);
                    }
                }
                Some(TlRow { entity, id }) => self.open_record(entity, id, None),
                None => {}
            },
            KeyCode::Char('e') => {
                // expand all / collapse all
                if tl.expanded.is_empty() {
                    tl.expanded = self.r.projects.keys().copied().collect();
                } else {
                    tl.expanded.clear();
                }
            }
            _ => {}
        }
    }

    fn unit_days(&self) -> i32 {
        match self.views.timeline.zoom {
            Zoom::Day => 1,
            Zoom::Week => 7,
            Zoom::Month => 30,
        }
    }

    pub fn timeline_action(&mut self, a: Action) {
        let unit = self.unit_days();
        let tl = &mut self.views.timeline;
        match a {
            Action::ZoomIn | Action::ZoomOut => {
                let center = tl.date_at(tl.axis.x + tl.axis.width / 2);
                tl.zoom = match (tl.zoom, a) {
                    (Zoom::Month, Action::ZoomIn) => Zoom::Week,
                    (Zoom::Week, Action::ZoomIn) => Zoom::Day,
                    (Zoom::Day, Action::ZoomOut) => Zoom::Week,
                    (Zoom::Week, Action::ZoomOut) => Zoom::Month,
                    (z, _) => z,
                };
                let (c, d) = tl.zoom.scale();
                tl.origin = center.add_days(-((tl.axis.width / 2) as i32) * d / c);
            }
            Action::Today => {
                let today = self.r.today;
                self.views.timeline.goto_today(today);
                if let Some(i) = self.views.timeline.rows.iter().position(|row| {
                    let (s, d) = dates_of_r(&self.r, *row);
                    s.is_some_and(|s| s <= today) && d.is_some_and(|d| d >= today)
                }) {
                    self.views.timeline.cur = i;
                }
            }
            Action::TimeBack | Action::TimeFwd => {
                let (c, d) = tl.zoom.scale();
                let shift = (tl.axis.width as i32 / 4).max(4) * d / c;
                tl.origin = tl.origin.add_days(if a == Action::TimeBack { -shift } else { shift });
            }
            Action::BarLeft => self.shift_bar(-unit, -unit),
            Action::BarRight => self.shift_bar(unit, unit),
            Action::ShrinkDue => self.shift_bar(0, -unit),
            Action::ExtendDue => self.shift_bar(0, unit),
            _ => {}
        }
    }

    /// Move start and/or due of the selected bar (one mutate).
    fn shift_bar(&mut self, ds: i32, dd: i32) {
        let Some(row) = self.views.timeline.rows.get(self.views.timeline.cur).copied() else { return };
        self.shift_row(row, ds, dd);
    }

    fn shift_row(&mut self, row: TlRow, ds: i32, dd: i32) {
        let (s, d) = dates_of_r(&self.r, row);
        let (Some(s0), Some(d0)) = (s, d) else {
            self.toast("This row has no dates yet — set Start and Due first (^E).".into(), crate::app::Level::Info);
            return;
        };
        let (ns, nd) = (s0.add_days(ds), d0.add_days(dd));
        if nd < ns {
            self.toast("Due can't be before start.".into(), crate::app::Level::Warn);
            return;
        }
        let mut ops = vec![];
        if ns != s0 {
            ops.push(Op::set(row.entity, row.id, "start_date", Value::from(s0.iso()), Value::from(ns.iso())));
        }
        if nd != d0 {
            ops.push(Op::set(row.entity, row.id, "due_date", Value::from(d0.iso()), Value::from(nd.iso())));
        }
        let label = format!("{} {} → {}", self.r.row_label(row.entity, row.id), ns.iso(), nd.iso());
        self.mutate(ops, label, PendKind::Cells, None);
    }

    pub fn timeline_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        match m.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = matches!(m.kind, MouseEventKind::ScrollDown);
                let tl = &mut self.views.timeline;
                if m.modifiers.contains(KeyModifiers::SHIFT) || x > tl.axis.x && y < tl.body_y {
                    let (c, d) = tl.zoom.scale();
                    tl.origin = tl.origin.add_days(if down { 6 * d / c } else { -6 * d / c });
                } else {
                    let n = tl.rows.len();
                    tl.cur = if down { (tl.cur + 3).min(n.saturating_sub(1)) } else { tl.cur.saturating_sub(3) };
                }
            }
            MouseEventKind::Down(btn) => {
                let tl = &self.views.timeline;
                if y < tl.body_y {
                    return;
                }
                let row_idx = tl.top + (y - tl.body_y) as usize;
                if row_idx >= tl.rows.len() {
                    return;
                }
                let row = tl.rows[row_idx];
                let on_bar = tl.bars.iter().find(|(r, rw)| *rw == row && rect_contains(Rect::new(r.x, r.y, r.width + 1, 1), x, y)).map(|(r, _)| *r);
                let left = x < tl.axis.x;
                self.views.timeline.cur = row_idx;
                if btn == MouseButton::Right {
                    self.open_context_at(x, y);
                    return;
                }
                let dbl = self.is_double(x, y);
                if dbl {
                    self.open_record(row.entity, row.id, None);
                    return;
                }
                if left && row.entity == Entity::Project {
                    let tl = &mut self.views.timeline;
                    if !tl.expanded.remove(&row.id) {
                        tl.expanded.insert(row.id);
                    }
                } else if let Some(r) = on_bar {
                    let resize = x + 1 >= r.right();
                    self.drag = Some(Drag::Bar { task_or_project: (row.entity, row.id), x0: x, resize });
                }
            }
            _ => {}
        }
    }

    pub fn timeline_drag(&mut self, x: u16, _y: u16) {
        let Some(Drag::Bar { task_or_project, x0, resize, .. }) = &self.drag else { return };
        let (c, d) = self.views.timeline.zoom.scale();
        let delta = (x as i32 - *x0 as i32) * d / c;
        let row = TlRow { entity: task_or_project.0, id: task_or_project.1 };
        self.views.timeline.preview = Some(if *resize { (row, 0, delta) } else { (row, delta, delta) });
    }

    pub fn timeline_drop(&mut self, _x: u16, _y: u16) {
        let preview = self.views.timeline.preview.take();
        self.drag = None;
        if let Some((row, ds, dd)) = preview {
            if ds != 0 || dd != 0 {
                self.shift_row(row, ds, dd);
            }
        }
    }
}
