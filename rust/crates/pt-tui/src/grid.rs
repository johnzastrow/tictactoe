//! The reusable data grid (ui-spec §3): frozen columns, horizontal scroll,
//! multi-sort, grouping with collapsible subtotal rows, footer totals, a
//! selection/presence gutter, conflict / pending / flash cell states, and
//! mouse hit-testing. Rows and cells come from `pt_core::grid`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Instant;

use pt_core::grid::{self as cg, Align, Column, GridRow, SortSpec};
use pt_core::model::{CfEntity, Entity};
use pt_core::query::Query;
use pt_core::replica::Replica;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::theme::Theme;
use crate::widgets::draw::{bar, fill, pad, put, width};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GridKind {
    Projects,
    Tasks,
    ProjectTasks,
    Templates,
    TemplateTasks,
    Fields,
    Users,
    Programs,
    Activity,
}

impl GridKind {
    pub fn entity(self) -> Option<Entity> {
        Some(match self {
            GridKind::Projects => Entity::Project,
            GridKind::Tasks | GridKind::ProjectTasks => Entity::Task,
            GridKind::Templates => Entity::Template,
            GridKind::TemplateTasks => Entity::TemplateTask,
            GridKind::Fields => Entity::CustomField,
            GridKind::Users => Entity::User,
            GridKind::Programs => Entity::Program,
            GridKind::Activity => return None,
        })
    }

    fn columns(self, r: &Replica) -> Vec<Column> {
        match self {
            GridKind::Projects => cg::project_columns(r),
            GridKind::Tasks => cg::task_columns(r, true),
            GridKind::ProjectTasks => cg::task_columns(r, false),
            GridKind::Templates => cg::template_columns(),
            GridKind::TemplateTasks => cg::template_task_columns(),
            GridKind::Fields => cg::custom_field_columns(),
            GridKind::Users => cg::user_columns(),
            GridKind::Programs => cg::program_columns(),
            GridKind::Activity => cg::activity_columns(),
        }
    }

    fn frozen(self) -> usize {
        match self {
            GridKind::Projects => 2,
            GridKind::Tasks => 3,
            GridKind::ProjectTasks => 2,
            GridKind::Activity => 2,
            _ => 1,
        }
    }

    fn default_sort(self) -> Vec<SortSpec> {
        let s = |k: &str, desc| SortSpec { key: k.into(), desc };
        match self {
            GridKind::Projects => vec![s("code", false)],
            GridKind::Tasks => vec![s("project", false), s("seq", false)],
            GridKind::ProjectTasks => vec![s("seq", false)],
            GridKind::Templates => vec![s("name", false)],
            GridKind::TemplateTasks => vec![s("seq", false)],
            GridKind::Fields => vec![s("entity", false), s("seq", false)],
            GridKind::Users => vec![s("username", false)],
            GridKind::Programs => vec![s("name", false)],
            GridKind::Activity => vec![s("ts", true)],
        }
    }

    fn default_group(self) -> Option<String> {
        match self {
            GridKind::Tasks => Some("project".into()),
            _ => None,
        }
    }

    fn cf_entity(self) -> Option<CfEntity> {
        match self {
            GridKind::Projects => Some(CfEntity::Project),
            GridKind::Tasks | GridKind::ProjectTasks => Some(CfEntity::Task),
            _ => None,
        }
    }
}

/// How rows are filtered.
pub enum RowFilter<'a> {
    None,
    Query(&'a Query),
    Text(&'a str),
}

/// What the mouse hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Header(usize),
    Border(usize),
    Cell(usize, usize),
    Gutter(usize),
    Footer,
    Empty,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub header_y: u16,
    pub body_y: u16,
    pub body_h: u16,
    /// (visible column index, x, width)
    pub cols: Vec<(usize, u16, u16)>,
}

/// Other users' focus by row: (user id, initial, field).
pub type Presence = HashMap<(Entity, i64), Vec<(i64, String, Option<String>)>>;

/// Cell decorations supplied by the app each frame.
pub struct Decor<'a> {
    pub focused: bool,
    pub editing: Option<(usize, usize)>,
    /// (entity, id, field) → conflict
    pub conflicts: &'a HashSet<(Entity, i64, String)>,
    pub pending: &'a HashSet<(Entity, i64, String)>,
    pub flashes: &'a HashMap<(Entity, i64), Instant>,
    /// (entity, id) → (user id, initial, field)
    pub presence: &'a Presence,
}

pub struct GridState {
    pub kind: GridKind,
    pub cols: Vec<Column>,
    pub frozen: usize,
    pub sort: Vec<SortSpec>,
    pub group: Option<String>,
    pub collapsed: HashSet<String>,
    pub rows: Vec<GridRow>,
    pub ids: Vec<i64>,
    pub totals: Vec<Option<f64>>,
    pub parent: Option<i64>,
    /// Bumped whenever filter/sort/group/parent change.
    pub generation: u64,
    built: (u64, u64),
    cf_sig: Vec<(i64, String)>,
    pub cur: usize,
    pub ccol: usize,
    pub top: usize,
    pub hscroll: usize,
    pub marks: BTreeSet<i64>,
    pub anchor: Option<usize>,
    pub layout: Layout,
    cur_id: Option<i64>,
    cur_group: Option<String>,
}

impl GridState {
    pub fn new(kind: GridKind, r: &Replica) -> GridState {
        let mut g = GridState {
            kind,
            cols: kind.columns(r),
            frozen: kind.frozen(),
            sort: kind.default_sort(),
            group: kind.default_group(),
            collapsed: HashSet::new(),
            rows: vec![],
            ids: vec![],
            totals: vec![],
            parent: None,
            generation: 1,
            built: (u64::MAX, 0),
            cf_sig: vec![],
            cur: 0,
            ccol: 0,
            top: 0,
            hscroll: 0,
            marks: BTreeSet::new(),
            anchor: None,
            layout: Layout::default(),
            cur_id: None,
            cur_group: None,
        };
        g.cf_sig = g.cf_signature(r);
        g
    }

    pub fn entity(&self) -> Option<Entity> {
        self.kind.entity()
    }

    fn cf_signature(&self, r: &Replica) -> Vec<(i64, String)> {
        match self.kind.cf_entity() {
            Some(e) => r.custom_fields_for(e).iter().map(|c| (c.id, c.name.clone())).collect(),
            None => vec![],
        }
    }

    /// Custom fields changed: rebuild columns, keeping the user's order, widths and visibility.
    fn sync_columns(&mut self, r: &Replica) {
        let sig = self.cf_signature(r);
        if sig == self.cf_sig {
            return;
        }
        self.cf_sig = sig;
        let fresh = self.kind.columns(r);
        let mut out: Vec<Column> = Vec::new();
        for old in &self.cols {
            if let Some(mut c) = fresh.iter().find(|c| c.key == old.key).cloned() {
                c.width = old.width;
                c.hidden = old.hidden;
                out.push(c);
            }
        }
        for c in fresh {
            if !out.iter().any(|o| o.key == c.key) {
                let pos = out.iter().position(|o| o.key == "notes").unwrap_or(out.len());
                out.insert(pos, c);
            }
        }
        self.cols = out;
        self.generation += 1;
    }

    pub fn reset_columns(&mut self, r: &Replica) {
        self.cols = self.kind.columns(r);
        self.generation += 1;
    }

    pub fn visible(&self) -> Vec<usize> {
        self.cols.iter().enumerate().filter(|(_, c)| !c.hidden).map(|(i, _)| i).collect()
    }

    pub fn vis_cols(&self) -> Vec<&Column> {
        self.cols.iter().filter(|c| !c.hidden).collect()
    }

    pub fn current_col(&self) -> Option<&Column> {
        self.vis_cols().get(self.ccol).copied()
    }

    pub fn current_id(&self) -> Option<i64> {
        self.rows.get(self.cur).and_then(GridRow::id)
    }

    pub fn current_row(&self) -> Option<&GridRow> {
        self.rows.get(self.cur)
    }

    /// Rebuild rows if the replica or settings changed. Keeps the cursor on the same row id.
    pub fn refresh(&mut self, r: &Replica, filter: RowFilter<'_>) {
        self.sync_columns(r);
        if self.built == (r.version, self.generation) {
            return;
        }
        self.built = (r.version, self.generation);
        let mut ids: Vec<i64> = match self.kind {
            GridKind::Projects => r.projects.keys().copied().collect(),
            GridKind::Tasks => r.tasks.keys().copied().collect(),
            GridKind::ProjectTasks => self.parent.map(|p| r.project_tasks(p).to_vec()).unwrap_or_default(),
            GridKind::Templates => r.templates.keys().copied().collect(),
            GridKind::TemplateTasks => self.parent.map(|t| r.template_tasks_of(t).iter().map(|x| x.id).collect()).unwrap_or_default(),
            GridKind::Fields => r.custom_fields.keys().copied().collect(),
            GridKind::Users => r.users.keys().copied().collect(),
            GridKind::Programs => r.programs.keys().copied().collect(),
            GridKind::Activity => r.activity.keys().copied().collect(),
        };
        match filter {
            RowFilter::Query(q) if !q.is_empty() => ids.retain(|id| match self.kind {
                GridKind::Projects => r.projects.get(id).is_some_and(|p| q.matches_project(p, r)),
                GridKind::Tasks | GridKind::ProjectTasks => r.tasks.get(id).is_some_and(|t| q.matches_task(t, r)),
                GridKind::Activity => r.activity.get(id).is_some_and(|a| q.matches_activity(a, r)),
                _ => true,
            }),
            RowFilter::Text(t) if !t.trim().is_empty() => {
                let needle = t.trim().to_lowercase();
                let cols = self.vis_cols().into_iter().cloned().collect::<Vec<_>>();
                ids.retain(|id| cols.iter().any(|c| cg::cell(r, c, *id).text.to_lowercase().contains(&needle)));
            }
            _ => {}
        }
        self.marks.retain(|m| ids.contains(m));
        self.rows = cg::build_rows(r, &self.cols, ids.clone(), &self.sort, self.group.as_deref(), &self.collapsed);
        self.totals = cg::totals(r, &self.cols, &ids);
        self.ids = ids;
        // restore the cursor
        let found = match (&self.cur_id, &self.cur_group) {
            (Some(id), _) => self.rows.iter().position(|row| row.id() == Some(*id)),
            (None, Some(g)) => self.rows.iter().position(|row| matches!(row, GridRow::Group { key, .. } if key == g)),
            _ => None,
        };
        // the row may now be inside a collapsed group: land on its header
        let found = found.or_else(|| {
            let id = self.cur_id?;
            let gcol = self.cols.iter().find(|c| Some(&c.key) == self.group.as_ref())?;
            let key = cg::cell(r, gcol, id).sort.group_key();
            self.rows.iter().position(|row| matches!(row, GridRow::Group { key: k, .. } if *k == key))
        });
        self.cur = found.unwrap_or(self.cur).min(self.rows.len().saturating_sub(1));
        self.remember();
    }

    /// Force a rebuild on the next refresh.
    pub fn touch(&mut self) {
        self.generation += 1;
    }

    fn remember(&mut self) {
        match self.rows.get(self.cur) {
            Some(&GridRow::Data(id)) => {
                self.cur_id = Some(id);
                self.cur_group = None;
            }
            Some(GridRow::Group { key, .. }) => {
                self.cur_id = None;
                self.cur_group = Some(key.clone());
            }
            None => {}
        }
    }

    pub fn select_id(&mut self, id: i64) -> bool {
        self.cur_id = Some(id);
        match self.rows.iter().position(|r| r.id() == Some(id)) {
            Some(i) => {
                self.cur = i;
                self.remember();
                true
            }
            None => false,
        }
    }

    pub fn select_col_key(&mut self, key: &str) {
        if let Some(i) = self.vis_cols().iter().position(|c| c.key == key) {
            self.ccol = i;
        }
    }

    pub fn move_row(&mut self, delta: i64) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as i64;
        self.cur = (self.cur as i64 + delta).clamp(0, n - 1) as usize;
        self.remember();
    }

    pub fn set_row(&mut self, row: usize) {
        if !self.rows.is_empty() {
            self.cur = row.min(self.rows.len() - 1);
            self.remember();
        }
    }

    pub fn move_col(&mut self, delta: i64) {
        let n = self.vis_cols().len() as i64;
        if n > 0 {
            self.ccol = (self.ccol as i64 + delta).clamp(0, n - 1) as usize;
        }
    }

    /// Tab / Shift+Tab: next cell, wrapping across data rows.
    pub fn tab(&mut self, forward: bool) {
        let n = self.vis_cols().len();
        if n == 0 {
            return;
        }
        if forward {
            if self.ccol + 1 < n {
                self.ccol += 1;
            } else if self.cur + 1 < self.rows.len() {
                self.ccol = 0;
                self.move_row(1);
            }
        } else if self.ccol > 0 {
            self.ccol -= 1;
        } else if self.cur > 0 {
            self.ccol = n - 1;
            self.move_row(-1);
        }
    }

    pub fn page(&self) -> i64 {
        (self.layout.body_h as i64).max(1)
    }

    /// Collapse or expand the group under the cursor (or containing it).
    pub fn toggle_group(&mut self, expand: Option<bool>) -> bool {
        let key = match self.rows.get(self.cur) {
            Some(GridRow::Group { key, .. }) => key.clone(),
            _ => return false,
        };
        let collapsed = self.collapsed.contains(&key);
        let want_collapsed = match expand {
            Some(e) => !e,
            None => !collapsed,
        };
        if want_collapsed {
            self.collapsed.insert(key);
        } else {
            self.collapsed.remove(&key);
        }
        self.generation += 1;
        true
    }

    pub fn set_all_collapsed(&mut self, collapsed: bool) {
        if collapsed {
            for row in &self.rows {
                if let GridRow::Group { key, .. } = row {
                    self.collapsed.insert(key.clone());
                }
            }
        } else {
            self.collapsed.clear();
        }
        self.generation += 1;
    }

    /// ^O: cycle ▲ → ▼ → off for a column; `add` keeps other keys (Shift+click).
    pub fn cycle_sort(&mut self, key: &str, add: bool) {
        let existing = self.sort.iter().position(|s| s.key == key);
        match existing {
            Some(i) if !self.sort[i].desc => self.sort[i].desc = true,
            Some(i) => {
                self.sort.remove(i);
            }
            None => {
                if !add {
                    self.sort.clear();
                }
                self.sort.push(SortSpec { key: key.to_string(), desc: false });
            }
        }
        if !add && existing.is_some() {
            self.sort.retain(|s| s.key == key);
        }
        self.generation += 1;
    }

    pub fn toggle_group_by(&mut self, key: &str) {
        self.group = if self.group.as_deref() == Some(key) { None } else { Some(key.to_string()) };
        self.collapsed.clear();
        self.generation += 1;
    }

    pub fn toggle_mark(&mut self) {
        if let Some(id) = self.current_id() {
            if !self.marks.remove(&id) {
                self.marks.insert(id);
            }
            self.anchor = Some(self.cur);
        }
    }

    /// Shift+↑/↓: extend the marked range from the anchor.
    pub fn extend_marks(&mut self, delta: i64) {
        let anchor = *self.anchor.get_or_insert(self.cur);
        self.move_row(delta);
        let (a, b) = (anchor.min(self.cur), anchor.max(self.cur));
        for row in &self.rows[a..=b.min(self.rows.len().saturating_sub(1))] {
            if let Some(id) = row.id() {
                self.marks.insert(id);
            }
        }
    }

    pub fn mark_all(&mut self) {
        if self.marks.len() == self.ids.len() {
            self.marks.clear();
        } else {
            self.marks = self.ids.iter().copied().collect();
        }
    }

    /// Marked ids in display order (or the current row when nothing is marked).
    pub fn selection(&self) -> Vec<i64> {
        if self.marks.is_empty() {
            return self.current_id().into_iter().collect();
        }
        self.rows.iter().filter_map(GridRow::id).filter(|id| self.marks.contains(id)).collect()
    }

    /// Rows in display order starting at the cursor (for paste).
    pub fn data_rows_from_cursor(&self) -> Vec<i64> {
        self.rows.iter().skip(self.cur).filter_map(GridRow::id).collect()
    }

    // ---- rendering ----------------------------------------------------------------

    fn ensure_col_visible(&mut self, avail: u16, widths: &[u16]) {
        let frozen = self.frozen.min(widths.len());
        if self.ccol < frozen {
            return;
        }
        let frozen_w: u16 = widths[..frozen].iter().map(|w| w + 1).sum::<u16>() + if frozen > 0 { 1 } else { 0 };
        let room = avail.saturating_sub(frozen_w);
        let idx = self.ccol - frozen;
        if idx < self.hscroll {
            self.hscroll = idx;
        }
        loop {
            let used: u16 = widths[frozen + self.hscroll..=self.ccol].iter().map(|w| w + 1).sum();
            if used <= room || self.hscroll >= idx {
                break;
            }
            self.hscroll += 1;
        }
    }

    pub fn render(&mut self, buf: &mut Buffer, area: Rect, r: &Replica, t: &Theme, d: &Decor<'_>) {
        fill(buf, area, t.base());
        self.layout = Layout { area, ..Default::default() };
        if area.height < 3 || area.width < 10 {
            return;
        }
        let cols: Vec<Column> = self.vis_cols().into_iter().cloned().collect();
        if cols.is_empty() {
            return;
        }
        self.ccol = self.ccol.min(cols.len() - 1);
        let widths: Vec<u16> = cols.iter().map(|c| c.width).collect();
        let gutter = 2u16;
        let has_footer = cols.iter().any(|c| c.sum) && !self.ids.is_empty();
        let body_h = area.height.saturating_sub(1 + has_footer as u16);
        // reserve the last column for a scrollbar when rows overflow
        let scrollbar = self.rows.len() > body_h as usize;
        let right = area.right() - scrollbar as u16;
        let avail = right.saturating_sub(area.x + gutter);
        self.ensure_col_visible(avail, &widths);
        // column x positions
        let mut spans: Vec<(usize, u16, u16)> = Vec::new();
        let mut x = area.x + gutter;
        let frozen = self.frozen.min(cols.len());
        for (i, w) in widths.iter().enumerate().take(frozen) {
            let w = (*w).min(right.saturating_sub(x));
            spans.push((i, x, w));
            x += w + 1;
        }
        let sep_x = if frozen > 0 && frozen < cols.len() {
            let sx = x;
            x += 1;
            Some(sx)
        } else {
            None
        };
        for (i, w) in widths.iter().enumerate().skip(frozen + self.hscroll) {
            if x >= right {
                break;
            }
            let w = (*w).min(right - x);
            spans.push((i, x, w));
            x += w + 1;
        }
        let header_y = area.y;
        let body_y = area.y + 1;
        self.layout = Layout { area, header_y, body_y, body_h, cols: spans.clone() };

        // scroll vertically to keep the cursor visible
        let h = body_h as usize;
        if self.cur < self.top {
            self.top = self.cur;
        } else if self.cur >= self.top + h {
            self.top = self.cur + 1 - h;
        }
        self.top = self.top.min(self.rows.len().saturating_sub(h.min(self.rows.len()).max(1)));

        // header
        let hs = Style::new().bg(t.header_bg).fg(t.fg).add_modifier(Modifier::BOLD);
        fill(buf, Rect::new(area.x, header_y, area.width, 1), hs);
        if self.hscroll > 0 {
            put(buf, area.x, header_y, "◂", hs.fg(t.accent), 1);
        }
        for &(i, cx, w) in &spans {
            let c = &cols[i];
            let mut title = c.title.clone();
            if let Some(pos) = self.sort.iter().position(|s| s.key == c.key) {
                let arrow = if self.sort[pos].desc { "▼" } else { "▲" };
                title = if self.sort.len() > 1 { format!("{title}{arrow}{}", pos + 1) } else { format!("{title}{arrow}") };
            }
            if self.group.as_deref() == Some(c.key.as_str()) {
                title = format!("⊞{title}");
            }
            let st = if d.focused && i == self.ccol { hs.fg(t.accent) } else { hs };
            put(buf, cx, header_y, &pad(&title, w as usize, c.align == Align::Right), st, w as usize);
        }
        if spans.last().is_some_and(|s| s.0 + 1 < cols.len()) {
            put(buf, area.right() - 1, header_y, "▸", hs.fg(t.accent), 1);
        }

        if self.rows.is_empty() {
            let msg = if self.kind == GridKind::ProjectTasks && self.parent.is_none() {
                "Select a project to see its tasks"
            } else {
                "No rows match the filter"
            };
            put(buf, area.x + 3, body_y + 1, msg, t.muted(), area.width as usize - 4);
        }

        let now = Instant::now();
        let ent = self.entity();
        for (vis_row, row_idx) in (self.top..self.rows.len()).take(h).enumerate() {
            let y = body_y + vis_row as u16;
            let is_cur = row_idx == self.cur;
            match self.rows[row_idx].clone() {
                GridRow::Group { label, count, collapsed, sums, style, .. } => {
                    let bg = if is_cur && d.focused { t.cur_bg } else if is_cur { t.sel_bg } else { t.panel };
                    let gs = Style::new().bg(bg).fg(t.fg).add_modifier(Modifier::BOLD);
                    fill(buf, Rect::new(area.x, y, area.width, 1), gs);
                    let arrow = if collapsed { "▸" } else { "▾" };
                    let first_sum_x = spans
                        .iter()
                        .find(|(i, _, _)| sums.get(self.col_index(&cols[*i])).is_some_and(|s| s.is_some()))
                        .map_or(area.right(), |s| s.1);
                    let room = first_sum_x.saturating_sub(area.x + 1) as usize;
                    let label_style = gs.patch(t.cell(&style));
                    let used = put(buf, area.x + 1, y, &format!("{arrow} "), gs.fg(t.accent), 2);
                    let used2 = put(buf, area.x + 1 + used, y, &label, label_style, room.saturating_sub(used as usize + 6));
                    put(buf, area.x + 2 + used + used2, y, &format!("({count})"), gs.fg(t.muted), 8);
                    for &(i, cx, w) in &spans {
                        if let Some(Some(v)) = sums.get(self.col_index(&cols[i])) {
                            put(buf, cx, y, &pad(&cg::fmt_hours(*v), w as usize, true), gs.fg(t.muted), w as usize);
                        }
                    }
                }
                GridRow::Data(id) => {
                    let flash = ent.and_then(|e| d.flashes.get(&(e, id))).is_some_and(|t0| now.duration_since(*t0).as_millis() < 1200);
                    let marked = self.marks.contains(&id);
                    let bg = if flash {
                        t.flash
                    } else if is_cur {
                        t.sel_bg
                    } else if marked {
                        mix_marked(t)
                    } else if vis_row % 2 == 1 {
                        t.zebra
                    } else {
                        t.bg
                    };
                    let row_style = Style::new().bg(bg).fg(t.fg);
                    fill(buf, Rect::new(area.x, y, area.width, 1), row_style);
                    // gutter: mark + presence
                    let peers = ent.and_then(|e| d.presence.get(&(e, id)));
                    if let Some((uid, initial, _)) = peers.and_then(|p| p.first()) {
                        put(buf, area.x, y, initial, row_style.fg(t.bg).bg(t.user(*uid)).add_modifier(Modifier::BOLD), 1);
                    }
                    if marked {
                        put(buf, area.x + 1, y, "●", row_style.fg(t.accent), 1);
                    } else if is_cur {
                        put(buf, area.x + 1, y, "▶", row_style.fg(t.accent), 1);
                    }
                    for &(i, cx, w) in &spans {
                        let c = &cols[i];
                        let cell = if self.kind == GridKind::Activity { cg::activity_cell(r, &c.key, id) } else { cg::cell(r, c, id) };
                        let mut st = row_style;
                        let is_cur_cell = is_cur && i == self.ccol;
                        if is_cur_cell && d.focused {
                            st = st.bg(t.cur_bg);
                        }
                        let fkey = c.field.clone().unwrap_or_default();
                        let mut text = cell.text.clone();
                        if let Some(e) = ent {
                            if d.conflicts.contains(&(e, id, fkey.clone())) && !fkey.is_empty() {
                                st = st.bg(t.err).fg(t.bg);
                                text = format!("⚠ {text}");
                            } else if d.pending.contains(&(e, id, fkey.clone())) && !fkey.is_empty() {
                                st = st.add_modifier(Modifier::ITALIC | Modifier::DIM);
                            }
                            if let Some(p) = peers.and_then(|p| p.iter().find(|(_, _, f)| f.as_deref() == Some(fkey.as_str()))) {
                                st = st.add_modifier(Modifier::UNDERLINED).underline_color(t.user(p.0));
                            }
                        }
                        if d.editing == Some((row_idx, i)) {
                            continue; // the editor draws here
                        }
                        draw_cell(buf, cx, y, w, &cell, &text, c.align, st, t);
                    }
                    if let Some(sx) = sep_x {
                        put(buf, sx, y, "│", row_style.fg(t.border), 1);
                    }
                }
            }
        }
        if let Some(sx) = sep_x {
            put(buf, sx, header_y, "│", hs.fg(t.border), 1);
        }
        // footer totals
        if has_footer {
            let fy = area.bottom() - 1;
            let fs = Style::new().bg(t.header_bg).fg(t.muted);
            fill(buf, Rect::new(area.x, fy, area.width, 1), fs);
            let label = format!("Σ {} rows{}", self.ids.len(), if self.marks.is_empty() { String::new() } else { format!(" · {} marked", self.marks.len()) });
            let first_w = spans.first().map_or(10, |s| s.2 as usize + 2);
            put(buf, area.x, fy, &label, fs.add_modifier(Modifier::BOLD), first_w.max(12));
            for &(i, cx, w) in &spans {
                if let Some(Some(v)) = self.totals.get(self.col_index(&cols[i])) {
                    put(buf, cx, fy, &pad(&cg::fmt_hours(*v), w as usize, true), fs.fg(t.fg).add_modifier(Modifier::BOLD), w as usize);
                }
            }
        }
        // scrollbar
        if self.rows.len() > h && h > 2 {
            let sx = area.right() - 1;
            let thumb = ((h * h) / self.rows.len()).max(1);
            let pos = (self.top * (h - thumb)) / (self.rows.len() - h).max(1);
            for i in 0..h {
                let sym = if i >= pos && i < pos + thumb { "┃" } else { "│" };
                put(buf, sx, body_y + i as u16, sym, Style::new().fg(if i >= pos && i < pos + thumb { t.accent } else { t.border }), 1);
            }
        }
    }

    /// Index of a visible column in `self.cols` (sums/totals are aligned to all columns).
    fn col_index(&self, c: &Column) -> usize {
        self.cols.iter().position(|x| x.key == c.key).unwrap_or(usize::MAX)
    }

    /// Screen rect of a cell from the last render.
    pub fn cell_rect(&self, row: usize, vcol: usize) -> Option<Rect> {
        if row < self.top || row >= self.top + self.layout.body_h as usize {
            return None;
        }
        let y = self.layout.body_y + (row - self.top) as u16;
        self.layout.cols.iter().find(|c| c.0 == vcol).map(|&(_, x, w)| Rect::new(x, y, w, 1))
    }

    pub fn hit(&self, x: u16, y: u16) -> Hit {
        let l = &self.layout;
        if !crate::widgets::draw::rect_contains(l.area, x, y) {
            return Hit::Empty;
        }
        let col = l.cols.iter().find(|&&(_, cx, w)| x >= cx && x < cx + w).map(|c| c.0);
        let border = l.cols.iter().find(|&&(_, cx, w)| x == cx + w).map(|c| c.0);
        if y == l.header_y {
            if let Some(b) = border {
                return Hit::Border(b);
            }
            return col.map_or(Hit::Empty, Hit::Header);
        }
        if y >= l.body_y && y < l.body_y + l.body_h {
            let row = self.top + (y - l.body_y) as usize;
            if row >= self.rows.len() {
                return Hit::Empty;
            }
            if x < l.area.x + 2 {
                return Hit::Gutter(row);
            }
            return col.map_or(Hit::Gutter(row), |c| Hit::Cell(row, c));
        }
        if y >= l.body_y + l.body_h {
            return Hit::Footer;
        }
        Hit::Empty
    }

    pub fn resize_col(&mut self, vcol: usize, new_w: u16) {
        let idx = self.visible().get(vcol).copied();
        if let Some(i) = idx {
            self.cols[i].width = new_w.clamp(3, 80);
        }
    }

    pub fn scroll(&mut self, delta: i64) {
        let n = self.rows.len();
        let h = self.layout.body_h as usize;
        self.top = (self.top as i64 + delta).clamp(0, n.saturating_sub(h) as i64) as usize;
        // keep the cursor inside the viewport
        if self.cur < self.top {
            self.set_row(self.top);
        } else if self.cur >= self.top + h {
            self.set_row(self.top + h - 1);
        }
    }

    /// TSV of the selection (marked rows or the current cell) for ^C.
    pub fn copy_text(&self, r: &Replica) -> String {
        let cols: Vec<&Column> = self.vis_cols();
        let ent = self.entity();
        let val = |c: &Column, id: i64| -> String {
            match (ent, &c.field) {
                (Some(e), Some(f)) if c.editable() => cg::edit_text(r, e, f, id),
                _ => cg::cell(r, c, id).text,
            }
            .replace(['\t', '\n'], " ")
        };
        if self.marks.is_empty() {
            match (self.current_id(), cols.get(self.ccol)) {
                (Some(id), Some(c)) => val(c, id),
                _ => String::new(),
            }
        } else {
            self.selection()
                .iter()
                .map(|id| cols.iter().map(|c| val(c, *id)).collect::<Vec<_>>().join("\t"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

fn mix_marked(t: &Theme) -> Color {
    t.panel
}

#[allow(clippy::too_many_arguments)]
fn draw_cell(buf: &mut Buffer, x: u16, y: u16, w: u16, cell: &cg::Cell, text: &str, align: Align, st: Style, t: &Theme) {
    let w = w as usize;
    if w == 0 {
        return;
    }
    let cs = st.patch(t.cell(&cell.style));
    if let cg::Style::Bar { pct, level } = cell.style {
        let tw = width(text).min(w);
        let bw = w.saturating_sub(tw + 1);
        let color = match level {
            2 => t.err,
            1 => t.warn,
            _ => {
                if text.contains('/') {
                    t.accent
                } else {
                    t.ok
                }
            }
        };
        if bw > 0 {
            bar(buf, x, y, bw, pct, color, t.border);
        }
        put(buf, x + (w - tw) as u16, y, text, st.fg(if level > 0 { color } else { t.fg }), tw);
        return;
    }
    let glyph = Theme::glyph(&cell.style).unwrap_or("");
    let s = format!("{glyph}{text}");
    let s = if glyph.is_empty() || text.is_empty() { text.to_string() } else { s };
    put(buf, x, y, &pad(&s, w, align == Align::Right), cs, w);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::sample;
    use pt_core::query::Target;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(g: &mut GridState, r: &Replica, w: u16, h: u16) -> String {
        let (c, p, f, pr) = (HashSet::new(), HashSet::new(), HashMap::new(), Presence::new());
        let d = Decor { focused: true, editing: None, conflicts: &c, pending: &p, flashes: &f, presence: &pr };
        let t = Theme::new(0);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|fr| {
            let area = fr.area();
            g.render(fr.buffer_mut(), area, r, &t, &d)
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn tasks_grouped_by_project() {
        let r = sample();
        let mut g = GridState::new(GridKind::Tasks, &r);
        g.refresh(&r, RowFilter::None);
        assert!(matches!(g.rows[0], GridRow::Group { .. }));
        let out = render(&mut g, &r, 120, 12);
        assert!(out.contains("▾ NWR-004  POS refresh"), "{out}");
        assert!(out.contains("Load test checkout"));
        assert!(out.contains("Σ 5 rows"), "{out}");
        // collapse
        g.toggle_group(Some(false));
        g.refresh(&r, RowFilter::None);
        assert_eq!(g.rows.len(), 5, "first group collapsed");
    }

    #[test]
    fn projects_frozen_and_hscroll() {
        let r = sample();
        let mut g = GridState::new(GridKind::Projects, &r);
        g.refresh(&r, RowFilter::None);
        g.ccol = g.vis_cols().len() - 1;
        let out = render(&mut g, &r, 60, 6);
        assert!(out.contains("NWR-004"), "frozen code column stays: {out}");
        assert!(g.hscroll > 0);
        assert!(out.lines().next().unwrap().starts_with('◂'));
    }

    #[test]
    fn sort_cycle_and_filter() {
        let r = sample();
        let mut g = GridState::new(GridKind::Tasks, &r);
        g.group = None;
        g.cycle_sort("est_hours", false);
        g.refresh(&r, RowFilter::None);
        assert_eq!(g.rows[0].id(), Some(100));
        g.cycle_sort("est_hours", false);
        g.refresh(&r, RowFilter::None);
        assert_eq!(g.rows[0].id(), Some(111));
        let q = Query::compile("status:done", Target::Tasks, &r);
        g.touch();
        g.refresh(&r, RowFilter::Query(&q));
        assert_eq!(g.ids, vec![100]);
    }

    #[test]
    fn hit_testing() {
        let r = sample();
        let mut g = GridState::new(GridKind::Tasks, &r);
        g.group = None;
        g.refresh(&r, RowFilter::None);
        render(&mut g, &r, 120, 10);
        let (_, x, w) = g.layout.cols[1];
        assert_eq!(g.hit(x, 0), Hit::Header(1));
        assert_eq!(g.hit(x + w, 0), Hit::Border(1));
        assert_eq!(g.hit(x, 2), Hit::Cell(1, 1));
        assert_eq!(g.hit(0, 2), Hit::Gutter(1));
    }

    #[test]
    fn copy_tsv() {
        let r = sample();
        let mut g = GridState::new(GridKind::ProjectTasks, &r);
        g.parent = Some(10);
        g.refresh(&r, RowFilter::None);
        g.select_col_key("title");
        assert_eq!(g.copy_text(&r), "Kickoff");
        g.mark_all();
        let tsv = g.copy_text(&r);
        assert_eq!(tsv.lines().count(), 3);
        assert!(tsv.lines().nth(1).unwrap().contains("\tLoad test checkout\t"));
    }
}
