//! Board view (ui-spec §4.3): status columns, cascading Program ▸ Project
//! scope, swimlanes by assignee, keyboard moves and drag & drop.

use leptos::prelude::*;
use pt_core::date::Date;
use pt_core::grid;
use pt_core::model::*;
use pt_core::protocol::Op;
use pt_core::query::{Query, Target};
use serde_json::Value;

use crate::app::{Act, App, Ctx, Level, Modal, use_app};
use crate::cells::{avatar, priority_mark};
use crate::forms::{Ctl, Field};

const CAP: usize = 120;

/// Cards per status column (and per lane), in display order.
fn layout(app: App) -> (Vec<Option<i64>>, Vec<Vec<Vec<i64>>>) {
    let q = app.q.board.get();
    let prog = app.board.program.get();
    let proj = app.board.project.get();
    let lanes_on = app.board.lanes.get();
    app.rep.with(|r| {
        let q = Query::compile(&q, Target::Tasks, r);
        let mut ids: Vec<&Task> = r
            .tasks
            .values()
            .filter(|t| proj.is_none_or(|p| t.project_id == p))
            .filter(|t| {
                prog.is_none_or(|g| {
                    r.projects
                        .get(&t.project_id)
                        .is_some_and(|p| p.program_id == Some(g))
                })
            })
            .filter(|t| q.matches_task(t, r))
            .collect();
        ids.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| {
                    a.due_date
                        .as_deref()
                        .unwrap_or("9999")
                        .cmp(b.due_date.as_deref().unwrap_or("9999"))
                })
                .then(a.id.cmp(&b.id))
        });
        let lanes: Vec<Option<i64>> = if lanes_on {
            let mut l: Vec<Option<i64>> = ids.iter().map(|t| t.assignee_id).collect();
            l.sort_by_key(|u| (u.is_none(), u.map(|u| r.user_name(Some(u)).to_string())));
            l.dedup();
            l
        } else {
            vec![None]
        };
        let grid: Vec<Vec<Vec<i64>>> = lanes
            .iter()
            .map(|lane| {
                TaskStatus::ALL
                    .iter()
                    .map(|s| {
                        ids.iter()
                            .filter(|t| t.status == *s && (!lanes_on || t.assignee_id == *lane))
                            .map(|t| t.id)
                            .collect()
                    })
                    .collect()
            })
            .collect();
        (lanes, grid)
    })
}

/// Flattened column (all lanes) for keyboard movement.
fn column(grid: &[Vec<Vec<i64>>], c: usize) -> Vec<i64> {
    grid.iter()
        .flat_map(|lane| lane[c].iter().take(CAP).copied())
        .collect()
}

fn move_card(app: App, id: i64, to: TaskStatus, lane: Option<Option<i64>>) {
    let mut ops = Vec::new();
    app.rep.with_untracked(|r| {
        if let Some(t) = r.tasks.get(&id) {
            if t.status != to {
                ops.push(Op::set(
                    Entity::Task,
                    id,
                    "status",
                    Value::from(t.status.as_str()),
                    Value::from(to.as_str()),
                ));
            }
            if let Some(l) = lane {
                if t.assignee_id != l {
                    ops.push(Op::set(
                        Entity::Task,
                        id,
                        "assignee_id",
                        t.assignee_id.map_or(Value::Null, Value::from),
                        l.map_or(Value::Null, Value::from),
                    ));
                }
            }
        }
    });
    if !ops.is_empty() {
        app.mutate(ops, move |r| {
            if r.ok && r.conflicts.is_empty() && r.error.is_none() {
                app.say(Level::Ok, format!("Moved to {}.", to.label()));
            }
        });
    }
}

pub fn act(app: App, a: Act) {
    match a {
        Act::Lanes => app.board.lanes.update(|l| *l = !*l),
        Act::MoveLeft | Act::MoveRight => {
            let Some(id) = app.board.sel.get_untracked() else {
                return;
            };
            let Some(cur) = app
                .rep
                .with_untracked(|r| r.tasks.get(&id).map(|t| t.status))
            else {
                return;
            };
            let i = cur.index() as i64 + if a == Act::MoveLeft { -1 } else { 1 };
            if let Some(to) = TaskStatus::ALL.get(i.clamp(0, 4) as usize) {
                app.board.col.set(to.index());
                move_card(app, id, *to, None);
            }
        }
        _ => {}
    }
}

fn on_key(app: App, e: &web_sys::KeyboardEvent) -> bool {
    let k = e.key();
    let b = app.board;
    let (_, grid) = layout(app);
    let col = b.col.get_untracked().min(4);
    let list = column(&grid, col);
    let pos = b
        .sel
        .get_untracked()
        .and_then(|s| list.iter().position(|x| *x == s))
        .unwrap_or(0);
    let pick = |c: usize, p: usize| {
        let l = column(&grid, c);
        b.col.set(c);
        b.sel.set(l.get(p.min(l.len().saturating_sub(1))).copied());
    };
    match k.as_str() {
        "ArrowLeft" | "<" if e.shift_key() || k == "<" => act(app, Act::MoveLeft),
        "ArrowRight" | ">" if e.shift_key() || k == ">" => act(app, Act::MoveRight),
        "ArrowLeft" => pick(col.saturating_sub(1), pos),
        "ArrowRight" => pick((col + 1).min(4), pos),
        "ArrowUp" => pick(col, pos.saturating_sub(1)),
        "ArrowDown" => pick(col, pos + 1),
        "Home" => pick(col, 0),
        "End" => pick(col, usize::MAX),
        "Enter" => {
            if let Some(id) = b.sel.get_untracked() {
                app.modal.set(Some(Modal::Record {
                    entity: Entity::Task,
                    id,
                }));
            }
        }
        _ => return false,
    }
    true
}

#[component]
pub fn BoardView() -> impl IntoView {
    let app = use_app();
    let b = app.board;
    let data = Memo::new(move |_| layout(app));
    // keep a selection
    Effect::new(move |_| {
        let (_, grid) = data.get();
        let sel = b.sel.get_untracked();
        let all: Vec<i64> = grid
            .iter()
            .flat_map(|l| l.iter().flatten().copied())
            .collect();
        if sel.is_none_or(|s| !all.contains(&s)) {
            let c = b.col.get_untracked().min(4);
            let col = column(&grid, c);
            b.sel
                .set(col.first().copied().or_else(|| all.first().copied()));
        }
    });
    let program = RwSignal::new(b.program.get_untracked().map_or(Value::Null, Value::from));
    let project = RwSignal::new(b.project.get_untracked().map_or(Value::Null, Value::from));
    Effect::new(move |_| {
        let p = program.get().as_i64();
        b.program.set(p);
        // cascade: a project outside the program is cleared
        let cur = project.get_untracked().as_i64();
        if cur.is_some_and(|c| {
            app.rep.with_untracked(|r| {
                r.projects
                    .get(&c)
                    .is_some_and(|x| p.is_some() && x.program_id != p)
            })
        }) {
            project.set(Value::Null);
        }
    });
    Effect::new(move |_| b.project.set(project.get().as_i64()));
    let programs = Signal::derive(move || {
        let mut c = app
            .rep
            .with(|r| grid::choices(r, Entity::Project, "program_id", 0, None));
        if let Some(first) = c.first_mut() {
            first.label = "All programs".into();
        }
        c
    });
    let projects = Signal::derive(move || {
        let p = program.get().as_i64();
        let mut v = vec![grid::Choice {
            value: Value::Null,
            label: "All projects".into(),
            detail: String::new(),
            style: grid::Style::Muted,
        }];
        v.extend(app.rep.with(|r| {
            grid::choices(r, Entity::Task, "project_id", 0, None)
                .into_iter()
                .filter(|c| {
                    p.is_none()
                        || c.value
                            .as_i64()
                            .and_then(|id| r.projects.get(&id))
                            .is_some_and(|x| x.program_id == p)
                })
                .collect::<Vec<_>>()
        }));
        v
    });
    let drag_over = RwSignal::new(None::<(usize, usize)>);
    view! {
        <div class="board-wrap">
            <div class="boardbar">
                <div class="scope"><Field label="Program".into() ctl=Ctl::Combo value=program choices=programs /></div>
                <span class="sep-arrow">"▸"</span>
                <div class="scope"><Field label="Project".into() ctl=Ctl::Combo value=project choices=projects /></div>
                <button class="btn sm" class:on=move || b.lanes.get() on:click=move |_| act(app, Act::Lanes) title="^R">
                    {move || if b.lanes.get() { "☰ Swimlanes: assignee" } else { "☰ Swimlanes: off" }}
                </button>
                <span class="muted">{move || { let (_, g) = data.get(); format!("{} cards", g.iter().flat_map(|l| l.iter()).map(|c| c.len()).sum::<usize>()) }}</span>
            </div>
            <div class="board" id="board" tabindex="0" data-pane="1"
                on:focus=move |_| app.ctx.set(Ctx::Board)
                on:keydown=move |e: web_sys::KeyboardEvent| { if !e.ctrl_key() && !e.meta_key() && !e.alt_key() && on_key(app, &e) { e.prevent_default(); e.stop_propagation(); } }>
                <div class="bcols head">
                    {TaskStatus::ALL.iter().enumerate().map(|(ci, s)| {
                        let s = *s;
                        view! {
                            <div class="bhead" class:cur=move || b.col.get() == ci>
                                <span class=format!("chip ts-{}", s.as_str())>{s.label()}</span>
                                <span class="bcount">{move || {
                                    let (_, g) = data.get();
                                    let ids: Vec<i64> = g.iter().flat_map(|l| l[ci].iter().copied()).collect();
                                    let h: f64 = app.rep.with(|r| ids.iter().filter_map(|i| r.tasks.get(i)).map(|t| t.est_hours.unwrap_or(0.0)).sum());
                                    format!("{} · {} h", ids.len(), grid::fmt_hours(h))
                                }}</span>
                            </div>
                        }
                    }).collect_view()}
                </div>
                {move || {
                    let (lanes, g) = data.get();
                    let lanes_on = b.lanes.get();
                    lanes.into_iter().zip(g).enumerate().map(|(li, (lane, cols))| {
                        let lane_name = app.rep.with_untracked(|r| lane.map(|u| r.user_name(Some(u)).to_string()).unwrap_or_else(|| "Unassigned".into()));
                        let total: usize = cols.iter().map(|c| c.len()).sum();
                        view! {
                            {lanes_on.then(|| view! {
                                <div class="lane-h">{lane.map(|u| avatar(u, &lane_name).into_any())}{lane_name.clone()}<span class="muted">{format!(" · {total}")}</span></div>
                            })}
                            <div class="bcols">
                                {cols.into_iter().enumerate().map(|(ci, cards)| {
                                    let more = cards.len().saturating_sub(CAP);
                                    let status = TaskStatus::ALL[ci];
                                    view! {
                                        <div class="bcol" data-status=status.as_str() class:over=move || drag_over.get() == Some((li, ci))
                                            on:dragover=move |e: web_sys::DragEvent| { e.prevent_default(); drag_over.set(Some((li, ci))); }
                                            on:dragleave=move |_| drag_over.set(None)
                                            on:drop=move |e: web_sys::DragEvent| {
                                                e.prevent_default();
                                                drag_over.set(None);
                                                let id = e.data_transfer().and_then(|d| d.get_data("text/plain").ok()).and_then(|s| s.parse::<i64>().ok());
                                                if let Some(id) = id {
                                                    b.col.set(ci);
                                                    b.sel.set(Some(id));
                                                    move_card(app, id, status, lanes_on.then_some(lane));
                                                }
                                            }>
                                            {cards.into_iter().take(CAP).map(|id| view! { <Card id=id col=ci /> }).collect_view()}
                                            {(more > 0).then(|| view! { <div class="bmore muted">{format!("+{more} more — narrow the filter")}</div> })}
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        }
                    }).collect_view()
                }}
            </div>
        </div>
    }
}

#[component]
fn Card(id: i64, col: usize) -> impl IntoView {
    let app = use_app();
    let b = app.board;
    let sel = move || b.sel.get() == Some(id);
    let el = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        if sel() {
            if let Some(e) = el.get() {
                let o = web_sys::ScrollIntoViewOptions::new();
                o.set_block(web_sys::ScrollLogicalPosition::Nearest);
                e.scroll_into_view_with_scroll_into_view_options(&o);
            }
        }
    });
    let watcher = move || {
        let me = app.rep.with_untracked(|r| r.conn);
        app.peers.with(|ps| {
            ps.iter()
                .find(|p| {
                    p.conn != me
                        && p.focus
                            .as_ref()
                            .is_some_and(|f| f.entity == Entity::Task && f.id == id)
                })
                .map(|p| p.user_id)
        })
    };
    move || {
        let Some((t, code, assignee, today, blocked)) = app.rep.with(|r| {
            r.tasks.get(&id).map(|t| {
                (
                    t.clone(),
                    r.project_code(t.project_id).to_string(),
                    t.assignee_id.and_then(|a| r.users.get(&a).cloned()),
                    r.today,
                    r.task_blocked(t),
                )
            })
        }) else {
            return ().into_any();
        };
        let due = t.due_date.as_deref().and_then(Date::parse_iso);
        let due_cls = match due {
            Some(d) if t.status != TaskStatus::Done && d < today => "due c-err",
            Some(d) if t.status != TaskStatus::Done && d.days() - today.days() <= 7 => "due c-warn",
            _ => "due muted",
        };
        let due_txt = due
            .map(|d| d.short(today.year()).trim().to_string())
            .unwrap_or_default();
        let pct = t
            .est_hours
            .filter(|e| *e > 0.0)
            .map(|e| t.actual_hours / e * 100.0);
        let flash = move || app.flash.with(|f| f.contains(&(Entity::Task, id)));
        view! {
            <div class="card2" data-id=id class:sel=sel class:flash=flash class:blocked=blocked node_ref=el draggable="true"
                on:dragstart=move |e: web_sys::DragEvent| { if let Some(d) = e.data_transfer() { let _ = d.set_data("text/plain", &id.to_string()); d.set_effect_allowed("move"); } }
                on:mousedown=move |_| { b.sel.set(Some(id)); b.col.set(col); }
                on:dblclick=move |_| app.modal.set(Some(Modal::Record { entity: Entity::Task, id }))>
                <div class="c-top">
                    <span class="ccode">{format!("{code} #{}", t.seq)}</span>
                    <span class=format!("pri pri-{}", t.priority.as_str())><i>{priority_mark(t.priority)}</i></span>
                    {move || watcher().map(|u| view! { <span class="pav" style=format!("background:var(--u{})", u.rem_euclid(8))>"👁"</span> })}
                    <span class="spacer"></span>
                    <span class=due_cls>{due_txt}</span>
                </div>
                <div class="ctitle">{t.title.clone()}</div>
                <div class="c-bot">
                    {assignee.map(|u| avatar(u.id, &u.display_name).into_any())}
                    <span class="muted">{t.phase.clone()}</span>
                    <span class="spacer"></span>
                    {pct.map(|p| view! { <span class="bar"><span class={if p > 100.0 { "fill l2" } else { "fill l0" }} style=format!("width:{}%", p.min(100.0))></span></span> })}
                    <span class="muted hrs">{format!("{}/{}h", grid::fmt_hours(t.actual_hours), t.est_hours.map(grid::fmt_hours).unwrap_or_else(|| "–".into()))}</span>
                </div>
            </div>
        }
        .into_any()
    }
}
