//! The filter-bar query language (protocol §9).
//!
//! `Query::compile` tokenises the input, resolves every field and value
//! against the replica (so `@alice` becomes a user id once, not per row) and
//! records errors with their character span for underlining. Terms that fail
//! to compile are ignored when matching.

use std::collections::HashSet;

use crate::date::{Date, parse_natural};
use crate::model::*;
use crate::replica::Replica;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Projects,
    Tasks,
    Activity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Colon,
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryError {
    /// Char offsets into the input.
    pub start: usize,
    pub end: usize,
    pub message: String,
}

#[derive(Clone, Debug)]
enum Pred {
    Text(String),
    TextField { field: TextField, cmp: Cmp, values: Vec<String> },
    Enum { field: EnumField, values: HashSet<&'static str> },
    Ids { field: RefField, ids: HashSet<i64>, none: bool },
    Num { field: NumField, cmp: Cmp, value: Option<f64> },
    Date { field: DateField, cmp: Cmp, value: Option<Date> },
    Tag(Vec<String>),
    Cf { id: i64, kind: CfKind, cmp: Cmp, values: Vec<String>, num: Option<f64>, date: Option<Date> },
    Is(IsPred),
    Has(HasPred),
}

#[derive(Clone, Copy, Debug)]
enum TextField {
    Code,
    Name,
    Title,
    Phase,
    Notes,
    Action,
    Entity,
}
#[derive(Clone, Copy, Debug)]
enum EnumField {
    Status,
    Priority,
    Health,
}
#[derive(Clone, Copy, Debug)]
enum RefField {
    Person,
    Program,
    Project,
}
#[derive(Clone, Copy, Debug)]
enum NumField {
    Budget,
    Est,
    Actual,
    Var,
    Pct,
}
#[derive(Clone, Copy, Debug)]
enum DateField {
    Start,
    Due,
}
#[derive(Clone, Copy, Debug)]
enum IsPred {
    Open,
    Done,
    Overdue,
    Blocked,
    Mine,
    Unassigned,
}
#[derive(Clone, Copy, Debug)]
enum HasPred {
    Notes,
    Deps,
    Due,
    Start,
    Tags,
    Budget,
}

#[derive(Clone, Debug)]
struct Term {
    neg: bool,
    pred: Pred,
}

#[derive(Clone, Debug, Default)]
pub struct Query {
    terms: Vec<Term>,
    pub errors: Vec<QueryError>,
    pub source: String,
}

/// Field names offered by autocomplete for a target (built-ins only).
pub fn field_names(t: Target) -> &'static [&'static str] {
    match t {
        Target::Projects => &[
            "code", "name", "status", "priority", "health", "owner", "program", "tag", "start", "due",
            "budget", "est", "actual", "pct", "notes", "is", "has",
        ],
        Target::Tasks => &[
            "project", "title", "phase", "status", "priority", "assignee", "program", "tag", "start",
            "due", "est", "actual", "var", "notes", "is", "has",
        ],
        Target::Activity => &["user", "action", "entity"],
    }
}

pub fn is_values(t: Target) -> &'static [&'static str] {
    match t {
        Target::Tasks => &["open", "done", "overdue", "blocked", "mine", "unassigned"],
        Target::Projects => &["open", "done", "overdue", "blocked", "mine", "unassigned"],
        Target::Activity => &["mine"],
    }
}

pub fn has_values(t: Target) -> &'static [&'static str] {
    match t {
        Target::Tasks => &["notes", "deps", "due", "start", "tags"],
        Target::Projects => &["notes", "due", "start", "tags", "budget"],
        Target::Activity => &[],
    }
}

/// Split into (token, char start, char end), honouring double quotes.
fn tokenize(input: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let mut tok = String::new();
        let mut in_q = false;
        while i < chars.len() && (in_q || !chars[i].is_whitespace()) {
            if chars[i] == '"' {
                in_q = !in_q;
            } else {
                tok.push(chars[i]);
            }
            i += 1;
        }
        out.push((tok, start, i));
    }
    out
}

fn split_op(tok: &str) -> Option<(&str, Cmp, &str)> {
    let pos = tok.find([':', '>', '<', '='])?;
    let field = &tok[..pos];
    if field.is_empty() || !field.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
        return None;
    }
    let rest = &tok[pos..];
    let (cmp, len) = if rest.starts_with(">=") {
        (Cmp::Ge, 2)
    } else if rest.starts_with("<=") {
        (Cmp::Le, 2)
    } else {
        match rest.as_bytes()[0] {
            b':' => (Cmp::Colon, 1),
            b'>' => (Cmp::Gt, 1),
            b'<' => (Cmp::Lt, 1),
            _ => (Cmp::Eq, 1),
        }
    };
    Some((field, cmp, &rest[len..]))
}

fn enum_names(field: EnumField, target: Target) -> Vec<(&'static str, &'static str)> {
    match (field, target) {
        (EnumField::Status, Target::Projects) => {
            ProjectStatus::ALL.iter().map(|s| (s.as_str(), s.label())).collect()
        }
        (EnumField::Status, _) => TaskStatus::ALL.iter().map(|s| (s.as_str(), s.label())).collect(),
        (EnumField::Priority, _) => Priority::ALL.iter().map(|s| (s.as_str(), s.label())).collect(),
        (EnumField::Health, _) => Health::ALL.iter().map(|s| (s.as_str(), s.label())).collect(),
    }
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn compile(input: &str, target: Target, r: &Replica) -> Query {
        let mut q = Query {
            source: input.to_string(),
            ..Default::default()
        };
        for (tok, start, end) in tokenize(input) {
            let (neg, body) = match tok.strip_prefix('-') {
                Some(b) if !b.is_empty() => (true, b.to_string()),
                _ => (false, tok.clone()),
            };
            match compile_term(&body, target, r) {
                Ok(Some(pred)) => q.terms.push(Term { neg, pred }),
                Ok(None) => {}
                Err(message) => q.errors.push(QueryError { start, end, message }),
            }
        }
        q
    }

    pub fn matches_project(&self, p: &Project, r: &Replica) -> bool {
        self.terms.iter().all(|t| eval_project(&t.pred, p, r) != t.neg)
    }

    pub fn matches_task(&self, t: &Task, r: &Replica) -> bool {
        self.terms.iter().all(|term| eval_task(&term.pred, t, r) != term.neg)
    }

    pub fn matches_activity(&self, a: &Activity, r: &Replica) -> bool {
        self.terms.iter().all(|t| eval_activity(&t.pred, a, r) != t.neg)
    }
}

fn compile_term(body: &str, target: Target, r: &Replica) -> Result<Option<Pred>, String> {
    if let Some(tag) = body.strip_prefix('#') {
        if tag.is_empty() {
            return Ok(None);
        }
        return Ok(Some(Pred::Tag(tag.split(',').map(|s| s.to_lowercase()).collect())));
    }
    if let Some(who) = body.strip_prefix('@') {
        if who.is_empty() {
            return Ok(None);
        }
        return resolve_users(who, r).map(|(ids, none)| Some(Pred::Ids { field: RefField::Person, ids, none }));
    }
    let Some((field, cmp, value)) = split_op(body) else {
        return Ok(Some(Pred::Text(body.to_lowercase())));
    };
    let field = field.to_lowercase();
    if value.is_empty() {
        return Ok(None); // still typing
    }
    let values: Vec<String> = value.split(',').filter(|s| !s.is_empty()).map(|s| s.to_lowercase()).collect();
    let colon_or_eq = matches!(cmp, Cmp::Colon | Cmp::Eq);
    let need_colon = |pred: Pred| {
        if colon_or_eq {
            Ok(Some(pred))
        } else {
            Err(format!("'{field}' only supports ':'"))
        }
    };
    let text = |f: TextField| need_colon(Pred::TextField { field: f, cmp, values: values.clone() });
    match (target, field.as_str()) {
        (_, "is") => {
            let p = match value.to_lowercase().as_str() {
                "open" => IsPred::Open,
                "done" | "closed" => IsPred::Done,
                "overdue" | "late" => IsPred::Overdue,
                "blocked" => IsPred::Blocked,
                "mine" => IsPred::Mine,
                "unassigned" => IsPred::Unassigned,
                other => return Err(format!("unknown is:{other}")),
            };
            Ok(Some(Pred::Is(p)))
        }
        (_, "has") => {
            let p = match value.to_lowercase().as_str() {
                "notes" => HasPred::Notes,
                "deps" => HasPred::Deps,
                "due" => HasPred::Due,
                "start" => HasPred::Start,
                "tags" | "tag" => HasPred::Tags,
                "budget" => HasPred::Budget,
                other => return Err(format!("unknown has:{other}")),
            };
            Ok(Some(Pred::Has(p)))
        }
        (Target::Activity, "user" | "by") => {
            let (ids, none) = resolve_users(value, r)?;
            Ok(Some(Pred::Ids { field: RefField::Person, ids, none }))
        }
        (Target::Activity, "action") => text(TextField::Action),
        (Target::Activity, "entity") => text(TextField::Entity),
        (Target::Activity, _) => Err(format!("unknown field '{field}'")),
        (Target::Projects, "code") => text(TextField::Code),
        (Target::Projects, "name") => text(TextField::Name),
        (Target::Tasks, "title" | "name") => text(TextField::Title),
        (Target::Tasks, "phase") => text(TextField::Phase),
        (_, "notes") => text(TextField::Notes),
        (_, "status" | "priority" | "health") => {
            let ef = match field.as_str() {
                "status" => EnumField::Status,
                "priority" => EnumField::Priority,
                _ => {
                    if target == Target::Tasks {
                        return Err("tasks have no health; try project health in the Projects view".into());
                    }
                    EnumField::Health
                }
            };
            if !colon_or_eq {
                return Err(format!("'{field}' only supports ':'"));
            }
            let names = enum_names(ef, target);
            let mut set = HashSet::new();
            for v in &values {
                let norm = v.replace(' ', "_");
                let exact: Vec<_> = names.iter().filter(|(n, l)| *n == norm || l.to_lowercase() == *v).collect();
                let hits: Vec<_> = if exact.is_empty() {
                    names.iter().filter(|(n, l)| n.starts_with(&norm) || l.to_lowercase().starts_with(v.as_str())).collect()
                } else {
                    exact
                };
                if hits.is_empty() {
                    return Err(format!("unknown {field} '{v}'"));
                }
                set.extend(hits.iter().map(|(n, _)| *n));
            }
            Ok(Some(Pred::Enum { field: ef, values: set }))
        }
        (_, "owner" | "assignee" | "who") => {
            if !colon_or_eq {
                return Err(format!("'{field}' only supports ':'"));
            }
            let (ids, none) = resolve_users(value, r)?;
            Ok(Some(Pred::Ids { field: RefField::Person, ids, none }))
        }
        (_, "program") => {
            if !colon_or_eq {
                return Err("'program' only supports ':'".into());
            }
            let mut ids = HashSet::new();
            let mut none = false;
            for v in &values {
                if v == "none" {
                    none = true;
                    continue;
                }
                let hit: Vec<i64> = r
                    .programs
                    .values()
                    .filter(|p| {
                        p.prefix.to_lowercase() == *v
                            || p.name.to_lowercase().contains(v.as_str())
                            || p.client.to_lowercase().contains(v.as_str())
                    })
                    .map(|p| p.id)
                    .collect();
                if hit.is_empty() {
                    return Err(format!("no program matches '{v}'"));
                }
                ids.extend(hit);
            }
            Ok(Some(Pred::Ids { field: RefField::Program, ids, none }))
        }
        (Target::Tasks, "project") => {
            if !colon_or_eq {
                return Err("'project' only supports ':'".into());
            }
            let mut ids = HashSet::new();
            for v in &values {
                let exact: Vec<i64> = r.projects.values().filter(|p| p.code.to_lowercase() == *v).map(|p| p.id).collect();
                let hit = if exact.is_empty() {
                    r.projects
                        .values()
                        .filter(|p| p.code.to_lowercase().starts_with(v.as_str()) || p.name.to_lowercase().contains(v.as_str()))
                        .map(|p| p.id)
                        .collect()
                } else {
                    exact
                };
                if hit.is_empty() {
                    return Err(format!("no project matches '{v}'"));
                }
                ids.extend(hit);
            }
            Ok(Some(Pred::Ids { field: RefField::Project, ids, none: false }))
        }
        (_, "tag" | "tags") => need_colon(Pred::Tag(values)),
        (_, "start" | "due") => {
            let df = if field == "start" { DateField::Start } else { DateField::Due };
            let v = value.to_lowercase();
            let date = if v == "none" {
                if !colon_or_eq {
                    return Err("'none' only works with ':'".into());
                }
                None
            } else {
                Some(parse_natural(&v, r.today)?.ok_or("missing date")?)
            };
            Ok(Some(Pred::Date { field: df, cmp, value: date }))
        }
        (t, "budget" | "est" | "actual" | "var" | "pct") => {
            let nf = match field.as_str() {
                "budget" if t == Target::Projects => NumField::Budget,
                "est" => NumField::Est,
                "actual" => NumField::Actual,
                "var" if t == Target::Tasks => NumField::Var,
                "pct" if t == Target::Projects => NumField::Pct,
                _ => return Err(format!("unknown field '{field}'")),
            };
            let n = if value.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(value.trim_end_matches(['h', '%']).parse::<f64>().map_err(|_| format!("'{value}' is not a number"))?)
            };
            Ok(Some(Pred::Num { field: nf, cmp, value: n }))
        }
        (t, name) => {
            let ent = if t == Target::Projects { CfEntity::Project } else { CfEntity::Task };
            let Some(cf) = r.custom_fields_for(ent).into_iter().find(|c| c.query_name() == name) else {
                return Err(format!("unknown field '{name}'"));
            };
            let mut pred = Pred::Cf { id: cf.id, kind: cf.kind, cmp, values: values.clone(), num: None, date: None };
            if let Pred::Cf { num, date, .. } = &mut pred {
                match cf.kind {
                    CfKind::Number => {
                        *num = Some(value.parse::<f64>().map_err(|_| format!("'{value}' is not a number"))?)
                    }
                    CfKind::Date => *date = Some(parse_natural(value, r.today)?.ok_or("missing date")?),
                    _ if !colon_or_eq => return Err(format!("'{name}' only supports ':'")),
                    _ => {}
                }
            }
            Ok(Some(pred))
        }
    }
}

/// `me`, `none`, usernames, or a prefix of a display-name word; comma = OR.
fn resolve_users(value: &str, r: &Replica) -> Result<(HashSet<i64>, bool), String> {
    let mut ids = HashSet::new();
    let mut none = false;
    for v in value.split(',').filter(|s| !s.is_empty()) {
        let v = v.to_lowercase();
        match v.as_str() {
            "me" => {
                ids.insert(r.me_id());
            }
            "none" | "nobody" => none = true,
            _ => {
                let exact: Vec<i64> = r.users.values().filter(|u| u.username == v).map(|u| u.id).collect();
                let hit: Vec<i64> = if exact.is_empty() {
                    r.users
                        .values()
                        .filter(|u| {
                            u.username.starts_with(&v)
                                || u.display_name.to_lowercase().split_whitespace().any(|w| w.starts_with(&v))
                        })
                        .map(|u| u.id)
                        .collect()
                } else {
                    exact
                };
                if hit.is_empty() {
                    return Err(format!("no user matches '{v}'"));
                }
                ids.extend(hit);
            }
        }
    }
    Ok((ids, none))
}

fn cmp_num(a: f64, cmp: Cmp, b: f64) -> bool {
    match cmp {
        Cmp::Colon | Cmp::Eq => (a - b).abs() < 1e-9,
        Cmp::Gt => a > b,
        Cmp::Ge => a >= b,
        Cmp::Lt => a < b,
        Cmp::Le => a <= b,
    }
}

fn cmp_date(a: Option<Date>, cmp: Cmp, b: Option<Date>) -> bool {
    match (a, b) {
        (None, None) => matches!(cmp, Cmp::Colon | Cmp::Eq),
        (Some(a), Some(b)) => match cmp {
            Cmp::Colon | Cmp::Eq => a == b,
            Cmp::Gt => a > b,
            Cmp::Ge => a >= b,
            Cmp::Lt => a < b,
            Cmp::Le => a <= b,
        },
        _ => false,
    }
}

fn text_match(hay: &str, cmp: Cmp, values: &[String]) -> bool {
    let h = hay.to_lowercase();
    values.iter().any(|v| if cmp == Cmp::Eq { h == *v } else { h.contains(v.as_str()) })
}

fn parse_date(s: &Option<String>) -> Option<Date> {
    s.as_deref().and_then(Date::parse_iso)
}

fn cf_match(cf: &CfValues, id: i64, kind: CfKind, cmp: Cmp, values: &[String], num: Option<f64>, date: Option<Date>) -> bool {
    let v = cf.get(&id.to_string());
    match kind {
        CfKind::Number => match (v.and_then(|v| v.as_f64()), num) {
            (Some(a), Some(b)) => cmp_num(a, cmp, b),
            _ => false,
        },
        CfKind::Date => cmp_date(v.and_then(|v| v.as_str()).and_then(Date::parse_iso), cmp, date),
        CfKind::Enum => v
            .and_then(|v| v.as_str())
            .is_some_and(|s| values.iter().any(|x| s.to_lowercase() == *x)),
        CfKind::Text => v.and_then(|v| v.as_str()).is_some_and(|s| text_match(s, cmp, values)),
    }
}

fn eval_project(pred: &Pred, p: &Project, r: &Replica) -> bool {
    match pred {
        Pred::Text(t) => {
            p.code.to_lowercase().contains(t)
                || p.name.to_lowercase().contains(t)
                || p.notes.to_lowercase().contains(t)
                || p.tags.iter().any(|g| g.contains(t))
                || r.program_name(p.program_id).to_lowercase().contains(t)
        }
        Pred::TextField { field, cmp, values } => match field {
            TextField::Code => text_match(&p.code, *cmp, values),
            TextField::Name | TextField::Title => text_match(&p.name, *cmp, values),
            TextField::Notes => text_match(&p.notes, *cmp, values),
            _ => false,
        },
        Pred::Enum { field, values } => match field {
            EnumField::Status => values.contains(p.status.as_str()),
            EnumField::Priority => values.contains(p.priority.as_str()),
            EnumField::Health => values.contains(p.health.as_str()),
        },
        Pred::Ids { field, ids, none } => {
            let v = match field {
                RefField::Person => p.owner_id,
                RefField::Program => p.program_id,
                RefField::Project => Some(p.id),
            };
            match v {
                Some(id) => ids.contains(&id),
                None => *none,
            }
        }
        Pred::Num { field, cmp, value } => {
            let m = r.metrics(p.id);
            let a = match field {
                NumField::Budget => p.budget_hours,
                NumField::Est => Some(m.est_total),
                NumField::Actual => Some(m.actual_total),
                NumField::Pct => Some(m.pct_done()),
                NumField::Var => Some(m.actual_total - m.est_total),
            };
            match (a, value) {
                (Some(a), Some(b)) => cmp_num(a, *cmp, *b),
                (None, None) => true,
                _ => false,
            }
        }
        Pred::Date { field, cmp, value } => {
            let d = match field {
                DateField::Start => parse_date(&p.start_date),
                DateField::Due => parse_date(&p.due_date),
            };
            cmp_date(d, *cmp, *value)
        }
        Pred::Tag(tags) => tags.iter().any(|t| p.tags.contains(t)),
        Pred::Cf { id, kind, cmp, values, num, date } => cf_match(&p.cf, *id, *kind, *cmp, values, *num, *date),
        Pred::Is(i) => match i {
            IsPred::Open => r.project_open(p),
            IsPred::Done => p.status == ProjectStatus::Done,
            IsPred::Overdue => r.project_overdue(p),
            IsPred::Blocked => r.metrics(p.id).tasks_blocked > 0,
            IsPred::Mine => p.owner_id == Some(r.me_id()),
            IsPred::Unassigned => p.owner_id.is_none(),
        },
        Pred::Has(h) => match h {
            HasPred::Notes => !p.notes.trim().is_empty(),
            HasPred::Due => p.due_date.is_some(),
            HasPred::Start => p.start_date.is_some(),
            HasPred::Tags => !p.tags.is_empty(),
            HasPred::Budget => p.budget_hours.is_some(),
            HasPred::Deps => false,
        },
    }
}

fn eval_task(pred: &Pred, t: &Task, r: &Replica) -> bool {
    let project = r.projects.get(&t.project_id);
    match pred {
        Pred::Text(s) => {
            t.title.to_lowercase().contains(s)
                || t.phase.to_lowercase().contains(s)
                || t.notes.to_lowercase().contains(s)
                || t.tags.iter().any(|g| g.contains(s))
                || project.is_some_and(|p| p.code.to_lowercase().contains(s))
        }
        Pred::TextField { field, cmp, values } => match field {
            TextField::Title | TextField::Name => text_match(&t.title, *cmp, values),
            TextField::Phase => text_match(&t.phase, *cmp, values),
            TextField::Notes => text_match(&t.notes, *cmp, values),
            TextField::Code => project.is_some_and(|p| text_match(&p.code, *cmp, values)),
            _ => false,
        },
        Pred::Enum { field, values } => match field {
            EnumField::Status => values.contains(t.status.as_str()),
            EnumField::Priority => values.contains(t.priority.as_str()),
            EnumField::Health => project.is_some_and(|p| values.contains(p.health.as_str())),
        },
        Pred::Ids { field, ids, none } => {
            let v = match field {
                RefField::Person => t.assignee_id,
                RefField::Program => project.and_then(|p| p.program_id),
                RefField::Project => Some(t.project_id),
            };
            match v {
                Some(id) => ids.contains(&id),
                None => *none,
            }
        }
        Pred::Num { field, cmp, value } => {
            let a = match field {
                NumField::Est => t.est_hours,
                NumField::Actual => Some(t.actual_hours),
                NumField::Var => t.est_hours.map(|e| t.actual_hours - e),
                NumField::Budget | NumField::Pct => None,
            };
            match (a, value) {
                (Some(a), Some(b)) => cmp_num(a, *cmp, *b),
                (None, None) => true,
                _ => false,
            }
        }
        Pred::Date { field, cmp, value } => {
            let d = match field {
                DateField::Start => parse_date(&t.start_date),
                DateField::Due => parse_date(&t.due_date),
            };
            cmp_date(d, *cmp, *value)
        }
        Pred::Tag(tags) => tags.iter().any(|g| t.tags.contains(g)),
        Pred::Cf { id, kind, cmp, values, num, date } => cf_match(&t.cf, *id, *kind, *cmp, values, *num, *date),
        Pred::Is(i) => match i {
            IsPred::Open => t.status != TaskStatus::Done,
            IsPred::Done => t.status == TaskStatus::Done,
            IsPred::Overdue => r.task_overdue(t),
            IsPred::Blocked => r.task_blocked(t),
            IsPred::Mine => t.assignee_id == Some(r.me_id()),
            IsPred::Unassigned => t.assignee_id.is_none(),
        },
        Pred::Has(h) => match h {
            HasPred::Notes => !t.notes.trim().is_empty(),
            HasPred::Deps => !t.deps.is_empty(),
            HasPred::Due => t.due_date.is_some(),
            HasPred::Start => t.start_date.is_some(),
            HasPred::Tags => !t.tags.is_empty(),
            HasPred::Budget => false,
        },
    }
}

fn eval_activity(pred: &Pred, a: &Activity, r: &Replica) -> bool {
    match pred {
        Pred::Text(s) => a.label.to_lowercase().contains(s) || r.username(a.user_id).contains(s.as_str()),
        Pred::TextField { field: TextField::Action, cmp, values } => text_match(a.action.as_str(), *cmp, values),
        Pred::TextField { field: TextField::Entity, cmp, values } => text_match(&a.entity, *cmp, values),
        Pred::Ids { ids, none, .. } => a.user_id.map_or(*none, |u| ids.contains(&u)),
        Pred::Is(IsPred::Mine) => a.user_id == Some(r.me_id()),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replica::tests::sample;

    fn tasks(q: &str) -> Vec<i64> {
        let r = sample();
        let q = Query::compile(q, Target::Tasks, &r);
        assert!(q.errors.is_empty(), "{:?}", q.errors);
        r.tasks.values().filter(|t| q.matches_task(t, &r)).map(|t| t.id).collect()
    }

    #[test]
    fn basics() {
        assert_eq!(tasks(""), vec![100, 101, 102]);
        assert_eq!(tasks("load"), vec![101]);
        assert_eq!(tasks("status:done"), vec![100]);
        assert_eq!(tasks("status:in"), vec![101]);
        assert_eq!(tasks("status:todo,done"), vec![100, 102]);
        assert_eq!(tasks("-status:done"), vec![101, 102]);
        assert_eq!(tasks("@me"), vec![100]);
        assert_eq!(tasks("@bob"), vec![101]);
        assert_eq!(tasks("assignee:none"), vec![102]);
        assert_eq!(tasks("is:overdue"), vec![101]);
        assert_eq!(tasks("is:open due<+30d"), vec![101]);
        assert_eq!(tasks("est>=10"), vec![101]);
        assert_eq!(tasks("est:none"), vec![102]);
        assert_eq!(tasks("project:nwr"), vec![100, 101, 102]);
        assert_eq!(tasks("\"load test\""), vec![101]);
        assert_eq!(tasks("due:none"), Vec::<i64>::new());
    }

    #[test]
    fn errors_have_spans() {
        let r = sample();
        let q = Query::compile("status:zzz frob:1 ok", Target::Tasks, &r);
        assert_eq!(q.errors.len(), 2);
        assert_eq!((q.errors[0].start, q.errors[0].end), (0, 10));
        assert_eq!((q.errors[1].start, q.errors[1].end), (11, 17));
        // the valid term still applies
        assert_eq!(r.tasks.values().filter(|t| q.matches_task(t, &r)).count(), 0);
    }

    #[test]
    fn projects() {
        let r = sample();
        let q = Query::compile("program:northwind health:amber pct>30 #retail", Target::Projects, &r);
        assert!(q.errors.is_empty(), "{:?}", q.errors);
        assert!(q.matches_project(&r.projects[&10], &r));
        let q = Query::compile("pct>50", Target::Projects, &r);
        assert!(!q.matches_project(&r.projects[&10], &r));
    }
}
