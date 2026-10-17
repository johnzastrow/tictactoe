//! Timeline / Gantt (ui-spec §4.4): project tree on the left, SVG bars on
//! the right, zoom, today line, weekend shading, dependency arrows, drag to
//! move or resize, and keyboard nudges.

use leptos::prelude::*;
use pt_core::date::{Date, MONTH_NAMES};
use pt_core::model::*;
use pt_core::protocol::Op;
use pt_core::query::{Query, Target};
use serde_json::Value;
use wasm_bindgen::JsCast;

use crate::app::{Act, App, Ctx, Level, Modal, use_app};

const ROW: f64 = 24.0;
const HEAD: f64 = 34.0;
const LEFT_W: f64 = 300.0;

#[derive(Clone, PartialEq, Debug)]
struct TRow {
    entity: Entity,
    id: i64,
    label: String,
    sub: String,
    start: Option<Date>,
    due: Option<Date>,
    pct: f64,
    cls: String,
    depth: u8,
    expanded: bool,
    milestone: bool,
    deps: Vec<i64>,
}

fn px_per_day(zoom: usize) -> f64 {
    match zoom {
        0 => 26.0,
        1 => 7.0,
        _ => 2.2,
    }
}

fn unit_days(zoom: usize) -> i32 {
    match zoom {
        0 => 1,
        1 => 7,
        _ => 30,
    }
}

fn rows(app: App) -> Vec<TRow> {
    let q = app.q.timeline.get();
    let expanded = app.tl.expanded.get();
    app.rep.with(|r| {
        let q = Query::compile(&q, Target::Projects, r);
        let mut ps: Vec<&Project> = r
            .projects
            .values()
            .filter(|p| q.matches_project(p, r))
            .collect();
        ps.sort_by(|a, b| a.start_date.cmp(&b.start_date).then(a.code.cmp(&b.code)));
        let mut out = Vec::new();
        for p in ps {
            let m = r.metrics(p.id);
            let open = expanded.contains(&p.id);
            out.push(TRow {
                entity: Entity::Project,
                id: p.id,
                label: format!("{} {}", p.code, p.name),
                sub: format!("{}/{}", m.tasks_done, m.tasks_total),
                start: p.start_date.as_deref().and_then(Date::parse_iso),
                due: p.due_date.as_deref().and_then(Date::parse_iso),
                pct: m.pct_done(),
                cls: format!("pb h-{}", p.health.as_str()),
                depth: 0,
                expanded: open,
                milestone: false,
                deps: vec![],
            });
            if open {
                for tid in r.project_tasks(p.id) {
                    let t = &r.tasks[tid];
                    let s = t.start_date.as_deref().and_then(Date::parse_iso);
                    let d = t.due_date.as_deref().and_then(Date::parse_iso);
                    out.push(TRow {
                        entity: Entity::Task,
                        id: t.id,
                        label: format!("#{} {}", t.seq, t.title),
                        sub: r.username(t.assignee_id).to_string(),
                        start: s,
                        due: d,
                        pct: t.est_hours.filter(|e| *e > 0.0).map_or(
                            if t.status == TaskStatus::Done {
                                100.0
                            } else {
                                0.0
                            },
                            |e| (t.actual_hours / e * 100.0).min(100.0),
                        ),
                        cls: format!("tb ts-{}", t.status.as_str()),
                        depth: 1,
                        expanded: false,
                        milestone: s == d && t.est_hours.unwrap_or(0.0) == 0.0,
                        deps: t.deps.clone(),
                    });
                }
            }
        }
        out
    })
}

/// Shift start and due (move) or due only (resize) by `days`, as one mutate.
fn shift(app: App, e: Entity, id: i64, days: i32, resize: bool) {
    if days == 0 {
        return;
    }
    let ops = app.rep.with_untracked(|r| {
        let (s, d) = match e {
            Entity::Project => r
                .projects
                .get(&id)
                .map(|p| (p.start_date.clone(), p.due_date.clone())),
            _ => r
                .tasks
                .get(&id)
                .map(|t| (t.start_date.clone(), t.due_date.clone())),
        }?;
        let mv = |v: &Option<String>| {
            v.as_deref()
                .and_then(Date::parse_iso)
                .map(|x| x.add_days(days).iso())
        };
        let mut ops = Vec::new();
        if !resize {
            if let Some(ns) = mv(&s) {
                ops.push(Op::set(
                    e,
                    id,
                    "start_date",
                    s.clone().map_or(Value::Null, Value::from),
                    Value::from(ns),
                ));
            }
        }
        if let Some(nd) = mv(&d) {
            ops.push(Op::set(
                e,
                id,
                "due_date",
                d.clone().map_or(Value::Null, Value::from),
                Value::from(nd),
            ));
        }
        Some(ops)
    });
    if let Some(ops) = ops.filter(|o| !o.is_empty()) {
        app.mutate(ops, move |r| {
            if r.ok && r.error.is_none() && r.conflicts.is_empty() {
                app.say(
                    Level::Ok,
                    format!(
                        "{} {} day{}.",
                        if resize { "Due date moved" } else { "Moved" },
                        if days > 0 {
                            format!("+{days}")
                        } else {
                            days.to_string()
                        },
                        if days.abs() == 1 { "" } else { "s" }
                    ),
                );
            }
        });
    }
}

pub fn act(app: App, a: Act) {
    let tl = app.tl;
    let z = tl.zoom.get_untracked();
    match a {
        Act::ZoomIn => tl.zoom.set(z.saturating_sub(1)),
        Act::ZoomOut => tl.zoom.set((z + 1).min(2)),
        Act::Today => tl.scroll_to_today.update(|n| *n += 1),
        Act::MoveLeft | Act::MoveRight | Act::Shrink | Act::Extend => {
            let Some((e, id)) = tl.sel.get_untracked() else {
                return;
            };
            let u = unit_days(z);
            match a {
                Act::MoveLeft => shift(app, e, id, -u, false),
                Act::MoveRight => shift(app, e, id, u, false),
                Act::Shrink => shift(app, e, id, -u, true),
                _ => shift(app, e, id, u, true),
            }
        }
        _ => {}
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Drag {
    entity: Entity,
    id: i64,
    resize: bool,
    x0: f64,
    dx: f64,
}

#[component]
pub fn TimelineView() -> impl IntoView {
    let app = use_app();
    let tl = app.tl;
    let data = Memo::new(move |_| rows(app));
    let today = app.rep.with_untracked(|r| r.today);
    // date range covering every visible bar, padded
    let range = Memo::new(move |_| {
        let rs = data.get();
        let min = rs
            .iter()
            .filter_map(|r| r.start)
            .min()
            .unwrap_or(today)
            .min(today)
            .add_days(-14);
        let max = rs
            .iter()
            .filter_map(|r| r.due)
            .max()
            .unwrap_or(today)
            .max(today)
            .add_days(30);
        (min.week_start(), max)
    });
    let ppd = move || px_per_day(tl.zoom.get());
    let x_of = move |d: Date| (d.days() - range.get().0.days()) as f64 * ppd();
    let drag = RwSignal::new(None::<Drag>);
    let scroller = NodeRef::<leptos::html::Div>::new();
    // keep the selection valid and visible
    Effect::new(move |_| {
        let rs = data.get();
        if tl
            .sel
            .get_untracked()
            .is_none_or(|s| !rs.iter().any(|r| (r.entity, r.id) == s))
        {
            let i = tl.cur.get_untracked().min(rs.len().saturating_sub(1));
            tl.sel.set(rs.get(i).map(|r| (r.entity, r.id)));
        }
        if let Some(i) = tl
            .sel
            .get_untracked()
            .and_then(|s| rs.iter().position(|r| (r.entity, r.id) == s))
        {
            tl.cur.set(i);
        }
    });
    Effect::new(move |_| {
        let i = tl.cur.get();
        if let Some(d) = scroller.get_untracked() {
            let top = i as f64 * ROW;
            let st = d.scroll_top() as f64;
            let h = d.client_height() as f64 - HEAD - ROW;
            if top < st {
                d.set_scroll_top(top as i32);
            } else if top > st + h {
                d.set_scroll_top((top - h) as i32);
            }
        }
    });
    // jump to today on open and on `t`
    Effect::new(move |_| {
        let _ = tl.scroll_to_today.get();
        let _ = tl.zoom.get();
        request_animation_frame(move || {
            if let Some(d) = scroller.get_untracked() {
                let x = x_of(today);
                d.set_scroll_left((x - d.client_width() as f64 * 0.3).max(0.0) as i32);
            }
        });
    });
    let on_key = move |e: web_sys::KeyboardEvent| {
        let rs = data.get_untracked();
        let n = rs.len();
        let cur = tl.cur.get_untracked().min(n.saturating_sub(1));
        let set = |i: usize| {
            tl.cur.set(i);
            tl.sel.set(rs.get(i).map(|r| (r.entity, r.id)));
        };
        let row = rs.get(cur).cloned();
        match e.key().as_str() {
            "ArrowDown" => set((cur + 1).min(n.saturating_sub(1))),
            "ArrowUp" => set(cur.saturating_sub(1)),
            "PageDown" => set((cur + 15).min(n.saturating_sub(1))),
            "PageUp" => set(cur.saturating_sub(15)),
            "Home" => set(0),
            "End" => set(n.saturating_sub(1)),
            "ArrowRight" if e.shift_key() => act(app, Act::MoveRight),
            "ArrowLeft" if e.shift_key() => act(app, Act::MoveLeft),
            "ArrowRight" | "Enter" => {
                if let Some(r) = row.clone().filter(|r| r.entity == Entity::Project) {
                    tl.expanded.update(|x| {
                        if e.key() == "Enter" && x.contains(&r.id) {
                            x.remove(&r.id);
                        } else {
                            x.insert(r.id);
                        }
                    });
                } else if e.key() == "Enter" {
                    if let Some(r) = row {
                        app.modal.set(Some(Modal::Record {
                            entity: r.entity,
                            id: r.id,
                        }));
                    }
                }
            }
            "ArrowLeft" => {
                if let Some(r) = row {
                    if r.entity == Entity::Project {
                        tl.expanded.update(|x| {
                            x.remove(&r.id);
                        });
                    } else if let Some(pi) =
                        rs[..cur].iter().rposition(|x| x.entity == Entity::Project)
                    {
                        set(pi);
                    }
                }
            }
            "[" => act(app, Act::Shrink),
            "]" => act(app, Act::Extend),
            "+" | "=" => act(app, Act::ZoomIn),
            "-" | "_" => act(app, Act::ZoomOut),
            "t" | "T" => act(app, Act::Today),
            _ => return,
        }
        e.prevent_default();
        e.stop_propagation();
    };
    let on_move = move |e: web_sys::PointerEvent| {
        if let Some(mut d) = drag.get_untracked() {
            d.dx = e.client_x() as f64 - d.x0;
            drag.set(Some(d));
        }
    };
    let on_up = move |_e: web_sys::PointerEvent| {
        if let Some(d) = drag.get_untracked() {
            drag.set(None);
            let days = (d.dx / ppd()).round() as i32;
            shift(app, d.entity, d.id, days, d.resize);
        }
    };
    let width = move || {
        let (a, b) = range.get();
        (b.days() - a.days()) as f64 * ppd()
    };
    view! {
        <div class="pane main full tl-pane">
            <div class="ptitle">
                <span>"Timeline"</span>
                <span class="muted">{move || format!("{} rows · zoom: {}", data.with(|d| d.len()), ["day", "week", "month"][tl.zoom.get()])}</span>
                <span class="spacer"></span>
                <button class="btn sm" on:click=move |_| act(app, Act::ZoomIn)>"+ Zoom in"</button>
                <button class="btn sm" on:click=move |_| act(app, Act::ZoomOut)>"− Zoom out"</button>
                <button class="btn sm" on:click=move |_| act(app, Act::Today)>"t Today"</button>
                <span class="legend"><i class="lg h-green"></i>"on track "<i class="lg h-amber"></i>"at risk "<i class="lg h-red"></i>"off track"</span>
            </div>
            <div class="tl" id="timeline" tabindex="0" data-pane="1" node_ref=scroller
                on:focus=move |_| app.ctx.set(Ctx::Timeline) on:keydown=on_key
                on:pointermove=on_move on:pointerup=on_up>
                <div class="tl-in" style:width=move || format!("{}px", width() + LEFT_W)>
                    <div class="tl-top">
                        <div class="tl-corner" style:width=format!("{LEFT_W}px")>"Project / task"</div>
                        <svg class="tl-axis" width=width height=HEAD>
                            {move || {
                                let (a, b) = range.get();
                                let z = tl.zoom.get();
                                let mut out = Vec::new();
                                let mut d = a;
                                while d <= b {
                                    let x = x_of(d);
                                    if d.day() == 1 {
                                        out.push(view! { <line x1=x x2=x y1="0" y2=HEAD class="mline" /> }.into_any());
                                        out.push(view! { <text x=x + 4.0 y="13" class="mtext">{format!("{} {}", MONTH_NAMES[d.month() as usize - 1], d.year())}</text> }.into_any());
                                    }
                                    if z == 0 {
                                        out.push(view! { <text x=x + ppd() / 2.0 y="29" class="dtext" text-anchor="middle">{d.day()}</text> }.into_any());
                                    } else if z == 1 && d.weekday() == 0 {
                                        out.push(view! { <text x=x + 2.0 y="29" class="dtext">{d.day()}</text> }.into_any());
                                    }
                                    d = d.add_days(1);
                                }
                                let tx = x_of(today);
                                out.push(view! { <line x1=tx x2=tx y1="4" y2=HEAD class="today" /> }.into_any());
                                out.push(view! { <text x=tx - 3.0 y="13" class="todayt" text-anchor="end">"today"</text> }.into_any());
                                out.collect_view()
                            }}
                        </svg>
                    </div>
                    <div class="tl-body">
                        <div class="tl-left" style:width=format!("{LEFT_W}px")>
                            {move || data.get().into_iter().enumerate().map(|(i, r)| {
                                let (e, id) = (r.entity, r.id);
                                let cls = move || if tl.cur.get() == i { "tl-row cur" } else { "tl-row" };
                                let tog = if r.entity == Entity::Project { if r.expanded { "▾" } else { "▸" } } else { "" };
                                view! {
                                    <div class=cls style:padding-left=format!("{}px", 6 + r.depth as usize * 16)
                                        on:mousedown=move |_| { tl.cur.set(i); tl.sel.set(Some((e, id))); }
                                        on:dblclick=move |_| app.modal.set(Some(Modal::Record { entity: e, id }))>
                                        <span class="tog" on:mousedown=move |ev| { ev.stop_propagation(); if e == Entity::Project { tl.expanded.update(|x| { if !x.remove(&id) { x.insert(id); } }); } }>{tog}</span>
                                        <span class="tl-l" class:strong={r.depth == 0}>{r.label.clone()}</span>
                                        <span class="tl-s muted">{r.sub.clone()}</span>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                        <svg class="tl-svg" width=width height=move || data.with(|d| d.len()) as f64 * ROW>
                            <defs>
                                <marker id="arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                                    <path d="M0,0 L8,4 L0,8 z" class="arrowhead" />
                                </marker>
                            </defs>
                            {move || {
                                let (a, b) = range.get();
                                let z = tl.zoom.get();
                                let h = data.with(|d| d.len()) as f64 * ROW;
                                let mut out = Vec::new();
                                let mut d = a;
                                while d <= b {
                                    let x = x_of(d);
                                    if z < 2 && d.weekday() == 5 {
                                        out.push(view! { <rect x=x y="0" width=ppd() * 2.0 height=h class="we" /> }.into_any());
                                    }
                                    if d.day() == 1 {
                                        out.push(view! { <line x1=x x2=x y1="0" y2=h class="mline" /> }.into_any());
                                    } else if z == 1 && d.weekday() == 0 {
                                        out.push(view! { <line x1=x x2=x y1="0" y2=h class="wline" /> }.into_any());
                                    }
                                    d = d.add_days(1);
                                }
                                out.collect_view()
                            }}
                            {move || {
                                let rs = data.get();
                                let dr = drag.get();
                                let cur_i = tl.cur.get();
                                let pos: std::collections::HashMap<i64, usize> = rs.iter().enumerate().filter(|(_, r)| r.entity == Entity::Task).map(|(i, r)| (r.id, i)).collect();
                                let mut arrows = Vec::new();
                                let bars = rs.iter().enumerate().map(|(i, r)| {
                                    let y = i as f64 * ROW;
                                    let cur = cur_i == i;
                                    let bg = if cur { "rowbg cur" } else if i % 2 == 1 { "rowbg odd" } else { "rowbg" };
                                    let (Some(s), Some(d)) = (r.start.or(r.due), r.due.or(r.start)) else {
                                        return view! { <rect x="0" y=y width="100%" height=ROW class=bg /> }.into_any();
                                    };
                                    let mut x = x_of(s);
                                    let mut w = ((d.days() - s.days() + 1) as f64 * ppd()).max(4.0);
                                    if let Some(dd) = dr.filter(|dd| dd.entity == r.entity && dd.id == r.id) {
                                        if dd.resize { w = (w + dd.dx).max(4.0); } else { x += dd.dx; }
                                    }
                                    for dep in &r.deps {
                                        if let Some(di) = pos.get(dep) {
                                            if let Some(dd) = rs[*di].due {
                                                let x1 = x_of(dd) + ppd();
                                                let y1 = *di as f64 * ROW + ROW / 2.0;
                                                let y2 = y + ROW / 2.0;
                                                let mid = x1 + 6.0;
                                                let path = format!("M{x1},{y1} H{mid} V{y2} H{}", x - 1.0);
                                                arrows.push(view! { <path d=path class="dep" marker-end="url(#arrow)" /> }.into_any());
                                            }
                                        }
                                    }
                                    let (e, id) = (r.entity, r.id);
                                    let start_drag = move |resize: bool| move |ev: web_sys::PointerEvent| {
                                        ev.prevent_default();
                                        ev.stop_propagation();
                                        tl.cur.set(i);
                                        tl.sel.set(Some((e, id)));
                                        if let Some(t) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                                            let _ = t.set_pointer_capture(ev.pointer_id());
                                        }
                                        drag.set(Some(Drag { entity: e, id, resize, x0: ev.client_x() as f64, dx: 0.0 }));
                                    };
                                    let bh = if r.depth == 0 { 14.0 } else { 11.0 };
                                    let by = y + (ROW - bh) / 2.0;
                                    let label = r.label.clone();
                                    let title = format!("{label}\n{} → {}  ·  {:.0}%  ·  drag to move, drag the right edge to resize", s.iso(), d.iso(), r.pct);
                                    if r.milestone {
                                        let cx = x + ppd() / 2.0;
                                        let pts = format!("{},{} {},{} {},{} {},{}", cx, by - 2.0, cx + 7.0, by + bh / 2.0, cx, by + bh + 2.0, cx - 7.0, by + bh / 2.0);
                                        return view! {
                                            <rect x="0" y=y width="100%" height=ROW class=bg />
                                            <polygon points=pts class=format!("ms {}", r.cls) on:pointerdown=start_drag(false)><title>{title}</title></polygon>
                                        }.into_any();
                                    }
                                    view! {
                                        <rect x="0" y=y width="100%" height=ROW class=bg />
                                        <g class={if cur { "bar-g sel" } else { "bar-g" }}>
                                            <rect x=x y=by width=w height=bh rx="3" class=r.cls.clone() on:pointerdown=start_drag(false)><title>{title}</title></rect>
                                            <rect x=x y=by width=w * r.pct / 100.0 height=bh rx="3" class="prog" pointer-events="none" />
                                            <rect x=x + w - 5.0 y=by width="7" height=bh class="grip" on:pointerdown=start_drag(true) />
                                            {(w > 70.0 && r.depth == 0).then(|| view! { <text x=x + 5.0 y=by + bh - 4.0 class="btext" pointer-events="none">{label.chars().take((w / 7.0) as usize).collect::<String>()}</text> })}
                                        </g>
                                    }.into_any()
                                }).collect::<Vec<_>>();
                                let tx = x_of(today);
                                let h = rs.len() as f64 * ROW;
                                view! { {bars}{arrows}<line x1=tx x2=tx y1="0" y2=h class="today" /> }
                            }}
                        </svg>
                    </div>
                </div>
            </div>
        </div>
    }
}
