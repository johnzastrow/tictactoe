//! Board (kanban, ui-spec §4.3): status columns, cards, optional swimlanes by
//! assignee, a cascading Program ▸ Project scope, keyboard moves and mouse
//! drag-and-drop between columns (and lanes, which reassigns).

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use pt_core::date::Date;
use pt_core::grid::{Style as CS, fmt_hours};
use pt_core::model::{Entity, Priority, TaskStatus};
use pt_core::protocol::Op;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::Value;

use crate::app::{App, Drag, PendKind};
use crate::keymap::Action;
use crate::theme::Theme;
use crate::views;
use crate::widgets::draw::{bar, fill, pad, put, rect_contains};

const CARD_H: u16 = 3;

pub struct Lane {
    pub user: Option<i64>,
    pub label: String,
    pub cells: [Vec<i64>; 5],
}

#[derive(Default)]
pub struct BoardView {
    pub program: Option<i64>,
    pub project: Option<i64>,
    pub lanes: bool,
    pub col: usize,
    pub current: Option<i64>,
    pub shown: usize,
    pub scroll: u16,
    pub model: Vec<Lane>,
    cards: Vec<(Rect, i64)>,
    cols: Vec<(Rect, usize)>,
    lane_rows: Vec<(u16, u16, usize)>,
    pub scope_rect: Rect,
    drop: Option<(usize, usize)>,
}

impl BoardView {
    /// (lane, column, index) of the current card.
    fn position(&self) -> Option<(usize, usize, usize)> {
        let id = self.current?;
        for (li, lane) in self.model.iter().enumerate() {
            for (ci, cell) in lane.cells.iter().enumerate() {
                if let Some(i) = cell.iter().position(|x| *x == id) {
                    return Some((li, ci, i));
                }
            }
        }
        None
    }

    fn first_in(&self, ci: usize) -> Option<i64> {
        self.model.iter().find_map(|l| l.cells[ci].first().copied())
    }
}

fn build(app: &App) -> Vec<Lane> {
    let r = &app.r;
    let q = &app.queries[views::BOARD].query;
    let b = &app.views.board;
    let mut tasks: Vec<&pt_core::model::Task> = r
        .tasks
        .values()
        .filter(|t| {
            let p = r.projects.get(&t.project_id);
            b.project.is_none_or(|pid| t.project_id == pid)
                && b.program.is_none_or(|pg| p.is_some_and(|p| p.program_id == Some(pg)))
                && q.matches_task(t, r)
        })
        .collect();
    // most urgent first: priority, then due date
    tasks.sort_by(|a, b| {
        b.priority
            .index()
            .cmp(&a.priority.index())
            .then(a.due_date.as_deref().unwrap_or("9999").cmp(b.due_date.as_deref().unwrap_or("9999")))
            .then(a.id.cmp(&b.id))
    });
    let mut lanes: Vec<Lane> = Vec::new();
    if b.lanes {
        let mut users: Vec<Option<i64>> = tasks.iter().map(|t| t.assignee_id).collect();
        users.sort_by_key(|u| (u.is_none(), u.map(|id| r.user_name(Some(id)).to_string())));
        users.dedup();
        for u in users {
            let label = match u {
                Some(id) => r.user_name(Some(id)).to_string(),
                None => "Unassigned".into(),
            };
            lanes.push(Lane { user: u, label, cells: Default::default() });
        }
    } else {
        lanes.push(Lane { user: None, label: String::new(), cells: Default::default() });
    }
    for t in tasks {
        let li = if b.lanes { lanes.iter().position(|l| l.user == t.assignee_id).unwrap_or(0) } else { 0 };
        lanes[li].cells[t.status.index()].push(t.id);
    }
    lanes
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let model = build(app);
    let presence = app.presence_map();
    let r = &app.r;
    let b = &mut app.views.board;
    b.model = model;
    b.shown = b.model.iter().map(|l| l.cells.iter().map(Vec::len).sum::<usize>()).sum();
    if b.position().is_none() {
        b.current = b.first_in(b.col).or_else(|| (0..5).find_map(|c| b.first_in(c)));
    }
    if let Some((_, ci, _)) = b.position() {
        b.col = ci;
    }
    fill(buf, area, t.base());
    // scope bar
    let sy = area.y;
    let st = Style::new().bg(t.panel).fg(t.fg);
    fill(buf, Rect::new(area.x, sy, area.width, 1), st);
    let prog = b.program.and_then(|p| r.programs.get(&p)).map_or("All programs".to_string(), |p| p.name.clone());
    let proj = b.project.and_then(|p| r.projects.get(&p)).map_or("All projects".to_string(), |p| format!("{} {}", p.code, p.name));
    let scope = format!(" Scope: [{prog} ▾] ▸ [{proj} ▾] ");
    let w = put(buf, area.x + 1, sy, &scope, st.fg(t.accent).add_modifier(Modifier::BOLD), area.width as usize - 2);
    b.scope_rect = Rect::new(area.x + 1, sy, w, 1);
    put(buf, area.x + 2 + w, sy, "S scope · ^R swimlanes · ⇧←→ move card · drag cards with the mouse", st.fg(t.muted), (area.width.saturating_sub(w + 4)) as usize);

    // columns
    let cw = (area.width.saturating_sub(4)) / 5;
    let hy = sy + 1;
    b.cols.clear();
    for (ci, s) in TaskStatus::ALL.iter().enumerate() {
        let x = area.x + 1 + ci as u16 * (cw + 1);
        let n: usize = b.model.iter().map(|l| l.cells[ci].len()).sum();
        let hours: f64 = b.model.iter().flat_map(|l| l.cells[ci].iter()).filter_map(|id| r.tasks.get(id)).map(|t| t.est_hours.unwrap_or(0.0)).sum();
        let color = t.task_status_color(*s);
        let focused = ci == b.col;
        let hs = Style::new().bg(if b.drop.is_some_and(|d| d.1 == ci) { t.flash } else if focused { t.sel_bg } else { t.header_bg }).fg(color).add_modifier(Modifier::BOLD);
        fill(buf, Rect::new(x, hy, cw, 1), hs);
        let g = Theme::glyph(&CS::TaskStatus(*s)).unwrap_or("");
        let info_w = if cw >= 26 { 14 } else { 5 };
        put(buf, x + 1, hy, &format!("{g}{}", s.label()), hs, (cw as usize).saturating_sub(info_w + 1));
        let info = if cw >= 26 { format!("{n} · {}h ", fmt_hours(hours)) } else { format!("{n} ") };
        put(buf, x + cw - info.chars().count() as u16, hy, &info, hs.fg(t.muted).remove_modifier(Modifier::BOLD), info.len());
        b.cols.push((Rect::new(x, hy, cw, area.bottom() - hy), ci));
    }

    // body: lanes stacked, scrolled by lines
    let body = Rect::new(area.x, hy + 1, area.width, area.bottom().saturating_sub(hy + 1));
    let lane_h: Vec<u16> = b
        .model
        .iter()
        .map(|l| {
            let maxn = l.cells.iter().map(Vec::len).max().unwrap_or(0).max(1) as u16;
            maxn * (CARD_H + 1) + u16::from(b.lanes)
        })
        .collect();
    // keep the current card visible
    if let Some((li, _, idx)) = b.position() {
        let top: u16 = lane_h[..li].iter().sum::<u16>() + u16::from(b.lanes) + idx as u16 * (CARD_H + 1);
        if top < b.scroll {
            b.scroll = top.saturating_sub(u16::from(b.lanes));
        } else if top + CARD_H > b.scroll + body.height {
            b.scroll = top + CARD_H - body.height;
        }
    }
    b.cards.clear();
    b.lane_rows.clear();
    let mut ly: i32 = body.y as i32 - b.scroll as i32;
    let visible = |y: i32, h: u16| y >= body.y as i32 && y + h as i32 <= body.bottom() as i32;
    for (li, lane) in b.model.iter().enumerate() {
        let lane_top = ly;
        if b.lanes {
            if visible(ly, 1) {
                let n: usize = lane.cells.iter().map(Vec::len).sum();
                let ls = Style::new().bg(t.bg).fg(lane.user.map_or(t.muted, |u| t.user(u))).add_modifier(Modifier::BOLD);
                let label = format!("── {} ({n}) ", lane.label);
                put(buf, area.x + 1, ly as u16, &format!("{label}{}", "─".repeat((area.width as usize).saturating_sub(label.chars().count() + 2))), ls, area.width as usize - 2);
            }
            ly += 1;
        }
        for (ci, cell) in lane.cells.iter().enumerate() {
            let x = area.x + 1 + ci as u16 * (cw + 1);
            for (i, id) in cell.iter().enumerate() {
                let y = ly + i as i32 * (CARD_H + 1) as i32;
                if !visible(y, CARD_H) {
                    continue;
                }
                let rect = Rect::new(x, y as u16, cw, CARD_H);
                let selected = b.current == Some(*id);
                draw_card(buf, rect, *id, selected, &t, r, presence.get(&(Entity::Task, *id)).map(|v| v.as_slice()).unwrap_or(&[]), app.flashes.contains_key(&(Entity::Task, *id)));
                b.cards.push((rect, *id));
            }
        }
        ly += lane_h[li] as i32 - i32::from(b.lanes);
        b.lane_rows.push((lane_top.max(0) as u16, ly.max(0) as u16, li));
    }
    if b.shown == 0 {
        put(buf, area.x + 3, body.y + 1, "No tasks in this scope / filter. S changes the scope, ^F the filter.", t.muted(), area.width as usize - 6);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_card(buf: &mut Buffer, r: Rect, id: i64, selected: bool, t: &Theme, rep: &pt_core::replica::Replica, peers: &[(i64, String, Option<String>)], flash: bool) {
    let Some(task) = rep.tasks.get(&id) else { return };
    let bg = if flash { t.flash } else if selected { t.cur_bg } else { t.panel };
    let st = Style::new().bg(bg).fg(t.fg);
    fill(buf, r, st);
    let w = r.width as usize;
    // left edge in the priority colour
    let pc = t.priority(task.priority).fg.unwrap_or(t.fg);
    for y in r.y..r.bottom() {
        put(buf, r.x, y, if task.priority == Priority::Critical { "┃" } else { "▏" }, Style::new().bg(bg).fg(pc), 1);
    }
    let title = format!("#{} {}", task.seq, task.title);
    put(buf, r.x + 1, r.y, &title, st.add_modifier(Modifier::BOLD), w.saturating_sub(4));
    if let Some((uid, init, _)) = peers.first() {
        put(buf, r.right() - 2, r.y, init, Style::new().bg(t.user(*uid)).fg(t.bg).add_modifier(Modifier::BOLD), 1);
    }
    let code = rep.project_code(task.project_id);
    let who = task.assignee_id.and_then(|u| rep.users.get(&u)).map(|u| u.initials()).unwrap_or_else(|| "--".into());
    let x = r.x + 1 + put(buf, r.x + 1, r.y + 1, code, st.fg(t.muted), 10);
    let x = x + put(buf, x, r.y + 1, &format!(" {who}"), st.fg(task.assignee_id.map_or(t.muted, |u| t.user(u))), 4);
    if let Some(d) = task.due_date.as_deref().and_then(Date::parse_iso) {
        let ds = if rep.task_overdue(task) {
            st.fg(t.err)
        } else if d.days() - rep.today.days() <= 7 && task.status != TaskStatus::Done {
            st.fg(t.warn)
        } else {
            st.fg(t.muted)
        };
        put(buf, x, r.y + 1, &format!(" ⏱ {}", d.short(rep.today.year()).trim()), ds, (r.right() - x) as usize);
    }
    if rep.task_blocked(task) && task.status != TaskStatus::Blocked {
        put(buf, r.right() - 2, r.y + 1, "◂", st.fg(t.err), 1);
    }
    let est = task.est_hours.unwrap_or(0.0);
    let pct = if task.status == TaskStatus::Done { 100.0 } else if est > 0.0 { task.actual_hours * 100.0 / est } else { 0.0 };
    let label = format!(" {}/{}h", fmt_hours(task.actual_hours), task.est_hours.map_or("?".into(), fmt_hours));
    let bw = w.saturating_sub(label.chars().count() + 2).min(16);
    let color = if task.est_hours.is_some_and(|e| task.actual_hours > e) { t.err } else { t.task_status_color(task.status) };
    bar(buf, r.x + 1, r.y + 2, bw, pct.min(100.0), color, t.border);
    put(buf, r.x + 1 + bw as u16, r.y + 2, &pad(&label, w - bw - 1, false), st.fg(t.muted), w - bw - 1);
}

impl App {
    pub fn board_key(&mut self, k: KeyEvent) {
        let b = &mut self.views.board;
        let pos = b.position();
        match k.code {
            KeyCode::Left | KeyCode::Right => {
                let dir: i64 = if k.code == KeyCode::Left { -1 } else { 1 };
                let (li, ci, idx) = pos.unwrap_or((0, b.col, 0));
                let mut nc = ci as i64 + dir;
                // skip empty columns in this lane, but stop at the edge
                while (0..5).contains(&nc) {
                    let cell = &b.model[li].cells[nc as usize];
                    if !cell.is_empty() {
                        b.current = Some(cell[idx.min(cell.len() - 1)]);
                        b.col = nc as usize;
                        return;
                    }
                    nc += dir;
                }
                b.col = (ci as i64 + dir).clamp(0, 4) as usize;
            }
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                let Some((li, ci, idx)) = pos else { return };
                let step: i64 = match k.code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::PageUp => -5,
                    _ => 5,
                };
                let ni = idx as i64 + step;
                let cell = &b.model[li].cells[ci];
                if ni >= 0 && (ni as usize) < cell.len() {
                    b.current = Some(cell[ni as usize]);
                } else if step < 0 {
                    // previous lane with cards in this column
                    if let Some(l) = b.model[..li].iter().rev().find(|l| !l.cells[ci].is_empty()) {
                        b.current = l.cells[ci].last().copied();
                    } else {
                        b.current = cell.first().copied();
                    }
                } else if let Some(l) = b.model[li + 1..].iter().find(|l| !l.cells[ci].is_empty()) {
                    b.current = l.cells[ci].first().copied();
                } else {
                    b.current = cell.last().copied();
                }
            }
            KeyCode::Home => b.current = b.first_in(b.col),
            KeyCode::Esc => {
                b.program = None;
                b.project = None;
            }
            _ => {}
        }
    }

    pub fn board_action(&mut self, a: Action) {
        match a {
            Action::Swimlanes => {
                self.views.board.lanes = !self.views.board.lanes;
                self.views.board.scroll = 0;
            }
            Action::Scope => self.open_board_scope(),
            Action::CardLeft | Action::CardRight => {
                let Some(id) = self.views.board.current else { return };
                let Some(task) = self.r.tasks.get(&id) else { return };
                let i = task.status.index() as i64 + if a == Action::CardLeft { -1 } else { 1 };
                if !(0..5).contains(&i) {
                    return;
                }
                self.move_card(id, TaskStatus::ALL[i as usize], None);
            }
            _ => {}
        }
    }

    /// Change a card's status (and optionally its assignee) in one mutate.
    fn move_card(&mut self, id: i64, status: TaskStatus, assignee: Option<Option<i64>>) {
        let Some(task) = self.r.tasks.get(&id) else { return };
        let mut ops = vec![];
        if status != task.status {
            ops.push(Op::set(Entity::Task, id, "status", Value::from(task.status.as_str()), Value::from(status.as_str())));
        }
        if let Some(a) = assignee {
            if a != task.assignee_id {
                ops.push(Op::set(Entity::Task, id, "assignee_id", task.assignee_id.map_or(Value::Null, Value::from), a.map_or(Value::Null, Value::from)));
            }
        }
        let label = format!("{} → {}", self.r.task_ref(task), status.label());
        self.mutate(ops, label, PendKind::Cells, None);
        self.views.board.col = status.index();
    }

    pub fn board_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        let b = &mut self.views.board;
        match m.kind {
            MouseEventKind::ScrollDown => b.scroll = b.scroll.saturating_add(2),
            MouseEventKind::ScrollUp => b.scroll = b.scroll.saturating_sub(2),
            MouseEventKind::Down(btn) => {
                if rect_contains(b.scope_rect, x, y) {
                    self.open_board_scope();
                    return;
                }
                if let Some(&(_, id)) = b.cards.iter().find(|(r, _)| rect_contains(*r, x, y)) {
                    b.current = Some(id);
                    if btn == MouseButton::Right {
                        self.open_context_at(x, y);
                        return;
                    }
                    if btn == MouseButton::Left {
                        if self.is_double(x, y) {
                            self.open_record(Entity::Task, id, None);
                        } else {
                            self.drag = Some(Drag::Card { task: id });
                        }
                    }
                } else if let Some(&(_, ci)) = b.cols.iter().find(|(r, _)| rect_contains(*r, x, y)) {
                    b.col = ci;
                }
            }
            _ => {}
        }
    }

    fn board_target(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        let b = &self.views.board;
        let ci = b.cols.iter().find(|(r, _)| x >= r.x && x < r.right())?.1;
        let li = b.lane_rows.iter().find(|(a, z, _)| y >= *a && y < *z).map_or(0, |l| l.2);
        Some((li, ci))
    }

    pub fn board_drag(&mut self, x: u16, y: u16) {
        self.views.board.drop = self.board_target(x, y);
    }

    pub fn board_drop(&mut self, x: u16, y: u16) {
        let target = self.board_target(x, y);
        self.views.board.drop = None;
        let Some(Drag::Card { task }) = self.drag.take() else { return };
        let Some((li, ci)) = target else { return };
        let assignee = if self.views.board.lanes { self.views.board.model.get(li).map(|l| l.user) } else { None };
        self.move_card(task, TaskStatus::ALL[ci], assignee);
    }
}

