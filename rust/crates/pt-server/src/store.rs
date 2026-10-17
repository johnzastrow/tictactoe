//! SQLite store: loading rows in wire format and applying mutations with
//! per-field optimistic concurrency, activity logging and undo/redo
//! (protocol §5).
//!
//! Only the hub calls into this, from one thread, so there is exactly one
//! writer. Every SQL identifier comes from static tables (`Entity::table`,
//! `fields::FieldMeta::name`); every value is a bound parameter.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use pt_core::date::{Date, rfc3339};
use pt_core::fields::{self, FieldMeta, Kind, Perm, values_equal};
use pt_core::grid::fmt_hours;
use pt_core::model::*;
use pt_core::protocol::{Changes, Conflict, ErrorBody, Op, ResultMsg};
use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value, json};

pub const SCHEMA: &str = include_str!("../../../../docs/pt/schema.sql");
const ACTIVITY_IN_SNAPSHOT: i64 = 500;

pub struct Store {
    conn: Connection,
}

/// Result of a mutate / undo / redo.
#[derive(Debug, Default)]
pub struct Outcome {
    pub result: ResultMsg,
    pub changes: Option<Changes>,
    /// Users whose sessions must end (deactivated / password changed), with a session to keep.
    pub kick: Vec<(i64, Option<String>)>,
}

impl Outcome {
    fn error(e: ErrorBody) -> Outcome {
        Outcome {
            result: ResultMsg {
                ok: false,
                error: Some(e),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

#[derive(Debug)]
enum OpErr {
    Invalid(String),
    Forbidden(String),
    NotFound(String),
    Internal(String),
}

impl From<rusqlite::Error> for OpErr {
    fn from(e: rusqlite::Error) -> Self {
        OpErr::Internal(e.to_string())
    }
}

impl OpErr {
    fn body(self) -> ErrorBody {
        match self {
            OpErr::Invalid(m) => ErrorBody::new("invalid", m),
            OpErr::Forbidden(m) => ErrorBody::new("forbidden", m),
            OpErr::NotFound(m) => ErrorBody::new("not_found", m),
            OpErr::Internal(m) => {
                tracing::error!("store: {m}");
                ErrorBody::new("internal", "internal error")
            }
        }
    }
}

type R<T> = Result<T, OpErr>;
/// (activity id, action, entity, entity id, field, old JSON, new JSON)
type EntryRow = (i64, String, String, i64, String, Option<String>, Option<String>);
/// (group, changes, sessions to end)
type Finished = (i64, Changes, Vec<(i64, Option<String>)>);

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_ts() -> String {
    rfc3339(now_secs())
}

pub fn today() -> Date {
    pt_core::date::civil_from_unix(now_secs()).0
}

// ---- value conversion -------------------------------------------------------

fn to_sql(v: &Value) -> Sql {
    match v {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Sql::Integer(i),
            None => Sql::Real(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => Sql::Text(s.clone()),
        other => Sql::Text(other.to_string()),
    }
}

fn cf_from_text(kind: &str, s: String) -> Value {
    if kind == "number" {
        s.parse::<f64>().map_or(Value::Null, Value::from)
    } else {
        Value::String(s)
    }
}

fn cf_to_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

// ---- row loading --------------------------------------------------------------

#[derive(Clone, Copy)]
enum Filter {
    All,
    Id(i64),
    /// tasks of a project
    Project(i64),
}

impl Filter {
    fn id(self) -> Option<i64> {
        match self {
            Filter::Id(i) => Some(i),
            _ => None,
        }
    }
    fn project(self) -> Option<i64> {
        match self {
            Filter::Project(p) => Some(p),
            _ => None,
        }
    }
}

fn load_users(c: &Connection, f: Filter) -> rusqlite::Result<Vec<User>> {
    let mut st = c.prepare_cached(
        "SELECT id, username, display_name, role, active FROM users WHERE (?1 IS NULL OR id = ?1) ORDER BY id",
    )?;
    st.query_map(params![f.id()], |r| {
        Ok(User {
            id: r.get(0)?,
            username: r.get(1)?,
            display_name: r.get(2)?,
            role: Role::parse(&r.get::<_, String>(3)?).unwrap_or(Role::Viewer),
            active: r.get::<_, i64>(4)? != 0,
        })
    })?
    .collect()
}

fn load_programs(c: &Connection, f: Filter) -> rusqlite::Result<Vec<Program>> {
    let mut st = c.prepare_cached(
        "SELECT id, name, client, prefix, color FROM programs WHERE (?1 IS NULL OR id = ?1) ORDER BY name",
    )?;
    st.query_map(params![f.id()], |r| {
        Ok(Program {
            id: r.get(0)?,
            name: r.get(1)?,
            client: r.get(2)?,
            prefix: r.get(3)?,
            color: r.get(4)?,
        })
    })?
    .collect()
}

fn load_templates(c: &Connection, f: Filter) -> rusqlite::Result<Vec<Template>> {
    let mut st = c.prepare_cached(
        "SELECT id, name, description FROM templates WHERE (?1 IS NULL OR id = ?1) ORDER BY name",
    )?;
    st.query_map(params![f.id()], |r| {
        Ok(Template {
            id: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
        })
    })?
    .collect()
}

fn load_template_tasks(c: &Connection, f: Filter) -> rusqlite::Result<Vec<TemplateTask>> {
    let mut st = c.prepare_cached(
        "SELECT id, template_id, seq, title, phase, est_hours, offset_days, duration_days FROM template_tasks
         WHERE (?1 IS NULL OR id = ?1) AND (?2 IS NULL OR template_id = ?2) ORDER BY template_id, seq, id",
    )?;
    st.query_map(params![f.id(), f.project()], |r| {
        Ok(TemplateTask {
            id: r.get(0)?,
            template_id: r.get(1)?,
            seq: r.get(2)?,
            title: r.get(3)?,
            phase: r.get(4)?,
            est_hours: r.get(5)?,
            offset_days: r.get(6)?,
            duration_days: r.get(7)?,
        })
    })?
    .collect()
}

fn load_custom_fields(c: &Connection, f: Filter) -> rusqlite::Result<Vec<CustomField>> {
    let mut st = c.prepare_cached(
        "SELECT id, entity, name, kind, options, seq FROM custom_fields WHERE (?1 IS NULL OR id = ?1) ORDER BY entity, seq, id",
    )?;
    st.query_map(params![f.id()], |r| {
        let opts: String = r.get(4)?;
        Ok(CustomField {
            id: r.get(0)?,
            entity: CfEntity::parse(&r.get::<_, String>(1)?).unwrap_or(CfEntity::Task),
            name: r.get(2)?,
            kind: CfKind::parse(&r.get::<_, String>(3)?).unwrap_or(CfKind::Text),
            options: serde_json::from_str(&opts).unwrap_or_default(),
            seq: r.get(5)?,
        })
    })?
    .collect()
}

/// Side tables for projects or tasks: tags, cf values and (tasks) deps.
struct Side {
    tags: HashMap<i64, Vec<String>>,
    cf: HashMap<i64, CfValues>,
    deps: HashMap<i64, Vec<i64>>,
}

fn load_side(c: &Connection, entity: CfEntity, f: Filter) -> rusqlite::Result<Side> {
    let mut side = Side {
        tags: HashMap::new(),
        cf: HashMap::new(),
        deps: HashMap::new(),
    };
    let (tag_sql, cf_sql) = match entity {
        CfEntity::Project => (
            "SELECT project_id, tag FROM project_tags WHERE (?1 IS NULL OR project_id = ?1) ORDER BY tag",
            "SELECT v.entity_id, v.field_id, v.value, f.kind FROM custom_values v JOIN custom_fields f ON f.id = v.field_id
             WHERE f.entity = 'project' AND (?1 IS NULL OR v.entity_id = ?1)",
        ),
        CfEntity::Task => (
            "SELECT t.task_id, t.tag FROM task_tags t JOIN tasks k ON k.id = t.task_id
             WHERE (?1 IS NULL OR t.task_id = ?1) AND (?2 IS NULL OR k.project_id = ?2) ORDER BY t.tag",
            "SELECT v.entity_id, v.field_id, v.value, f.kind FROM custom_values v JOIN custom_fields f ON f.id = v.field_id
             JOIN tasks k ON k.id = v.entity_id
             WHERE f.entity = 'task' AND (?1 IS NULL OR v.entity_id = ?1) AND (?2 IS NULL OR k.project_id = ?2)",
        ),
    };
    let p = |st: &mut rusqlite::CachedStatement<'_>| -> rusqlite::Result<Vec<(i64, String)>> {
        let rows = if entity == CfEntity::Task {
            st.query_map(params![f.id(), f.project()], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            st.query_map(params![f.id()], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    };
    for (id, tag) in p(&mut c.prepare_cached(tag_sql)?)? {
        side.tags.entry(id).or_default().push(tag);
    }
    {
        let mut st = c.prepare_cached(cf_sql)?;
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(i64, i64, String, String)> {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        };
        let rows: Vec<(i64, i64, String, String)> = if entity == CfEntity::Task {
            st.query_map(params![f.id(), f.project()], map)?.collect::<rusqlite::Result<_>>()?
        } else {
            st.query_map(params![f.id()], map)?.collect::<rusqlite::Result<_>>()?
        };
        for (id, fid, val, kind) in rows {
            side.cf.entry(id).or_default().insert(fid.to_string(), cf_from_text(&kind, val));
        }
    }
    if entity == CfEntity::Task {
        let mut st = c.prepare_cached(
            "SELECT d.task_id, d.depends_on FROM task_deps d JOIN tasks k ON k.id = d.task_id
             WHERE (?1 IS NULL OR d.task_id = ?1) AND (?2 IS NULL OR k.project_id = ?2) ORDER BY d.depends_on",
        )?;
        let rows: Vec<(i64, i64)> = st
            .query_map(params![f.id(), f.project()], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (t, d) in rows {
            side.deps.entry(t).or_default().push(d);
        }
    }
    Ok(side)
}

fn load_projects(c: &Connection, f: Filter) -> rusqlite::Result<Vec<Project>> {
    let mut side = load_side(c, CfEntity::Project, f)?;
    let mut st = c.prepare_cached(
        "SELECT id, code, name, program_id, template_id, owner_id, status, priority, health, start_date, due_date,
                budget_hours, notes, created_at, updated_at, updated_by
         FROM projects WHERE (?1 IS NULL OR id = ?1) ORDER BY code",
    )?;
    st.query_map(params![f.id()], |r| {
        let id: i64 = r.get(0)?;
        Ok(Project {
            id,
            code: r.get(1)?,
            name: r.get(2)?,
            program_id: r.get(3)?,
            template_id: r.get(4)?,
            owner_id: r.get(5)?,
            status: ProjectStatus::parse(&r.get::<_, String>(6)?).unwrap_or(ProjectStatus::Planning),
            priority: Priority::parse(&r.get::<_, String>(7)?).unwrap_or(Priority::Medium),
            health: Health::parse(&r.get::<_, String>(8)?).unwrap_or(Health::Green),
            start_date: r.get(9)?,
            due_date: r.get(10)?,
            budget_hours: r.get(11)?,
            notes: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
            updated_by: r.get(15)?,
            tags: side.tags.remove(&id).unwrap_or_default(),
            cf: side.cf.remove(&id).unwrap_or_default(),
        })
    })?
    .collect()
}

fn load_tasks(c: &Connection, f: Filter) -> rusqlite::Result<Vec<Task>> {
    let mut side = load_side(c, CfEntity::Task, f)?;
    let mut st = c.prepare_cached(
        "SELECT id, project_id, seq, title, phase, status, priority, assignee_id, start_date, due_date,
                est_hours, actual_hours, notes, created_at, updated_at, updated_by
         FROM tasks WHERE (?1 IS NULL OR id = ?1) AND (?2 IS NULL OR project_id = ?2) ORDER BY project_id, seq",
    )?;
    st.query_map(params![f.id(), f.project()], |r| {
        let id: i64 = r.get(0)?;
        Ok(Task {
            id,
            project_id: r.get(1)?,
            seq: r.get(2)?,
            title: r.get(3)?,
            phase: r.get(4)?,
            status: TaskStatus::parse(&r.get::<_, String>(5)?).unwrap_or(TaskStatus::Todo),
            priority: Priority::parse(&r.get::<_, String>(6)?).unwrap_or(Priority::Medium),
            assignee_id: r.get(7)?,
            start_date: r.get(8)?,
            due_date: r.get(9)?,
            est_hours: r.get(10)?,
            actual_hours: r.get(11)?,
            notes: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
            updated_by: r.get(15)?,
            deps: side.deps.remove(&id).unwrap_or_default(),
            tags: side.tags.remove(&id).unwrap_or_default(),
            cf: side.cf.remove(&id).unwrap_or_default(),
        })
    })?
    .collect()
}

fn activity_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Activity> {
    let old: Option<String> = r.get(8)?;
    let new: Option<String> = r.get(9)?;
    let action: String = r.get(4)?;
    let state: String = r.get(11)?;
    let parse = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    // bundles are internal: clients only need set values
    let (old, new) = if action == "set" { (parse(old), parse(new)) } else { (Value::Null, Value::Null) };
    Ok(Activity {
        id: r.get(0)?,
        ts: r.get(1)?,
        user_id: r.get(2)?,
        group: r.get(3)?,
        action: Action::parse(&action).unwrap_or(Action::Set),
        entity: r.get(5)?,
        entity_id: r.get(6)?,
        field: r.get(7)?,
        old,
        new,
        label: r.get(10)?,
        state: ActState::parse(&state).unwrap_or(ActState::Dead),
    })
}

const ACTIVITY_COLS: &str = "id, ts, user_id, grp, action, entity, entity_id, field, old, new, label, state";

fn load_activity_ids(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<Activity>> {
    let mut st = c.prepare_cached(&format!("SELECT {ACTIVITY_COLS} FROM activity WHERE id = ?1"))?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(a) = st.query_row([id], activity_from_row).optional()? {
            out.push(a);
        }
    }
    Ok(out)
}

fn load_rows(c: &Connection, e: Entity, f: Filter, t: &mut Tables) -> rusqlite::Result<()> {
    match e {
        Entity::User => t.users.extend(load_users(c, f)?),
        Entity::Program => t.programs.extend(load_programs(c, f)?),
        Entity::Template => t.templates.extend(load_templates(c, f)?),
        Entity::TemplateTask => t.template_tasks.extend(load_template_tasks(c, f)?),
        Entity::Project => t.projects.extend(load_projects(c, f)?),
        Entity::Task => t.tasks.extend(load_tasks(c, f)?),
        Entity::CustomField => t.custom_fields.extend(load_custom_fields(c, f)?),
    }
    Ok(())
}

fn row_exists(c: &Connection, e: Entity, id: i64) -> rusqlite::Result<bool> {
    c.query_row(&format!("SELECT 1 FROM {} WHERE id = ?1", e.table()), [id], |_| Ok(()))
        .optional()
        .map(|o| o.is_some())
}

// ---- inserts (create, restore, seed) -------------------------------------------

fn nz(id: i64) -> Option<i64> {
    (id > 0).then_some(id)
}

pub(crate) fn insert_user(c: &Connection, u: &User, hash: &str, now: &str) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO users (id, username, display_name, role, password_hash, active, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![nz(u.id), u.username, u.display_name, u.role.as_str(), hash, u.active as i64, now],
    )?;
    Ok(c.last_insert_rowid())
}

pub(crate) fn insert_program(c: &Connection, p: &Program) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO programs (id, name, client, prefix, color) VALUES (?1,?2,?3,?4,?5)",
        params![nz(p.id), p.name, p.client, p.prefix, p.color],
    )?;
    Ok(c.last_insert_rowid())
}

pub(crate) fn insert_template(c: &Connection, t: &Template) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO templates (id, name, description) VALUES (?1,?2,?3)",
        params![nz(t.id), t.name, t.description],
    )?;
    Ok(c.last_insert_rowid())
}

pub(crate) fn insert_template_task(c: &Connection, t: &TemplateTask) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO template_tasks (id, template_id, seq, title, phase, est_hours, offset_days, duration_days)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![nz(t.id), t.template_id, t.seq, t.title, t.phase, t.est_hours, t.offset_days, t.duration_days],
    )?;
    Ok(c.last_insert_rowid())
}

pub(crate) fn insert_custom_field(c: &Connection, f: &CustomField) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO custom_fields (id, entity, name, kind, options, seq) VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            nz(f.id),
            f.entity.as_str(),
            f.name,
            f.kind.as_str(),
            serde_json::to_string(&f.options).unwrap_or_else(|_| "[]".into()),
            f.seq
        ],
    )?;
    Ok(c.last_insert_rowid())
}

fn write_cf(c: &Connection, entity_id: i64, cf: &CfValues) -> rusqlite::Result<()> {
    for (k, v) in cf {
        let Ok(fid) = k.parse::<i64>() else { continue };
        if let Some(text) = cf_to_text(v) {
            // skip values for fields that no longer exist
            c.execute(
                "INSERT OR REPLACE INTO custom_values (field_id, entity_id, value)
                 SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM custom_fields WHERE id = ?1)",
                params![fid, entity_id, text],
            )?;
        }
    }
    Ok(())
}

pub(crate) fn insert_project(c: &Connection, p: &Project) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO projects (id, code, name, program_id, template_id, owner_id, status, priority, health,
                               start_date, due_date, budget_hours, notes, created_at, updated_at, updated_by)
         VALUES (?1,?2,?3,
                 (SELECT id FROM programs WHERE id = ?4), (SELECT id FROM templates WHERE id = ?5),
                 (SELECT id FROM users WHERE id = ?6), ?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            nz(p.id),
            p.code,
            p.name,
            p.program_id,
            p.template_id,
            p.owner_id,
            p.status.as_str(),
            p.priority.as_str(),
            p.health.as_str(),
            p.start_date,
            p.due_date,
            p.budget_hours,
            p.notes,
            p.created_at,
            p.updated_at,
            p.updated_by
        ],
    )?;
    let id = c.last_insert_rowid();
    for t in &p.tags {
        c.execute("INSERT OR IGNORE INTO project_tags (project_id, tag) VALUES (?1, ?2)", params![id, t])?;
    }
    write_cf(c, id, &p.cf)?;
    Ok(id)
}

/// Inserts a task without its deps (callers add edges once all tasks exist).
pub(crate) fn insert_task(c: &Connection, t: &Task) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO tasks (id, project_id, seq, title, phase, status, priority, assignee_id, start_date, due_date,
                            est_hours, actual_hours, notes, created_at, updated_at, updated_by)
         VALUES (?1,?2,?3,?4,?5,?6,?7,(SELECT id FROM users WHERE id = ?8),?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            nz(t.id),
            t.project_id,
            t.seq,
            t.title,
            t.phase,
            t.status.as_str(),
            t.priority.as_str(),
            t.assignee_id,
            t.start_date,
            t.due_date,
            t.est_hours,
            t.actual_hours,
            t.notes,
            t.created_at,
            t.updated_at,
            t.updated_by
        ],
    )?;
    let id = c.last_insert_rowid();
    for tag in &t.tags {
        c.execute("INSERT OR IGNORE INTO task_tags (task_id, tag) VALUES (?1, ?2)", params![id, tag])?;
    }
    write_cf(c, id, &t.cf)?;
    Ok(id)
}

/// Adds a dependency edge if both tasks still exist and share a project.
pub(crate) fn insert_dep(c: &Connection, task: i64, dep: i64) -> rusqlite::Result<()> {
    c.execute(
        "INSERT OR IGNORE INTO task_deps (task_id, depends_on)
         SELECT a.id, b.id FROM tasks a JOIN tasks b ON a.project_id = b.project_id
         WHERE a.id = ?1 AND b.id = ?2 AND a.id <> b.id",
        params![task, dep],
    )?;
    Ok(())
}

// ---- the store ------------------------------------------------------------------

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Store> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_memory() -> anyhow::Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Store> {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000; PRAGMA synchronous = NORMAL;",
        )?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    pub fn conn(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn is_empty(&self) -> anyhow::Result<bool> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(n == 0)
    }

    pub fn snapshot(&self) -> anyhow::Result<Tables> {
        let c = &self.conn;
        let mut t = Tables::default();
        for e in Entity::ALL {
            load_rows(c, e, Filter::All, &mut t)?;
        }
        let mut st = c.prepare_cached(&format!(
            "SELECT {ACTIVITY_COLS} FROM (SELECT * FROM activity ORDER BY id DESC LIMIT ?1) ORDER BY id"
        ))?;
        t.activity = st
            .query_map([ACTIVITY_IN_SNAPSHOT], activity_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(t)
    }

    // ---- auth ----

    /// (user, password hash) for a login attempt.
    pub fn user_for_login(&self, username: &str) -> anyhow::Result<Option<(User, String)>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, password_hash FROM users WHERE username = ?1 COLLATE NOCASE",
                [username],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((id, hash)) = row else { return Ok(None) };
        Ok(load_users(&self.conn, Filter::Id(id))?.pop().map(|u| (u, hash)))
    }

    pub fn user(&self, id: i64) -> anyhow::Result<Option<User>> {
        Ok(load_users(&self.conn, Filter::Id(id))?.pop())
    }

    pub fn user_by_name(&self, username: &str) -> anyhow::Result<Option<User>> {
        Ok(self.user_for_login(username)?.map(|(u, _)| u))
    }

    pub fn password_hash(&self, id: i64) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT password_hash FROM users WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    pub fn set_password_hash(&self, id: i64, hash: &str) -> anyhow::Result<()> {
        self.conn
            .execute("UPDATE users SET password_hash = ?1 WHERE id = ?2", params![hash, id])?;
        Ok(())
    }

    pub fn list_users(&self) -> anyhow::Result<Vec<User>> {
        Ok(load_users(&self.conn, Filter::All)?)
    }

    pub fn add_user(&self, username: &str, display: &str, role: Role, hash: &str) -> anyhow::Result<i64> {
        let u = User {
            id: 0,
            username: username.to_string(),
            display_name: display.to_string(),
            role,
            active: true,
        };
        Ok(insert_user(&self.conn, &u, hash, &now_ts())?)
    }

    pub fn create_session(&self, token_hash: &str, user_id: i64, client: &str) -> anyhow::Result<()> {
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at, client) VALUES (?1,?2,?3,?4,?5)",
            params![token_hash, user_id, rfc3339(now), rfc3339(now + crate::auth::SESSION_TTL_SECS), client],
        )?;
        // opportunistic cleanup
        self.conn
            .execute("DELETE FROM sessions WHERE expires_at < ?1", [rfc3339(now)])?;
        Ok(())
    }

    /// The active user owning an unexpired session.
    pub fn session_user(&self, token_hash: &str) -> anyhow::Result<Option<User>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT s.user_id FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND u.active = 1",
                params![token_hash, now_ts()],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(id) => self.user(id),
            None => Ok(None),
        }
    }

    pub fn delete_session(&self, token_hash: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
        Ok(())
    }

    pub fn delete_user_sessions(&self, user_id: i64, keep: Option<&str>) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND (?2 IS NULL OR token_hash <> ?2)",
            params![user_id, keep],
        )?;
        Ok(())
    }

    /// The user who last set a field (for conflict replies).
    fn last_setter(c: &Connection, e: Entity, id: i64, field: &str) -> Option<i64> {
        c.query_row(
            "SELECT user_id FROM activity WHERE entity = ?1 AND entity_id = ?2 AND field = ?3 AND action = 'set'
             AND state <> 'undone' ORDER BY id DESC LIMIT 1",
            params![e.as_str(), id, field],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    // ---- writes ----

    pub fn mutate(&mut self, actor: &User, ops: &[Op]) -> Outcome {
        if actor.role == Role::Viewer {
            return Outcome::error(ErrorBody::new("forbidden", "viewers cannot make changes"));
        }
        let now = now_ts();
        let tx = match self.conn.transaction() {
            Ok(t) => t,
            Err(e) => return Outcome::error(OpErr::from(e).body()),
        };
        let mut cx = Cx::new(&tx, actor, &now);
        for (i, op) in ops.iter().enumerate() {
            if let Err(e) = cx.apply(i, op) {
                return Outcome::error(e.body().at(i));
            }
        }
        if let Err(e) = cx.validate() {
            return Outcome::error(e.body());
        }
        let mut result = ResultMsg {
            ok: true,
            conflicts: std::mem::take(&mut cx.conflicts),
            created: std::mem::take(&mut cx.created),
            ..Default::default()
        };
        if cx.entries.is_empty() {
            return Outcome {
                result,
                ..Default::default()
            };
        }
        match cx.finish(true) {
            Ok((group, changes, kick)) => {
                if let Err(e) = tx.commit() {
                    return Outcome::error(OpErr::from(e).body());
                }
                result.group = Some(group);
                Outcome {
                    result,
                    changes: Some(changes),
                    kick,
                }
            }
            Err(e) => Outcome::error(e.body()),
        }
    }

    pub fn undo(&mut self, actor: &User) -> Outcome {
        self.undo_redo(actor, true)
    }

    pub fn redo(&mut self, actor: &User) -> Outcome {
        self.undo_redo(actor, false)
    }

    fn undo_redo(&mut self, actor: &User, undo: bool) -> Outcome {
        if actor.role == Role::Viewer {
            return Outcome::error(ErrorBody::new("forbidden", "viewers cannot make changes"));
        }
        let now = now_ts();
        let tx = match self.conn.transaction() {
            Ok(t) => t,
            Err(e) => return Outcome::error(OpErr::from(e).body()),
        };
        let want = if undo { "done" } else { "undone" };
        let grp: Option<i64> = tx
            .query_row(
                "SELECT MAX(grp) FROM activity WHERE user_id = ?1 AND state = ?2",
                params![actor.id, want],
                |r| r.get(0),
            )
            .unwrap_or(None);
        let Some(grp) = grp else {
            let code = if undo { "nothing_to_undo" } else { "nothing_to_redo" };
            return Outcome::error(ErrorBody::new(code, if undo { "nothing to undo" } else { "nothing to redo" }));
        };
        let entries: Vec<EntryRow> = {
            let order = if undo { "DESC" } else { "ASC" };
            let mut st = match tx.prepare(&format!(
                "SELECT id, action, entity, entity_id, field, old, new FROM activity WHERE grp = ?1 ORDER BY id {order}"
            )) {
                Ok(s) => s,
                Err(e) => return Outcome::error(OpErr::from(e).body()),
            };
            let rows = st.query_map([grp], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
            });
            match rows.and_then(|r| r.collect()) {
                Ok(v) => v,
                Err(e) => return Outcome::error(OpErr::from(e).body()),
            }
        };
        let mut cx = Cx::new(&tx, actor, &now);
        let parse = |s: &Option<String>| s.as_deref().and_then(|s| serde_json::from_str::<Value>(s).ok()).unwrap_or(Value::Null);
        for (idx, (_, action, entity, entity_id, field, old, new)) in entries.iter().enumerate() {
            let Some(e) = Entity::parse(entity) else { continue };
            let (old, new) = (parse(old), parse(new));
            let res = match (action.as_str(), undo) {
                ("set", true) => cx.revert_set(idx, e, *entity_id, field, &new, &old),
                ("set", false) => cx.revert_set(idx, e, *entity_id, field, &old, &new),
                ("create", true) | ("delete", false) => cx.delete_row(e, *entity_id, false).map(|_| ()),
                ("create", false) => cx.restore(e, &new),
                ("delete", true) => cx.restore(e, &old),
                _ => Ok(()),
            };
            if let Err(err) = res {
                return Outcome::error(err.body());
            }
        }
        if !cx.conflicts.is_empty() {
            let conflicts = std::mem::take(&mut cx.conflicts);
            return Outcome {
                result: ResultMsg {
                    ok: false,
                    conflicts,
                    error: Some(ErrorBody::new(
                        "conflict",
                        "someone else changed this since; nothing was changed",
                    )),
                    ..Default::default()
                },
                ..Default::default()
            };
        }
        if let Err(e) = cx.validate() {
            return Outcome::error(e.body());
        }
        let new_state = if undo { "undone" } else { "done" };
        if let Err(e) = tx.execute("UPDATE activity SET state = ?1 WHERE grp = ?2", params![new_state, grp]) {
            return Outcome::error(OpErr::from(e).body());
        }
        let ids: Vec<i64> = entries.iter().map(|e| e.0).collect();
        cx.updated_activity.extend(ids);
        match cx.finish(false) {
            Ok((_, mut changes, kick)) => {
                if let Err(e) = tx.commit() {
                    return Outcome::error(OpErr::from(e).body());
                }
                changes.group = Some(grp);
                Outcome {
                    result: ResultMsg {
                        ok: true,
                        group: Some(grp),
                        ..Default::default()
                    },
                    changes: Some(changes),
                    kick,
                }
            }
            Err(e) => Outcome::error(e.body()),
        }
    }
}

// ---- mutation context ------------------------------------------------------------

struct Entry {
    action: Action,
    entity: Entity,
    id: i64,
    field: String,
    old: Option<Value>,
    new: Option<Value>,
    label: String,
    undoable: bool,
}

struct Cx<'a> {
    c: &'a Connection,
    actor: &'a User,
    now: &'a str,
    touched: BTreeSet<(Entity, i64)>,
    deleted: BTreeSet<(Entity, i64)>,
    entries: Vec<Entry>,
    conflicts: Vec<Conflict>,
    created: BTreeMap<String, i64>,
    updated_activity: Vec<i64>,
    kick: Vec<(i64, Option<String>)>,
}

impl<'a> Cx<'a> {
    fn new(c: &'a Connection, actor: &'a User, now: &'a str) -> Cx<'a> {
        Cx {
            c,
            actor,
            now,
            touched: BTreeSet::new(),
            deleted: BTreeSet::new(),
            entries: Vec::new(),
            conflicts: Vec::new(),
            created: BTreeMap::new(),
            updated_activity: Vec::new(),
            kick: Vec::new(),
        }
    }

    fn check_entity_perm(&self, e: Entity) -> R<()> {
        let admin_only = !matches!(e, Entity::Project | Entity::Task);
        if admin_only && self.actor.role != Role::Admin {
            return Err(OpErr::Forbidden(format!("only admins can change {}s", e.label())));
        }
        Ok(())
    }

    fn check_field_perm(&self, m: &FieldMeta) -> R<()> {
        if m.perm == Perm::Admin && self.actor.role != Role::Admin {
            return Err(OpErr::Forbidden(format!("only admins can change {}", m.label)));
        }
        Ok(())
    }

    fn apply(&mut self, idx: usize, op: &Op) -> R<()> {
        match op {
            Op::Set { entity, id, field, base, value, force } => {
                self.check_entity_perm(*entity)?;
                let current = self.read_field(*entity, *id, field)?.ok_or_else(|| {
                    OpErr::NotFound(format!("that {} no longer exists", entity.label()))
                })?;
                if !*force && !values_equal(&current, base) {
                    self.conflicts.push(Conflict {
                        index: idx,
                        entity: *entity,
                        id: *id,
                        field: field.clone(),
                        current,
                        by: Store::last_setter(self.c, *entity, *id, field),
                    });
                    return Ok(());
                }
                let value = self.normalize_set(*entity, *id, field, value)?;
                if values_equal(&current, &value) {
                    return Ok(());
                }
                self.write_field(*entity, *id, field, &value)?;
                let label = self.set_label(*entity, *id, field, &current, &value);
                self.entries.push(Entry {
                    action: Action::Set,
                    entity: *entity,
                    id: *id,
                    field: field.clone(),
                    old: Some(current),
                    new: Some(value),
                    label,
                    undoable: true,
                });
                Ok(())
            }
            Op::Create { entity, reference, values } => {
                self.check_entity_perm(*entity)?;
                let id = self.create(*entity, values)?;
                if let Some(r) = reference {
                    self.created.insert(r.clone(), id);
                }
                Ok(())
            }
            Op::Delete { entity, id } => {
                self.check_entity_perm(*entity)?;
                if *entity == Entity::User {
                    return Err(OpErr::Forbidden("users can't be deleted; set active to no instead".into()));
                }
                if !row_exists(self.c, *entity, *id)? {
                    return Err(OpErr::NotFound(format!("that {} no longer exists", entity.label())));
                }
                let label = format!("deleted {} {}", entity.label(), self.describe(*entity, *id));
                let bundle = self.delete_row(*entity, *id, true)?;
                self.entries.push(Entry {
                    action: Action::Delete,
                    entity: *entity,
                    id: *id,
                    field: String::new(),
                    old: bundle,
                    new: None,
                    label,
                    undoable: matches!(entity, Entity::Project | Entity::Task),
                });
                Ok(())
            }
        }
    }

    /// Validate a value for `set`, including references and uniqueness.
    fn normalize_set(&mut self, e: Entity, id: i64, field: &str, value: &Value) -> R<Value> {
        if let Some(fid) = fields::cf_id(field) {
            let cf = load_custom_fields(self.c, Filter::Id(fid))?
                .pop()
                .ok_or_else(|| OpErr::Invalid("that custom field was deleted".into()))?;
            let matches_entity = matches!((cf.entity, e), (CfEntity::Project, Entity::Project) | (CfEntity::Task, Entity::Task));
            if !matches_entity {
                return Err(OpErr::Invalid(format!("{} is not a {} field", cf.name, e.label())));
            }
            return fields::normalize_cf(cf.kind, &cf.options, &cf.name, value).map_err(OpErr::Invalid);
        }
        let meta = fields::field(e, field).ok_or_else(|| OpErr::Invalid(format!("unknown field '{field}'")))?;
        self.check_field_perm(meta)?;
        if meta.create_only {
            return Err(OpErr::Invalid(format!("{} can only be set when creating", meta.label)));
        }
        let v = fields::normalize(meta, value).map_err(OpErr::Invalid)?;
        self.check_value(e, id, meta, &v)?;
        Ok(v)
    }

    /// Reference existence, uniqueness and self-protection rules.
    fn check_value(&mut self, e: Entity, id: i64, meta: &FieldMeta, v: &Value) -> R<()> {
        match meta.kind {
            Kind::Ref(target) => {
                if let Some(rid) = v.as_i64() {
                    if !row_exists(self.c, target, rid)? {
                        return Err(OpErr::Invalid(format!("{} {} doesn't exist", target.label(), rid)));
                    }
                    if target == Entity::User {
                        let active: i64 = self.c.query_row("SELECT active FROM users WHERE id = ?1", [rid], |r| r.get(0))?;
                        if active == 0 {
                            return Err(OpErr::Invalid("that user is deactivated".into()));
                        }
                    }
                }
            }
            Kind::Code => {
                let clash: Option<i64> = self
                    .c
                    .query_row("SELECT id FROM projects WHERE code = ?1 AND id <> ?2", params![v.as_str(), id], |r| r.get(0))
                    .optional()?;
                if clash.is_some() {
                    return Err(OpErr::Invalid(format!("code {} is already used", v.as_str().unwrap_or(""))));
                }
            }
            _ => {}
        }
        if e == Entity::User && id == self.actor.id {
            if meta.name == "active" && v == &Value::Bool(false) {
                return Err(OpErr::Invalid("you can't deactivate yourself".into()));
            }
            if meta.name == "role" && v.as_str() != Some("admin") {
                return Err(OpErr::Invalid("you can't remove your own admin role".into()));
            }
        }
        if (e, meta.name) == (Entity::Program, "name") || (e, meta.name) == (Entity::Template, "name") {
            let clash: Option<i64> = self
                .c
                .query_row(&format!("SELECT id FROM {} WHERE name = ?1 AND id <> ?2", e.table()), params![v.as_str(), id], |r| r.get(0))
                .optional()?;
            if clash.is_some() {
                return Err(OpErr::Invalid(format!("the name '{}' is already used", v.as_str().unwrap_or(""))));
            }
        }
        if (e, meta.name) == (Entity::CustomField, "name") {
            let ent: Option<String> = self.c.query_row("SELECT entity FROM custom_fields WHERE id = ?1", [id], |r| r.get(0)).optional()?;
            let clash: Option<i64> = self
                .c
                .query_row(
                    "SELECT id FROM custom_fields WHERE name = ?1 AND entity = ?2 AND id <> ?3",
                    params![v.as_str(), ent, id],
                    |r| r.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(OpErr::Invalid("a field with that name already exists".into()));
            }
        }
        Ok(())
    }

    /// Current value; `None` if the row doesn't exist.
    fn read_field(&self, e: Entity, id: i64, field: &str) -> R<Option<Value>> {
        if !row_exists(self.c, e, id)? {
            return Ok(None);
        }
        if let Some(fid) = fields::cf_id(field) {
            let row: Option<(String, String)> = self
                .c
                .query_row(
                    "SELECT v.value, f.kind FROM custom_values v JOIN custom_fields f ON f.id = v.field_id
                     WHERE v.field_id = ?1 AND v.entity_id = ?2",
                    params![fid, id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            return Ok(Some(row.map_or(Value::Null, |(v, k)| cf_from_text(&k, v))));
        }
        let meta = fields::field(e, field).ok_or_else(|| OpErr::Invalid(format!("unknown field '{field}'")))?;
        let v = match meta.kind {
            Kind::Tags => {
                let (sql, _) = tag_table(e)?;
                let mut st = self.c.prepare_cached(sql)?;
                let tags: Vec<String> = st.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                Value::from(tags)
            }
            Kind::Deps => {
                let mut st = self.c.prepare_cached("SELECT depends_on FROM task_deps WHERE task_id = ?1 ORDER BY depends_on")?;
                let deps: Vec<i64> = st.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                Value::from(deps)
            }
            Kind::Password => Value::Null,
            _ => {
                let sql = format!("SELECT {} FROM {} WHERE id = ?1", meta.name, e.table());
                let raw: Sql = self.c.query_row(&sql, [id], |r| r.get(0))?;
                match (raw, meta.kind) {
                    (Sql::Null, _) => Value::Null,
                    (Sql::Integer(i), Kind::Bool) => Value::Bool(i != 0),
                    (Sql::Integer(i), Kind::Hours) => Value::from(i as f64),
                    (Sql::Integer(i), _) => Value::from(i),
                    (Sql::Real(f), _) => Value::from(f),
                    (Sql::Text(s), Kind::Options) => serde_json::from_str(&s).unwrap_or(Value::Array(vec![])),
                    (Sql::Text(s), _) => Value::String(s),
                    (Sql::Blob(_), _) => Value::Null,
                }
            }
        };
        Ok(Some(v))
    }

    fn write_field(&mut self, e: Entity, id: i64, field: &str, v: &Value) -> R<()> {
        if let Some(fid) = fields::cf_id(field) {
            match cf_to_text(v) {
                Some(t) => self.c.execute(
                    "INSERT OR REPLACE INTO custom_values (field_id, entity_id, value) VALUES (?1, ?2, ?3)",
                    params![fid, id, t],
                )?,
                None => self
                    .c
                    .execute("DELETE FROM custom_values WHERE field_id = ?1 AND entity_id = ?2", params![fid, id])?,
            };
        } else {
            let meta = fields::field(e, field).ok_or_else(|| OpErr::Invalid(format!("unknown field '{field}'")))?;
            match meta.kind {
                Kind::Tags => {
                    let (_, (del, ins)) = tag_table(e)?;
                    self.c.execute(del, [id])?;
                    for t in v.as_array().into_iter().flatten().filter_map(|t| t.as_str()) {
                        self.c.execute(ins, params![id, t])?;
                    }
                }
                Kind::Deps => {
                    self.c.execute("DELETE FROM task_deps WHERE task_id = ?1", [id])?;
                    for d in v.as_array().into_iter().flatten().filter_map(|d| d.as_i64()) {
                        self.c.execute("INSERT INTO task_deps (task_id, depends_on) VALUES (?1, ?2)", params![id, d])?;
                    }
                }
                Kind::Options => {
                    self.c.execute(
                        "UPDATE custom_fields SET options = ?1 WHERE id = ?2",
                        params![serde_json::to_string(v).unwrap_or_else(|_| "[]".into()), id],
                    )?;
                }
                Kind::Password => {
                    let hash = crate::auth::hash_password(v.as_str().unwrap_or_default())
                        .map_err(|e| OpErr::Internal(e.to_string()))?;
                    self.c.execute("UPDATE users SET password_hash = ?1 WHERE id = ?2", params![hash, id])?;
                }
                _ => {
                    let sql = format!("UPDATE {} SET {} = ?1 WHERE id = ?2", e.table(), meta.name);
                    self.c.execute(&sql, params![to_sql(v), id])?;
                }
            }
            if e == Entity::User && field == "active" && v == &Value::Bool(false) {
                self.c.execute("DELETE FROM sessions WHERE user_id = ?1", [id])?;
                self.kick.push((id, None));
            }
        }
        if matches!(e, Entity::Project | Entity::Task) {
            let sql = format!("UPDATE {} SET updated_at = ?1, updated_by = ?2 WHERE id = ?3", e.table());
            self.c.execute(&sql, params![self.now, self.actor.id, id])?;
        }
        self.touched.insert((e, id));
        Ok(())
    }

    /// Undo/redo of a `set`: apply `to` if the field still holds `expect`.
    fn revert_set(&mut self, idx: usize, e: Entity, id: i64, field: &str, expect: &Value, to: &Value) -> R<()> {
        let Some(current) = self.read_field(e, id, field)? else {
            self.conflicts.push(Conflict {
                index: idx,
                entity: e,
                id,
                field: field.to_string(),
                current: Value::Null,
                by: None,
            });
            return Ok(());
        };
        if !values_equal(&current, expect) {
            self.conflicts.push(Conflict {
                index: idx,
                entity: e,
                id,
                field: field.to_string(),
                current,
                by: Store::last_setter(self.c, e, id, field),
            });
            return Ok(());
        }
        // references may have disappeared since; check them again
        if let Some(meta) = fields::field(e, field) {
            self.check_value(e, id, meta, to)?;
        }
        self.write_field(e, id, field, to)
    }

    fn create(&mut self, e: Entity, values: &Map<String, Value>) -> R<i64> {
        let mut row = default_row(e, self.actor.id, self.now);
        let mut cf = Map::new();
        let mut password = None;
        for (k, v) in values {
            if let Some(fid) = fields::cf_id(k) {
                let def = load_custom_fields(self.c, Filter::Id(fid))?
                    .pop()
                    .ok_or_else(|| OpErr::Invalid("unknown custom field".into()))?;
                let nv = fields::normalize_cf(def.kind, &def.options, &def.name, v).map_err(OpErr::Invalid)?;
                if !nv.is_null() {
                    cf.insert(fid.to_string(), nv);
                }
                continue;
            }
            let meta = fields::field(e, k).ok_or_else(|| OpErr::Invalid(format!("unknown field '{k}'")))?;
            self.check_field_perm(meta)?;
            let nv = fields::normalize(meta, v).map_err(OpErr::Invalid)?;
            self.check_value(e, 0, meta, &nv)?;
            if meta.kind == Kind::Password {
                password = nv.as_str().map(String::from);
            } else {
                row.insert(k.clone(), nv);
            }
        }
        for m in fields::fields(e).iter().filter(|m| m.required) {
            let missing = if m.kind == Kind::Password {
                password.is_none()
            } else {
                row.get(m.name).is_none_or(|v| v.is_null() || v.as_str() == Some("") || v.as_i64() == Some(0))
            };
            if missing {
                return Err(OpErr::Invalid(format!("{} is required", m.label)));
            }
        }
        row.insert("cf".into(), Value::Object(cf));
        let rowv = Value::Object(row);
        let bad = |err: serde_json::Error| OpErr::Internal(format!("row build: {err}"));
        let id = match e {
            Entity::Project => {
                let p: Project = serde_json::from_value(rowv).map_err(bad)?;
                let id = insert_project(self.c, &p)?;
                if let Some(tid) = p.template_id {
                    self.instantiate(id, tid, &p)?;
                }
                id
            }
            Entity::Task => {
                let mut t: Task = serde_json::from_value(rowv).map_err(bad)?;
                t.seq = self
                    .c
                    .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM tasks WHERE project_id = ?1", [t.project_id], |r| r.get(0))?;
                let id = insert_task(self.c, &t)?;
                for d in &t.deps {
                    self.c.execute("INSERT INTO task_deps (task_id, depends_on) VALUES (?1, ?2)", params![id, d])?;
                }
                id
            }
            Entity::Program => insert_program(self.c, &serde_json::from_value(rowv).map_err(bad)?)?,
            Entity::Template => insert_template(self.c, &serde_json::from_value(rowv).map_err(bad)?)?,
            Entity::TemplateTask => {
                let mut t: TemplateTask = serde_json::from_value(rowv).map_err(bad)?;
                if !values.contains_key("seq") {
                    t.seq = self.c.query_row(
                        "SELECT COALESCE(MAX(seq), 0) + 1 FROM template_tasks WHERE template_id = ?1",
                        [t.template_id],
                        |r| r.get(0),
                    )?;
                }
                insert_template_task(self.c, &t)?
            }
            Entity::CustomField => {
                let mut f: CustomField = serde_json::from_value(rowv).map_err(bad)?;
                let clash: Option<i64> = self
                    .c
                    .query_row(
                        "SELECT id FROM custom_fields WHERE name = ?1 AND entity = ?2",
                        params![f.name, f.entity.as_str()],
                        |r| r.get(0),
                    )
                    .optional()?;
                if clash.is_some() {
                    return Err(OpErr::Invalid("a field with that name already exists".into()));
                }
                if !values.contains_key("seq") {
                    f.seq = self.c.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM custom_fields", [], |r| r.get(0))?;
                }
                insert_custom_field(self.c, &f)?
            }
            Entity::User => {
                let u: User = serde_json::from_value(rowv).map_err(bad)?;
                let taken: Option<i64> = self
                    .c
                    .query_row("SELECT id FROM users WHERE username = ?1 COLLATE NOCASE", [&u.username], |r| r.get(0))
                    .optional()?;
                if taken.is_some() {
                    return Err(OpErr::Invalid(format!("username {} is taken", u.username)));
                }
                let hash = crate::auth::hash_password(password.as_deref().unwrap_or_default())
                    .map_err(|e| OpErr::Internal(e.to_string()))?;
                insert_user(self.c, &u, &hash, self.now)?
            }
        };
        self.touched.insert((e, id));
        if e == Entity::Project {
            for tid in self.task_ids_of(id)? {
                self.touched.insert((Entity::Task, tid));
            }
        }
        let bundle = self.bundle(e, id)?;
        let label = format!("created {} {}", e.label(), self.describe(e, id));
        self.entries.push(Entry {
            action: Action::Create,
            entity: e,
            id,
            field: String::new(),
            old: None,
            new: bundle,
            label,
            undoable: e != Entity::User,
        });
        Ok(id)
    }

    /// Protocol §7: tasks from the template, dates from the project start.
    fn instantiate(&mut self, pid: i64, tid: i64, p: &Project) -> R<()> {
        let tts = load_template_tasks(self.c, Filter::Project(tid))?;
        let start = p.start_date.as_deref().and_then(Date::parse_iso).unwrap_or_else(today);
        let mut phase_last: Vec<(String, i64)> = Vec::new(); // (phase, last task id), in phase order
        for tt in tts {
            let s = start.add_days(tt.offset_days as i32);
            let t = Task {
                id: 0,
                project_id: pid,
                seq: tt.seq,
                title: tt.title.clone(),
                phase: tt.phase.clone(),
                status: TaskStatus::Todo,
                priority: Priority::Medium,
                assignee_id: p.owner_id,
                start_date: Some(s.iso()),
                due_date: Some(s.add_days(tt.duration_days.max(1) as i32 - 1).iso()),
                est_hours: tt.est_hours,
                actual_hours: 0.0,
                deps: vec![],
                tags: vec![],
                notes: String::new(),
                cf: Default::default(),
                created_at: self.now.to_string(),
                updated_at: self.now.to_string(),
                updated_by: Some(self.actor.id),
            };
            let id = insert_task(self.c, &t)?;
            match phase_last.iter().position(|(ph, _)| *ph == tt.phase) {
                Some(i) => phase_last[i].1 = id,
                None => {
                    if let Some((_, prev)) = phase_last.last() {
                        insert_dep(self.c, id, *prev)?;
                    }
                    phase_last.push((tt.phase.clone(), id));
                }
            }
        }
        Ok(())
    }

    fn task_ids_of(&self, pid: i64) -> R<Vec<i64>> {
        let mut st = self.c.prepare_cached("SELECT id FROM tasks WHERE project_id = ?1")?;
        Ok(st.query_map([pid], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Full row (plus dependent rows) as stored in activity for undo/redo.
    fn bundle(&self, e: Entity, id: i64) -> R<Option<Value>> {
        let mut t = Tables::default();
        load_rows(self.c, e, Filter::Id(id), &mut t)?;
        let row = match e {
            Entity::User => t.users.pop().map(|r| json!(r)),
            Entity::Program => t.programs.pop().map(|r| json!(r)),
            Entity::Template => t.templates.pop().map(|r| json!(r)),
            Entity::TemplateTask => t.template_tasks.pop().map(|r| json!(r)),
            Entity::Project => t.projects.pop().map(|r| json!(r)),
            Entity::Task => t.tasks.pop().map(|r| json!(r)),
            Entity::CustomField => t.custom_fields.pop().map(|r| json!(r)),
        };
        let Some(row) = row else { return Ok(None) };
        let mut b = json!({ "row": row });
        if e == Entity::Project {
            b["tasks"] = json!(load_tasks(self.c, Filter::Project(id))?);
        }
        if e == Entity::Task {
            let mut st = self.c.prepare_cached("SELECT task_id FROM task_deps WHERE depends_on = ?1 ORDER BY task_id")?;
            let deps: Vec<i64> = st.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            b["dependents"] = json!(deps);
        }
        Ok(Some(b))
    }

    /// Delete a row and its dependents; returns its bundle when asked.
    fn delete_row(&mut self, e: Entity, id: i64, want_bundle: bool) -> R<Option<Value>> {
        if !row_exists(self.c, e, id)? {
            return Ok(None);
        }
        let bundle = if want_bundle { self.bundle(e, id)? } else { None };
        let ids_of = |sql: &str, id: i64| -> R<Vec<i64>> {
            let mut st = self.c.prepare_cached(sql)?;
            Ok(st.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
        };
        match e {
            Entity::Project => {
                for tid in self.task_ids_of(id)? {
                    self.c.execute(
                        "DELETE FROM custom_values WHERE entity_id = ?1 AND field_id IN (SELECT id FROM custom_fields WHERE entity = 'task')",
                        [tid],
                    )?;
                    self.deleted.insert((Entity::Task, tid));
                }
                self.c.execute(
                    "DELETE FROM custom_values WHERE entity_id = ?1 AND field_id IN (SELECT id FROM custom_fields WHERE entity = 'project')",
                    [id],
                )?;
            }
            Entity::Task => {
                for dependent in ids_of("SELECT task_id FROM task_deps WHERE depends_on = ?1", id)? {
                    self.touched.insert((Entity::Task, dependent));
                }
                self.c.execute(
                    "DELETE FROM custom_values WHERE entity_id = ?1 AND field_id IN (SELECT id FROM custom_fields WHERE entity = 'task')",
                    [id],
                )?;
            }
            Entity::Program => {
                for p in ids_of("SELECT id FROM projects WHERE program_id = ?1", id)? {
                    self.touched.insert((Entity::Project, p));
                }
            }
            Entity::Template => {
                for p in ids_of("SELECT id FROM projects WHERE template_id = ?1", id)? {
                    self.touched.insert((Entity::Project, p));
                }
                for t in ids_of("SELECT id FROM template_tasks WHERE template_id = ?1", id)? {
                    self.deleted.insert((Entity::TemplateTask, t));
                }
            }
            Entity::CustomField => {
                let ent: String = self.c.query_row("SELECT entity FROM custom_fields WHERE id = ?1", [id], |r| r.get(0))?;
                let target = if ent == "project" { Entity::Project } else { Entity::Task };
                for row in ids_of("SELECT entity_id FROM custom_values WHERE field_id = ?1", id)? {
                    self.touched.insert((target, row));
                }
            }
            Entity::TemplateTask | Entity::User => {}
        }
        self.c.execute(&format!("DELETE FROM {} WHERE id = ?1", e.table()), [id])?;
        self.deleted.insert((e, id));
        self.touched.remove(&(e, id));
        Ok(bundle)
    }

    /// Re-insert a bundle with its original ids (undo of delete, redo of create).
    fn restore(&mut self, e: Entity, bundle: &Value) -> R<()> {
        let row = bundle.get("row").cloned().unwrap_or(Value::Null);
        let bad = |err: serde_json::Error| OpErr::Internal(format!("bad bundle: {err}"));
        match e {
            Entity::Project => {
                let p: Project = serde_json::from_value(row).map_err(bad)?;
                if row_exists(self.c, e, p.id)? {
                    return Ok(());
                }
                let clash: Option<i64> =
                    self.c.query_row("SELECT id FROM projects WHERE code = ?1", [&p.code], |r| r.get(0)).optional()?;
                if clash.is_some() {
                    return Err(OpErr::Invalid(format!("code {} has been reused; can't restore", p.code)));
                }
                insert_project(self.c, &p)?;
                let tasks: Vec<Task> = serde_json::from_value(bundle.get("tasks").cloned().unwrap_or(json!([]))).map_err(bad)?;
                for t in &tasks {
                    insert_task(self.c, t)?;
                    self.touched.insert((Entity::Task, t.id));
                    self.deleted.remove(&(Entity::Task, t.id));
                }
                for t in &tasks {
                    for d in &t.deps {
                        insert_dep(self.c, t.id, *d)?;
                    }
                }
                self.touched.insert((e, p.id));
                self.deleted.remove(&(e, p.id));
            }
            Entity::Task => {
                let mut t: Task = serde_json::from_value(row).map_err(bad)?;
                if row_exists(self.c, e, t.id)? {
                    return Ok(());
                }
                if !row_exists(self.c, Entity::Project, t.project_id)? {
                    return Err(OpErr::NotFound("its project was deleted; can't restore the task".into()));
                }
                let taken: Option<i64> = self
                    .c
                    .query_row("SELECT id FROM tasks WHERE project_id = ?1 AND seq = ?2", params![t.project_id, t.seq], |r| r.get(0))
                    .optional()?;
                if taken.is_some() {
                    t.seq = self
                        .c
                        .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM tasks WHERE project_id = ?1", [t.project_id], |r| r.get(0))?;
                }
                insert_task(self.c, &t)?;
                for d in &t.deps {
                    insert_dep(self.c, t.id, *d)?;
                }
                let dependents: Vec<i64> =
                    serde_json::from_value(bundle.get("dependents").cloned().unwrap_or(json!([]))).unwrap_or_default();
                for dep in dependents {
                    insert_dep(self.c, dep, t.id)?;
                    if row_exists(self.c, Entity::Task, dep)? {
                        self.touched.insert((Entity::Task, dep));
                    }
                }
                self.touched.insert((e, t.id));
                self.deleted.remove(&(e, t.id));
            }
            Entity::Program => {
                let p: Program = serde_json::from_value(row).map_err(bad)?;
                if !row_exists(self.c, e, p.id)? {
                    insert_program(self.c, &p)?;
                }
                self.touched.insert((e, p.id));
                self.deleted.remove(&(e, p.id));
            }
            Entity::Template => {
                let t: Template = serde_json::from_value(row).map_err(bad)?;
                if !row_exists(self.c, e, t.id)? {
                    insert_template(self.c, &t)?;
                }
                self.touched.insert((e, t.id));
                self.deleted.remove(&(e, t.id));
            }
            Entity::TemplateTask => {
                let t: TemplateTask = serde_json::from_value(row).map_err(bad)?;
                if !row_exists(self.c, Entity::Template, t.template_id)? {
                    return Err(OpErr::NotFound("its template was deleted".into()));
                }
                if !row_exists(self.c, e, t.id)? {
                    insert_template_task(self.c, &t)?;
                }
                self.touched.insert((e, t.id));
                self.deleted.remove(&(e, t.id));
            }
            Entity::CustomField => {
                let f: CustomField = serde_json::from_value(row).map_err(bad)?;
                if !row_exists(self.c, e, f.id)? {
                    insert_custom_field(self.c, &f)?;
                }
                self.touched.insert((e, f.id));
                self.deleted.remove(&(e, f.id));
            }
            Entity::User => return Err(OpErr::Invalid("user changes can't be undone".into())),
        }
        Ok(())
    }

    /// Cross-field rules over every touched row (protocol §5 rule 4).
    fn validate(&self) -> R<()> {
        for &(e, id) in &self.touched {
            if !matches!(e, Entity::Project | Entity::Task) || !row_exists(self.c, e, id)? {
                continue;
            }
            let sql = format!("SELECT start_date, due_date FROM {} WHERE id = ?1", e.table());
            let (s, d): (Option<String>, Option<String>) = self.c.query_row(&sql, [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if let (Some(s), Some(d)) = (s, d) {
                if d < s {
                    return Err(OpErr::Invalid(format!("{}: due date {d} is before start date {s}", self.describe(e, id))));
                }
            }
            if e == Entity::Task {
                self.check_deps(id)?;
            }
        }
        Ok(())
    }

    fn check_deps(&self, id: i64) -> R<()> {
        let pid: i64 = self.c.query_row("SELECT project_id FROM tasks WHERE id = ?1", [id], |r| r.get(0))?;
        // deps must stay inside the project
        let foreign: Option<i64> = self
            .c
            .query_row(
                "SELECT d.depends_on FROM task_deps d JOIN tasks b ON b.id = d.depends_on WHERE d.task_id = ?1 AND b.project_id <> ?2",
                params![id, pid],
                |r| r.get(0),
            )
            .optional()?;
        if foreign.is_some() {
            return Err(OpErr::Invalid("a task can only depend on tasks in the same project".into()));
        }
        let mut adj: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut st = self.c.prepare_cached(
            "SELECT d.task_id, d.depends_on FROM task_deps d JOIN tasks a ON a.id = d.task_id WHERE a.project_id = ?1",
        )?;
        for row in st.query_map([pid], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (a, b) = row?;
            adj.entry(a).or_default().push(b);
        }
        // DFS from `id` looking for a path back to `id`
        let mut stack: Vec<i64> = adj.get(&id).cloned().unwrap_or_default();
        let mut seen = HashSet::new();
        while let Some(n) = stack.pop() {
            if n == id {
                return Err(OpErr::Invalid(format!("{} would depend on itself (dependency loop)", self.describe(Entity::Task, id))));
            }
            if seen.insert(n) {
                stack.extend(adj.get(&n).into_iter().flatten());
            }
        }
        Ok(())
    }

    /// Writes activity, collects changed rows. `record`: false for undo/redo.
    fn finish(&mut self, record: bool) -> R<Finished> {
        let mut group = 0;
        if record {
            group = self.c.query_row("SELECT COALESCE(MAX(grp), 0) + 1 FROM activity", [], |r| r.get(0))?;
            // a new change makes this user's undone groups unreachable for redo
            let mut st = self.c.prepare_cached("SELECT id FROM activity WHERE user_id = ?1 AND state = 'undone'")?;
            let dead: Vec<i64> = st.query_map([self.actor.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            self.c.execute("UPDATE activity SET state = 'dead' WHERE user_id = ?1 AND state = 'undone'", [self.actor.id])?;
            self.updated_activity.extend(dead);
            let undoable = self.entries.iter().all(|e| e.undoable);
            let mut st = self.c.prepare_cached(
                "INSERT INTO activity (ts, user_id, grp, action, entity, entity_id, field, old, new, label, state)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            )?;
            for en in &self.entries {
                st.execute(params![
                    self.now,
                    self.actor.id,
                    group,
                    en.action.as_str(),
                    en.entity.as_str(),
                    en.id,
                    en.field,
                    en.old.as_ref().map(|v| v.to_string()),
                    en.new.as_ref().map(|v| v.to_string()),
                    en.label,
                    if undoable { "done" } else { "dead" },
                ])?;
                self.updated_activity.push(self.c.last_insert_rowid());
            }
        }
        let mut upserts = Tables::default();
        for &(e, id) in &self.touched {
            if !self.deleted.contains(&(e, id)) {
                load_rows(self.c, e, Filter::Id(id), &mut upserts)?;
            }
        }
        let mut deletes = Deletes::default();
        for &(e, id) in &self.deleted {
            deletes.get_mut(e).push(id);
        }
        self.updated_activity.sort();
        self.updated_activity.dedup();
        let activity = load_activity_ids(self.c, &self.updated_activity)?;
        let changes = Changes {
            by: Some(self.actor.id),
            group: (group > 0).then_some(group),
            upserts,
            deletes,
            activity,
        };
        Ok((group, changes, std::mem::take(&mut self.kick)))
    }

    // ---- labels ----

    fn describe(&self, e: Entity, id: i64) -> String {
        let q = |sql: &str| -> Option<String> { self.c.query_row(sql, [id], |r| r.get(0)).optional().ok().flatten() };
        match e {
            Entity::Project => q("SELECT code || ' ''' || name || '''' FROM projects WHERE id = ?1"),
            Entity::Task => q("SELECT p.code || ' #' || t.seq || ' ''' || t.title || '''' FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id = ?1"),
            Entity::User => q("SELECT username FROM users WHERE id = ?1"),
            Entity::Program => q("SELECT name FROM programs WHERE id = ?1"),
            Entity::Template => q("SELECT name FROM templates WHERE id = ?1"),
            Entity::TemplateTask => q("SELECT tt.title || ' (' || t.name || ')' FROM template_tasks tt JOIN templates t ON t.id = tt.template_id WHERE tt.id = ?1"),
            Entity::CustomField => q("SELECT name FROM custom_fields WHERE id = ?1"),
        }
        .unwrap_or_else(|| format!("#{id}"))
    }

    fn short_ref(&self, e: Entity, id: i64) -> String {
        let q = |sql: &str| -> Option<String> { self.c.query_row(sql, [id], |r| r.get(0)).optional().ok().flatten() };
        match e {
            Entity::Project => q("SELECT code FROM projects WHERE id = ?1"),
            Entity::Task => q("SELECT p.code || ' #' || t.seq FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id = ?1"),
            _ => Some(self.describe(e, id)),
        }
        .unwrap_or_else(|| format!("#{id}"))
    }

    fn fmt_value(&self, e: Entity, field: &str, v: &Value) -> String {
        let name = |sql: &str, id: i64| -> String {
            self.c.query_row(sql, [id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| id.to_string())
        };
        let s = match v {
            Value::Null => "∅".to_string(),
            Value::Bool(b) => if *b { "yes" } else { "no" }.into(),
            Value::Number(n) => match (field, n.as_i64()) {
                ("owner_id" | "assignee_id", Some(id)) => name("SELECT username FROM users WHERE id = ?1", id),
                ("program_id", Some(id)) => name("SELECT name FROM programs WHERE id = ?1", id),
                _ => n.as_f64().map(fmt_hours).unwrap_or_else(|| n.to_string()),
            },
            Value::String(s) => match (e, field) {
                (Entity::Project, "status") => ProjectStatus::parse(s).map_or(s.clone(), |x| x.label().into()),
                (Entity::Task, "status") => TaskStatus::parse(s).map_or(s.clone(), |x| x.label().into()),
                (_, "priority") => Priority::parse(s).map_or(s.clone(), |x| x.label().into()),
                (_, "health") => Health::parse(s).map_or(s.clone(), |x| x.label().into()),
                _ => s.clone(),
            },
            Value::Array(a) if field == "deps" => a
                .iter()
                .filter_map(|x| x.as_i64())
                .map(|id| name("SELECT '#' || seq FROM tasks WHERE id = ?1", id))
                .collect::<Vec<_>>()
                .join(" "),
            Value::Array(a) => a.iter().map(|x| x.as_str().unwrap_or("?").to_string()).collect::<Vec<_>>().join(" "),
            Value::Object(_) => "…".into(),
        };
        let s = s.replace('\n', " ");
        if s.chars().count() > 40 {
            format!("{}…", s.chars().take(39).collect::<String>())
        } else {
            s
        }
    }

    fn set_label(&self, e: Entity, id: i64, field: &str, old: &Value, new: &Value) -> String {
        let fname = match fields::cf_id(field) {
            Some(fid) => self
                .c
                .query_row("SELECT name FROM custom_fields WHERE id = ?1", [fid], |r| r.get::<_, String>(0))
                .unwrap_or_else(|_| field.to_string()),
            None => fields::field(e, field).map_or(field.to_string(), |m| m.label.to_lowercase()),
        };
        if field == "password" {
            return format!("{} password changed", self.short_ref(e, id));
        }
        format!(
            "{} {}: {} → {}",
            self.short_ref(e, id),
            fname,
            self.fmt_value(e, field, old),
            self.fmt_value(e, field, new)
        )
    }
}

fn tag_table(e: Entity) -> R<(&'static str, (&'static str, &'static str))> {
    match e {
        Entity::Project => Ok((
            "SELECT tag FROM project_tags WHERE project_id = ?1 ORDER BY tag",
            (
                "DELETE FROM project_tags WHERE project_id = ?1",
                "INSERT OR IGNORE INTO project_tags (project_id, tag) VALUES (?1, ?2)",
            ),
        )),
        Entity::Task => Ok((
            "SELECT tag FROM task_tags WHERE task_id = ?1 ORDER BY tag",
            (
                "DELETE FROM task_tags WHERE task_id = ?1",
                "INSERT OR IGNORE INTO task_tags (task_id, tag) VALUES (?1, ?2)",
            ),
        )),
        _ => Err(OpErr::Invalid("this entity has no tags".into())),
    }
}

/// Defaults for `create` (protocol §4); provided values overlay these.
fn default_row(e: Entity, actor: i64, now: &str) -> Map<String, Value> {
    let v = match e {
        Entity::Project => json!({
            "id": 0, "code": "", "name": "", "program_id": null, "template_id": null, "owner_id": null,
            "status": "planning", "priority": "medium", "health": "green", "start_date": null, "due_date": null,
            "budget_hours": null, "tags": [], "notes": "", "cf": {},
            "created_at": now, "updated_at": now, "updated_by": actor
        }),
        Entity::Task => json!({
            "id": 0, "project_id": 0, "seq": 0, "title": "", "phase": "", "status": "todo", "priority": "medium",
            "assignee_id": null, "start_date": null, "due_date": null, "est_hours": null, "actual_hours": 0.0,
            "deps": [], "tags": [], "notes": "", "cf": {},
            "created_at": now, "updated_at": now, "updated_by": actor
        }),
        Entity::Program => json!({ "id": 0, "name": "", "client": "", "prefix": "", "color": "#888888" }),
        Entity::Template => json!({ "id": 0, "name": "", "description": "" }),
        Entity::TemplateTask => json!({
            "id": 0, "template_id": 0, "seq": 0, "title": "", "phase": "", "est_hours": null,
            "offset_days": 0, "duration_days": 1
        }),
        Entity::CustomField => json!({ "id": 0, "entity": "task", "name": "", "kind": "text", "options": [], "seq": 0 }),
        Entity::User => json!({ "id": 0, "username": "", "display_name": "", "role": "member", "active": true }),
    };
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}
