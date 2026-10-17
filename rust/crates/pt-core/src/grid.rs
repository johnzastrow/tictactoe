//! Front-end-neutral grid model: columns per view, cell text + style hints,
//! sorting, grouping with subtotals, edit choices and text parsing for
//! typed/pasted values. Both the TUI and the WASM client render from this, so
//! the two grids agree on every value.

use std::cmp::Ordering;
use std::collections::HashSet;

use serde_json::Value;

use crate::date::{Date, parse_natural, relative_time};
use crate::fields::{self, Kind, round2, valid_tag};
use crate::fuzzy;
use crate::model::*;
use crate::replica::Replica;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Editor {
    None,
    Text,
    Multiline,
    Number,
    Int,
    Date,
    /// fixed list (enums)
    Select,
    /// fuzzy list (references)
    Combo,
    /// free text with suggestions (phase)
    Suggest,
    /// multi-value (tags, deps, options)
    Chips,
    Bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    /// Unique per view; persisted in column layouts. Writable columns use the field name.
    pub key: String,
    pub title: String,
    pub width: u16,
    pub align: Align,
    pub entity: Entity,
    /// Field written by edits (`status`, `cf.3`); None = computed/read-only.
    pub field: Option<String>,
    pub editor: Editor,
    /// Show a subtotal in group/footer rows.
    pub sum: bool,
    pub hidden: bool,
}

impl Column {
    fn new(entity: Entity, key: &str, title: &str, width: u16) -> Column {
        let meta = fields::field(entity, key);
        let editor = meta.map_or(Editor::None, |m| editor_for(m.kind, key));
        let align = match meta.map(|m| m.kind) {
            Some(Kind::Hours | Kind::Int { .. }) => Align::Right,
            _ => Align::Left,
        };
        Column {
            key: key.into(),
            title: title.into(),
            width,
            align,
            entity,
            field: meta.filter(|m| !m.create_only).map(|m| m.name.to_string()),
            editor: if meta.is_some_and(|m| m.create_only) { Editor::None } else { editor },
            sum: false,
            hidden: false,
        }
    }
    fn computed(entity: Entity, key: &str, title: &str, width: u16, align: Align) -> Column {
        Column {
            key: key.into(),
            title: title.into(),
            width,
            align,
            entity,
            field: None,
            editor: Editor::None,
            sum: false,
            hidden: false,
        }
    }
    fn sum(mut self) -> Self {
        self.sum = true;
        self
    }
    fn hide(mut self) -> Self {
        self.hidden = true;
        self
    }
    fn editor(mut self, e: Editor) -> Self {
        self.editor = e;
        self
    }
    pub fn editable(&self) -> bool {
        self.field.is_some() && self.editor != Editor::None
    }
}

fn editor_for(kind: Kind, name: &str) -> Editor {
    match kind {
        Kind::Text { max, .. } if max > 2000 => Editor::Multiline,
        Kind::Text { .. } if name == "phase" => Editor::Suggest,
        Kind::Text { .. } | Kind::Code | Kind::Username | Kind::Prefix | Kind::Color => Editor::Text,
        Kind::Enum(_) => Editor::Select,
        Kind::Ref(_) => Editor::Combo,
        Kind::Date => Editor::Date,
        Kind::Hours => Editor::Number,
        Kind::Int { .. } => Editor::Int,
        Kind::Tags | Kind::Deps | Kind::Options => Editor::Chips,
        Kind::Bool => Editor::Bool,
        Kind::Password => Editor::None,
    }
}

fn cf_columns(r: &Replica, e: CfEntity) -> Vec<Column> {
    let entity = if e == CfEntity::Project { Entity::Project } else { Entity::Task };
    r.custom_fields_for(e)
        .into_iter()
        .map(|cf| Column {
            key: cf.key(),
            title: cf.name.clone(),
            width: match cf.kind {
                CfKind::Number => 9,
                CfKind::Date => 10,
                _ => 12.max(cf.name.chars().count() as u16 + 1).min(20),
            },
            align: if cf.kind == CfKind::Number { Align::Right } else { Align::Left },
            entity,
            field: Some(cf.key()),
            editor: match cf.kind {
                CfKind::Text => Editor::Text,
                CfKind::Number => Editor::Number,
                CfKind::Date => Editor::Date,
                CfKind::Enum => Editor::Select,
            },
            sum: false,
            hidden: false,
        })
        .collect()
}

pub fn project_columns(r: &Replica) -> Vec<Column> {
    let e = Entity::Project;
    let mut v = vec![
        Column::new(e, "code", "Code", 9),
        Column::new(e, "name", "Name", 26),
        Column::new(e, "program_id", "Program", 16),
        Column::new(e, "owner_id", "Owner", 14),
        Column::new(e, "status", "Status", 10),
        Column::new(e, "priority", "Priority", 9),
        Column::new(e, "health", "Health", 7),
        Column::new(e, "start_date", "Start", 10),
        Column::new(e, "due_date", "Due", 10),
        Column::new(e, "budget_hours", "Budget h", 9).sum(),
        Column::computed(e, "est", "Est h", 8, Align::Right).sum(),
        Column::computed(e, "actual", "Actual h", 9, Align::Right).sum(),
        Column::computed(e, "burn", "Burn", 12, Align::Left),
        Column::computed(e, "done", "Done", 14, Align::Left),
        Column::computed(e, "overdue", "Late", 5, Align::Right).sum(),
        Column::new(e, "tags", "Tags", 18),
    ];
    v.extend(cf_columns(r, CfEntity::Project));
    v.push(Column::new(e, "notes", "Notes", 30).hide());
    v
}

pub fn task_columns(r: &Replica, with_project: bool) -> Vec<Column> {
    let e = Entity::Task;
    let mut v = Vec::new();
    if with_project {
        v.push(Column::computed(e, "project", "Project", 9, Align::Left));
    }
    v.extend([
        Column::computed(e, "seq", "#", 3, Align::Right),
        Column::new(e, "title", "Title", 28),
        Column::new(e, "phase", "Phase", 10),
        Column::new(e, "status", "Status", 11),
        Column::new(e, "priority", "Priority", 9),
        Column::new(e, "assignee_id", "Assignee", 14),
        Column::new(e, "start_date", "Start", 10),
        Column::new(e, "due_date", "Due", 10),
        Column::new(e, "est_hours", "Est h", 7).sum(),
        Column::new(e, "actual_hours", "Act h", 7).sum(),
        Column::computed(e, "var", "Var h", 7, Align::Right).sum(),
        Column::new(e, "deps", "Deps", 10),
        Column::new(e, "tags", "Tags", 14),
    ]);
    v.extend(cf_columns(r, CfEntity::Task));
    v.push(Column::new(e, "notes", "Notes", 30).hide());
    v
}

pub fn template_columns() -> Vec<Column> {
    let e = Entity::Template;
    vec![
        Column::new(e, "name", "Template", 22),
        Column::computed(e, "count", "Tasks", 6, Align::Right),
        Column::computed(e, "hours", "Est h", 7, Align::Right),
        Column::computed(e, "span", "Days", 5, Align::Right),
        Column::new(e, "description", "Description", 40).editor(Editor::Text),
    ]
}

pub fn template_task_columns() -> Vec<Column> {
    let e = Entity::TemplateTask;
    vec![
        Column::new(e, "seq", "Seq", 4),
        Column::new(e, "title", "Title", 28),
        Column::new(e, "phase", "Phase", 12).editor(Editor::Text),
        Column::new(e, "est_hours", "Est h", 7).sum(),
        Column::new(e, "offset_days", "Offset d", 8),
        Column::new(e, "duration_days", "Dur d", 6),
    ]
}

pub fn custom_field_columns() -> Vec<Column> {
    let e = Entity::CustomField;
    vec![
        Column::new(e, "entity", "Applies to", 10),
        Column::new(e, "name", "Name", 20),
        Column::new(e, "kind", "Kind", 8),
        Column::new(e, "options", "Options (enum)", 36),
        Column::new(e, "seq", "Order", 6),
    ]
}

pub fn user_columns() -> Vec<Column> {
    let e = Entity::User;
    vec![
        Column::computed(e, "username", "Username", 12, Align::Left),
        Column::new(e, "display_name", "Name", 20),
        Column::new(e, "role", "Role", 8),
        Column::new(e, "active", "Active", 7),
        Column::computed(e, "open_tasks", "Open tasks", 10, Align::Right).sum(),
        Column::computed(e, "open_hours", "Open est h", 10, Align::Right).sum(),
        Column::computed(e, "owned", "Projects", 8, Align::Right).sum(),
    ]
}

pub fn program_columns() -> Vec<Column> {
    let e = Entity::Program;
    vec![
        Column::new(e, "name", "Program", 22),
        Column::new(e, "client", "Client", 22),
        Column::new(e, "prefix", "Prefix", 7),
        Column::new(e, "color", "Colour", 8),
        Column::computed(e, "projects", "Projects", 8, Align::Right).sum(),
    ]
}

pub fn activity_columns() -> Vec<Column> {
    // Activity is not an entity; we borrow Entity::User only as a tag.
    let e = Entity::User;
    vec![
        Column::computed(e, "ts", "When", 9, Align::Left),
        Column::computed(e, "user", "Who", 14, Align::Left),
        Column::computed(e, "action", "Action", 7, Align::Left),
        Column::computed(e, "label", "Change", 60, Align::Left),
        Column::computed(e, "state", "State", 7, Align::Left),
    ]
}

/// Visual hint for a cell; front ends map these to colours.
#[derive(Clone, Debug, PartialEq)]
pub enum Style {
    Plain,
    Muted,
    Strong,
    ProjectStatus(ProjectStatus),
    TaskStatus(TaskStatus),
    Priority(Priority),
    Health(Health),
    Role(Role),
    Overdue,
    DueSoon,
    /// progress bar: percent (may exceed 100), and severity 0 ok / 1 warn / 2 bad
    Bar { pct: f64, level: u8 },
    Bad,
    Good,
    Tags,
    Color(String),
    User(i64),
    Struck,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SortKey {
    Null,
    Num(f64),
    Text(String),
}

impl SortKey {
    fn rank(&self) -> u8 {
        match self {
            SortKey::Num(_) => 0,
            SortKey::Text(_) => 1,
            SortKey::Null => 2,
        }
    }
    pub fn cmp_asc(&self, o: &SortKey) -> Ordering {
        match (self, o) {
            (SortKey::Num(a), SortKey::Num(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
            _ => self.rank().cmp(&o.rank()),
        }
    }
    pub fn group_key(&self) -> String {
        match self {
            SortKey::Null => String::new(),
            SortKey::Num(n) => format!("{n}"),
            SortKey::Text(s) => s.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub text: String,
    pub sort: SortKey,
    pub style: Style,
    /// numeric value for sums
    pub num: Option<f64>,
}

impl Cell {
    fn text(s: impl Into<String>) -> Cell {
        let s = s.into();
        Cell {
            sort: if s.is_empty() { SortKey::Null } else { SortKey::Text(s.to_lowercase()) },
            text: s,
            style: Style::Plain,
            num: None,
        }
    }
    fn styled(s: impl Into<String>, style: Style) -> Cell {
        let mut c = Cell::text(s);
        c.style = style;
        c
    }
    fn sort(mut self, k: SortKey) -> Cell {
        self.sort = k;
        self
    }
    fn hours(h: Option<f64>) -> Cell {
        match h {
            Some(h) => Cell {
                text: fmt_hours(h),
                sort: SortKey::Num(h),
                style: Style::Plain,
                num: Some(h),
            },
            None => Cell {
                text: String::new(),
                sort: SortKey::Null,
                style: Style::Plain,
                num: None,
            },
        }
    }
    fn int(n: i64) -> Cell {
        Cell {
            text: n.to_string(),
            sort: SortKey::Num(n as f64),
            style: Style::Plain,
            num: Some(n as f64),
        }
    }
    fn empty() -> Cell {
        Cell::text("")
    }
}

pub fn fmt_hours(h: f64) -> String {
    // `+ 0.0` turns -0.0 (e.g. the sum of an empty iterator) into 0.0
    let h = round2(h) + 0.0;
    if h.fract() == 0.0 {
        format!("{h:.0}")
    } else if (h * 10.0).fract().abs() < 1e-9 {
        format!("{h:.1}")
    } else {
        format!("{h:.2}")
    }
}

fn date_cell(r: &Replica, d: &Option<String>, open: bool) -> Cell {
    let Some(date) = d.as_deref().and_then(Date::parse_iso) else {
        return Cell::empty();
    };
    let style = if !open {
        Style::Plain
    } else if date < r.today {
        Style::Overdue
    } else if date.days() - r.today.days() <= 7 {
        Style::DueSoon
    } else {
        Style::Plain
    };
    Cell {
        text: date.iso(),
        sort: SortKey::Num(date.days() as f64),
        style,
        num: None,
    }
}

fn plain_date(d: &Option<String>) -> Cell {
    match d.as_deref().and_then(Date::parse_iso) {
        Some(date) => Cell::text(date.iso()).sort(SortKey::Num(date.days() as f64)),
        None => Cell::empty(),
    }
}

fn user_cell(r: &Replica, id: Option<i64>) -> Cell {
    match id.and_then(|id| r.users.get(&id)) {
        Some(u) => Cell::styled(u.display_name.clone(), Style::User(u.id)),
        None => Cell::empty(),
    }
}

fn cf_cell(r: &Replica, cf: &CfValues, key: &str) -> Cell {
    let Some(id) = fields::cf_id(key) else { return Cell::empty() };
    let Some(def) = r.custom_fields.get(&id) else { return Cell::empty() };
    match cf.get(&id.to_string()) {
        None | Some(Value::Null) => Cell::empty(),
        Some(v) => match def.kind {
            CfKind::Number => {
                let n = v.as_f64().unwrap_or(0.0);
                Cell {
                    text: fmt_hours(n),
                    sort: SortKey::Num(n),
                    style: Style::Plain,
                    num: Some(n),
                }
            }
            CfKind::Date => plain_date(&v.as_str().map(String::from)),
            _ => Cell::text(v.as_str().unwrap_or_default()),
        },
    }
}

fn tags_cell(tags: &[String]) -> Cell {
    Cell::styled(tags.join(" "), if tags.is_empty() { Style::Plain } else { Style::Tags })
}

fn bar(pct: f64, level: u8) -> Cell {
    Cell {
        text: format!("{:.0}%", pct.round()),
        sort: SortKey::Num(pct),
        style: Style::Bar { pct, level },
        num: None,
    }
}

/// The cell for row `id` of `col.entity` (or an activity id for activity columns).
pub fn cell(r: &Replica, col: &Column, id: i64) -> Cell {
    let key = col.key.as_str();
    match col.entity {
        Entity::Project => {
            let Some(p) = r.projects.get(&id) else { return Cell::empty() };
            let open = r.project_open(p);
            match key {
                "code" => Cell::styled(p.code.clone(), Style::Strong),
                "name" => Cell::text(p.name.clone()),
                "program_id" => match p.program_id.and_then(|i| r.programs.get(&i)) {
                    Some(pg) => Cell::styled(pg.name.clone(), Style::Color(pg.color.clone())),
                    None => Cell::empty(),
                },
                "owner_id" => user_cell(r, p.owner_id),
                "status" => Cell::styled(p.status.label(), Style::ProjectStatus(p.status))
                    .sort(SortKey::Num(p.status.index() as f64)),
                "priority" => Cell::styled(p.priority.label(), Style::Priority(p.priority))
                    .sort(SortKey::Num(p.priority.index() as f64)),
                "health" => Cell::styled(p.health.label(), Style::Health(p.health))
                    .sort(SortKey::Num(p.health.index() as f64)),
                "start_date" => plain_date(&p.start_date),
                "due_date" => date_cell(r, &p.due_date, open),
                "budget_hours" => Cell::hours(p.budget_hours),
                "est" => Cell::hours(Some(r.metrics(id).est_total)),
                "actual" => {
                    let mut c = Cell::hours(Some(r.metrics(id).actual_total));
                    if r.burn_pct(p).is_some_and(|b| b > 100.0) {
                        c.style = Style::Bad;
                    }
                    c
                }
                "burn" => match r.burn_pct(p) {
                    Some(b) => bar(b, if b > 100.0 { 2 } else if b > 90.0 { 1 } else { 0 }),
                    None => Cell::empty(),
                },
                "done" => {
                    let m = r.metrics(id);
                    let mut c = bar(m.pct_done(), 0);
                    c.text = format!("{}/{}", m.tasks_done, m.tasks_total);
                    c
                }
                "overdue" => {
                    let n = r.metrics(id).tasks_overdue;
                    let mut c = Cell::int(n as i64);
                    if n == 0 {
                        c.text.clear();
                    } else {
                        c.style = Style::Overdue;
                    }
                    c
                }
                "tags" => tags_cell(&p.tags),
                "notes" => Cell::text(first_line(&p.notes)),
                k if k.starts_with("cf.") => cf_cell(r, &p.cf, k),
                _ => Cell::empty(),
            }
        }
        Entity::Task => {
            let Some(t) = r.tasks.get(&id) else { return Cell::empty() };
            let open = t.status != TaskStatus::Done;
            match key {
                "project" => {
                    let code = r.project_code(t.project_id).to_string();
                    Cell::styled(code, Style::Strong)
                }
                "seq" => Cell::int(t.seq),
                "title" => Cell::styled(t.title.clone(), if open { Style::Plain } else { Style::Muted }),
                "phase" => Cell::text(t.phase.clone()),
                "status" => {
                    let s = if t.status != TaskStatus::Blocked && r.task_blocked(t) {
                        Style::TaskStatus(TaskStatus::Blocked)
                    } else {
                        Style::TaskStatus(t.status)
                    };
                    Cell::styled(t.status.label(), s).sort(SortKey::Num(t.status.index() as f64))
                }
                "priority" => Cell::styled(t.priority.label(), Style::Priority(t.priority))
                    .sort(SortKey::Num(t.priority.index() as f64)),
                "assignee_id" => user_cell(r, t.assignee_id),
                "start_date" => plain_date(&t.start_date),
                "due_date" => date_cell(r, &t.due_date, open),
                "est_hours" => Cell::hours(t.est_hours),
                "actual_hours" => {
                    let mut c = Cell::hours(Some(t.actual_hours));
                    if t.actual_hours == 0.0 {
                        c.text.clear();
                    }
                    if t.est_hours.is_some_and(|e| t.actual_hours > e) {
                        c.style = Style::Bad;
                    }
                    c
                }
                "var" => match t.est_hours {
                    Some(e) if t.actual_hours > 0.0 => {
                        let v = t.actual_hours - e;
                        let mut c = Cell::hours(Some(v));
                        if v > 0.0 {
                            c.text = format!("+{}", c.text);
                        }
                        c.style = if v > 0.0 { Style::Bad } else { Style::Good };
                        c
                    }
                    _ => Cell::empty(),
                },
                "deps" => {
                    let txt: Vec<String> = t
                        .deps
                        .iter()
                        .filter_map(|d| r.tasks.get(d))
                        .map(|d| format!("#{}", d.seq))
                        .collect();
                    let waiting = t.deps.iter().any(|d| r.task_status(*d).is_some_and(|s| s != TaskStatus::Done));
                    Cell::styled(txt.join(" "), if waiting && open { Style::DueSoon } else { Style::Muted })
                }
                "tags" => tags_cell(&t.tags),
                "notes" => Cell::text(first_line(&t.notes)),
                k if k.starts_with("cf.") => cf_cell(r, &t.cf, k),
                _ => Cell::empty(),
            }
        }
        Entity::Template => {
            let Some(t) = r.templates.get(&id) else { return Cell::empty() };
            let (n, h, span) = r.template_stats(id);
            match key {
                "name" => Cell::styled(t.name.clone(), Style::Strong),
                "description" => Cell::text(t.description.clone()),
                "count" => Cell::int(n as i64),
                "hours" => Cell::hours(Some(h)),
                "span" => Cell::int(span),
                _ => Cell::empty(),
            }
        }
        Entity::TemplateTask => {
            let Some(t) = r.template_tasks.get(&id) else { return Cell::empty() };
            match key {
                "seq" => Cell::int(t.seq),
                "title" => Cell::text(t.title.clone()),
                "phase" => Cell::text(t.phase.clone()),
                "est_hours" => Cell::hours(t.est_hours),
                "offset_days" => Cell::int(t.offset_days),
                "duration_days" => Cell::int(t.duration_days),
                _ => Cell::empty(),
            }
        }
        Entity::CustomField => {
            let Some(c) = r.custom_fields.get(&id) else { return Cell::empty() };
            match key {
                "entity" => Cell::text(c.entity.label()),
                "name" => Cell::styled(c.name.clone(), Style::Strong),
                "kind" => Cell::text(c.kind.label()),
                "options" => tags_cell(&c.options),
                "seq" => Cell::int(c.seq),
                _ => Cell::empty(),
            }
        }
        Entity::User if col.key == "ts" || ["user", "action", "label", "state"].contains(&key) => {
            activity_cell(r, key, id)
        }
        Entity::User => {
            let Some(u) = r.users.get(&id) else { return Cell::empty() };
            match key {
                "username" => Cell::styled(u.username.clone(), Style::User(u.id)),
                "display_name" => Cell::text(u.display_name.clone()),
                "role" => Cell::styled(u.role.label(), Style::Role(u.role)).sort(SortKey::Num(u.role.index() as f64)),
                "active" => Cell::styled(
                    if u.active { "✓ yes" } else { "✗ no" },
                    if u.active { Style::Good } else { Style::Muted },
                ),
                "open_tasks" | "open_hours" => {
                    let open = r.tasks.values().filter(|t| t.assignee_id == Some(id) && t.status != TaskStatus::Done);
                    if key == "open_tasks" {
                        Cell::int(open.count() as i64)
                    } else {
                        Cell::hours(Some(open.map(|t| t.est_hours.unwrap_or(0.0)).sum()))
                    }
                }
                "owned" => Cell::int(r.projects.values().filter(|p| p.owner_id == Some(id)).count() as i64),
                _ => Cell::empty(),
            }
        }
        Entity::Program => {
            let Some(p) = r.programs.get(&id) else { return Cell::empty() };
            match key {
                "name" => Cell::styled(p.name.clone(), Style::Color(p.color.clone())),
                "client" => Cell::text(p.client.clone()),
                "prefix" => Cell::styled(p.prefix.clone(), Style::Strong),
                "color" => Cell::styled(p.color.clone(), Style::Color(p.color.clone())),
                "projects" => Cell::int(r.projects.values().filter(|x| x.program_id == Some(id)).count() as i64),
                _ => Cell::empty(),
            }
        }
    }
}

/// Activity rows are addressed by activity id; `now` comes from the replica's today.
pub fn activity_cell(r: &Replica, key: &str, id: i64) -> Cell {
    let Some(a) = r.activity.get(&id) else { return Cell::empty() };
    let struck = a.state == ActState::Undone;
    let mut c = match key {
        // today: "14:03"; earlier: "12 Oct" (UTC; front ends may show activity_when instead)
        "ts" => {
            let day = a.ts.get(0..10).and_then(Date::parse_iso);
            let text = match day {
                Some(d) if d == r.today => a.ts.get(11..16).unwrap_or("").to_string(),
                Some(d) => d.short(r.today.year()).trim().to_string(),
                None => a.ts.clone(),
            };
            Cell::text(text).sort(SortKey::Num(a.id as f64))
        }
        "user" => user_cell(r, a.user_id),
        "action" => Cell::text(a.action.label()),
        "label" => Cell::text(a.label.clone()),
        "state" => Cell::styled(a.state.label(), if struck { Style::Muted } else { Style::Plain }),
        _ => Cell::empty(),
    };
    if struck && key == "label" {
        c.style = Style::Struck;
    }
    c
}

/// Relative time for activity rows given "now" in Unix seconds (front ends know the clock).
pub fn activity_when(a: &Activity, now_secs: i64) -> String {
    relative_time(&a.ts, now_secs)
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

// ---- sorting & grouping ---------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub key: String,
    pub desc: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridRow {
    Group {
        key: String,
        label: String,
        count: usize,
        collapsed: bool,
        /// aligned with the columns passed to `build_rows`
        sums: Vec<Option<f64>>,
        style: Style,
    },
    Data(i64),
}

impl GridRow {
    pub fn id(&self) -> Option<i64> {
        match self {
            GridRow::Data(id) => Some(*id),
            _ => None,
        }
    }
}

pub fn sort_ids(r: &Replica, cols: &[Column], ids: &mut [i64], sort: &[SortSpec]) {
    if sort.is_empty() {
        return;
    }
    let specs: Vec<(&Column, bool)> = sort
        .iter()
        .filter_map(|s| cols.iter().find(|c| c.key == s.key).map(|c| (c, s.desc)))
        .collect();
    let mut keyed: Vec<(Vec<SortKey>, i64)> = ids
        .iter()
        .map(|id| (specs.iter().map(|(c, _)| cell(r, c, *id).sort).collect(), *id))
        .collect();
    keyed.sort_by(|a, b| {
        for (i, (_, desc)) in specs.iter().enumerate() {
            let mut o = a.0[i].cmp_asc(&b.0[i]);
            // nulls stay last in both directions
            if *desc && !matches!((&a.0[i], &b.0[i]), (SortKey::Null, _) | (_, SortKey::Null)) {
                o = o.reverse();
            }
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
    for (slot, (_, id)) in ids.iter_mut().zip(keyed) {
        *slot = id;
    }
}

fn sums(r: &Replica, cols: &[Column], ids: &[i64]) -> Vec<Option<f64>> {
    cols.iter()
        .map(|c| {
            if !c.sum {
                return None;
            }
            Some(ids.iter().filter_map(|id| cell(r, c, *id).num).sum())
        })
        .collect()
}

/// Footer totals for the visible rows.
pub fn totals(r: &Replica, cols: &[Column], ids: &[i64]) -> Vec<Option<f64>> {
    sums(r, cols, ids)
}

/// Rows for a grid: sorted ids, optionally grouped by a column, with group
/// headers carrying counts and subtotals. Groups keep the order of their
/// first row after sorting by (group column, then the sort spec).
pub fn build_rows(
    r: &Replica,
    cols: &[Column],
    mut ids: Vec<i64>,
    sort: &[SortSpec],
    group_by: Option<&str>,
    collapsed: &HashSet<String>,
) -> Vec<GridRow> {
    let Some(gcol) = group_by.and_then(|g| cols.iter().find(|c| c.key == g)) else {
        sort_ids(r, cols, &mut ids, sort);
        return ids.into_iter().map(GridRow::Data).collect();
    };
    let mut spec = vec![SortSpec { key: gcol.key.clone(), desc: false }];
    spec.extend(sort.iter().filter(|s| s.key != gcol.key).cloned());
    sort_ids(r, cols, &mut ids, &spec);
    let mut out = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let c0 = cell(r, gcol, ids[i]);
        let key = c0.sort.group_key();
        let mut j = i + 1;
        while j < ids.len() && cell(r, gcol, ids[j]).sort.group_key() == key {
            j += 1;
        }
        let members = &ids[i..j];
        let is_collapsed = collapsed.contains(&key);
        let label = if c0.text.is_empty() { "(none)".to_string() } else { group_label(r, gcol, members[0], &c0) };
        out.push(GridRow::Group {
            key: key.clone(),
            label,
            count: members.len(),
            collapsed: is_collapsed,
            sums: sums(r, cols, members),
            style: c0.style.clone(),
        });
        if !is_collapsed {
            out.extend(members.iter().map(|id| GridRow::Data(*id)));
        }
        i = j;
    }
    out
}

fn group_label(r: &Replica, col: &Column, id: i64, c: &Cell) -> String {
    // Tasks grouped by project read better with the project name.
    if col.entity == Entity::Task && col.key == "project" {
        if let Some(p) = r.tasks.get(&id).and_then(|t| r.projects.get(&t.project_id)) {
            return format!("{}  {}", p.code, p.name);
        }
    }
    c.text.clone()
}

// ---- editing --------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub value: Value,
    pub label: String,
    pub detail: String,
    pub style: Style,
}

impl Choice {
    fn new(value: Value, label: impl Into<String>, detail: impl Into<String>, style: Style) -> Choice {
        Choice {
            value,
            label: label.into(),
            detail: detail.into(),
            style,
        }
    }
}

fn none_choice() -> Choice {
    Choice::new(Value::Null, "(none)", "", Style::Muted)
}

/// Choices for Select / Combo / Suggest / Chips editors of `field` on `entity`
/// row `id` (id 0 = a new row; `project_hint` gives the project for new tasks).
pub fn choices(r: &Replica, entity: Entity, field: &str, id: i64, project_hint: Option<i64>) -> Vec<Choice> {
    if let Some(cfid) = fields::cf_id(field) {
        let Some(cf) = r.custom_fields.get(&cfid) else { return vec![] };
        let mut v = vec![none_choice()];
        v.extend(cf.options.iter().map(|o| Choice::new(Value::from(o.as_str()), o.clone(), "", Style::Plain)));
        return v;
    }
    let users = |nullable: bool| {
        let mut v = if nullable { vec![none_choice()] } else { vec![] };
        let mut us: Vec<&User> = r.users.values().filter(|u| u.active).collect();
        us.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        v.extend(us.into_iter().map(|u| {
            Choice::new(Value::from(u.id), u.display_name.clone(), format!("@{} · {}", u.username, u.role.label()), Style::User(u.id))
        }));
        v
    };
    let task_project = || match entity {
        Entity::Task => r.tasks.get(&id).map(|t| t.project_id).or(project_hint),
        _ => project_hint,
    };
    match field {
        "status" if entity == Entity::Project => ProjectStatus::ALL
            .iter()
            .map(|s| Choice::new(Value::from(s.as_str()), s.label(), "", Style::ProjectStatus(*s)))
            .collect(),
        "status" => TaskStatus::ALL
            .iter()
            .map(|s| Choice::new(Value::from(s.as_str()), s.label(), "", Style::TaskStatus(*s)))
            .collect(),
        "priority" => Priority::ALL
            .iter()
            .map(|s| Choice::new(Value::from(s.as_str()), s.label(), "", Style::Priority(*s)))
            .collect(),
        "health" => Health::ALL
            .iter()
            .map(|s| Choice::new(Value::from(s.as_str()), s.label(), "", Style::Health(*s)))
            .collect(),
        "role" => Role::ALL
            .iter()
            .map(|s| Choice::new(Value::from(s.as_str()), s.label(), "", Style::Role(*s)))
            .collect(),
        "kind" => CfKind::ALL.iter().map(|s| Choice::new(Value::from(s.as_str()), s.label(), "", Style::Plain)).collect(),
        "entity" => CfEntity::ALL.iter().map(|s| Choice::new(Value::from(s.as_str()), s.label(), "", Style::Plain)).collect(),
        "active" => vec![
            Choice::new(Value::Bool(true), "✓ yes", "", Style::Good),
            Choice::new(Value::Bool(false), "✗ no", "", Style::Muted),
        ],
        "owner_id" | "assignee_id" => users(true),
        "program_id" => {
            let mut v = vec![none_choice()];
            v.extend(r.programs.values().map(|p| {
                Choice::new(Value::from(p.id), p.name.clone(), format!("{} · {}", p.prefix, p.client), Style::Color(p.color.clone()))
            }));
            v
        }
        "template_id" => {
            let mut v = vec![Choice::new(Value::Null, "(blank project)", "no tasks", Style::Muted)];
            v.extend(r.templates.values().map(|t| {
                let (n, h, span) = r.template_stats(t.id);
                Choice::new(Value::from(t.id), t.name.clone(), format!("{n} tasks, {} h, {span} days", fmt_hours(h)), Style::Plain)
            }));
            v
        }
        "project_id" => r
            .projects
            .values()
            .map(|p| Choice::new(Value::from(p.id), format!("{}  {}", p.code, p.name), r.program_name(p.program_id).to_string(), Style::ProjectStatus(p.status)))
            .collect(),
        "phase" => {
            let phases = match entity {
                Entity::Task => task_project().map(|pid| r.phases_for_project(pid)).unwrap_or_default(),
                _ => {
                    let mut all: Vec<String> = r.template_tasks.values().map(|t| t.phase.clone()).filter(|p| !p.is_empty()).collect();
                    all.sort();
                    all.dedup();
                    all
                }
            };
            phases.into_iter().map(|p| Choice::new(Value::from(p.as_str()), p, "", Style::Plain)).collect()
        }
        "tags" => r
            .tag_counts(if entity == Entity::Project { Entity::Project } else { Entity::Task })
            .into_iter()
            .map(|(t, n)| Choice::new(Value::from(t.as_str()), t, format!("{n}×"), Style::Tags))
            .collect(),
        "deps" => {
            let Some(pid) = task_project() else { return vec![] };
            r.project_tasks(pid)
                .iter()
                .filter(|tid| **tid != id)
                .filter_map(|tid| r.tasks.get(tid))
                .map(|t| Choice::new(Value::from(t.id), format!("#{} {}", t.seq, t.title), t.status.label(), Style::TaskStatus(t.status)))
                .collect()
        }
        "options" => r
            .custom_fields
            .get(&id)
            .map(|c| c.options.iter().map(|o| Choice::new(Value::from(o.as_str()), o.clone(), "", Style::Plain)).collect())
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// Fuzzy-filter choices by label (and detail), best first; empty query keeps order.
pub fn filter_choices(choices: &[Choice], query: &str) -> Vec<(usize, Vec<usize>)> {
    if query.trim().is_empty() {
        return (0..choices.len()).map(|i| (i, vec![])).collect();
    }
    let mut out: Vec<(usize, i32, Vec<usize>)> = choices
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let m = fuzzy::fuzzy_match(query, &c.label);
            match m {
                Some(m) => Some((i, m.score, m.indices)),
                None => fuzzy::fuzzy_match(query, &c.detail).map(|m| (i, m.score - 30, vec![])),
            }
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.into_iter().map(|(i, _, m)| (i, m)).collect()
}

/// Display text for an arbitrary field value (conflict messages, activity, chips).
pub fn display_value(r: &Replica, entity: Entity, field: &str, v: &Value) -> String {
    match v {
        Value::Null => "(empty)".into(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.into(),
        Value::Array(a) if field == "deps" => a
            .iter()
            .filter_map(|x| x.as_i64())
            .map(|id| r.tasks.get(&id).map_or(format!("task {id}"), |t| format!("#{}", t.seq)))
            .collect::<Vec<_>>()
            .join(" "),
        Value::Array(a) => a.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join(" "),
        Value::Number(n) => match field {
            "owner_id" | "assignee_id" => r.user_name(n.as_i64()).to_string(),
            "program_id" => r.program_name(n.as_i64()).to_string(),
            "template_id" => n.as_i64().and_then(|i| r.templates.get(&i)).map_or(n.to_string(), |t| t.name.clone()),
            "project_id" => n.as_i64().map_or(n.to_string(), |i| r.project_code(i).to_string()),
            _ => n.as_f64().map(fmt_hours).unwrap_or_else(|| n.to_string()),
        },
        Value::String(s) => match (entity, field) {
            (Entity::Project, "status") => ProjectStatus::parse(s).map_or(s.clone(), |x| x.label().into()),
            (Entity::Task, "status") => TaskStatus::parse(s).map_or(s.clone(), |x| x.label().into()),
            (_, "priority") => Priority::parse(s).map_or(s.clone(), |x| x.label().into()),
            (_, "health") => Health::parse(s).map_or(s.clone(), |x| x.label().into()),
            _ => s.clone(),
        },
        Value::Object(_) => "…".into(),
    }
}

/// Text placed in a text editor when editing starts (also used for copy).
pub fn edit_text(r: &Replica, entity: Entity, field: &str, id: i64) -> String {
    let v = r.field_value(entity, id, field);
    match (&v, field) {
        (Value::Null, _) => String::new(),
        (Value::String(s), _) => s.clone(),
        (Value::Number(n), "owner_id" | "assignee_id") => r.username(n.as_i64()).to_string(),
        (Value::Number(n), "program_id") => r.program_name(n.as_i64()).to_string(),
        (Value::Number(n), _) => n.as_f64().map(fmt_hours).unwrap_or_default(),
        (Value::Array(_), _) => display_value(r, entity, field, &v),
        (Value::Bool(b), _) => if *b { "yes" } else { "no" }.into(),
        _ => String::new(),
    }
}

/// Parse typed or pasted text into a field value. Accepts labels, usernames,
/// display names, natural dates, `#seq` deps and space/comma separated lists.
pub fn parse_input(r: &Replica, entity: Entity, field: &str, id: i64, text: &str) -> Result<Value, String> {
    let t = text.trim();
    if let Some(cfid) = fields::cf_id(field) {
        let cf = r.custom_fields.get(&cfid).ok_or("unknown custom field")?;
        if t.is_empty() {
            return Ok(Value::Null);
        }
        return match cf.kind {
            CfKind::Text => Ok(Value::from(t)),
            CfKind::Number => t.parse::<f64>().map(Value::from).map_err(|_| format!("'{t}' is not a number")),
            CfKind::Date => parse_natural(t, r.today).map(|d| d.map_or(Value::Null, |d| Value::from(d.iso()))),
            CfKind::Enum => cf
                .options
                .iter()
                .find(|o| o.eq_ignore_ascii_case(t))
                .or_else(|| cf.options.iter().find(|o| o.to_lowercase().starts_with(&t.to_lowercase())))
                .map(|o| Value::from(o.as_str()))
                .ok_or_else(|| format!("{} must be one of {}", cf.name, cf.options.join(", "))),
        };
    }
    let meta = fields::field(entity, field).ok_or_else(|| format!("unknown field {field}"))?;
    if t.is_empty() {
        return Ok(match meta.kind {
            Kind::Tags | Kind::Deps | Kind::Options => Value::Array(vec![]),
            Kind::Text { .. } => Value::from(""),
            Kind::Hours if !meta.nullable => Value::from(0),
            _ => Value::Null,
        });
    }
    let lower = t.to_lowercase();
    match meta.kind {
        Kind::Text { .. } | Kind::Code | Kind::Username | Kind::Prefix | Kind::Color | Kind::Password => {
            // notes keep their surrounding whitespace
            let notes = matches!(meta.kind, Kind::Text { max, .. } if max > 2000);
            Ok(Value::from(if notes { text } else { t }))
        }
        Kind::Enum(_) => {
            let ch = choices(r, entity, field, id, None);
            ch.iter()
                .find(|c| c.label.eq_ignore_ascii_case(t) || c.value.as_str().is_some_and(|v| v.eq_ignore_ascii_case(&lower.replace(' ', "_"))))
                .or_else(|| ch.iter().find(|c| c.label.to_lowercase().starts_with(&lower)))
                .map(|c| c.value.clone())
                .ok_or_else(|| format!("{} must be one of {}", meta.label, ch.iter().map(|c| c.label.as_str()).collect::<Vec<_>>().join(", ")))
        }
        Kind::Ref(target) => {
            let ch = choices(r, entity, field, id, None);
            let hit = match target {
                Entity::User => {
                    let at = lower.trim_start_matches('@');
                    r.users
                        .values()
                        .find(|u| u.username == at)
                        .or_else(|| r.users.values().find(|u| u.display_name.to_lowercase() == lower))
                        .or_else(|| r.users.values().find(|u| u.display_name.to_lowercase().starts_with(&lower)))
                        .map(|u| Value::from(u.id))
                }
                Entity::Program => r
                    .programs
                    .values()
                    .find(|p| p.name.to_lowercase() == lower || p.prefix.to_lowercase() == lower)
                    .or_else(|| r.programs.values().find(|p| p.name.to_lowercase().starts_with(&lower)))
                    .map(|p| Value::from(p.id)),
                Entity::Project => r
                    .projects
                    .values()
                    .find(|p| p.code.to_lowercase() == lower)
                    .map(|p| Value::from(p.id)),
                Entity::Template => r
                    .templates
                    .values()
                    .find(|p| p.name.to_lowercase() == lower)
                    .map(|p| Value::from(p.id)),
                _ => None,
            };
            if lower == "none" || lower == "(none)" {
                return Ok(Value::Null);
            }
            hit.or_else(|| {
                filter_choices(&ch, t).first().map(|(i, _)| ch[*i].value.clone()).filter(|v| !v.is_null())
            })
            .ok_or_else(|| format!("no {} matches '{t}'", target.label()))
        }
        Kind::Date => parse_natural(t, r.today).map(|d| d.map_or(Value::Null, |d| Value::from(d.iso()))),
        Kind::Hours => t
            .trim_end_matches(['h', 'H'])
            .trim()
            .parse::<f64>()
            .map(|x| Value::from(round2(x)))
            .map_err(|_| format!("'{t}' is not a number of hours")),
        Kind::Int { .. } => t.parse::<i64>().map(Value::from).map_err(|_| format!("'{t}' is not a whole number")),
        Kind::Bool => match lower.as_str() {
            "y" | "yes" | "true" | "1" | "✓" | "✓ yes" | "on" => Ok(Value::Bool(true)),
            "n" | "no" | "false" | "0" | "✗" | "✗ no" | "off" => Ok(Value::Bool(false)),
            _ => Err("type yes or no".into()),
        },
        Kind::Tags => {
            let tags: Vec<String> = t.split([' ', ',', ';']).filter(|s| !s.is_empty()).map(|s| s.trim_start_matches('#').to_lowercase()).collect();
            if let Some(bad) = tags.iter().find(|x| !valid_tag(x)) {
                return Err(format!("tag '{bad}' must be a–z, 0–9, '_', '-'"));
            }
            Ok(Value::from(tags))
        }
        Kind::Options => Ok(Value::from(t.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>())),
        Kind::Deps => {
            let pid = r.tasks.get(&id).map(|x| x.project_id).ok_or("unknown task")?;
            let mut out = Vec::new();
            for part in t.split([' ', ',', ';']).filter(|s| !s.is_empty()) {
                let seq: i64 = part.trim_start_matches('#').parse().map_err(|_| format!("'{part}' is not a task number like #3"))?;
                let dep = r
                    .project_tasks(pid)
                    .iter()
                    .find(|tid| r.tasks[*tid].seq == seq)
                    .ok_or_else(|| format!("no task #{seq} in this project"))?;
                out.push(*dep);
            }
            Ok(Value::from(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replica::tests::sample;
    use serde_json::json;

    #[test]
    fn cells() {
        let r = sample();
        let cols = task_columns(&r, true);
        let c = |k: &str, id| cell(&r, cols.iter().find(|c| c.key == k).unwrap(), id);
        assert_eq!(c("project", 101).text, "NWR-004");
        assert_eq!(c("status", 101).text, "In progress");
        assert_eq!(c("due_date", 101).style, Style::Overdue);
        assert_eq!(c("var", 100).text, "+1");
        assert_eq!(c("var", 101).text, "-6.5");
        assert_eq!(c("assignee_id", 101).text, "Bob Stone");
        let pcols = project_columns(&r);
        let burn = cell(&r, pcols.iter().find(|c| c.key == "burn").unwrap(), 10);
        assert_eq!(burn.text, "15%");
        assert_eq!(cell(&r, pcols.iter().find(|c| c.key == "done").unwrap(), 10).text, "1/3");
    }

    #[test]
    fn sort_and_group() {
        let r = sample();
        let cols = task_columns(&r, true);
        let rows = build_rows(&r, &cols, vec![100, 101, 102], &[SortSpec { key: "est_hours".into(), desc: true }], None, &HashSet::new());
        // nulls last even when descending
        assert_eq!(rows, vec![GridRow::Data(101), GridRow::Data(100), GridRow::Data(102)]);
        let rows = build_rows(&r, &cols, vec![100, 101, 102], &[], Some("project"), &HashSet::new());
        let GridRow::Group { label, count, sums, .. } = &rows[0] else { panic!() };
        assert_eq!(label, "NWR-004  POS refresh");
        assert_eq!(*count, 3);
        let est_idx = cols.iter().position(|c| c.key == "est_hours").unwrap();
        assert_eq!(sums[est_idx], Some(20.0));
        let mut collapsed = HashSet::new();
        collapsed.insert("nwr-004".to_string());
        assert_eq!(build_rows(&r, &cols, vec![100, 101, 102], &[], Some("project"), &collapsed).len(), 1);
    }

    #[test]
    fn parsing() {
        let r = sample();
        assert_eq!(parse_input(&r, Entity::Task, "status", 101, "done"), Ok(json!("done")));
        assert_eq!(parse_input(&r, Entity::Task, "status", 101, "In progress"), Ok(json!("in_progress")));
        assert_eq!(parse_input(&r, Entity::Task, "status", 101, "rev"), Ok(json!("review")));
        assert_eq!(parse_input(&r, Entity::Task, "assignee_id", 101, "bob"), Ok(json!(2)));
        assert_eq!(parse_input(&r, Entity::Task, "assignee_id", 101, "Alice Nguyen"), Ok(json!(1)));
        assert_eq!(parse_input(&r, Entity::Task, "due_date", 101, "fri"), Ok(json!("2026-10-23")));
        assert_eq!(parse_input(&r, Entity::Task, "est_hours", 101, "2.5h"), Ok(json!(2.5)));
        assert_eq!(parse_input(&r, Entity::Task, "deps", 102, "#1 #2"), Ok(json!([100, 101])));
        assert_eq!(parse_input(&r, Entity::Task, "tags", 102, "a, #B"), Ok(json!(["a", "b"])));
        assert!(parse_input(&r, Entity::Task, "deps", 102, "#9").is_err());
        assert_eq!(edit_text(&r, Entity::Task, "assignee_id", 101), "bob");
        assert_eq!(display_value(&r, Entity::Task, "status", &json!("in_progress")), "In progress");
    }

    #[test]
    fn hours_format() {
        assert_eq!(fmt_hours(std::iter::empty::<f64>().sum()), "0");
        assert_eq!(fmt_hours(2.5), "2.5");
        assert_eq!(fmt_hours(1.234), "1.23");
    }

    #[test]
    fn choice_filtering() {
        let r = sample();
        let ch = choices(&r, Entity::Task, "assignee_id", 101, None);
        assert_eq!(ch[0].label, "(none)");
        let f = filter_choices(&ch, "bo");
        assert_eq!(ch[f[0].0].label, "Bob Stone");
    }
}
