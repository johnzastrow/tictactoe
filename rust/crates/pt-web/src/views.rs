//! Grid-based views (ui-spec §4: 1, 2, 6, 7, 8, 9) and the task record card.

use leptos::prelude::*;
use pt_core::date::Date;
use pt_core::grid;
use pt_core::model::*;

use crate::app::{GridKey, use_app};
use crate::cells::{self, avatar};
use crate::gridview::{GridView, grid_title};

#[component]
pub fn ProjectsView() -> impl IntoView {
    let app = use_app();
    let g = app.grids.projects;
    let tg = app.grids.proj_tasks;
    let title = move || {
        app.rep
            .with(|r| {
                g.cur_id.get().and_then(|id| r.projects.get(&id)).map(|p| {
                    let m = r.metrics(p.id);
                    format!(
                        "Tasks of {} — {} · {}/{} done · {} h est · {} h actual",
                        p.code,
                        p.name,
                        m.tasks_done,
                        m.tasks_total,
                        grid::fmt_hours(m.est_total),
                        grid::fmt_hours(m.actual_total)
                    )
                })
            })
            .unwrap_or_else(|| "Tasks".into())
    };
    view! {
        <div class="split">
            <section class="pane main">
                {grid_title(app, g, "Projects")}
                <GridView key=GridKey::Projects empty="No projects match the filter. Press Esc in the filter bar, or ^F to change it." />
            </section>
            <Show when=move || app.detail.get()>
                <section class="pane detail">
                    <div class="ptitle"><span>{title}</span><span class="muted">"F6 to focus · Ins adds a task here"</span></div>
                    <GridView key=GridKey::ProjTasks empty="This project has no tasks yet. Press Ins to add one." />
                </section>
            </Show>
        </div>
        {move || { let _ = tg; }}
    }
}

#[component]
pub fn TasksView() -> impl IntoView {
    let app = use_app();
    let g = app.grids.tasks;
    view! {
        <div class="split row">
            <section class="pane main">
                {grid_title(app, g, "Tasks")}
                <GridView key=GridKey::Tasks empty="No tasks match the filter." />
            </section>
            <Show when=move || app.detail.get()>
                <section class="pane card-pane" data-pane="1" tabindex="0">
                    <TaskCard />
                </section>
            </Show>
        </div>
    }
}

/// Read-only record card for the task under the cursor.
#[component]
fn TaskCard() -> impl IntoView {
    let app = use_app();
    let g = app.grids.tasks;
    move || {
        let Some(id) = g.cur_id.get() else {
            return view! { <div class="card empty muted">"Select a task to see its details."</div> }.into_any();
        };
        app.rep.with(|r| {
            let Some(t) = r.tasks.get(&id).cloned() else { return ().into_any() };
            let p = r.projects.get(&t.project_id).cloned();
            let today = r.today;
            let due = t.due_date.as_deref().and_then(Date::parse_iso);
            let due_txt = due.map(|d| {
                let days = d.days() - today.days();
                let rel = match days {
                    0 => "today".into(),
                    1 => "tomorrow".into(),
                    d if d < 0 => format!("{} days late", -d),
                    d => format!("in {d} days"),
                };
                format!("{} ({rel})", d.long())
            }).unwrap_or_else(|| "—".into());
            let deps: Vec<(String, TaskStatus)> = t.deps.iter().filter_map(|d| r.tasks.get(d)).map(|d| (format!("#{} {}", d.seq, d.title), d.status)).collect();
            let blocks: Vec<String> = r.project_tasks(t.project_id).iter().filter_map(|x| r.tasks.get(x)).filter(|x| x.deps.contains(&id)).map(|x| format!("#{} {}", x.seq, x.title)).collect();
            let acts: Vec<Activity> = r.activity.values().rev().filter(|a| a.entity == "task" && a.entity_id == id).take(8).cloned().collect();
            let cfs: Vec<(String, String)> = r.custom_fields_for(CfEntity::Task).iter().map(|c| {
                (c.name.clone(), t.cf.get(&c.id.to_string()).map(|v| grid::display_value(r, Entity::Task, &c.key(), v)).unwrap_or_else(|| "—".into()))
            }).collect();
            let var = t.est_hours.map(|e| t.actual_hours - e);
            let pct = t.est_hours.filter(|e| *e > 0.0).map(|e| (t.actual_hours / e * 100.0).min(150.0));
            let assignee = t.assignee_id.and_then(|a| r.users.get(&a).cloned());
            view! {
                <div class="card">
                    <div class="card-h">
                        <span class="code">{format!("{} #{}", p.as_ref().map_or("?", |p| p.code.as_str()), t.seq)}</span>
                        <span class=format!("chip ts-{}", t.status.as_str())>{t.status.label()}</span>
                        <span class=format!("pri pri-{}", t.priority.as_str())><i>{cells::priority_mark(t.priority)}</i>" "{t.priority.label()}</span>
                    </div>
                    <h3>{t.title.clone()}</h3>
                    <div class="muted">{p.as_ref().map(|p| format!("{} · {}", p.name, r.program_name(p.program_id))).unwrap_or_default()}</div>
                    <dl class="kv">
                        <dt>"Phase"</dt><dd>{if t.phase.is_empty() { "—".into() } else { t.phase.clone() }}</dd>
                        <dt>"Assignee"</dt><dd>{match assignee { Some(u) => view! { {avatar(u.id, &u.display_name)}" "{u.display_name.clone()} }.into_any(), None => "Unassigned".into_any() }}</dd>
                        <dt>"Start"</dt><dd>{t.start_date.as_deref().and_then(Date::parse_iso).map(|d| d.long()).unwrap_or_else(|| "—".into())}</dd>
                        <dt>"Due"</dt><dd class:c-err=r.task_overdue(&t)>{due_txt}</dd>
                        <dt>"Hours"</dt><dd>
                            {format!("{} actual / {} est", grid::fmt_hours(t.actual_hours), t.est_hours.map(grid::fmt_hours).unwrap_or_else(|| "—".into()))}
                            {var.map(|v| view! { <span class:c-err={v > 0.0} class:c-ok={v <= 0.0}>{format!("  ({}{})", if v > 0.0 { "+" } else { "" }, grid::fmt_hours(v))}</span> })}
                            {pct.map(|p| view! { <div class="bar wide"><span class={if p > 100.0 { "fill l2" } else { "fill l0" }} style=format!("width:{}%", p.min(100.0))></span></div> })}
                        </dd>
                        <dt>"Tags"</dt><dd>{if t.tags.is_empty() { view! { "—" }.into_any() } else { t.tags.iter().map(|x| view! { <span class="tag">{x.clone()}</span> }).collect_view().into_any() }}</dd>
                        {cfs.into_iter().map(|(k, v)| view! { <dt>{k}</dt><dd>{v}</dd> }).collect_view()}
                    </dl>
                    <h4>"Depends on"</h4>
                    {if deps.is_empty() { view! { <div class="muted">"Nothing"</div> }.into_any() } else {
                        deps.into_iter().map(|(l, s)| view! { <div class="depline"><span class=format!("chip ts-{}", s.as_str())>{s.label()}</span>" "{l}</div> }).collect_view().into_any()
                    }}
                    <h4>"Blocks"</h4>
                    {if blocks.is_empty() { view! { <div class="muted">"Nothing"</div> }.into_any() } else {
                        blocks.into_iter().map(|l| view! { <div class="depline">{l}</div> }).collect_view().into_any()
                    }}
                    <h4>"Notes"</h4>
                    <div class="notes">{if t.notes.trim().is_empty() { "—".to_string() } else { t.notes.clone() }}</div>
                    <h4>"Recent activity"</h4>
                    {if acts.is_empty() { view! { <div class="muted">"No changes yet"</div> }.into_any() } else {
                        acts.into_iter().map(|a| {
                            let who = r.user_name(a.user_id).to_string();
                            let when = grid::activity_when(&a, crate::util::now_secs());
                            view! { <div class="actline" class:struck=a.state == ActState::Undone><span class="muted">{when}</span>" "<b>{who}</b>" "{a.label.clone()}</div> }
                        }).collect_view().into_any()
                    }}
                    <div class="card-foot muted">"^E edit all fields · F2 edit cell"</div>
                </div>
            }
            .into_any()
        })
    }
}

#[component]
pub fn ActivityView() -> impl IntoView {
    let app = use_app();
    let g = app.grids.activity;
    view! {
        <section class="pane main full">
            {grid_title(app, g, "Activity — newest first · Enter opens the record · ^Z/^Y undo/redo your own changes")}
            <GridView key=GridKey::Activity empty="No activity matches." />
        </section>
    }
}

#[component]
pub fn TemplatesView() -> impl IntoView {
    let app = use_app();
    let g = app.grids.templates;
    let tg = app.grids.template_tasks;
    view! {
        <div class="split row">
            <section class="pane narrow">
                {grid_title(app, g, "Templates")}
                <GridView key=GridKey::Templates />
            </section>
            <section class="pane main col">
                {grid_title(app, tg, "Template tasks")}
                <GridView key=GridKey::TemplateTasks empty="This template has no tasks. Admins can press Ins to add one." />
                <MiniGantt />
            </section>
        </div>
    }
}

/// A small Gantt preview of the selected template (offset + duration).
#[component]
fn MiniGantt() -> impl IntoView {
    let app = use_app();
    let g = app.grids.templates;
    move || {
        let Some(tid) = g.cur_id.get() else {
            return ().into_any();
        };
        app.rep.with(|r| {
            let tts: Vec<TemplateTask> = r.template_tasks_of(tid).into_iter().cloned().collect();
            let span = tts.iter().map(|t| t.offset_days + t.duration_days).max().unwrap_or(1).max(1) as f64;
            let mut phases: Vec<String> = Vec::new();
            for t in &tts {
                if !phases.contains(&t.phase) {
                    phases.push(t.phase.clone());
                }
            }
            view! {
                <div class="minigantt">
                    <div class="ptitle"><span>"Preview"</span><span class="muted">{format!("{} days · {} tasks · {} phases", span, tts.len(), phases.len())}</span></div>
                    <div class="mg">
                        {tts.iter().map(|t| {
                            let ph = phases.iter().position(|p| *p == t.phase).unwrap_or(0) % 8;
                            let left = t.offset_days as f64 / span * 100.0;
                            let width = (t.duration_days as f64 / span * 100.0).max(0.8);
                            view! {
                                <div class="mg-row">
                                    <span class="mg-l">{format!("{:>2} {}", t.seq, t.title)}</span>
                                    <span class="mg-t"><span class=format!("mg-b ph{ph}") style=format!("left:{left}%;width:{width}%")
                                        title=format!("{} · day {}–{} · {}", t.phase, t.offset_days, t.offset_days + t.duration_days, t.est_hours.map(grid::fmt_hours).unwrap_or_default())></span></span>
                                </div>
                            }
                        }).collect_view()}
                        <div class="mg-row axis"><span class="mg-l"></span><span class="mg-t">
                            {(0..=4).map(|i| view! { <span class="mg-tick" style=format!("left:{}%", i * 25)>{format!("d{}", (span * i as f64 / 4.0).round())}</span> }).collect_view()}
                        </span></div>
                    </div>
                </div>
            }
            .into_any()
        })
    }
}

#[component]
pub fn FieldsView() -> impl IntoView {
    let app = use_app();
    let g = app.grids.fields;
    view! {
        <section class="pane main full">
            {grid_title(app, g, "Custom fields — new fields appear as columns in every client immediately")}
            <GridView key=GridKey::Fields empty="No custom fields. Admins can press Ins to add one." />
        </section>
    }
}

#[component]
pub fn UsersView() -> impl IntoView {
    let app = use_app();
    let g = app.grids.users;
    let pg = app.grids.programs;
    view! {
        <div class="split">
            <section class="pane main">
                {grid_title(app, g, "Users")}
                <GridView key=GridKey::Users />
            </section>
            <section class="pane detail">
                {grid_title(app, pg, "Programs")}
                <GridView key=GridKey::Programs />
            </section>
        </div>
    }
}
