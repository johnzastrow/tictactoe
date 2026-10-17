//! Test fixtures: a small but varied replica for render and behaviour tests.

use pt_core::date::Date;
use pt_core::model::*;
use pt_core::protocol::{ServerInfo, ServerMsg};
use pt_core::replica::Replica;
use serde_json::json;

pub fn today() -> Date {
    Date::parse_iso("2026-10-17").unwrap()
}

fn user(id: i64, name: &str, disp: &str, role: Role) -> User {
    User { id, username: name.into(), display_name: disp.into(), role, active: true }
}

#[allow(clippy::too_many_arguments)]
fn task(id: i64, pid: i64, seq: i64, title: &str, phase: &str, status: TaskStatus, start: &str, due: &str, est: Option<f64>, act: f64, who: Option<i64>) -> Task {
    Task {
        id,
        project_id: pid,
        seq,
        title: title.into(),
        phase: phase.into(),
        status,
        priority: Priority::Medium,
        assignee_id: who,
        start_date: Some(start.into()),
        due_date: Some(due.into()),
        est_hours: est,
        actual_hours: act,
        deps: vec![],
        tags: vec![],
        notes: String::new(),
        cf: Default::default(),
        created_at: "2026-09-01T00:00:00Z".into(),
        updated_at: "2026-10-01T00:00:00Z".into(),
        updated_by: Some(1),
    }
}

#[allow(clippy::too_many_arguments)]
fn project(id: i64, code: &str, name: &str, program: i64, owner: i64, status: ProjectStatus, health: Health, start: &str, due: &str, budget: f64) -> Project {
    Project {
        id,
        code: code.into(),
        name: name.into(),
        program_id: Some(program),
        template_id: Some(1),
        owner_id: Some(owner),
        status,
        priority: Priority::High,
        health,
        start_date: Some(start.into()),
        due_date: Some(due.into()),
        budget_hours: Some(budget),
        tags: vec!["retail".into()],
        notes: "Sponsor: Northwind.".into(),
        cf: [("1".to_string(), json!("PO-4521"))].into_iter().collect(),
        created_at: "2026-09-01T00:00:00Z".into(),
        updated_at: "2026-10-01T00:00:00Z".into(),
        updated_by: Some(1),
    }
}

/// A loaded replica logged in as alice (member).
pub fn sample() -> Replica {
    let mut r = Replica::new(today());
    r.apply(&ServerMsg::Welcome {
        protocol: 1,
        server: ServerInfo { implementation: "rust".into(), version: "0.4.0".into() },
        me: user(2, "alice", "Alice Nguyen", Role::Member),
        conn: 1,
    });
    let mut t3 = task(102, 10, 3, "Go live", "Release", TaskStatus::Todo, "2026-11-20", "2026-12-01", None, 0.0, None);
    t3.deps = vec![101];
    let mut t2 = task(101, 10, 2, "Load test checkout", "Test", TaskStatus::InProgress, "2026-10-05", "2026-10-10", Some(16.0), 9.5, Some(3));
    t2.tags = vec!["perf".into()];
    r.load(Tables {
        users: vec![
            user(1, "admin", "Ada Admin", Role::Admin),
            user(2, "alice", "Alice Nguyen", Role::Member),
            user(3, "bob", "Bob Stone", Role::Member),
            user(4, "victor", "Victor Viewer", Role::Viewer),
        ],
        programs: vec![
            Program { id: 1, name: "Northwind Retail".into(), client: "Northwind Traders".into(), prefix: "NWR".into(), color: "#4f9dde".into() },
            Program { id: 2, name: "Municipal GIS".into(), client: "City of Riverton".into(), prefix: "GIS".into(), color: "#3fb68b".into() },
        ],
        templates: vec![Template { id: 1, name: "Standard Project".into(), description: "General".into() }],
        template_tasks: vec![
            TemplateTask { id: 1, template_id: 1, seq: 1, title: "Kickoff".into(), phase: "Initiate".into(), est_hours: Some(4.0), offset_days: 0, duration_days: 1 },
            TemplateTask { id: 2, template_id: 1, seq: 2, title: "Build".into(), phase: "Execute".into(), est_hours: Some(40.0), offset_days: 1, duration_days: 20 },
        ],
        projects: vec![
            project(10, "NWR-004", "POS refresh", 1, 2, ProjectStatus::Active, Health::Amber, "2026-09-01", "2026-12-15", 100.0),
            project(11, "GIS-001", "Parcel fabric migration", 2, 3, ProjectStatus::Planning, Health::Green, "2026-11-02", "2027-01-15", 300.0),
        ],
        tasks: vec![
            task(100, 10, 1, "Kickoff", "Initiate", TaskStatus::Done, "2026-09-01", "2026-09-02", Some(4.0), 5.0, Some(2)),
            t2,
            t3,
            task(110, 11, 1, "Define area of interest", "Scope", TaskStatus::Todo, "2026-11-02", "2026-11-03", Some(4.0), 0.0, Some(3)),
            task(111, 11, 2, "Field survey", "Collect", TaskStatus::Blocked, "2026-11-04", "2026-11-20", Some(40.0), 2.0, Some(2)),
        ],
        custom_fields: vec![
            CustomField { id: 1, entity: CfEntity::Project, name: "Contract #".into(), kind: CfKind::Text, options: vec![], seq: 1 },
            CustomField { id: 4, entity: CfEntity::Task, name: "Billable".into(), kind: CfKind::Enum, options: vec!["Yes".into(), "No".into()], seq: 4 },
        ],
        activity: vec![Activity {
            id: 1,
            ts: "2026-10-17T09:30:00Z".into(),
            user_id: Some(3),
            group: 1,
            action: Action::Set,
            entity: "task".into(),
            entity_id: 101,
            field: "status".into(),
            old: json!("todo"),
            new: json!("in_progress"),
            label: "NWR-004 #2 status: To do → In progress".into(),
            state: ActState::Done,
        }],
    });
    r
}
