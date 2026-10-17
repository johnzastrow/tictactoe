//! Read-only record card for the current task (Tasks view detail pane):
//! every field, dependencies with their status, notes and recent activity.

use pt_core::date::Date;
use pt_core::grid::{Style as CS, fmt_hours};
use pt_core::model::TaskStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::theme::Theme;
use crate::widgets::draw::{bar, fill, frame, put};

pub fn render(app: &App, buf: &mut Buffer, area: Rect, id: Option<i64>) {
    let t = &app.t;
    let r = &app.r;
    let Some(task) = id.and_then(|id| r.tasks.get(&id)) else {
        let inner = frame(buf, area, "Task", t, false);
        put(buf, inner.x + 1, inner.y + 1, "Select a task row to see its card.", t.muted().bg(t.panel), inner.width as usize - 2);
        return;
    };
    let title = format!("{}  ^E edits", r.task_ref(task));
    let inner = frame(buf, area, &title, t, false);
    fill(buf, inner, t.panel());
    let w = inner.width as usize;
    let p = Style::new().bg(t.panel).fg(t.fg);
    let lbl = Style::new().bg(t.panel).fg(t.muted);
    let mut y = inner.y;
    let line = |buf: &mut Buffer, y: &mut u16, label: &str, value: &str, st: Style| {
        if *y >= inner.bottom() {
            return;
        }
        put(buf, inner.x + 1, *y, label, lbl, 10);
        put(buf, inner.x + 11, *y, value, st, w.saturating_sub(12));
        *y += 1;
    };
    put(buf, inner.x + 1, y, &task.title, p.add_modifier(Modifier::BOLD), w - 2);
    y += 1;
    let project = r.projects.get(&task.project_id);
    if let Some(pr) = project {
        put(buf, inner.x + 1, y, &format!("{} · {}", pr.name, r.program_name(pr.program_id)), lbl, w - 2);
        y += 1;
    }
    y += 1;
    let blocked = r.task_blocked(task);
    let status_style = p.patch(t.cell(&CS::TaskStatus(if blocked { TaskStatus::Blocked } else { task.status })));
    let glyph = Theme::glyph(&CS::TaskStatus(task.status)).unwrap_or("");
    let status = format!("{glyph}{}{}", task.status.label(), if blocked && task.status != TaskStatus::Blocked { " (waiting on deps)" } else { "" });
    line(buf, &mut y, "Status", &status, status_style);
    line(buf, &mut y, "Priority", task.priority.label(), p.patch(t.priority(task.priority)));
    let who = r.user_name(task.assignee_id);
    line(buf, &mut y, "Assignee", if who.is_empty() { "(nobody)" } else { who }, task.assignee_id.map_or(lbl, |u| p.fg(t.user(u))));
    line(buf, &mut y, "Phase", &task.phase, p);
    let fmt_d = |d: &Option<String>| d.as_deref().and_then(Date::parse_iso).map_or("—".to_string(), |d| d.long());
    line(buf, &mut y, "Start", &fmt_d(&task.start_date), p);
    let due_style = if r.task_overdue(task) { p.fg(t.err) } else { p };
    line(buf, &mut y, "Due", &fmt_d(&task.due_date), due_style);
    // hours bar
    if y < inner.bottom() {
        let est = task.est_hours.unwrap_or(0.0);
        put(buf, inner.x + 1, y, "Hours", lbl, 10);
        let pct = if est > 0.0 { task.actual_hours * 100.0 / est } else { 0.0 };
        let bw = (w.saturating_sub(30)).clamp(4, 20);
        let color = if pct > 100.0 { t.err } else if pct > 90.0 { t.warn } else { t.ok };
        bar(buf, inner.x + 11, y, bw, pct.min(100.0), color, t.border);
        put(buf, inner.x + 12 + bw as u16, y, &format!("{} / {} h", fmt_hours(task.actual_hours), task.est_hours.map_or("?".into(), fmt_hours)), p, 16);
        y += 1;
    }
    if !task.tags.is_empty() {
        line(buf, &mut y, "Tags", &task.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "), p.fg(t.accent2));
    }
    for cf in r.custom_fields_for(pt_core::model::CfEntity::Task) {
        if let Some(v) = task.cf.get(&cf.id.to_string()) {
            let s = v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
            line(buf, &mut y, &cf.name, &s, p);
        }
    }
    if !task.deps.is_empty() && y < inner.bottom() {
        y += 1;
        put(buf, inner.x + 1, y, "Waits for", lbl.add_modifier(Modifier::BOLD), 12);
        y += 1;
        for d in task.deps.iter().filter_map(|d| r.tasks.get(d)) {
            if y >= inner.bottom() {
                break;
            }
            let st = p.patch(t.cell(&CS::TaskStatus(d.status)));
            let g = Theme::glyph(&CS::TaskStatus(d.status)).unwrap_or("");
            put(buf, inner.x + 2, y, &format!("{g}#{} {}", d.seq, d.title), st, w - 3);
            y += 1;
        }
    }
    let dependents: Vec<_> = r.project_tasks(task.project_id).iter().filter_map(|i| r.tasks.get(i)).filter(|x| x.deps.contains(&task.id)).collect();
    if !dependents.is_empty() && y + 1 < inner.bottom() {
        put(buf, inner.x + 1, y, &format!("Blocks: {}", dependents.iter().map(|d| format!("#{}", d.seq)).collect::<Vec<_>>().join(" ")), lbl, w - 2);
        y += 1;
    }
    if !task.notes.trim().is_empty() && y + 1 < inner.bottom() {
        y += 1;
        put(buf, inner.x + 1, y, "Notes", lbl.add_modifier(Modifier::BOLD), 10);
        y += 1;
        for l in wrap(&task.notes, w - 3) {
            if y >= inner.bottom() {
                break;
            }
            put(buf, inner.x + 2, y, &l, p, w - 3);
            y += 1;
        }
    }
    let recent: Vec<_> = r.activity.values().rev().filter(|a| a.entity == "task" && a.entity_id == task.id).take(4).collect();
    if !recent.is_empty() && y + 2 < inner.bottom() {
        y += 1;
        put(buf, inner.x + 1, y, "Recent changes", lbl.add_modifier(Modifier::BOLD), 20);
        y += 1;
        for a in recent {
            if y >= inner.bottom() {
                break;
            }
            let who = r.username(a.user_id);
            put(buf, inner.x + 2, y, &format!("{} {}: {}", a.ts.get(5..10).unwrap_or(""), who, a.label), lbl, w - 3);
            y += 1;
        }
    }
}

/// Greedy word wrap by display columns.
pub fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out = vec![];
    for para in s.lines() {
        let mut line = String::new();
        for word in para.split_whitespace() {
            if !line.is_empty() && crate::widgets::draw::width(&line) + 1 + crate::widgets::draw::width(word) > w {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        out.push(line);
    }
    out
}
