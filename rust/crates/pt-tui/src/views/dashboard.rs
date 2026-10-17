//! Dashboard (ui-spec §4.5): KPI tiles, bar charts, a workload heatmap, an
//! activity sparkline and an at-risk table. Every tile drills down into a
//! filtered grid with Enter (or a double-click).

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use pt_core::date::Date;
use pt_core::grid::fmt_hours;
use pt_core::model::{Health, ProjectStatus, TaskStatus};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::App;
use crate::theme::Theme;
use crate::views;
use crate::widgets::draw::{bar, fill, frame, pad, put, rect_contains};

#[derive(Default)]
pub struct DashView {
    pub focus: usize,
    tiles: Vec<Rect>,
}

const TILES: usize = 11;

/// (title, target view, query, group column)
fn drill(i: usize) -> (&'static str, usize, &'static str, Option<&'static str>) {
    match i {
        0 => ("Active projects", views::PROJECTS, "status:active", None),
        1 => ("Overdue tasks", views::TASKS, "is:overdue", None),
        2 => ("Blocked tasks", views::TASKS, "is:blocked", None),
        3 => ("Hours used / budget", views::PROJECTS, "is:open has:budget", None),
        4 => ("Due this week", views::TASKS, "is:open due<=eow", None),
        5 => ("Projects by status", views::PROJECTS, "", Some("status")),
        6 => ("Tasks by status", views::TASKS, "", Some("status")),
        7 => ("Hours by owner — budget vs actual", views::PROJECTS, "is:open", Some("owner_id")),
        8 => ("Workload: open estimated hours by week", views::TASKS, "is:open", Some("assignee_id")),
        9 => ("Activity — last 30 days", views::ACTIVITY, "", None),
        _ => ("At risk: red/amber health or burn > 90%", views::PROJECTS, "is:open health:red,amber", None),
    }
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    fill(buf, area, t.base());
    let r = &app.r;
    let today = r.today;
    let mut tiles: Vec<Rect> = Vec::with_capacity(TILES);
    // KPI row
    let kh = 5u16;
    let kw = area.width / 5;
    for i in 0..5 {
        tiles.push(Rect::new(area.x + i as u16 * kw, area.y, if i == 4 { area.width - 4 * kw } else { kw }, kh));
    }
    // 3 rows × 2 columns below
    let rest_y = area.y + kh;
    let rest_h = area.height.saturating_sub(kh);
    let rh = rest_h / 3;
    let cw = area.width / 2;
    for row in 0..3u16 {
        for col in 0..2u16 {
            let h = if row == 2 { rest_h - 2 * rh } else { rh };
            tiles.push(Rect::new(area.x + col * cw, rest_y + row * rh, if col == 1 { area.width - cw } else { cw }, h));
        }
    }
    let focus = app.views.dash.focus.min(TILES - 1);
    let open_tasks: Vec<&pt_core::model::Task> = r.tasks.values().filter(|t| t.status != TaskStatus::Done).collect();
    let active = r.projects.values().filter(|p| p.status == ProjectStatus::Active).count();
    let overdue = open_tasks.iter().filter(|t| r.task_overdue(t)).count();
    let blocked = open_tasks.iter().filter(|t| r.task_blocked(t)).count();
    let eow = pt_core::date::parse_natural("eow", today).ok().flatten().unwrap_or(today);
    let due_week = open_tasks.iter().filter(|t| t.due_date.as_deref().and_then(Date::parse_iso).is_some_and(|d| d >= today && d <= eow)).count();
    let (used, budget): (f64, f64) = r
        .projects
        .values()
        .filter(|p| r.project_open(p))
        .map(|p| (r.metrics(p.id).actual_total, p.budget_hours.unwrap_or(0.0)))
        .fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    let kpis: [(String, String, Color); 5] = [
        (active.to_string(), format!("of {} projects", r.projects.len()), t.ok),
        (overdue.to_string(), format!("of {} open tasks", open_tasks.len()), if overdue > 0 { t.err } else { t.ok }),
        (blocked.to_string(), "waiting or blocked".into(), if blocked > 0 { t.warn } else { t.ok }),
        (format!("{:.0}%", if budget > 0.0 { used * 100.0 / budget } else { 0.0 }), format!("{} / {} h", fmt_hours(used.round()), fmt_hours(budget.round())), t.info),
        (due_week.to_string(), format!("by {}", eow.short(today.year()).trim()), t.accent),
    ];
    for (i, (big, small, color)) in kpis.iter().enumerate() {
        let inner = frame(buf, tiles[i], drill(i).0, &t, focus == i);
        put(buf, inner.x + 1, inner.y, big, Style::new().fg(*color).bg(t.panel).add_modifier(Modifier::BOLD), inner.width as usize - 2);
        put(buf, inner.x + 1, inner.y + 1, small, Style::new().fg(t.muted).bg(t.panel), inner.width as usize - 2);
        if i == 3 && budget > 0.0 {
            let pct = used * 100.0 / budget;
            bar(buf, inner.x + 1, inner.y + 2, inner.width as usize - 2, pct.min(100.0), if pct > 100.0 { t.err } else if pct > 90.0 { t.warn } else { t.ok }, t.border);
        }
    }
    // 5: projects by status
    {
        let inner = frame(buf, tiles[5], drill(5).0, &t, focus == 5);
        let max = ProjectStatus::ALL.iter().map(|s| r.projects.values().filter(|p| p.status == *s).count()).max().unwrap_or(1).max(1);
        for (i, s) in ProjectStatus::ALL.iter().enumerate() {
            if i as u16 >= inner.height {
                break;
            }
            let n = r.projects.values().filter(|p| p.status == *s).count();
            let y = inner.y + i as u16;
            put(buf, inner.x + 1, y, &pad(s.label(), 10, false), Style::new().bg(t.panel).patch(t.project_status(*s)), 10);
            let bw = inner.width.saturating_sub(18) as usize;
            let color = t.project_status(*s).fg.unwrap_or(t.fg);
            bar(buf, inner.x + 12, y, bw, n as f64 * 100.0 / max as f64, color, t.panel);
            put(buf, inner.x + 13 + bw as u16, y, &n.to_string(), Style::new().bg(t.panel).fg(t.fg), 4);
        }
    }
    // 6: tasks by status — one stacked bar plus legend
    {
        let inner = frame(buf, tiles[6], drill(6).0, &t, focus == 6);
        let total = r.tasks.len().max(1);
        let w = inner.width.saturating_sub(2) as usize;
        let mut x = inner.x + 1;
        let mut ly = inner.y + 2;
        let mut lx = inner.x + 1;
        for &s in TaskStatus::ALL {
            let n = r.tasks.values().filter(|tk| tk.status == s).count();
            let seg = (n * w).div_ceil(total).min((inner.right() - 1 - x) as usize);
            let c = t.task_status_color(s);
            for i in 0..seg {
                put(buf, x + i as u16, inner.y, "█", Style::new().fg(c).bg(t.panel), 1);
                if inner.height > 4 {
                    put(buf, x + i as u16, inner.y + 1, "▀", Style::new().fg(c).bg(t.panel), 1);
                }
            }
            x += seg as u16;
            let label = format!("■ {} {n} ({:.0}%)  ", s.label(), n as f64 * 100.0 / total as f64);
            if lx + label.chars().count() as u16 > inner.right() {
                lx = inner.x + 1;
                ly += 1;
            }
            if ly < inner.bottom() {
                put(buf, lx, ly, &label, Style::new().fg(c).bg(t.panel), label.chars().count());
            }
            lx += label.chars().count() as u16;
        }
        let est: f64 = open_tasks.iter().map(|t| t.est_hours.unwrap_or(0.0)).sum();
        let act: f64 = r.tasks.values().map(|t| t.actual_hours).sum();
        if ly + 2 < inner.bottom() {
            put(buf, inner.x + 1, ly + 2, &format!("{} h estimated still open · {} h logged in total", fmt_hours(est.round()), fmt_hours(act.round())), Style::new().fg(t.muted).bg(t.panel), inner.width as usize - 2);
        }
    }
    // 7: hours by owner (budget vs actual)
    {
        let inner = frame(buf, tiles[7], drill(7).0, &t, focus == 7);
        let mut by: BTreeMap<String, (f64, f64, i64)> = BTreeMap::new();
        for p in r.projects.values().filter(|p| r.project_open(p)) {
            let e = by.entry(r.user_name(p.owner_id).to_string()).or_insert((0.0, 0.0, p.owner_id.unwrap_or(0)));
            e.0 += p.budget_hours.unwrap_or(0.0);
            e.1 += r.metrics(p.id).actual_total;
        }
        let max = by.values().map(|v| v.0.max(v.1)).fold(1.0, f64::max);
        let bw = inner.width.saturating_sub(30) as usize;
        for (i, (name, (b, a, uid))) in by.iter().enumerate() {
            if i as u16 >= inner.height {
                break;
            }
            let y = inner.y + i as u16;
            put(buf, inner.x + 1, y, &pad(if name.is_empty() { "(none)" } else { name }, 14, false), Style::new().fg(t.user(*uid)).bg(t.panel), 14);
            // budget as the track, actual as the fill
            bar(buf, inner.x + 16, y, bw, (a / max * 100.0).min(100.0), if a > b { t.err } else { t.info }, t.border);
            let budget_x = inner.x + 16 + ((b / max) * bw as f64).round().min(bw as f64 - 1.0) as u16;
            put(buf, budget_x, y, "┃", Style::new().fg(t.warn).bg(t.border), 1);
            put(buf, inner.x + 17 + bw as u16, y, &format!("{}/{}", fmt_hours(a.round()), fmt_hours(b.round())), Style::new().fg(t.muted).bg(t.panel), 12);
        }
    }
    // 8: workload heatmap
    {
        let inner = frame(buf, tiles[8], drill(8).0, &t, focus == 8);
        let week0 = today.week_start();
        let weeks = 8.min((inner.width.saturating_sub(14) / 6) as i32);
        for w in 0..weeks {
            let d = week0.add_days(w * 7);
            put(buf, inner.x + 14 + w as u16 * 6, inner.y, &format!("{:>5}", format!("{}/{}", d.day(), d.month())), Style::new().fg(t.muted).bg(t.panel), 5);
        }
        let mut users: Vec<&pt_core::model::User> = r.users.values().filter(|u| u.active && u.can_edit()).collect();
        users.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        for (i, u) in users.iter().enumerate() {
            let y = inner.y + 1 + i as u16;
            if y >= inner.bottom() {
                break;
            }
            put(buf, inner.x + 1, y, &u.display_name, Style::new().fg(t.user(u.id)).bg(t.panel), 12);
            for w in 0..weeks {
                let ws = week0.add_days(w * 7);
                let we = ws.add_days(6);
                let hours: f64 = open_tasks
                    .iter()
                    .filter(|tk| tk.assignee_id == Some(u.id))
                    .filter(|tk| tk.due_date.as_deref().and_then(Date::parse_iso).is_some_and(|d| d >= ws && d <= we))
                    .map(|tk| (tk.est_hours.unwrap_or(0.0) - tk.actual_hours).max(0.0))
                    .sum();
                let c = heat(hours, &t);
                let txt = if hours > 0.0 { format!("{:>4.0} ", hours) } else { "   · ".into() };
                put(buf, inner.x + 14 + w as u16 * 6, y, &txt, Style::new().bg(c).fg(if hours > 24.0 { Color::Black } else { t.fg }), 5);
            }
        }
    }
    // 9: activity sparkline + latest
    {
        let inner = frame(buf, tiles[9], drill(9).0, &t, focus == 9);
        let mut per_day = [0u64; 30];
        for a in r.activity.values() {
            if let Some(d) = a.ts.get(0..10).and_then(Date::parse_iso) {
                let age = today.days() - d.days();
                if (0..30).contains(&age) {
                    per_day[29 - age as usize] += 1;
                }
            }
        }
        let max = per_day.iter().copied().max().unwrap_or(1).max(1);
        const SPARK: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
        let mut x = inner.x + 1;
        for n in per_day {
            let i = ((n * 7) / max) as usize;
            let sym = if n == 0 { " " } else { SPARK[i] };
            put(buf, x, inner.y, sym, Style::new().fg(t.accent).bg(t.panel), 1);
            x += 1;
        }
        let total: u64 = per_day.iter().sum();
        put(buf, x + 1, inner.y, &format!("{total} changes · peak {max}/day"), Style::new().fg(t.muted).bg(t.panel), inner.right().saturating_sub(x + 2) as usize);
        for (i, a) in r.activity.values().rev().take(inner.height.saturating_sub(1) as usize).enumerate() {
            let y = inner.y + 1 + i as u16;
            let who = r.username(a.user_id);
            put(buf, inner.x + 1, y, &format!("{} {:<7} {}", a.ts.get(5..16).unwrap_or("").replace('T', " "), who, a.label), Style::new().fg(if a.state == pt_core::model::ActState::Undone { t.muted } else { t.fg }).bg(t.panel), inner.width as usize - 2);
        }
    }
    // 10: at-risk table
    {
        let inner = frame(buf, tiles[10], drill(10).0, &t, focus == 10);
        let mut risky: Vec<(&pt_core::model::Project, f64)> = r
            .projects
            .values()
            .filter(|p| r.project_open(p))
            .map(|p| (p, r.burn_pct(p).unwrap_or(0.0)))
            .filter(|(p, b)| p.health != Health::Green || *b > 90.0)
            .collect();
        risky.sort_by(|a, b| b.0.health.index().cmp(&a.0.health.index()).then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)));
        put(buf, inner.x + 1, inner.y, &format!("{:<9} {:<22} {:>6} {:>5}", "Code", "Name", "Burn", "Late"), Style::new().fg(t.muted).bg(t.panel).add_modifier(Modifier::BOLD), inner.width as usize - 2);
        for (i, (p, burn)) in risky.iter().take(10).enumerate() {
            let y = inner.y + 1 + i as u16;
            if y >= inner.bottom() {
                break;
            }
            let hc = t.health_color(p.health);
            put(buf, inner.x + 1, y, "●", Style::new().fg(hc).bg(t.panel), 1);
            let m = r.metrics(p.id);
            put(buf, inner.x + 3, y, &format!("{:<8} {:<22} {:>5.0}% {:>5}", p.code, crate::widgets::draw::fit(&p.name, 22), burn, m.tasks_overdue), Style::new().fg(t.fg).bg(t.panel).patch(if *burn > 100.0 { Style::new().fg(t.err) } else { Style::new() }), inner.width as usize - 4);
        }
        if risky.is_empty() {
            put(buf, inner.x + 1, inner.y + 1, "✓ Nothing at risk", Style::new().fg(t.ok).bg(t.panel), 30);
        }
    }
    app.views.dash.tiles = tiles;
}

fn heat(h: f64, t: &Theme) -> Color {
    if h <= 0.0 {
        t.panel
    } else if h <= 8.0 {
        t.sel_bg
    } else if h <= 24.0 {
        t.info
    } else if h <= 40.0 {
        t.warn
    } else {
        t.err
    }
}

impl App {
    pub fn dashboard_key(&mut self, k: KeyEvent) {
        let d = &mut self.views.dash;
        d.focus = match k.code {
            KeyCode::Tab | KeyCode::Right => (d.focus + 1) % TILES,
            KeyCode::BackTab | KeyCode::Left => (d.focus + TILES - 1) % TILES,
            KeyCode::Down => match d.focus {
                0..=4 => 5 + (d.focus >= 3) as usize,
                f if f + 2 < TILES => f + 2,
                f => f,
            },
            KeyCode::Up => match d.focus {
                5 | 6 => (d.focus - 5) * 3,
                f if f >= 7 => f - 2,
                f => f,
            },
            _ => d.focus,
        };
    }

    pub fn dashboard_drill(&mut self) {
        let (_, view, query, group) = drill(self.views.dash.focus);
        self.switch_view(view);
        self.queries[view].input.set(query);
        let grid = match view {
            views::PROJECTS => Some(&mut self.views.projects.main),
            views::TASKS => Some(&mut self.views.tasks),
            _ => None,
        };
        if let Some(g) = grid {
            g.group = group.map(String::from).or(if view == views::TASKS { Some("project".into()) } else { None });
            g.collapsed.clear();
            if self.views.dash.focus == 3 || self.views.dash.focus == 10 {
                g.sort = vec![pt_core::grid::SortSpec { key: "burn".into(), desc: true }];
            }
            g.touch();
            g.cur = 0;
        }
        self.set_status(format!("Drilled into: {}", drill(self.views.dash.focus).0), crate::app::Level::Info);
    }

    pub fn dashboard_mouse(&mut self, m: MouseEvent) {
        if let MouseEventKind::Down(MouseButton::Left) = m.kind {
            if let Some(i) = self.views.dash.tiles.iter().position(|r| rect_contains(*r, m.column, m.row)) {
                let dbl = self.is_double(m.column, m.row);
                self.views.dash.focus = i;
                if dbl {
                    self.dashboard_drill();
                }
            }
        }
    }
}
