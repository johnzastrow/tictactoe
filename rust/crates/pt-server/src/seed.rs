//! `pt seed`: a realistic demo dataset relative to today — 10 users,
//! 8 programs, 5 templates, 50 projects with 10–40 tasks each, custom
//! fields, and 30 days of synthetic activity for the dashboard sparkline.
//!
//! Randomness here is a seeded SplitMix64 so every run produces the same
//! data (not security-relevant). Passwords come from the caller.

use pt_core::date::{Date, rfc3339};
use pt_core::model::*;
use rusqlite::params;

use crate::store::{
    Store, insert_custom_field, insert_dep, insert_program, insert_project, insert_task, insert_template,
    insert_template_task, insert_user, now_secs, now_ts, today,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
    fn chance(&mut self, p: f64) -> bool {
        self.f() < p
    }
}

const USERS: &[(&str, &str, Role)] = &[
    ("admin", "Ada Admin", Role::Admin),
    ("alice", "Alice Nguyen", Role::Member),
    ("bob", "Bob Stone", Role::Member),
    ("carol", "Carol Diaz", Role::Member),
    ("dave", "Dave Okafor", Role::Member),
    ("erin", "Erin Walsh", Role::Member),
    ("frank", "Frank Moreau", Role::Member),
    ("grace", "Grace Kim", Role::Member),
    ("heidi", "Heidi Larsen", Role::Member),
    ("victor", "Victor Viewer", Role::Viewer),
];

const PROGRAMS: &[(&str, &str, &str, &str, &[&str])] = &[
    ("Northwind Retail", "Northwind Traders", "NWR", "#4f9dde", &[
        "POS refresh", "Loyalty app v2", "Store Wi-Fi upgrade", "Inventory RFID pilot", "Online returns portal",
        "Holiday campaign site", "Supplier EDI onboarding",
    ]),
    ("Contoso Health", "Contoso Medical Group", "CTH", "#e05d6f", &[
        "Patient portal redesign", "EHR data migration", "Telehealth rollout", "HIPAA audit prep", "Lab results API",
        "Clinic scheduling",
    ]),
    ("Fabrikam Energy", "Fabrikam Power & Light", "FAB", "#f2a541", &[
        "Smart meter analytics", "Outage map", "Substation GIS update", "Field crew mobile app", "Rate case data pack",
        "Solar interconnect queue",
    ]),
    ("Municipal GIS", "City of Riverton", "GIS", "#3fb68b", &[
        "Parcel fabric migration", "Street centerline QA", "Storm drain inventory", "Zoning web map",
        "911 address points", "LiDAR canopy study", "Snow route optimizer",
    ]),
    ("Internal IT", "Internal", "ITX", "#9b7ede", &[
        "Laptop refresh", "SSO rollout", "Backup modernization", "Ticketing migration", "Network segmentation",
        "Endpoint EDR",
    ]),
    ("Research & Data", "University Partners", "RND", "#2bb5c9", &[
        "Survey data warehouse", "Grant reporting dashboard", "Research data catalog", "ML forecasting pilot",
        "Open data portal", "Sensor network ingest",
    ]),
    ("Facilities", "Internal", "FAC", "#b08968", &[
        "HVAC controls upgrade", "Office move: floor 3", "EV chargers install", "Badge access upgrade",
        "Space utilization study",
    ]),
    ("Marketing", "Internal", "MKT", "#e47fc4", &[
        "Website relaunch", "Annual conference", "Brand refresh", "Customer webinar series", "Partner summit",
        "Case study program", "Q4 product launch",
    ]),
];

// (title, phase, est hours, offset days, duration days)
type TT = (&'static str, &'static str, f64, i64, i64);

const TEMPLATES: &[(&str, &str, &[TT])] = &[
    ("Standard Project", "General-purpose lifecycle: initiate, plan, execute, close.", &[
        ("Kickoff meeting", "Initiate", 4.0, 0, 1),
        ("Stakeholder map", "Initiate", 6.0, 1, 3),
        ("Project charter", "Initiate", 8.0, 2, 4),
        ("Requirements workshop", "Plan", 12.0, 6, 5),
        ("Schedule & budget", "Plan", 10.0, 8, 4),
        ("Risk register", "Plan", 6.0, 10, 3),
        ("Deliver the work", "Execute", 80.0, 14, 30),
        ("Status reporting", "Execute", 16.0, 14, 40),
        ("Quality review", "Execute", 16.0, 40, 7),
        ("Handover", "Close", 8.0, 48, 4),
        ("Lessons learned", "Close", 4.0, 52, 2),
        ("Closeout report", "Close", 6.0, 53, 3),
    ]),
    ("Software Release", "Discovery to hypercare for an app or service release.", &[
        ("Backlog grooming", "Discover", 8.0, 0, 3),
        ("UX wireframes", "Design", 20.0, 3, 7),
        ("API design", "Design", 16.0, 4, 6),
        ("Sprint 1", "Build", 60.0, 10, 14),
        ("Sprint 2", "Build", 60.0, 24, 14),
        ("Sprint 3", "Build", 60.0, 38, 14),
        ("Integration tests", "Test", 24.0, 50, 7),
        ("Load test", "Test", 16.0, 54, 4),
        ("Security review", "Test", 12.0, 54, 5),
        ("User acceptance testing", "Test", 20.0, 58, 6),
        ("Release notes", "Release", 4.0, 63, 2),
        ("Go-live", "Release", 8.0, 65, 1),
        ("Hypercare", "Release", 16.0, 66, 10),
    ]),
    ("Data Migration", "Move data between systems with reconciliation.", &[
        ("Source inventory", "Assess", 10.0, 0, 5),
        ("Data profiling", "Assess", 16.0, 4, 6),
        ("Field mapping", "Map", 24.0, 10, 8),
        ("Transform scripts", "Map", 40.0, 16, 12),
        ("Trial load", "Migrate", 16.0, 28, 4),
        ("Reconcile counts", "Validate", 12.0, 32, 4),
        ("Cutover plan", "Migrate", 8.0, 34, 3),
        ("Final load", "Migrate", 12.0, 40, 2),
        ("Business sign-off", "Validate", 4.0, 42, 3),
    ]),
    ("GIS Mapping Project", "Collect, process and publish spatial data.", &[
        ("Define area of interest", "Scope", 4.0, 0, 2),
        ("Source imagery", "Collect", 12.0, 2, 6),
        ("Field survey", "Collect", 40.0, 6, 15),
        ("Digitize features", "Process", 60.0, 14, 20),
        ("Topology QA", "Process", 16.0, 32, 5),
        ("Attribute QA", "Process", 12.0, 34, 5),
        ("Symbology & labels", "Publish", 8.0, 38, 4),
        ("Publish web map", "Publish", 8.0, 42, 2),
        ("Metadata", "Publish", 6.0, 42, 3),
        ("User training", "Publish", 6.0, 45, 2),
    ]),
    ("Event", "Plan, promote and run an event.", &[
        ("Venue booking", "Plan", 6.0, 0, 5),
        ("Event budget", "Plan", 6.0, 2, 4),
        ("Speaker outreach", "Plan", 16.0, 5, 15),
        ("Agenda", "Plan", 8.0, 15, 6),
        ("Registration site", "Promote", 12.0, 12, 6),
        ("Email campaign", "Promote", 10.0, 20, 15),
        ("Social posts", "Promote", 8.0, 22, 20),
        ("AV check", "Run", 4.0, 40, 1),
        ("Run of show", "Run", 6.0, 38, 3),
        ("Event day", "Run", 24.0, 42, 1),
        ("Attendee survey", "Wrap", 4.0, 43, 5),
        ("Wrap-up report", "Wrap", 6.0, 46, 4),
    ]),
];

const AD_HOC: &[&str] = &[
    "Vendor follow-up", "Fix review comments", "Update documentation", "Demo to sponsor", "Data cleanup",
    "Performance tuning", "Accessibility pass", "Budget re-forecast", "Procure licenses", "Training session",
    "Bug triage", "Steering committee prep", "Risk mitigation plan", "Test environment setup", "Change request",
    "Security exception review", "Update project plan", "Customer interview", "Draft FAQ", "Escalation call",
];

const TAGS: &[&str] = &[
    "q4", "compliance", "customer-facing", "pilot", "infra", "mobile", "data", "gis", "security", "high-visibility",
    "vendor", "quick-win",
];

fn template_for(name: &str, program: &str) -> usize {
    let n = name.to_lowercase();
    if program == "Municipal GIS" || n.contains("gis") || n.contains("map") || n.contains("lidar") {
        3
    } else if n.contains("migration") || n.contains("warehouse") || n.contains("ingest") || n.contains("catalog") {
        2
    } else if n.contains("conference") || n.contains("summit") || n.contains("webinar") || n.contains("launch") {
        4
    } else if n.contains("app") || n.contains("portal") || n.contains("api") || n.contains("site") || n.contains("sso")
        || n.contains("dashboard") || n.contains("relaunch")
    {
        1
    } else {
        0
    }
}

pub struct SeedReport {
    pub users: usize,
    pub projects: usize,
    pub tasks: usize,
}

/// Fill an empty database. `password` is set for every seeded user.
pub fn seed(store: &mut Store, password: &str) -> anyhow::Result<SeedReport> {
    if !store.is_empty()? {
        anyhow::bail!("the database already has data (use --force to wipe it first)");
    }
    let today = today();
    let now = now_ts();
    let mut rng = Rng(42);
    let c = store.conn().transaction()?;

    // users (one hash each: every user gets their own salt)
    let mut user_ids = Vec::new();
    for (name, disp, role) in USERS {
        let hash = crate::auth::hash_password(password)?;
        let u = User { id: 0, username: name.to_string(), display_name: disp.to_string(), role: *role, active: true };
        user_ids.push(insert_user(&c, &u, &hash, &now)?);
    }
    let members: Vec<i64> = user_ids[1..9].to_vec();

    // custom fields
    let cf = |entity, name: &str, kind, options: &[&str], seq| CustomField {
        id: 0,
        entity,
        name: name.into(),
        kind,
        options: options.iter().map(|s| s.to_string()).collect(),
        seq,
    };
    let cf_contract = insert_custom_field(&c, &cf(CfEntity::Project, "Contract #", CfKind::Text, &[], 1))?;
    let cf_risk = insert_custom_field(&c, &cf(CfEntity::Project, "Risk score", CfKind::Number, &[], 2))?;
    let cf_region = insert_custom_field(&c, &cf(CfEntity::Project, "Region", CfKind::Enum, &["North", "South", "East", "West"], 3))?;
    let cf_billable = insert_custom_field(&c, &cf(CfEntity::Task, "Billable", CfKind::Enum, &["Yes", "No"], 4))?;
    let cf_ticket = insert_custom_field(&c, &cf(CfEntity::Task, "Ticket", CfKind::Text, &[], 5))?;

    // templates
    let mut templates: Vec<(i64, Vec<TemplateTask>)> = Vec::new();
    for (name, desc, tasks) in TEMPLATES {
        let tid = insert_template(&c, &Template { id: 0, name: name.to_string(), description: desc.to_string() })?;
        let mut tts = Vec::new();
        for (i, (title, phase, est, off, dur)) in tasks.iter().enumerate() {
            let mut tt = TemplateTask {
                id: 0,
                template_id: tid,
                seq: i as i64 + 1,
                title: title.to_string(),
                phase: phase.to_string(),
                est_hours: Some(*est),
                offset_days: *off,
                duration_days: *dur,
            };
            tt.id = insert_template_task(&c, &tt)?;
            tts.push(tt);
        }
        templates.push((tid, tts));
    }

    let mut n_projects = 0;
    let mut n_tasks = 0;
    let mut task_refs: Vec<(String, i64, String)> = Vec::new(); // for synthetic activity
    for (pi, (pname, client, prefix, color, names)) in PROGRAMS.iter().enumerate() {
        let program_id = insert_program(&c, &Program {
            id: 0,
            name: pname.to_string(),
            client: client.to_string(),
            prefix: prefix.to_string(),
            color: color.to_string(),
        })?;
        for (k, name) in names.iter().enumerate() {
            let tix = template_for(name, pname);
            let (tid, tts) = &templates[tix];
            let span = tts.iter().map(|t| t.offset_days + t.duration_days).max().unwrap_or(30);
            // where in its life the project is: <0 not started, >1 finished
            let phase_roll = rng.f();
            let progress = if phase_roll < 0.12 {
                -rng.f() * 0.5
            } else if phase_roll < 0.25 {
                1.0 + rng.f() * 0.6
            } else {
                rng.f() * 0.95
            };
            let start = today.add_days(-(progress * span as f64).round() as i32);
            let stretch = 1.0 + rng.f() * 0.25;
            let due = start.add_days((span as f64 * stretch) as i32);
            let status = if progress < 0.0 {
                ProjectStatus::Planning
            } else if progress > 1.0 {
                if rng.chance(0.15) { ProjectStatus::Cancelled } else { ProjectStatus::Done }
            } else if rng.chance(0.08) {
                ProjectStatus::OnHold
            } else {
                ProjectStatus::Active
            };
            let owner = *rng.pick(&members);
            let est_total: f64 = tts.iter().map(|t| t.est_hours.unwrap_or(0.0)).sum();
            let mut tags: Vec<String> = Vec::new();
            for _ in 0..rng.range(0, 3) {
                let t = rng.pick(TAGS).to_string();
                if !tags.contains(&t) {
                    tags.push(t);
                }
            }
            tags.sort();
            let mut cfv = CfValues::new();
            cfv.insert(cf_contract.to_string(), format!("PO-{}", 4000 + rng.below(6000)).into());
            cfv.insert(cf_risk.to_string(), serde_json::Value::from(rng.range(1, 10)));
            cfv.insert(cf_region.to_string(), (*rng.pick(&["North", "South", "East", "West"])).into());
            let priority = *rng.pick(&[Priority::Low, Priority::Medium, Priority::Medium, Priority::High, Priority::Critical]);
            let code = format!("{prefix}-{:03}", k + 1);
            let p = Project {
                id: 0,
                code: code.clone(),
                name: name.to_string(),
                program_id: Some(program_id),
                template_id: Some(*tid),
                owner_id: Some(owner),
                status,
                priority,
                health: Health::Green,
                start_date: Some(start.iso()),
                due_date: Some(due.iso()),
                budget_hours: Some((est_total * (1.0 + rng.f() * 0.35) / 5.0).round() * 5.0),
                tags,
                notes: if rng.chance(0.4) {
                    format!("Sponsor: {}.\nKey risk: {}.", client, rng.pick(&["vendor delays", "scope creep", "data quality", "staff availability", "budget approval"]))
                } else {
                    String::new()
                },
                cf: cfv,
                created_at: rfc3339(now_secs() - 86400 * (60 + pi as i64)),
                updated_at: now.clone(),
                updated_by: Some(owner),
            };
            let pid = insert_project(&c, &p)?;
            n_projects += 1;

            // template tasks + ad hoc extras
            let mut tasks: Vec<(i64, String, String)> = Vec::new(); // (id, phase, title)
            let mut phase_last: Vec<(String, i64)> = Vec::new();
            let mut seq = 0;
            let mut total_actual = 0.0;
            let mut total_overdue = 0;
            let extra = rng.range(0, 14) as usize;
            let mut plan: Vec<(String, String, f64, i64, i64)> = tts
                .iter()
                .map(|t| (t.title.clone(), t.phase.clone(), t.est_hours.unwrap_or(4.0), t.offset_days, t.duration_days))
                .collect();
            for _ in 0..extra {
                let tt = rng.pick(tts);
                let mut title = rng.pick(AD_HOC).to_string();
                if title == "Change request" {
                    title = format!("Change request #{}", rng.range(1, 40));
                }
                plan.push((title, tt.phase.clone(), [2.0, 4.0, 6.0, 8.0, 12.0, 16.0][rng.below(6) as usize], tt.offset_days + rng.range(0, 5), rng.range(1, 6)));
            }
            plan.sort_by_key(|p| p.3);
            for (title, phase, est, off, dur) in plan {
                seq += 1;
                let s = start.add_days(off as i32);
                let d = s.add_days(dur.max(1) as i32 - 1);
                let finished_project = status == ProjectStatus::Done;
                let t_status = if finished_project || (d < today && rng.chance(0.85)) {
                    TaskStatus::Done
                } else if s <= today {
                    *rng.pick(&[TaskStatus::InProgress, TaskStatus::InProgress, TaskStatus::Review, TaskStatus::Blocked, TaskStatus::Todo])
                } else {
                    TaskStatus::Todo
                };
                let actual = match t_status {
                    TaskStatus::Done => (est * (0.6 + rng.f() * 0.9) * 4.0).round() / 4.0,
                    TaskStatus::Todo => 0.0,
                    _ => (est * rng.f() * 0.9 * 4.0).round() / 4.0,
                };
                total_actual += actual;
                if t_status != TaskStatus::Done && d < today {
                    total_overdue += 1;
                }
                let mut tcf = CfValues::new();
                if rng.chance(0.7) {
                    tcf.insert(cf_billable.to_string(), (if rng.chance(0.75) { "Yes" } else { "No" }).into());
                }
                if rng.chance(0.2) {
                    tcf.insert(cf_ticket.to_string(), format!("{prefix}-{}", rng.range(100, 999)).into());
                }
                let t = Task {
                    id: 0,
                    project_id: pid,
                    seq,
                    title: title.clone(),
                    phase: phase.clone(),
                    status: t_status,
                    priority: *rng.pick(&[Priority::Low, Priority::Medium, Priority::Medium, Priority::Medium, Priority::High, Priority::Critical]),
                    assignee_id: if rng.chance(0.08) { None } else if rng.chance(0.35) { Some(owner) } else { Some(*rng.pick(&members)) },
                    start_date: Some(s.iso()),
                    due_date: Some(d.iso()),
                    est_hours: if rng.chance(0.05) { None } else { Some(est) },
                    actual_hours: actual,
                    deps: vec![],
                    tags: if rng.chance(0.2) { vec![rng.pick(TAGS).to_string()] } else { vec![] },
                    notes: if rng.chance(0.15) { "Waiting on confirmation from the client.".into() } else { String::new() },
                    cf: tcf,
                    created_at: p.created_at.clone(),
                    updated_at: now.clone(),
                    updated_by: Some(owner),
                };
                let id = insert_task(&c, &t)?;
                n_tasks += 1;
                match phase_last.iter().position(|(ph, _)| *ph == phase) {
                    Some(i) => phase_last[i].1 = id,
                    None => {
                        if let Some((_, prev)) = phase_last.last() {
                            insert_dep(&c, id, *prev)?;
                        }
                        phase_last.push((phase.clone(), id));
                    }
                }
                tasks.push((id, phase, title.clone()));
                task_refs.push((format!("{code} #{seq}"), id, title));
            }
            // a few extra deps inside the project
            for _ in 0..rng.range(0, 3) {
                let a = rng.below(tasks.len() as u64) as usize;
                let b = rng.below(tasks.len() as u64) as usize;
                if a > b {
                    insert_dep(&c, tasks[a].0, tasks[b].0)?;
                }
            }
            // health from the numbers, with some noise
            let burn = total_actual / p.budget_hours.unwrap_or(1.0);
            let health = if total_overdue > 3 || burn > 1.05 {
                Health::Red
            } else if total_overdue > 0 || burn > 0.9 || rng.chance(0.1) {
                Health::Amber
            } else {
                Health::Green
            };
            c.execute("UPDATE projects SET health = ?1 WHERE id = ?2", params![health.as_str(), pid])?;
        }
    }

    // 30 days of synthetic history (not undoable)
    let states = ["To do → In progress", "In progress → Review", "Review → Done", "In progress → Blocked", "Blocked → In progress"];
    let mut grp = 0;
    let base = now_secs() - 30 * 86400;
    for day in 0..30 {
        let weekday = Date::from_days(((base + day * 86400) / 86400) as i32).weekday();
        let n = if weekday >= 5 { rng.range(0, 4) } else { rng.range(8, 30) };
        let mut ts: Vec<i64> = (0..n).map(|_| base + day * 86400 + 8 * 3600 + rng.range(0, 10 * 3600)).collect();
        ts.sort();
        for t in ts {
            grp += 1;
            let (r, id, _) = &task_refs[rng.below(task_refs.len() as u64) as usize];
            let who = *rng.pick(&members);
            let (field, label) = if rng.chance(0.6) {
                ("status", format!("{r} status: {}", rng.pick(&states)))
            } else {
                ("actual_hours", format!("{r} actual h: {} → {}", rng.range(0, 20), rng.range(20, 40)))
            };
            c.execute(
                "INSERT INTO activity (ts, user_id, grp, action, entity, entity_id, field, old, new, label, state)
                 VALUES (?1, ?2, ?3, 'set', 'task', ?4, ?5, NULL, NULL, ?6, 'dead')",
                params![rfc3339(t), who, grp, id, field, label],
            )?;
        }
    }
    c.commit()?;
    let _ = today;
    Ok(SeedReport { users: USERS.len(), projects: n_projects, tasks: n_tasks })
}

/// Wipe all data (for `pt seed --force`).
pub fn wipe(store: &mut Store) -> anyhow::Result<()> {
    store.conn().execute_batch(
        "PRAGMA foreign_keys = OFF;
         DELETE FROM activity; DELETE FROM custom_values; DELETE FROM custom_fields; DELETE FROM task_tags;
         DELETE FROM project_tags; DELETE FROM task_deps; DELETE FROM tasks; DELETE FROM projects;
         DELETE FROM template_tasks; DELETE FROM templates; DELETE FROM programs; DELETE FROM sessions;
         DELETE FROM users; DELETE FROM sqlite_sequence;
         PRAGMA foreign_keys = ON;",
    )?;
    Ok(())
}
