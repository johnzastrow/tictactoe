//! Screen layout (ui-spec §1): menu bar, tab bar, query bar, the view,
//! status line, nano-style hint bar, toasts and overlays. Rendering records
//! hit rectangles in `app.hits` for mouse handling.

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use pt_core::grid::GridRow;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, Level, Phase};
use crate::dialogs;
use crate::grid::{Decor, GridState};
use crate::keymap::{self, Action};
use crate::net::ConnState;
use crate::theme::Theme;
use crate::views;
use crate::widgets::draw::{fill, frame, put, rect_contains, shadow, width};

pub struct MenuState {
    pub open: usize,
    pub sel: usize,
}

#[derive(Clone, Debug)]
pub enum MenuAct {
    Action(Action),
    Filter(usize),
    Sep,
}

pub const MENUS: [(&str, char); 5] = [("File", 'f'), ("Edit", 'e'), ("View", 'v'), ("Data", 'd'), ("Help", 'h')];

pub fn menu_for_letter(c: char) -> Option<usize> {
    MENUS.iter().position(|(_, l)| *l == c.to_ascii_lowercase())
}

pub fn has_query_bar(view: usize) -> bool {
    view != views::DASHBOARD
}

pub fn menu_items(app: &App, i: usize) -> Vec<(String, String, MenuAct)> {
    let ctx = app.ctx();
    let a = |act: Action| (act.label().to_string(), keymap::key_for(act, ctx).unwrap_or_default(), MenuAct::Action(act));
    let sep = || (String::new(), String::new(), MenuAct::Sep);
    match i {
        0 => vec![a(Action::NewProject), a(Action::NewTask), sep(), a(Action::SetPassword), a(Action::Logout), a(Action::Quit)],
        1 => vec![
            a(Action::Undo),
            a(Action::Redo),
            sep(),
            a(Action::Copy),
            a(Action::Paste),
            a(Action::FillDown),
            a(Action::Clear),
            sep(),
            a(Action::Edit),
            a(Action::EditRecord),
            a(Action::Delete),
            a(Action::MarkAll),
        ],
        2 => {
            let mut v: Vec<(String, String, MenuAct)> = (0..9).map(|n| a(Action::View(n))).collect();
            v.push(sep());
            v.push(a(Action::ToggleDetail));
            v.push(a(Action::Columns));
            v.push((format!("Theme ▸ {}", app.t.name), "^T".into(), MenuAct::Action(Action::Theme)));
            v.push(a(Action::Palette));
            v.push(a(Action::Redraw));
            v
        }
        3 => {
            let mut v = vec![
                a(Action::Filter),
                a(Action::ClearFilter),
                sep(),
                a(Action::SortColumn),
                a(Action::SortDialog),
                a(Action::GroupColumn),
                a(Action::ClearGroup),
                sep(),
                a(Action::SaveFilter),
            ];
            for (n, f) in app.saved_filters.iter().enumerate() {
                v.push((format!("  {} ({})", f.name, keymap::VIEW_NAMES[f.view]), String::new(), MenuAct::Filter(n)));
            }
            v
        }
        _ => vec![a(Action::Help), a(Action::Palette), a(Action::About)],
    }
}

impl App {
    fn run_menu(&mut self, act: MenuAct) {
        self.menu = None;
        match act {
            MenuAct::Action(a) => self.action(a),
            MenuAct::Filter(i) => self.apply_filter(i),
            MenuAct::Sep => {}
        }
    }

    pub fn menu_key(&mut self, k: KeyEvent) {
        let Some(m) = self.menu.as_mut() else { return };
        let open = m.open;
        let items = menu_items(self, open);
        let m = self.menu.as_mut().unwrap();
        let n = items.len();
        let step = |from: usize, down: bool| -> usize {
            let mut i = from;
            for _ in 0..n {
                i = if down { (i + 1) % n } else { (i + n - 1) % n };
                if !matches!(items[i].2, MenuAct::Sep) {
                    break;
                }
            }
            i
        };
        match k.code {
            KeyCode::Esc | KeyCode::F(10) => self.menu = None,
            KeyCode::Left => {
                m.open = (m.open + MENUS.len() - 1) % MENUS.len();
                m.sel = 0;
            }
            KeyCode::Right => {
                m.open = (m.open + 1) % MENUS.len();
                m.sel = 0;
            }
            KeyCode::Up => m.sel = step(m.sel, false),
            KeyCode::Down => m.sel = step(m.sel, true),
            KeyCode::Enter => {
                let act = items[m.sel].2.clone();
                self.run_menu(act);
            }
            KeyCode::Char(c) => {
                let c = c.to_ascii_lowercase();
                if let Some(it) = items.iter().find(|it| it.0.trim_start().to_lowercase().starts_with(c) && !matches!(it.2, MenuAct::Sep)) {
                    let act = it.2.clone();
                    self.run_menu(act);
                }
            }
            _ => {}
        }
    }

    pub fn menu_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        if let Some(&(_, i)) = self.hits.menus.iter().find(|(r, _)| rect_contains(*r, x, y)) {
            if matches!(m.kind, MouseEventKind::Down(_)) || matches!(m.kind, MouseEventKind::Moved) {
                if let Some(ms) = self.menu.as_mut() {
                    if ms.open != i {
                        ms.open = i;
                        ms.sel = 0;
                    } else if matches!(m.kind, MouseEventKind::Down(_)) {
                        self.menu = None;
                    }
                }
            }
            return;
        }
        let Some(ms) = &self.menu else { return };
        let items = menu_items(self, ms.open);
        let r = menu_rect(self, ms.open, &items);
        if rect_contains(r, x, y) && y > r.y && y < r.bottom() - 1 {
            let row = (y - r.y - 1) as usize;
            if row < items.len() && !matches!(items[row].2, MenuAct::Sep) {
                if let Some(ms) = self.menu.as_mut() {
                    ms.sel = row;
                }
                if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
                    let act = items[row].2.clone();
                    self.run_menu(act);
                }
            }
        } else if matches!(m.kind, MouseEventKind::Down(_)) {
            self.menu = None;
        }
    }
}

fn menu_rect(app: &App, i: usize, items: &[(String, String, MenuAct)]) -> Rect {
    let x = app.hits.menus.iter().find(|(_, j)| *j == i).map_or(0, |(r, _)| r.x);
    let w = items.iter().map(|(l, k, _)| width(l) + width(k) + 6).max().unwrap_or(20).max(22) as u16;
    Rect::new(x, 1, w, items.len() as u16 + 2)
}

pub fn draw(app: &mut App, f: &mut Frame) {
    let area = f.area();
    let t = app.t.clone();
    let buf = f.buffer_mut();
    fill(buf, area, t.base());
    app.hits = Default::default();
    if area.width < 80 || area.height < 24 {
        let msg = format!("pt needs at least 80×24 — this window is {}×{}. Make it bigger, or ^Q to quit.", area.width, area.height);
        put(buf, area.x + 1, area.y + area.height / 2, &msg, Style::new().fg(t.warn).bg(t.bg), area.width.saturating_sub(2) as usize);
        return;
    }
    if app.phase == Phase::Login {
        draw_splash(buf, area, &t);
    } else {
        app.prepare();
        draw_main(app, buf, area);
    }
    // dialogs
    let mut ds = std::mem::take(&mut app.dialogs);
    for d in ds.iter_mut() {
        dialogs::render(d, buf, area, app);
    }
    app.dialogs = ds;
    if app.phase == Phase::Login {
        draw_hints(app, buf, Rect::new(area.x, area.bottom() - 1, area.width, 1));
    }
}

fn draw_splash(buf: &mut Buffer, area: Rect, t: &Theme) {
    let logo = [
        r"          __",
        r"   ____  / /_",
        r"  / __ \/ __/",
        r" / /_/ / /_",
        r" / .___/\__/",
        r"/_/   projects & tasks",
    ];
    for (i, l) in logo.iter().enumerate() {
        put(buf, area.x + 3, area.y + 1 + i as u16, l, Style::new().fg(t.accent).bg(t.bg).add_modifier(Modifier::BOLD), 40);
    }
}

fn draw_main(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    // menu bar
    let mb = Style::new().bg(t.header_bg).fg(t.fg);
    fill(buf, Rect::new(area.x, area.y, area.width, 1), mb);
    let mut x = area.x + 1;
    for (i, (name, _)) in MENUS.iter().enumerate() {
        let open = app.menu.as_ref().is_some_and(|m| m.open == i);
        let st = if open { Style::new().bg(t.accent).fg(t.bg) } else { mb };
        let r = Rect::new(x, area.y, width(name) as u16 + 2, 1);
        fill(buf, r, st);
        put(buf, x + 1, area.y, &name[..1], st.add_modifier(Modifier::UNDERLINED | Modifier::BOLD), 1);
        put(buf, x + 2, area.y, &name[1..], st, 10);
        app.hits.menus.push((r, i));
        x += r.width + 1;
    }
    // right side: user, connection
    let (dot, dot_c, state) = match app.conn_state {
        ConnState::Online if app.r.is_loaded() => ("●", t.ok, ""),
        ConnState::Online | ConnState::Connecting => ("●", t.warn, " connecting…"),
        ConnState::Reconnecting => ("●", if (app.start_time.elapsed().as_millis() / 500) % 2 == 0 { t.warn } else { t.err }, " reconnecting…"),
    };
    let me = app.r.me.as_ref().map(|u| format!("{} ({})", u.username, u.role.as_str())).unwrap_or_default();
    let srv = app.r.server.as_ref().map(|s| format!("{} {}", s.implementation, s.version)).unwrap_or_default();
    let right = format!("pt · {me} · {srv}{state} ");
    let rw = width(&right) as u16 + 2;
    let rx = area.right().saturating_sub(rw);
    put(buf, rx, area.y, dot, mb.fg(dot_c), 1);
    put(buf, rx + 2, area.y, &right, mb.fg(t.muted), rw as usize);

    // tabs
    let ty = area.y + 1;
    fill(buf, Rect::new(area.x, ty, area.width, 1), Style::new().bg(t.bg));
    let mut x = area.x;
    // narrow terminals get short tab names so all nine fit
    const SHORT: [&str; 9] = ["Proj", "Tasks", "Board", "Time", "Dash", "Act", "Tmpl", "Fields", "Users"];
    let full: usize = keymap::VIEW_NAMES.iter().enumerate().map(|(i, n)| width(n) + width(&(i + 1).to_string()) + 4).sum();
    let compact = full > area.width as usize;
    let names: &[&str] = if compact { &SHORT } else { &keymap::VIEW_NAMES };
    let gap = u16::from(!compact);
    for (i, name) in names.iter().enumerate() {
        let label = format!(" {} {} ", i + 1, name);
        let w = width(&label) as u16;
        if x + w >= area.right() {
            break;
        }
        let active = i == app.view;
        let st = if active {
            Style::new().bg(t.accent).fg(t.bg).add_modifier(Modifier::BOLD)
        } else if app.hover.is_some_and(|(hx, hy)| hy == ty && hx >= x && hx < x + w) {
            Style::new().bg(t.sel_bg).fg(t.fg)
        } else {
            Style::new().bg(t.panel).fg(t.fg)
        };
        put(buf, x, ty, &label, st, w as usize);
        put(buf, x + 1, ty, &(i + 1).to_string(), st.fg(if active { t.bg } else { t.accent }), 1);
        app.hits.tabs.push((Rect::new(x, ty, w, 1), i));
        x += w + gap;
    }

    // query bar
    let qy = area.y + 2;
    draw_query_bar(app, buf, Rect::new(area.x, qy, area.width, 1));

    // bottom: status + hints
    let hint_lines = 1u16;
    let status_y = area.bottom() - 1 - hint_lines;
    let main = Rect::new(area.x, qy + 1, area.width, status_y.saturating_sub(qy + 1));
    app.hits.main = main;

    if !app.r.is_loaded() {
        let msg = match app.conn_state {
            ConnState::Reconnecting => format!("Can't reach {} — retrying…", app.url.base()),
            _ => format!("Connecting to {}…", app.url.base()),
        };
        put(buf, main.x + 2, main.y + 2, &msg, Style::new().fg(t.muted).bg(t.bg), main.width as usize - 4);
    } else {
        draw_view(app, buf, main);
    }
    draw_status(app, buf, Rect::new(area.x, status_y, area.width, 1));
    draw_hints(app, buf, Rect::new(area.x, status_y + 1, area.width, hint_lines));

    // query suggestions
    if app.focus_query {
        let qb = &app.queries[app.view];
        if !qb.sugg.is_empty() {
            let w = qb.sugg.iter().map(|s| width(s)).max().unwrap_or(10).max(16) as u16 + 4;
            let x = (area.x + 10 + qb.input.cursor as u16).min(area.right().saturating_sub(w));
            let r = Rect::new(x, qy + 1, w, (qb.sugg.len() as u16 + 2).min(14));
            shadow(buf, r);
            let inner = frame(buf, r, "Tab", &t, true);
            for (i, s) in qb.sugg.iter().enumerate().take(inner.height as usize) {
                let sel = qb.sel == i + 1;
                let st = if sel { Style::new().bg(t.cur_bg).fg(t.fg) } else { t.panel() };
                fill(buf, Rect::new(inner.x, inner.y + i as u16, inner.width, 1), st);
                put(buf, inner.x + 1, inner.y + i as u16, s, st, inner.width as usize - 2);
            }
        }
    }
    // menus
    if let Some(ms) = &app.menu {
        let items = menu_items(app, ms.open);
        let r = menu_rect(app, ms.open, &items);
        shadow(buf, r);
        let inner = frame(buf, r, "", &t, true);
        for (i, (label, key, act)) in items.iter().enumerate() {
            let y = inner.y + i as u16;
            if matches!(act, MenuAct::Sep) {
                put(buf, inner.x, y, &"─".repeat(inner.width as usize), Style::new().fg(t.border).bg(t.panel), inner.width as usize);
                continue;
            }
            let sel = i == ms.sel;
            let st = if sel { Style::new().bg(t.cur_bg).fg(t.fg) } else { t.panel() };
            fill(buf, Rect::new(inner.x, y, inner.width, 1), st);
            let lx = inner.x + 1;
            put(buf, lx, y, label, st, inner.width as usize - 2);
            if let Some(c) = label.trim_start().chars().next() {
                let off = (width(label) - width(label.trim_start())) as u16;
                put(buf, lx + off, y, &c.to_string(), st.add_modifier(Modifier::UNDERLINED | Modifier::BOLD), 1);
            }
            put(buf, inner.right().saturating_sub(width(key) as u16 + 1), y, key, st.fg(t.muted), 12);
        }
    }
    // toasts (top right)
    draw_toasts(app, buf, area);
}

fn draw_query_bar(app: &mut App, buf: &mut Buffer, r: Rect) {
    let t = app.t.clone();
    let st = Style::new().bg(t.panel).fg(t.fg);
    fill(buf, r, st);
    if !has_query_bar(app.view) {
        let today = app.r.today.long();
        put(buf, r.x + 1, r.y, &format!("Dashboard · {today} · Tab/arrows move between tiles, Enter drills down"), st.fg(t.muted), r.width as usize - 2);
        return;
    }
    let focused = app.focus_query;
    let label = if focused { "Filter ▸ " } else { "Filter ▹ " };
    put(buf, r.x + 1, r.y, label, st.fg(if focused { t.accent } else { t.muted }).add_modifier(Modifier::BOLD), 9);
    // right side: counts / sort / group
    let (shown, total, sort, group) = view_counts(app);
    let mut right = format!("[ {shown} / {total} ]");
    if let Some(s) = sort {
        right.push_str(&format!("  ⇅ {s}"));
    }
    if let Some(g) = group {
        right.push_str(&format!("  ⊞ {g}"));
    }
    let rw = width(&right) as u16;
    let ix = r.x + 10;
    let iw = r.width.saturating_sub(12 + rw + 2);
    let input_rect = Rect::new(ix, r.y, iw, 1);
    app.hits.query = input_rect;
    let qb = &mut app.queries[app.view];
    let text_empty = qb.input.is_empty();
    qb.input.render(buf, input_rect, if focused { Style::new().bg(t.sel_bg).fg(t.fg) } else { st }, focused);
    if text_empty && !focused {
        let hint = match app.view {
            views::PROJECTS | views::TIMELINE => "^F  e.g.  status:active @me health:red due<+30d #q4",
            views::TASKS | views::BOARD => "^F  e.g.  is:open @me due<eow -status:done project:nwr",
            views::ACTIVITY => "^F  e.g.  @bob action:set label text",
            _ => "^F  type to filter rows",
        };
        put(buf, ix, r.y, hint, st.fg(t.muted).add_modifier(Modifier::ITALIC), iw as usize);
    }
    // underline bad terms (when the text fits without scrolling)
    let qb = &app.queries[app.view];
    if width(&qb.input.text()) < iw as usize {
        for e in &qb.query.errors {
            for c in e.start..e.end {
                if let Some(cell) = buf.cell_mut((ix + c as u16, r.y)) {
                    cell.set_style(Style::new().fg(t.err).add_modifier(Modifier::UNDERLINED));
                }
            }
        }
    }
    put(buf, r.right().saturating_sub(rw + 1), r.y, &right, st.fg(t.muted), rw as usize);
    if let Some(e) = qb.query.errors.first() {
        let msg = format!("✗ {}", e.message);
        let mw = width(&msg).min(40) as u16;
        let x = r.right().saturating_sub(rw + mw + 3);
        if x > ix + width(&qb.input.text()) as u16 + 2 {
            put(buf, x, r.y, &msg, st.fg(t.err), mw as usize);
        }
    }
}

fn view_counts(app: &App) -> (usize, usize, Option<String>, Option<String>) {
    let r = &app.r;
    let grid_info = |g: &GridState| {
        let sort = g.sort.first().and_then(|s| g.cols.iter().find(|c| c.key == s.key).map(|c| format!("{} {}", c.title, if s.desc { "▼" } else { "▲" })));
        let group = g.group.as_ref().and_then(|k| g.cols.iter().find(|c| &c.key == k).map(|c| c.title.clone()));
        (sort, group)
    };
    match app.view {
        views::PROJECTS => {
            let (s, g) = grid_info(&app.views.projects.main);
            (app.views.projects.main.ids.len(), r.projects.len(), s, g)
        }
        views::TASKS => {
            let (s, g) = grid_info(&app.views.tasks);
            (app.views.tasks.ids.len(), r.tasks.len(), s, g)
        }
        views::ACTIVITY => (app.views.activity.ids.len(), r.activity.len(), None, None),
        views::BOARD => (app.views.board.shown, r.tasks.len(), None, if app.views.board.lanes { Some("Assignee".into()) } else { None }),
        views::TIMELINE => (app.views.timeline.shown, r.projects.len(), None, None),
        views::TEMPLATES => (app.views.templates.main.ids.len(), r.templates.len(), None, None),
        views::FIELDS => (app.views.fields.ids.len(), r.custom_fields.len(), None, None),
        views::USERS => (app.views.users.main.ids.len(), r.users.len(), None, None),
        _ => (0, 0, None, None),
    }
}

/// A one-line pane title.
fn pane_title(buf: &mut Buffer, r: Rect, title: &str, focused: bool, t: &Theme) {
    let st = if focused { Style::new().bg(t.sel_bg).fg(t.accent).add_modifier(Modifier::BOLD) } else { Style::new().bg(t.panel).fg(t.muted) };
    fill(buf, r, st);
    put(buf, r.x + 1, r.y, title, st, r.width as usize - 2);
}

fn draw_grid(app: &mut App, buf: &mut Buffer, sel: views::GridSel, area: Rect, focused: bool) {
    let presence = app.presence_map();
    let conflicts = app.conflicts.keys().cloned().collect();
    let editing = app.editor.as_ref().filter(|e| e.grid == sel).map(|e| (e.row, e.vcol));
    let d = Decor {
        focused: focused && !app.focus_query,
        editing,
        conflicts: &conflicts,
        pending: &app.pending_cells,
        flashes: &app.flashes,
        presence: &presence,
    };
    let t = app.t.clone();
    let r = &app.r;
    let g = app.views.grid(sel);
    g.render(buf, area, r, &t, &d);
}

fn draw_view(app: &mut App, buf: &mut Buffer, main: Rect) {
    let t = app.t.clone();
    use views::GridSel as G;
    match app.view {
        views::PROJECTS => {
            let show = app.views.projects.show_detail;
            let pane = app.views.projects.pane;
            if !show {
                draw_grid(app, buf, G::Projects, main, true);
            } else {
                let wide = main.width >= 160;
                let (a, b) = if wide {
                    let lw = main.width * 11 / 20;
                    (Rect::new(main.x, main.y, lw, main.height), Rect::new(main.x + lw + 1, main.y, main.width - lw - 1, main.height))
                } else {
                    let th = main.height * 11 / 20;
                    (Rect::new(main.x, main.y, main.width, th), Rect::new(main.x, main.y + th, main.width, main.height - th))
                };
                draw_grid(app, buf, G::Projects, a, pane == 0);
                if wide {
                    for y in main.y..main.bottom() {
                        put(buf, a.right(), y, "│", Style::new().fg(t.border).bg(t.bg), 1);
                    }
                }
                let title = match app.views.projects.main.current_id().and_then(|id| app.r.projects.get(&id)) {
                    Some(p) => {
                        let m = app.r.metrics(p.id);
                        format!("Tasks of {} {} — {}/{} done, {} h of {} h · F6 to switch panes · Ins adds a task", p.code, p.name, m.tasks_done, m.tasks_total, pt_core::grid::fmt_hours(m.actual_total), p.budget_hours.map_or("?".into(), pt_core::grid::fmt_hours))
                    }
                    None => "Tasks".into(),
                };
                pane_title(buf, Rect::new(b.x, b.y, b.width, 1), &title, pane == 1, &t);
                draw_grid(app, buf, G::ProjectTasks, Rect::new(b.x, b.y + 1, b.width, b.height.saturating_sub(1)), pane == 1);
            }
        }
        views::TASKS => {
            if app.views.tasks_card {
                let wide = main.width >= 140;
                let (a, b) = if wide {
                    let cw = 46.min(main.width / 3);
                    (Rect::new(main.x, main.y, main.width - cw, main.height), Rect::new(main.right() - cw, main.y, cw, main.height))
                } else {
                    let ch = 9.min(main.height / 3);
                    (Rect::new(main.x, main.y, main.width, main.height - ch), Rect::new(main.x, main.bottom() - ch, main.width, ch))
                };
                draw_grid(app, buf, G::Tasks, a, true);
                let id = app.views.tasks.current_id();
                crate::views::card::render(app, buf, b, id);
            } else {
                draw_grid(app, buf, G::Tasks, main, true);
            }
        }
        views::BOARD => crate::views::board::render(app, buf, main),
        views::TIMELINE => crate::views::timeline::render(app, buf, main),
        views::DASHBOARD => crate::views::dashboard::render(app, buf, main),
        views::ACTIVITY => draw_grid(app, buf, G::Activity, main, true),
        views::FIELDS => {
            pane_title(buf, Rect::new(main.x, main.y, main.width, 1), "Custom fields — adding one adds a column to every client's grid. Ins adds (admin).", true, &t);
            draw_grid(app, buf, G::Fields, Rect::new(main.x, main.y + 1, main.width, main.height - 1), true);
        }
        views::TEMPLATES => {
            let pane = app.views.templates.pane;
            let th = (main.height / 3).max(5);
            let gantt_h = if main.height > 24 { 9 } else { 0 };
            let a = Rect::new(main.x, main.y, main.width, th);
            let b = Rect::new(main.x, main.y + th, main.width, main.height - th - gantt_h);
            draw_grid(app, buf, G::Templates, a, pane == 0);
            let name = app.views.templates.main.current_id().and_then(|id| app.r.templates.get(&id)).map(|t| t.name.clone()).unwrap_or_default();
            pane_title(buf, Rect::new(b.x, b.y, b.width, 1), &format!("Tasks in template “{name}” — F6 to switch panes · Ins adds a task"), pane == 1, &t);
            draw_grid(app, buf, G::TemplateTasks, Rect::new(b.x, b.y + 1, b.width, b.height.saturating_sub(1)), pane == 1);
            if gantt_h > 0 {
                let g = Rect::new(main.x, main.bottom() - gantt_h, main.width, gantt_h);
                crate::views::timeline::render_template_preview(app, buf, g);
            }
        }
        views::USERS => {
            let pane = app.views.users.pane;
            let th = main.height * 3 / 5;
            let a = Rect::new(main.x, main.y + 1, main.width, th - 1);
            let b = Rect::new(main.x, main.y + th, main.width, main.height - th);
            pane_title(buf, Rect::new(main.x, main.y, main.width, 1), "Users — Ins adds (admin) · right-click ▸ Set password · users are deactivated, never deleted", pane == 0, &t);
            draw_grid(app, buf, G::Users, a, pane == 0);
            pane_title(buf, Rect::new(b.x, b.y, b.width, 1), "Programs (clients) — F6 to switch panes · Ins adds (admin)", pane == 1, &t);
            draw_grid(app, buf, G::Programs, Rect::new(b.x, b.y + 1, b.width, b.height.saturating_sub(1)), pane == 1);
        }
        _ => {}
    }
    // inline editor on top of its cell
    if let Some(ce) = app.editor.as_mut() {
        let g = app.views.grid_ref(ce.grid);
        let cell = g.cell_rect(ce.row, ce.vcol).unwrap_or(Rect::new(g.layout.area.x + 2, g.layout.body_y, 20, 1));
        let w = cell.width.max(match ce.ed.kind {
            crate::widgets::editor::EdKind::Chips { .. } => 40,
            crate::widgets::editor::EdKind::Date => 24,
            _ => 22,
        });
        let r = Rect::new(cell.x.min(main.right().saturating_sub(w)), cell.y, w.min(main.width), 1);
        ce.ed.render_inline(buf, r, &t, true);
        ce.ed.render_popup(buf, r, main, &t);
    }
}

fn draw_status(app: &mut App, buf: &mut Buffer, r: Rect) {
    let t = &app.t;
    let st = Style::new().bg(t.header_bg).fg(t.fg);
    fill(buf, r, st);
    // right: presence + counters
    let mut right_parts: Vec<(String, Style)> = vec![];
    for u in app.r.online_users() {
        right_parts.push((format!(" {} ", u.initials()), Style::new().bg(t.user(u.id)).fg(t.bg).add_modifier(Modifier::BOLD)));
        right_parts.push((" ".into(), st));
    }
    let online = app.r.peers.len();
    right_parts.push((format!(" online: {online} "), st.fg(t.muted)));
    if !app.pending.is_empty() {
        right_parts.push((format!("⟳ {} pending ", app.pending.len()), st.fg(t.warn)));
    }
    if !app.conflicts.is_empty() {
        right_parts.push((format!("⚠ {} conflict ", app.conflicts.len()), st.fg(t.err)));
    }
    let rw: usize = right_parts.iter().map(|(s, _)| width(s)).sum();
    let mut x = r.right().saturating_sub(rw as u16);
    for (s, sst) in &right_parts {
        x += put(buf, x, r.y, s, *sst, width(s));
    }
    // left: message
    let on_conflict = app.current_cell_key().is_some_and(|k| app.conflicts.contains_key(&k)) && app.editor.is_none();
    let (msg, lvl) = match &app.status {
        _ if on_conflict => (context_line(app), Level::Warn),
        Some((m, l, _)) => (m.clone(), *l),
        None if !app.r.can_edit() && app.r.is_loaded() => ("Read-only account (viewer)".into(), Level::Info),
        None => {
            let ctx = context_line(app);
            (ctx, Level::Info)
        }
    };
    let color = match lvl {
        Level::Ok => t.ok,
        Level::Warn => t.warn,
        Level::Err => t.err,
        Level::Info => t.muted,
    };
    put(buf, r.x + 1, r.y, &msg, st.fg(color), (r.width as usize).saturating_sub(rw + 3));
}

/// Default status text: where the cursor is.
fn context_line(app: &App) -> String {
    if let Some(key) = app.current_cell_key() {
        if let Some(c) = app.conflicts.get(&key) {
            let who = c.by.map(|b| app.r.user_name(Some(b)).to_string()).unwrap_or_else(|| "Someone".into());
            let theirs = pt_core::grid::display_value(&app.r, key.0, &key.2, &c.current);
            let mine = pt_core::grid::display_value(&app.r, key.0, &key.2, &c.mine);
            return format!("⚠ {who} set this to “{theirs}”; yours was “{mine}”. Enter edits again from theirs (^S in the editor forces), Esc dismisses.");
        }
    }
    let Some(gs) = app.views.active_grid(app.view) else { return String::new() };
    let g = app.views.grid_ref(gs);
    match g.current_row() {
        Some(GridRow::Group { label, count, .. }) => format!("{label} — {count} rows · ←/→ collapse/expand · +/- all"),
        Some(GridRow::Data(id)) => {
            let col = g.current_col().map(|c| c.title.clone()).unwrap_or_default();
            let what = g.entity().map(|e| app.r.row_label(e, *id)).unwrap_or_default();
            let edit = g.current_col().is_some_and(|c| c.editable());
            format!("{what} · {col}{}", if edit { " · Enter/type to edit" } else { " (calculated)" })
        }
        None => String::new(),
    }
}

fn draw_hints(app: &mut App, buf: &mut Buffer, r: Rect) {
    let t = app.t.clone();
    fill(buf, r, Style::new().bg(t.bg));
    let entries: Vec<(String, String, Option<Action>)> = if let Some(d) = app.dialogs.last() {
        d.hints().into_iter().map(|(k, l)| (k.to_string(), l.to_string(), None)).collect()
    } else if app.menu.is_some() {
        [("←→", "Menus"), ("↑↓", "Items"), ("Enter", "Run"), ("letter", "Jump"), ("Esc", "Close")].iter().map(|(k, l)| (k.to_string(), l.to_string(), None)).collect()
    } else if let Some(ce) = &app.editor {
        keymap::editor_hints(ce.ed.kind).into_iter().map(|(k, l)| (k.to_string(), l.to_string(), None)).collect()
    } else if app.focus_query {
        [("Tab", "Complete"), ("↑↓", "Suggestions"), ("Enter", "Apply"), ("Esc", "Back to grid"), ("^U", "Clear")].iter().map(|(k, l)| (k.to_string(), l.to_string(), None)).collect()
    } else {
        keymap::hints(app.ctx()).into_iter().map(|(k, a)| (k, short_label(a).to_string(), Some(a))).collect()
    };
    let mut x = r.x;
    for (k, l, a) in entries {
        let kw = width(&k) as u16;
        let lw = width(&l) as u16;
        if x + kw + lw + 2 > r.right() {
            break;
        }
        let kr = Rect::new(x, r.y, kw + lw + 2, 1);
        let hover = app.hover.is_some_and(|(hx, hy)| rect_contains(kr, hx, hy));
        put(buf, x, r.y, &k, t.key(), kw as usize);
        put(buf, x + kw, r.y, &format!(" {l}"), Style::new().fg(if hover { t.accent } else { t.fg }).bg(t.bg), lw as usize + 1);
        if let Some(a) = a {
            app.hits.hints.push((kr, a));
        }
        x += kw + lw + 3;
    }
}

fn short_label(a: Action) -> &'static str {
    match a {
        Action::Palette => "Commands",
        Action::Filter => "Filter",
        Action::Edit => "Edit",
        Action::New => "New",
        Action::EditRecord => "Form",
        Action::Delete => "Delete",
        Action::Undo => "Undo",
        Action::SortColumn => "Sort",
        Action::GroupColumn => "Group",
        Action::ToggleDetail => "Detail",
        Action::Columns => "Columns",
        Action::FillDown => "Fill↓",
        Action::Help => "Help",
        Action::Menu => "Menu",
        Action::Quit => "Quit",
        other => other.label(),
    }
}

fn draw_toasts(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let mut y = area.y + 3;
    let mut rect = Rect::default();
    for toast in app.toasts.iter().rev() {
        let w = (width(&toast.text) as u16 + 4).min(70).min(area.width - 2);
        let lines = (width(&toast.text) as u16).div_ceil(w.saturating_sub(4).max(1)).clamp(1, 3);
        let r = Rect::new(area.right().saturating_sub(w + 2), y, w, lines + 2);
        let color = match toast.level {
            Level::Ok => t.ok,
            Level::Warn => t.warn,
            Level::Err => t.err,
            Level::Info => t.info,
        };
        shadow(buf, r);
        fill(buf, r, Style::new().bg(t.panel));
        for yy in r.y..r.bottom() {
            put(buf, r.x, yy, "▌", Style::new().fg(color).bg(t.panel), 1);
        }
        // wrap by chars
        let chars: Vec<char> = toast.text.chars().collect();
        let per = (w - 4) as usize;
        for (i, chunk) in chars.chunks(per.max(1)).take(lines as usize).enumerate() {
            let s: String = chunk.iter().collect();
            put(buf, r.x + 2, r.y + 1 + i as u16, &s, Style::new().fg(t.fg).bg(t.panel), per);
        }
        rect = if rect.width == 0 { r } else { rect.union(r) };
        y += r.height + 1;
        if y > area.bottom().saturating_sub(6) {
            break;
        }
    }
    app.hits.toasts = rect;
}
