//! Dashboard (ui-spec §4.5): KPI tiles, status bars, hours by owner,
//! workload heatmap, activity sparkline and at-risk projects. Every tile and
//! most marks drill down into a filtered grid.

use std::collections::BTreeMap;

use leptos::prelude::*;
use pt_core::date::Date;
use pt_core::grid::fmt_hours;
use pt_core::model::*;
use wasm_bindgen::JsCast;

use crate::app::{App, Ctx, ViewId, use_app};
use crate::cells::avatar;
use crate::util;

fn drill(app: App, view: ViewId, q: &str) {
    match view {
        ViewId::Projects => app.q.projects.set(q.to_string()),
        ViewId::Tasks => app.q.tasks.set(q.to_string()),
        _ => {}
    }
    app.set_view(view);
}

#[derive(Clone, PartialEq)]
struct Kpi {
    label: &'static str,
    value: String,
    sub: String,
    level: &'static str,
    view: ViewId,
    q: String,
    gauge: Option<f64>,
}

#[component]
pub fn DashboardView() -> impl IntoView {
    let app = use_app();
    let on_key = move |e: web_sys::KeyboardEvent| {
        let k = e.key();
        if !matches!(
            k.as_str(),
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Enter"
        ) {
            return;
        }
        let Ok(list) = util::document().query_selector_all("#dashboard .tile") else {
            return;
        };
        let tiles: Vec<web_sys::HtmlElement> = (0..list.length())
            .filter_map(|i| list.item(i))
            .filter_map(|n| n.dyn_into().ok())
            .collect();
        let active = util::active_element();
        let cur = tiles
            .iter()
            .position(|t| active.as_ref().is_some_and(|a| t.is_same_node(Some(a))));
        if k == "Enter" {
            if let Some(i) = cur {
                tiles[i].click();
                e.prevent_default();
            }
            return;
        }
        let n = tiles.len();
        let next = match (k.as_str(), cur) {
            (_, None) => 0,
            ("ArrowLeft" | "ArrowUp", Some(i)) => (i + n - 1) % n,
            (_, Some(i)) => (i + 1) % n,
        };
        if let Some(t) = tiles.get(next) {
            let _ = t.focus();
        }
        e.prevent_default();
        e.stop_propagation();
    };
    view! {
        <div class="dash" id="dashboard" tabindex="-1" data-pane="1" on:focusin=move |_| app.ctx.set(Ctx::Dashboard) on:keydown=on_key>
            <Kpis />
            <div class="dgrid">
                <ProjectsByStatus />
                <TasksByStatus />
                <HoursByOwner />
                <Workload />
                <ActivityTile />
                <AtRisk />
            </div>
        </div>
    }
}

#[component]
fn Kpis() -> impl IntoView {
    let app = use_app();
    let kpis = Memo::new(move |_| {
        app.rep.with(|r| {
            let active = r
                .projects
                .values()
                .filter(|p| p.status == ProjectStatus::Active)
                .count();
            let late_p = r.projects.values().filter(|p| r.project_overdue(p)).count();
            let overdue = r.tasks.values().filter(|t| r.task_overdue(t)).count();
            let blocked = r.tasks.values().filter(|t| r.task_blocked(t)).count();
            let open: Vec<&Project> = r.projects.values().filter(|p| r.project_open(p)).collect();
            let budget: f64 = open.iter().filter_map(|p| p.budget_hours).sum();
            let used: f64 = open.iter().map(|p| r.metrics(p.id).actual_total).sum();
            let eow = r
                .today
                .add_days((4 - r.today.weekday() as i32).rem_euclid(7));
            let due_week = r
                .tasks
                .values()
                .filter(|t| t.status != TaskStatus::Done)
                .filter(|t| {
                    t.due_date
                        .as_deref()
                        .and_then(Date::parse_iso)
                        .is_some_and(|d| d >= r.today && d <= eow)
                })
                .count();
            let gauge = if budget > 0.0 {
                used / budget * 100.0
            } else {
                0.0
            };
            vec![
                Kpi {
                    label: "Active projects",
                    value: active.to_string(),
                    sub: format!("{} total · {late_p} late", r.projects.len()),
                    level: "info",
                    view: ViewId::Projects,
                    q: "status:active".into(),
                    gauge: None,
                },
                Kpi {
                    label: "Overdue tasks",
                    value: overdue.to_string(),
                    sub: "past due, not done".into(),
                    level: if overdue > 0 { "err" } else { "ok" },
                    view: ViewId::Tasks,
                    q: "is:overdue".into(),
                    gauge: None,
                },
                Kpi {
                    label: "Blocked tasks",
                    value: blocked.to_string(),
                    sub: "blocked or waiting on deps".into(),
                    level: if blocked > 0 { "warn" } else { "ok" },
                    view: ViewId::Tasks,
                    q: "is:blocked".into(),
                    gauge: None,
                },
                Kpi {
                    label: "Hours used / budget",
                    value: format!(
                        "{} / {}",
                        fmt_hours(used.round()),
                        fmt_hours(budget.round())
                    ),
                    sub: format!("{gauge:.0}% of budget on open projects"),
                    level: if gauge > 100.0 {
                        "err"
                    } else if gauge > 85.0 {
                        "warn"
                    } else {
                        "ok"
                    },
                    view: ViewId::Projects,
                    q: "is:open has:budget".into(),
                    gauge: Some(gauge),
                },
                Kpi {
                    label: "Due this week",
                    value: due_week.to_string(),
                    sub: format!("through {}", eow.long()),
                    level: "info",
                    view: ViewId::Tasks,
                    q: "is:open due>=today due<=eow".into(),
                    gauge: None,
                },
            ]
        })
    });
    view! {
        <div class="kpis">
            {move || kpis.get().into_iter().map(|k| {
                let (v, q) = (k.view, k.q.clone());
                view! {
                    <div class=format!("tile kpi {}", k.level) tabindex="0" title=format!("Enter: show {}", k.q) on:click=move |_| drill(app, v, &q)>
                        <div class="kl">{k.label}</div>
                        <div class="kv2">{k.value}</div>
                        {k.gauge.map(|g| view! { <div class="gauge"><span class={if g > 100.0 { "fill l2" } else if g > 85.0 { "fill l1" } else { "fill l0" }} style=format!("width:{}%", g.min(100.0))></span></div> })}
                        <div class="ks muted">{k.sub}</div>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

#[component]
fn ProjectsByStatus() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="tile chart" tabindex="0" on:click=move |_| drill(app, ViewId::Projects, "")>
            <h4>"Projects by status"</h4>
            {move || app.rep.with(|r| {
                let counts: Vec<(ProjectStatus, usize)> = ProjectStatus::ALL.iter().map(|s| (*s, r.projects.values().filter(|p| p.status == *s).count())).collect();
                let max = counts.iter().map(|c| c.1).max().unwrap_or(1).max(1);
                counts.into_iter().map(|(s, n)| view! {
                    <div class="hbar" on:click=move |e| { e.stop_propagation(); drill(app, ViewId::Projects, &format!("status:{}", s.as_str())); }>
                        <span class="hl2">{s.label()}</span>
                        <span class="htrack"><span class=format!("hfill ps-{}", s.as_str()) style=format!("width:{}%", n as f64 / max as f64 * 100.0)></span></span>
                        <span class="hn">{n}</span>
                    </div>
                }).collect_view()
            })}
        </div>
    }
}

#[component]
fn TasksByStatus() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="tile chart" tabindex="0" on:click=move |_| drill(app, ViewId::Tasks, "is:open")>
            <h4>"Tasks by status"</h4>
            {move || app.rep.with(|r| {
                let total = r.tasks.len().max(1) as f64;
                let counts: Vec<(TaskStatus, usize)> = TaskStatus::ALL.iter().map(|s| (*s, r.tasks.values().filter(|t| t.status == *s).count())).collect();
                view! {
                    <div class="stack">
                        {counts.iter().map(|(s, n)| {
                            let s = *s;
                            view! { <span class=format!("seg ts-{}", s.as_str()) style=format!("width:{}%", *n as f64 / total * 100.0) title=format!("{}: {n}", s.label())
                                on:click=move |e| { e.stop_propagation(); drill(app, ViewId::Tasks, &format!("status:{}", s.as_str())); }></span> }
                        }).collect_view()}
                    </div>
                    <div class="legend2">
                        {counts.into_iter().map(|(s, n)| view! {
                            <span class="lgi" on:click=move |e| { e.stop_propagation(); drill(app, ViewId::Tasks, &format!("status:{}", s.as_str())); }>
                                <i class=format!("sw2 ts-{}", s.as_str())></i>{s.label()}" "<b>{n}</b>" "<span class="muted">{format!("{:.0}%", n as f64 / total * 100.0)}</span>
                            </span>
                        }).collect_view()}
                    </div>
                }
            })}
        </div>
    }
}

#[component]
fn HoursByOwner() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="tile chart" tabindex="0" on:click=move |_| drill(app, ViewId::Projects, "is:open")>
            <h4>"Hours by owner — budget vs actual (open projects)"</h4>
            {move || app.rep.with(|r| {
                let mut by: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
                for p in r.projects.values().filter(|p| r.project_open(p)) {
                    if let Some(o) = p.owner_id {
                        let e = by.entry(o).or_default();
                        e.0 += p.budget_hours.unwrap_or(0.0);
                        e.1 += r.metrics(p.id).actual_total;
                    }
                }
                let max = by.values().map(|(b, a)| b.max(*a)).fold(1.0, f64::max);
                by.into_iter().map(|(uid, (b, a))| {
                    let u = r.users.get(&uid).cloned();
                    let name = u.as_ref().map(|u| u.display_name.clone()).unwrap_or_default();
                    let uname = u.as_ref().map(|u| u.username.clone()).unwrap_or_default();
                    view! {
                        <div class="pair" on:click=move |e| { e.stop_propagation(); drill(app, ViewId::Projects, &format!("@{uname} is:open")); }>
                            <span class="hl2">{avatar(uid, &name)}{name.clone()}</span>
                            <span class="ptrack">
                                <span class="pb1" style=format!("width:{}%", b / max * 100.0) title=format!("budget {} h", fmt_hours(b))></span>
                                <span class={if a > b { "pb2 over" } else { "pb2" }} style=format!("width:{}%", a / max * 100.0) title=format!("actual {} h", fmt_hours(a))></span>
                            </span>
                            <span class="hn">{format!("{}/{}", fmt_hours(a.round()), fmt_hours(b.round()))}</span>
                        </div>
                    }
                }).collect_view()
            })}
            <div class="legend2"><span class="lgi"><i class="sw2 pb1"></i>"budget"</span><span class="lgi"><i class="sw2 pb2"></i>"actual"</span><span class="lgi"><i class="sw2 over"></i>"over budget"</span></div>
        </div>
    }
}

#[component]
fn Workload() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="tile chart wide2" tabindex="0" on:click=move |_| drill(app, ViewId::Tasks, "is:open due<+8w")>
            <h4>"Workload — open estimated hours by due week"</h4>
            {move || app.rep.with(|r| {
                let w0 = r.today.week_start();
                let mut cells: BTreeMap<(i64, i32), f64> = BTreeMap::new();
                for t in r.tasks.values().filter(|t| t.status != TaskStatus::Done) {
                    let (Some(u), Some(d)) = (t.assignee_id, t.due_date.as_deref().and_then(Date::parse_iso)) else { continue };
                    let wk = (d.days() - w0.days()).div_euclid(7);
                    if (0..8).contains(&wk) {
                        *cells.entry((u, wk)).or_default() += t.est_hours.unwrap_or(0.0);
                    }
                }
                let max = cells.values().copied().fold(1.0, f64::max);
                let mut users: Vec<&User> = r.users.values().filter(|u| u.active && u.role != Role::Viewer).collect();
                users.sort_by(|a, b| a.display_name.cmp(&b.display_name));
                view! {
                    <table class="heat">
                        <thead><tr><th></th>{(0..8).map(|w| { let d = w0.add_days(w * 7); view! { <th>{d.short(r.today.year()).trim().to_string()}</th> } }).collect_view()}<th>"Σ"</th></tr></thead>
                        <tbody>
                            {users.into_iter().map(|u| {
                                let uname = u.username.clone();
                                let total: f64 = (0..8).map(|w| cells.get(&(u.id, w)).copied().unwrap_or(0.0)).sum();
                                view! {
                                    <tr>
                                        <th class="hu">{avatar(u.id, &u.display_name)}{u.display_name.clone()}</th>
                                        {(0..8).map(|w| {
                                            let h = cells.get(&(u.id, w)).copied().unwrap_or(0.0);
                                            let a = (h / max).sqrt();
                                            let style = if h > 0.0 { format!("background: color-mix(in srgb, var(--accent) {:.0}%, transparent)", 12.0 + a * 78.0) } else { String::new() };
                                            let from = w0.add_days(w * 7);
                                            let to = from.add_days(6);
                                            let q = format!("@{uname} is:open due>={} due<={}", from.iso(), to.iso());
                                            view! { <td style=style class:hot={h > 40.0} title=format!("{} h", fmt_hours(h)) on:click=move |e| { e.stop_propagation(); drill(app, ViewId::Tasks, &q); }>{if h > 0.0 { fmt_hours(h.round()) } else { String::new() }}</td> }
                                        }).collect_view()}
                                        <td class="tot">{fmt_hours(total.round())}</td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                }
            })}
        </div>
    }
}

#[component]
fn ActivityTile() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="tile chart" tabindex="0" on:click=move |_| app.set_view(ViewId::Activity)>
            <h4>"Activity — changes per day, last 30 days"</h4>
            {move || app.rep.with(|r| {
                let mut per: Vec<usize> = vec![0; 30];
                let start = r.today.add_days(-29);
                for a in r.activity.values() {
                    if let Some(d) = a.ts.get(0..10).and_then(Date::parse_iso) {
                        let i = d.days() - start.days();
                        if (0..30).contains(&i) {
                            per[i as usize] += 1;
                        }
                    }
                }
                let max = *per.iter().max().unwrap_or(&1).max(&1) as f64;
                let pts: Vec<String> = per.iter().enumerate().map(|(i, n)| format!("{},{}", i as f64 * 10.0, 50.0 - *n as f64 / max * 46.0)).collect();
                let area = format!("0,50 {} 290,50", pts.join(" "));
                let total: usize = per.iter().sum();
                let latest: Vec<Activity> = r.activity.values().rev().take(7).cloned().collect();
                view! {
                    <svg class="spark" viewBox="0 0 290 52" preserveAspectRatio="none">
                        <polygon points=area class="sa" />
                        <polyline points=pts.join(" ") class="sl" />
                    </svg>
                    <div class="muted small">{format!("{total} changes · peak {} in a day", max as usize)}</div>
                    <div class="latest">
                        {latest.into_iter().map(|a| {
                            let who = r.user_name(a.user_id).to_string();
                            let when = pt_core::grid::activity_when(&a, util::now_secs());
                            view! { <div class="actline"><span class="muted">{when}</span>" "<b>{who}</b>" "{a.label.clone()}</div> }
                        }).collect_view()}
                    </div>
                }
            })}
        </div>
    }
}

#[component]
fn AtRisk() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="tile chart" tabindex="0" on:click=move |_| drill(app, ViewId::Projects, "is:open health:red,amber")>
            <h4>"Top 10 at-risk projects"</h4>
            {move || app.rep.with(|r| {
                let mut v: Vec<(f64, &Project)> = r
                    .projects
                    .values()
                    .filter(|p| r.project_open(p))
                    .map(|p| {
                        let burn = r.burn_pct(p).unwrap_or(0.0);
                        let h = match p.health { Health::Red => 200.0, Health::Amber => 100.0, Health::Green => 0.0 };
                        (h + burn + r.metrics(p.id).tasks_overdue as f64 * 10.0, p)
                    })
                    .filter(|(s, p)| *s > 90.0 || p.health != Health::Green)
                    .collect();
                v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                view! {
                    <table class="mini">
                        <thead><tr><th>"Project"</th><th>"Health"</th><th>"Burn"</th><th>"Late"</th><th>"Owner"</th></tr></thead>
                        <tbody>
                            {v.into_iter().take(10).map(|(_, p)| {
                                let id = p.id;
                                let burn = r.burn_pct(p);
                                let m = r.metrics(p.id);
                                view! {
                                    <tr on:click=move |e| { e.stop_propagation(); app.goto(Entity::Project, id); }>
                                        <td><b>{p.code.clone()}</b>" "{p.name.clone()}</td>
                                        <td><span class=format!("health h-{}", p.health.as_str())><i>"●"</i>" "{p.health.label()}</span></td>
                                        <td class={if burn.unwrap_or(0.0) > 100.0 { "c-err" } else if burn.unwrap_or(0.0) > 90.0 { "c-warn" } else { "" }}>{burn.map(|b| format!("{b:.0}%")).unwrap_or_default()}</td>
                                        <td class:c-err={m.tasks_overdue > 0}>{m.tasks_overdue}</td>
                                        <td>{r.user_name(p.owner_id).to_string()}</td>
                                    </tr>
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                }
            })}
        </div>
    }
}
