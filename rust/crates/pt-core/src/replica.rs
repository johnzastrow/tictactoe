//! Client-side copy of the server's data, kept current by `snapshot` and
//! `changes` messages, plus the indexes and computed metrics every view needs
//! (protocol §8).

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::date::Date;
use crate::model::*;
use crate::protocol::{Changes, Peer, ServerInfo, ServerMsg};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub tasks_total: usize,
    pub tasks_done: usize,
    pub tasks_open: usize,
    pub tasks_blocked: usize,
    pub tasks_overdue: usize,
    pub est_total: f64,
    pub actual_total: f64,
    pub next_due: Option<Date>,
}

impl Metrics {
    pub fn pct_done(&self) -> f64 {
        if self.tasks_total == 0 {
            0.0
        } else {
            self.tasks_done as f64 * 100.0 / self.tasks_total as f64
        }
    }
}

/// What a message changed, so UIs can flash rows and toast.
#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub snapshot: bool,
    pub by: Option<i64>,
    pub upserted: Vec<(Entity, i64)>,
    pub deleted: Vec<(Entity, i64)>,
    pub activity: Vec<Activity>,
    pub presence: bool,
}

#[derive(Clone, Debug)]
pub struct Replica {
    pub me: Option<User>,
    pub conn: u64,
    pub server: Option<ServerInfo>,
    pub today: Date,
    pub users: BTreeMap<i64, User>,
    pub programs: BTreeMap<i64, Program>,
    pub templates: BTreeMap<i64, Template>,
    pub template_tasks: BTreeMap<i64, TemplateTask>,
    pub projects: BTreeMap<i64, Project>,
    pub tasks: BTreeMap<i64, Task>,
    pub custom_fields: BTreeMap<i64, CustomField>,
    pub activity: BTreeMap<i64, Activity>,
    pub peers: Vec<Peer>,
    /// Bumped on every change; views cache derived data against it.
    pub version: u64,
    tasks_by_project: HashMap<i64, Vec<i64>>,
    metrics: HashMap<i64, Metrics>,
    loaded: bool,
}

impl Replica {
    pub fn new(today: Date) -> Replica {
        Replica {
            me: None,
            conn: 0,
            server: None,
            today,
            users: BTreeMap::new(),
            programs: BTreeMap::new(),
            templates: BTreeMap::new(),
            template_tasks: BTreeMap::new(),
            projects: BTreeMap::new(),
            tasks: BTreeMap::new(),
            custom_fields: BTreeMap::new(),
            activity: BTreeMap::new(),
            peers: Vec::new(),
            version: 0,
            tasks_by_project: HashMap::new(),
            metrics: HashMap::new(),
            loaded: false,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn me_id(&self) -> i64 {
        self.me.as_ref().map_or(0, |u| u.id)
    }

    pub fn can_edit(&self) -> bool {
        self.me.as_ref().is_some_and(User::can_edit)
    }

    pub fn is_admin(&self) -> bool {
        self.me.as_ref().is_some_and(User::is_admin)
    }

    pub fn set_today(&mut self, today: Date) {
        if today != self.today {
            self.today = today;
            self.reindex();
        }
    }

    /// Apply any server message. Returns what changed.
    pub fn apply(&mut self, msg: &ServerMsg) -> Applied {
        let mut out = Applied::default();
        match msg {
            ServerMsg::Welcome { me, conn, server, .. } => {
                self.me = Some(me.clone());
                self.conn = *conn;
                self.server = Some(server.clone());
            }
            ServerMsg::Snapshot { data } => {
                self.load(data.clone());
                out.snapshot = true;
            }
            ServerMsg::Changes(c) => out = self.apply_changes(c),
            ServerMsg::Presence { peers } => {
                self.peers = peers.clone();
                out.presence = true;
            }
            _ => return out,
        }
        self.version += 1;
        out
    }

    pub fn load(&mut self, t: Tables) {
        self.users = t.users.into_iter().map(|r| (r.id, r)).collect();
        self.programs = t.programs.into_iter().map(|r| (r.id, r)).collect();
        self.templates = t.templates.into_iter().map(|r| (r.id, r)).collect();
        self.template_tasks = t.template_tasks.into_iter().map(|r| (r.id, r)).collect();
        self.projects = t.projects.into_iter().map(|r| (r.id, r)).collect();
        self.tasks = t.tasks.into_iter().map(|r| (r.id, r)).collect();
        self.custom_fields = t.custom_fields.into_iter().map(|r| (r.id, r)).collect();
        self.activity = t.activity.into_iter().map(|r| (r.id, r)).collect();
        self.loaded = true;
        if let Some(me) = &self.me {
            if let Some(u) = self.users.get(&me.id) {
                self.me = Some(u.clone());
            }
        }
        self.reindex();
        self.version += 1;
    }

    pub fn apply_changes(&mut self, c: &Changes) -> Applied {
        let mut out = Applied {
            by: c.by,
            ..Default::default()
        };
        macro_rules! up {
            ($field:ident, $ent:expr) => {
                for r in &c.upserts.$field {
                    out.upserted.push(($ent, r.id));
                    self.$field.insert(r.id, r.clone());
                }
                for id in &c.deletes.$field {
                    out.deleted.push(($ent, *id));
                    self.$field.remove(id);
                }
            };
        }
        up!(users, Entity::User);
        up!(programs, Entity::Program);
        up!(templates, Entity::Template);
        up!(template_tasks, Entity::TemplateTask);
        up!(projects, Entity::Project);
        up!(tasks, Entity::Task);
        up!(custom_fields, Entity::CustomField);
        // A deleted project takes its tasks with it even if the server only
        // listed the project.
        for id in &c.deletes.projects {
            self.tasks.retain(|_, t| t.project_id != *id);
        }
        for a in c.activity.iter().chain(&c.upserts.activity) {
            self.activity.insert(a.id, a.clone());
            out.activity.push(a.clone());
        }
        // keep the activity log bounded
        while self.activity.len() > 2000 {
            let first = *self.activity.keys().next().unwrap();
            self.activity.remove(&first);
        }
        if let Some(me) = &self.me {
            if let Some(u) = self.users.get(&me.id) {
                self.me = Some(u.clone());
            }
        }
        self.reindex();
        self.version += 1;
        out
    }

    fn reindex(&mut self) {
        let mut by_project: HashMap<i64, Vec<i64>> = HashMap::new();
        for t in self.tasks.values() {
            by_project.entry(t.project_id).or_default().push(t.id);
        }
        for ids in by_project.values_mut() {
            ids.sort_by_key(|id| self.tasks[id].seq);
        }
        let mut metrics = HashMap::new();
        for p in self.projects.values() {
            let mut m = Metrics::default();
            for id in by_project.get(&p.id).map(Vec::as_slice).unwrap_or(&[]) {
                let t = &self.tasks[id];
                m.tasks_total += 1;
                if t.status == TaskStatus::Done {
                    m.tasks_done += 1;
                } else {
                    m.tasks_open += 1;
                    if let Some(d) = t.due_date.as_deref().and_then(Date::parse_iso) {
                        if d < self.today {
                            m.tasks_overdue += 1;
                        }
                        if m.next_due.is_none_or(|n| d < n) {
                            m.next_due = Some(d);
                        }
                    }
                }
                if t.status == TaskStatus::Blocked {
                    m.tasks_blocked += 1;
                }
                m.est_total += t.est_hours.unwrap_or(0.0);
                m.actual_total += t.actual_hours;
            }
            metrics.insert(p.id, m);
        }
        self.tasks_by_project = by_project;
        self.metrics = metrics;
    }

    // ---- lookups -------------------------------------------------------

    pub fn project_tasks(&self, pid: i64) -> &[i64] {
        self.tasks_by_project.get(&pid).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn metrics(&self, pid: i64) -> Metrics {
        self.metrics.get(&pid).copied().unwrap_or_default()
    }

    pub fn user_name(&self, id: Option<i64>) -> &str {
        id.and_then(|id| self.users.get(&id)).map_or("", |u| u.display_name.as_str())
    }

    pub fn username(&self, id: Option<i64>) -> &str {
        id.and_then(|id| self.users.get(&id)).map_or("", |u| u.username.as_str())
    }

    pub fn program_name(&self, id: Option<i64>) -> &str {
        id.and_then(|id| self.programs.get(&id)).map_or("", |p| p.name.as_str())
    }

    pub fn project_code(&self, id: i64) -> &str {
        self.projects.get(&id).map_or("?", |p| p.code.as_str())
    }

    /// `NWR-004 #5`
    pub fn task_ref(&self, t: &Task) -> String {
        format!("{} #{}", self.project_code(t.project_id), t.seq)
    }

    pub fn task_status(&self, id: i64) -> Option<TaskStatus> {
        self.tasks.get(&id).map(|t| t.status)
    }

    /// Protocol §8: blocked = status blocked, or a dependency not done while work is under way.
    pub fn task_blocked(&self, t: &Task) -> bool {
        t.status == TaskStatus::Blocked
            || (matches!(t.status, TaskStatus::InProgress | TaskStatus::Review)
                && t.deps.iter().any(|d| self.task_status(*d).is_some_and(|s| s != TaskStatus::Done)))
    }

    pub fn task_overdue(&self, t: &Task) -> bool {
        t.status != TaskStatus::Done
            && t.due_date.as_deref().and_then(Date::parse_iso).is_some_and(|d| d < self.today)
    }

    pub fn project_overdue(&self, p: &Project) -> bool {
        !matches!(p.status, ProjectStatus::Done | ProjectStatus::Cancelled)
            && p.due_date.as_deref().and_then(Date::parse_iso).is_some_and(|d| d < self.today)
    }

    pub fn project_open(&self, p: &Project) -> bool {
        !matches!(p.status, ProjectStatus::Done | ProjectStatus::Cancelled)
    }

    pub fn burn_pct(&self, p: &Project) -> Option<f64> {
        let b = p.budget_hours?;
        if b <= 0.0 {
            return None;
        }
        Some(self.metrics(p.id).actual_total * 100.0 / b)
    }

    pub fn custom_fields_for(&self, e: CfEntity) -> Vec<&CustomField> {
        let mut v: Vec<&CustomField> = self.custom_fields.values().filter(|c| c.entity == e).collect();
        v.sort_by(|a, b| a.seq.cmp(&b.seq).then(a.id.cmp(&b.id)));
        v
    }

    pub fn template_tasks_of(&self, tid: i64) -> Vec<&TemplateTask> {
        let mut v: Vec<&TemplateTask> = self.template_tasks.values().filter(|t| t.template_id == tid).collect();
        v.sort_by(|a, b| a.seq.cmp(&b.seq).then(a.id.cmp(&b.id)));
        v
    }

    /// (task count, estimated hours, span in days) for the New Project dialog.
    pub fn template_stats(&self, tid: i64) -> (usize, f64, i64) {
        let tt = self.template_tasks_of(tid);
        let hours = tt.iter().map(|t| t.est_hours.unwrap_or(0.0)).sum();
        let span = tt.iter().map(|t| t.offset_days + t.duration_days).max().unwrap_or(0);
        (tt.len(), hours, span)
    }

    /// Phases offered for a task in this project: the project's template
    /// phases in template order, then any other phases already in use.
    pub fn phases_for_project(&self, pid: i64) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(tid) = self.projects.get(&pid).and_then(|p| p.template_id) {
            for t in self.template_tasks_of(tid) {
                if !t.phase.is_empty() && !out.contains(&t.phase) {
                    out.push(t.phase.clone());
                }
            }
        }
        for id in self.project_tasks(pid) {
            let ph = &self.tasks[id].phase;
            if !ph.is_empty() && !out.contains(ph) {
                out.push(ph.clone());
            }
        }
        out
    }

    /// Next free project code for a program prefix, e.g. `NWR-008`.
    pub fn next_code(&self, program_id: Option<i64>) -> String {
        let prefix = program_id
            .and_then(|id| self.programs.get(&id))
            .map(|p| p.prefix.clone())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "PRJ".into());
        let max = self
            .projects
            .values()
            .filter_map(|p| p.code.strip_prefix(&format!("{prefix}-")))
            .filter_map(|n| n.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("{prefix}-{:03}", max + 1)
    }

    /// All tags in use for an entity kind, most used first.
    pub fn tag_counts(&self, e: Entity) -> Vec<(String, usize)> {
        let mut m: HashMap<&str, usize> = HashMap::new();
        let tags: Box<dyn Iterator<Item = &String>> = match e {
            Entity::Project => Box::new(self.projects.values().flat_map(|p| &p.tags)),
            _ => Box::new(self.tasks.values().flat_map(|t| &t.tags)),
        };
        for t in tags {
            *m.entry(t.as_str()).or_default() += 1;
        }
        let mut v: Vec<(String, usize)> = m.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// Current value of a field as JSON: what an edit sends as `base`.
    pub fn field_value(&self, e: Entity, id: i64, field: &str) -> Value {
        if let Some(cf) = field.strip_prefix("cf.") {
            let cf_map = match e {
                Entity::Project => self.projects.get(&id).map(|p| &p.cf),
                Entity::Task => self.tasks.get(&id).map(|t| &t.cf),
                _ => None,
            };
            return cf_map.and_then(|m| m.get(cf)).cloned().unwrap_or(Value::Null);
        }
        let row = match e {
            Entity::User => self.users.get(&id).and_then(|r| serde_json::to_value(r).ok()),
            Entity::Program => self.programs.get(&id).and_then(|r| serde_json::to_value(r).ok()),
            Entity::Template => self.templates.get(&id).and_then(|r| serde_json::to_value(r).ok()),
            Entity::TemplateTask => self.template_tasks.get(&id).and_then(|r| serde_json::to_value(r).ok()),
            Entity::Project => self.projects.get(&id).and_then(|r| serde_json::to_value(r).ok()),
            Entity::Task => self.tasks.get(&id).and_then(|r| serde_json::to_value(r).ok()),
            Entity::CustomField => self.custom_fields.get(&id).and_then(|r| serde_json::to_value(r).ok()),
        };
        row.and_then(|r| r.get(field).cloned()).unwrap_or(Value::Null)
    }

    pub fn exists(&self, e: Entity, id: i64) -> bool {
        match e {
            Entity::User => self.users.contains_key(&id),
            Entity::Program => self.programs.contains_key(&id),
            Entity::Template => self.templates.contains_key(&id),
            Entity::TemplateTask => self.template_tasks.contains_key(&id),
            Entity::Project => self.projects.contains_key(&id),
            Entity::Task => self.tasks.contains_key(&id),
            Entity::CustomField => self.custom_fields.contains_key(&id),
        }
    }

    /// Short label for a row in toasts and the activity view.
    pub fn row_label(&self, e: Entity, id: i64) -> String {
        match e {
            Entity::Project => self.projects.get(&id).map_or(format!("project {id}"), |p| p.code.clone()),
            Entity::Task => self.tasks.get(&id).map_or(format!("task {id}"), |t| self.task_ref(t)),
            Entity::User => self.users.get(&id).map_or(format!("user {id}"), |u| u.username.clone()),
            Entity::Program => self.programs.get(&id).map_or(format!("program {id}"), |p| p.name.clone()),
            Entity::Template => self.templates.get(&id).map_or(format!("template {id}"), |t| t.name.clone()),
            Entity::TemplateTask => format!("template task {id}"),
            Entity::CustomField => self.custom_fields.get(&id).map_or(format!("field {id}"), |c| c.name.clone()),
        }
    }

    /// Other connections (not ours) and where they are.
    pub fn other_peers(&self) -> impl Iterator<Item = &Peer> {
        self.peers.iter().filter(move |p| p.conn != self.conn)
    }

    /// Distinct online users (including me), in a stable order.
    pub fn online_users(&self) -> Vec<&User> {
        let mut ids: Vec<i64> = self.peers.iter().map(|p| p.user_id).collect();
        ids.sort();
        ids.dedup();
        ids.iter().filter_map(|id| self.users.get(id)).collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::protocol::Changes;

    pub fn sample() -> Replica {
        let today = Date::parse_iso("2026-10-17").unwrap();
        let mut r = Replica::new(today);
        let u = |id, name: &str, disp: &str| User {
            id,
            username: name.into(),
            display_name: disp.into(),
            role: Role::Member,
            active: true,
        };
        let t = |id, pid, seq, title: &str, status, due: Option<&str>, est, act, assignee| Task {
            id,
            project_id: pid,
            seq,
            title: title.into(),
            phase: "Build".into(),
            status,
            priority: Priority::Medium,
            assignee_id: assignee,
            start_date: None,
            due_date: due.map(String::from),
            est_hours: est,
            actual_hours: act,
            deps: vec![],
            tags: vec![],
            notes: String::new(),
            cf: Default::default(),
            created_at: String::new(),
            updated_at: String::new(),
            updated_by: None,
        };
        r.me = Some(u(1, "alice", "Alice Nguyen"));
        r.load(Tables {
            users: vec![u(1, "alice", "Alice Nguyen"), u(2, "bob", "Bob Stone")],
            programs: vec![Program {
                id: 1,
                name: "Northwind Retail".into(),
                client: "Northwind".into(),
                prefix: "NWR".into(),
                color: "#4f9dde".into(),
            }],
            projects: vec![Project {
                id: 10,
                code: "NWR-004".into(),
                name: "POS refresh".into(),
                program_id: Some(1),
                template_id: None,
                owner_id: Some(1),
                status: ProjectStatus::Active,
                priority: Priority::High,
                health: Health::Amber,
                start_date: Some("2026-09-01".into()),
                due_date: Some("2026-12-15".into()),
                budget_hours: Some(100.0),
                tags: vec!["retail".into()],
                notes: String::new(),
                cf: Default::default(),
                created_at: String::new(),
                updated_at: String::new(),
                updated_by: None,
            }],
            tasks: vec![
                t(100, 10, 1, "Kickoff", TaskStatus::Done, Some("2026-09-02"), Some(4.0), 5.0, Some(1)),
                t(101, 10, 2, "Load test checkout", TaskStatus::InProgress, Some("2026-10-10"), Some(16.0), 9.5, Some(2)),
                t(102, 10, 3, "Go live", TaskStatus::Todo, Some("2026-12-01"), None, 0.0, None),
            ],
            ..Default::default()
        });
        r
    }

    #[test]
    fn metrics_and_lookups() {
        let r = sample();
        let m = r.metrics(10);
        assert_eq!((m.tasks_total, m.tasks_done, m.tasks_overdue), (3, 1, 1));
        assert_eq!(m.est_total, 20.0);
        assert_eq!(m.actual_total, 14.5);
        assert!((r.burn_pct(&r.projects[&10]).unwrap() - 14.5).abs() < 1e-9);
        assert_eq!(r.project_tasks(10), &[100, 101, 102]);
        assert_eq!(r.next_code(Some(1)), "NWR-005");
        assert_eq!(r.field_value(Entity::Task, 101, "status"), serde_json::json!("in_progress"));
        assert_eq!(r.task_ref(&r.tasks[&102]), "NWR-004 #3");
    }

    #[test]
    fn changes_apply() {
        let mut r = sample();
        let mut t = r.tasks[&102].clone();
        t.status = TaskStatus::Done;
        let c = Changes {
            by: Some(2),
            group: Some(1),
            upserts: Tables { tasks: vec![t], ..Default::default() },
            deletes: Deletes { tasks: vec![100], ..Default::default() },
            activity: vec![],
        };
        let a = r.apply(&ServerMsg::Changes(c));
        assert_eq!(a.upserted, vec![(Entity::Task, 102)]);
        assert_eq!(a.deleted, vec![(Entity::Task, 100)]);
        let m = r.metrics(10);
        assert_eq!((m.tasks_total, m.tasks_done), (2, 1));
    }
}
