//! The virtualised, editable data grid (ui-spec §3).
//!
//! One scroll container holds a sticky header, top/bottom spacers and only
//! the rows in view. Frozen columns and the selection gutter use
//! `position: sticky`, so horizontal scrolling keeps them in place.

use leptos::prelude::*;
use pt_core::grid::{self, Column, GridRow};

use crate::app::{App, Grid, GridKey, Mark, Modal, use_app};
use crate::cells::{self, initials};
use crate::editor::CellEditor;
use crate::gridops::{self, row_h};
use crate::util;

pub const GUTTER: f64 = 30.0;
const OVERSCAN: usize = 8;

pub fn px(w: u16) -> f64 {
    (w as f64 * 7.5 + 16.0).round()
}

/// Left offsets for the frozen columns (sticky).
fn frozen_lefts(cols: &[Column], frozen: usize) -> Vec<Option<f64>> {
    let mut x = GUTTER;
    cols.iter()
        .enumerate()
        .map(|(i, c)| {
            if i < frozen {
                let l = x;
                x += px(c.width);
                Some(l)
            } else {
                None
            }
        })
        .collect()
}

fn total_width(cols: &[Column]) -> f64 {
    GUTTER + cols.iter().map(|c| px(c.width)).sum::<f64>()
}

#[component]
pub fn GridView(key: GridKey, #[prop(optional)] empty: Option<&'static str>) -> impl IntoView {
    let app = use_app();
    let g = app.grid(key);
    let el = NodeRef::<leptos::html::Div>::new();
    let scroll = RwSignal::new(0.0f64);
    let rh = move || if app.compact.get() { 21.0 } else { 25.0 };
    let range = Memo::new(move |_| {
        let n = g.rows.with(|r| r.len());
        let h = rh();
        let _ = el.get();
        let viewport = util::viewport().1.max(400.0);
        let start = ((scroll.get() / h) as usize).saturating_sub(OVERSCAN);
        let end = (start + (viewport / h) as usize + OVERSCAN * 2).min(n);
        (start.min(n), end, n)
    });

    // Keep the cursor visible when it moves (keyboard or programmatic).
    Effect::new(move |_| {
        let row = g.cur_row.get();
        let col = g.cur_col.get();
        let Some(div) = el.get_untracked() else {
            return;
        };
        let h = row_h(&app);
        let top = row as f64 * h;
        let header = h + 4.0;
        let view_h = div.client_height() as f64 - header - h;
        let st = div.scroll_top() as f64;
        if top < st {
            div.set_scroll_top(top as i32);
        } else if top > st + view_h {
            div.set_scroll_top((top - view_h) as i32);
        }
        let cols = g.cols.get_untracked();
        if col >= g.frozen && col < cols.len() {
            let lefts: f64 = cols[..g.frozen.min(cols.len())]
                .iter()
                .map(|c| px(c.width))
                .sum::<f64>()
                + GUTTER;
            let x: f64 = cols[..col].iter().map(|c| px(c.width)).sum::<f64>() + GUTTER;
            let w = px(cols[col].width);
            let sl = div.scroll_left() as f64;
            let vw = div.client_width() as f64;
            if x - sl < lefts {
                div.set_scroll_left((x - lefts).max(0.0) as i32);
            } else if x + w - sl > vw {
                div.set_scroll_left((x + w - vw) as i32);
            }
        }
    });

    let on_key = move |e: web_sys::KeyboardEvent| {
        if gridops::key(app, g, &e) {
            e.prevent_default();
            e.stop_propagation();
        }
    };
    let on_paste = move |e: web_sys::ClipboardEvent| {
        if g.edit.get_untracked().is_some() {
            return;
        }
        if let Some(t) = e
            .clipboard_data()
            .and_then(|d| d.get_data("text/plain").ok())
        {
            e.prevent_default();
            gridops::paste(app, g, &t);
        }
    };
    let width = move || format!("{}px", g.cols.with(|c| total_width(c)));
    let n_rows = move || g.rows.with(|r| r.len());
    view! {
        <div class="grid" id=key.dom_id() tabindex="0" data-pane="1" node_ref=el
            on:keydown=on_key
            on:focus=move |_| gridops::on_focus(app, g)
            on:scroll=move |_| { if let Some(d) = el.get_untracked() { scroll.set(d.scroll_top() as f64); } }
            on:paste=on_paste>
            <div class="gin" style:width=width>
                <Header g=g />
                <div style:height=move || format!("{}px", range.get().0 as f64 * rh())></div>
                <For each=move || { let (s, e, _) = range.get(); (s..e).collect::<Vec<_>>() }
                    key=|i| *i
                    children=move |i| view! { <Row g=g i=i /> } />
                <div style:height=move || { let (_, e, n) = range.get(); format!("{}px", (n - e) as f64 * rh()) }></div>
                <Footer g=g />
            </div>
            <Show when=move || n_rows() == 0>
                <div class="gempty">{empty.unwrap_or("No rows match.")}</div>
            </Show>
        </div>
    }
}

#[component]
fn Header(g: Grid) -> impl IntoView {
    let app = use_app();
    view! {
        <div class="ghead">
            <div class="gutter hcell" on:click=move |_| gridops::mark_all(g) title="Mark all (^A)">
                {move || if g.marks.with(|m| m.is_empty()) { "".to_string() } else { format!("{}", g.marks.with(|m| m.len())) }}
            </div>
            {move || {
                let cols = g.cols.get();
                let lefts = frozen_lefts(&cols, g.frozen);
                let sort = g.sort.get();
                let group = g.group.get();
                cols.into_iter().enumerate().map(|(ci, c)| {
                    let w = px(c.width);
                    let left = lefts[ci];
                    let si = sort.iter().position(|s| s.key == c.key);
                    let arrow = si.map(|i| {
                        let a = if sort[i].desc { "▼" } else { "▲" };
                        if sort.len() > 1 { format!("{a}{}", i + 1) } else { a.to_string() }
                    });
                    let grouped = group.as_deref() == Some(c.key.as_str());
                    let key = c.key.clone();
                    let key2 = c.key.clone();
                    let title = c.title.clone();
                    let cls = format!("hcell{}{}{}", if left.is_some() { " frz" } else { "" },
                        if matches!(c.align, grid::Align::Right) { " r" } else { "" },
                        if ci + 1 == g.frozen { " frz-last" } else { "" });
                    view! {
                        <div class=cls style:width=format!("{w}px") style:left=left.map(|l| format!("{l}px"))
                            title=format!("{title} — click to sort, Shift+click to add, drag edge to resize")
                            on:click=move |e: web_sys::MouseEvent| { gridops::toggle_sort(g, &key, e.shift_key()); }>
                            <span class="ht">{c.title.clone()}</span>
                            {grouped.then(|| view! { <span class="hg" title="grouped">"⊞"</span> })}
                            {arrow.map(|a| view! { <span class="ha">{a}</span> })}
                            <span class="rs" on:click=|e| e.stop_propagation()
                                on:pointerdown=move |e: web_sys::PointerEvent| start_resize(app, g, key2.clone(), e)></span>
                        </div>
                    }
                }).collect_view()
            }}
        </div>
    }
}

/// Drag a header edge to resize a column (width in characters).
fn start_resize(_app: App, g: Grid, key: String, e: web_sys::PointerEvent) {
    e.prevent_default();
    e.stop_propagation();
    let start_x = e.client_x() as f64;
    let start_w = g
        .all_cols
        .with_untracked(|c| c.iter().find(|c| c.key == key).map_or(10, |c| c.width));
    let handles: StoredValue<Vec<WindowListenerHandle>, LocalStorage> =
        StoredValue::new_local(vec![]);
    let k = key.clone();
    let mv = window_event_listener(leptos::ev::pointermove, move |e| {
        let dx = e.client_x() as f64 - start_x;
        let w = ((start_w as f64 + dx / 7.5).round() as i64).clamp(3, 120) as u16;
        g.layout.update(|l| {
            l.widths.insert(k.clone(), w);
        });
    });
    let up = window_event_listener(leptos::ev::pointerup, move |_| {
        handles.update_value(|h| {
            for x in h.drain(..) {
                x.remove();
            }
        });
    });
    handles.update_value(|h| {
        h.push(mv);
        h.push(up);
    });
}

#[component]
fn Row(g: Grid, i: usize) -> impl IntoView {
    move || match g.rows.with(|r| r.get(i).cloned()) {
        Some(GridRow::Data(id)) => view! { <DataRow g=g i=i id=id /> }.into_any(),
        Some(GridRow::Group {
            key,
            label,
            count,
            collapsed,
            sums,
            style,
        }) => group_row(g, i, key, label, count, collapsed, sums, style).into_any(),
        None => ().into_any(),
    }
}

#[allow(clippy::too_many_arguments)]
fn group_row(
    g: Grid,
    i: usize,
    key: String,
    label: String,
    count: usize,
    collapsed: bool,
    sums: Vec<Option<f64>>,
    style: grid::Style,
) -> impl IntoView {
    let cur = move || g.cur_row.get() == i;
    let k2 = key.clone();
    view! {
        <div class="row grp" class:currow=cur on:mousedown=move |_| { gridops::set_cursor(g, i, g.cur_col.get_untracked()); gridops::focus_grid(g); }
            on:dblclick=move |_| gridops::toggle_group(g, None)>
            <div class="gutter gtog" on:click=move |_| { g.collapsed.update(|c| { if !c.remove(&k2) { c.insert(k2.clone()); } }); }>
                {if collapsed { "▸" } else { "▾" }}
            </div>
            {move || {
                let cols = g.cols.get();
                let lefts = frozen_lefts(&cols, g.frozen);
                let frozen_w: f64 = cols.iter().take(g.frozen.max(1)).map(|c| px(c.width)).sum();
                let lbl = label.clone();
                let dot = crate::widgets::style_dot(&style);
                let mut out = vec![view! {
                    <div class="cell glabel frz" style:width=format!("{frozen_w}px") style:left=format!("{GUTTER}px") title=lbl.clone()>
                        {dot}<b>{lbl.clone()}</b><span class="gcount">{count}</span>
                    </div>
                }.into_any()];
                for (ci, c) in cols.iter().enumerate().skip(g.frozen.max(1)) {
                    let _ = lefts[ci];
                    let sum = sums.get(all_index(g, &c.key)).copied().flatten();
                    let txt = sum.map(fmt_total).unwrap_or_default();
                    out.push(view! { <div class="cell r gsum" style:width=format!("{}px", px(c.width))>{txt}</div> }.into_any());
                }
                out.collect_view()
            }}
        </div>
    }
}

/// Index of a column key within `all_cols` (group sums align with it).
fn all_index(g: Grid, key: &str) -> usize {
    g.all_cols
        .with_untracked(|c| c.iter().position(|c| c.key == key).unwrap_or(usize::MAX))
}

#[component]
fn DataRow(g: Grid, i: usize, id: i64) -> impl IntoView {
    let app = use_app();
    let cls = move || {
        let mut s = String::from("row");
        if i % 2 == 1 {
            s.push_str(" odd");
        }
        if g.cur_row.get() == i {
            s.push_str(" currow");
        }
        if g.marks.with(|m| m.contains(&id)) {
            s.push_str(" marked");
        }
        if app.flash.with(|f| f.contains(&(g.entity, id))) {
            s.push_str(" flash");
        }
        s
    };
    // other users focused on this row
    let watchers = move || {
        let me = app.rep.with_untracked(|r| r.conn);
        app.peers.with(|ps| {
            ps.iter()
                .filter(|p| {
                    p.conn != me
                        && p.focus
                            .as_ref()
                            .is_some_and(|f| f.entity == g.entity && f.id == id)
                })
                .map(|p| (p.user_id, p.focus.as_ref().and_then(|f| f.field.clone())))
                .collect::<Vec<_>>()
        })
    };
    let gutter = move || {
        let w = watchers();
        if let Some((uid, _)) = w.first() {
            let name = app
                .rep
                .with_untracked(|r| r.user_name(Some(*uid)).to_string());
            let st = format!("background:var(--u{})", uid.rem_euclid(8));
            return view! { <span class="pav" style=st title=format!("{name} is here")>{initials(&name)}</span> }.into_any();
        }
        if g.marks.with(|m| m.contains(&id)) {
            view! { <span class="mk">"●"</span> }.into_any()
        } else {
            ().into_any()
        }
    };
    view! {
        <div class=cls data-id=id>
            <div class="gutter" on:mousedown=move |_| { gridops::set_cursor(g, i, g.cur_col.get_untracked()); gridops::mark_row(g); gridops::focus_grid(g); }>
                {gutter}
            </div>
            {move || {
                let cols = g.cols.get();
                let lefts = frozen_lefts(&cols, g.frozen);
                cols.into_iter().enumerate().map(|(ci, c)| {
                    let left = lefts[ci];
                    view! { <GridCell g=g i=i ci=ci id=id col=c left=left watchers=Signal::derive(watchers) /> }
                }).collect_view()
            }}
        </div>
    }
}

#[component]
fn GridCell(
    g: Grid,
    i: usize,
    ci: usize,
    id: i64,
    col: Column,
    left: Option<f64>,
    watchers: Signal<Vec<(i64, Option<String>)>>,
) -> impl IntoView {
    let app = use_app();
    let field = col.field.clone().unwrap_or_default();
    let field_c = field.clone();
    let editing = {
        let key = col.key.clone();
        Memo::new(move |_| {
            g.edit
                .with(|e| e.as_ref().filter(|e| e.id == id && e.col == key).cloned())
        })
    };
    let is_cur = move || g.cur_row.get() == i && g.cur_col.get() == ci;
    let entity = g.entity;
    let cls = {
        let field = field.clone();
        let align = col.align;
        let frozen_last = ci + 1 == g.frozen;
        move || {
            let mut s = String::from("cell");
            if left.is_some() {
                s.push_str(" frz");
            }
            if frozen_last {
                s.push_str(" frz-last");
            }
            if matches!(align, grid::Align::Right) {
                s.push_str(" r");
            }
            if is_cur() {
                s.push_str(" cur");
            }
            if editing.with(|e| e.is_some()) {
                s.push_str(" editing");
            }
            if !field.is_empty() {
                match app.mark_of(entity, id, &field) {
                    Some(Mark::Pending) => s.push_str(" pending"),
                    Some(Mark::Conflict(_)) => s.push_str(" conflict"),
                    None => {}
                }
            }
            s
        }
    };
    let title = {
        let field = field.clone();
        move || match app.mark_of(entity, id, &field) {
            Some(Mark::Conflict(m)) => {
                format!("⚠ {m}. Enter: edit from their value · Esc: dismiss")
            }
            _ => String::new(),
        }
    };
    let underline = move || {
        watchers.with(|w| {
            w.iter()
                .find(|(_, f)| f.as_deref() == Some(field_c.as_str()) && !field_c.is_empty())
                .map(|(u, _)| format!("inset 0 -2px 0 var(--u{})", u.rem_euclid(8)))
        })
    };
    let col2 = col.clone();
    let content = move || {
        if let Some(st) = editing.get() {
            return view! { <CellEditor g=g st=st /> }.into_any();
        }
        let c = app.rep.with(|r| {
            if g.key == GridKey::Activity && col2.key == "ts" {
                let mut c = grid::cell(r, &col2, id);
                if let Some(a) = r.activity.get(&id) {
                    c.text = grid::activity_when(a, util::now_secs());
                }
                c
            } else {
                grid::cell(r, &col2, id)
            }
        });
        let conflict = matches!(
            app.mark_of(entity, id, col2.field.as_deref().unwrap_or("-")),
            Some(Mark::Conflict(_))
        );
        view! { {conflict.then(|| view! { <i class="cw">"⚠"</i> })}{cells::render(c)} }.into_any()
    };
    let w = px(col.width);
    view! {
        <div class=cls data-col=col.key.clone() style:width=format!("{w}px") style:left=left.map(|l| format!("{l}px")) style:box-shadow=underline title=title
            on:mousedown=move |e: web_sys::MouseEvent| {
                if g.edit.get_untracked().is_some_and(|ed| ed.id == id && ed.col == col.key) { return; }
                if e.button() == 0 || e.button() == 2 {
                    gridops::set_cursor(g, i, ci);
                    if e.shift_key() { gridops::extend_marks(g, 0); }
                    gridops::focus_grid(g);
                }
            }
            on:dblclick=move |_| gridops::start_edit(app, g, None)
            on:contextmenu=move |e: web_sys::MouseEvent| {
                e.prevent_default();
                app.modal.set(Some(Modal::Context { x: e.client_x() as f64, y: e.client_y() as f64, grid: g.key }));
            }>
            {content}
        </div>
    }
}

#[component]
fn Footer(g: Grid) -> impl IntoView {
    let app = use_app();
    view! {
        <div class="gfoot">
            <div class="gutter">"Σ"</div>
            {move || {
                let cols = g.cols.get();
                let ids = g.ids.get();
                let all = g.all_cols.get();
                let sums = app.rep.with(|r| grid::totals(r, &all, &ids));
                let lefts = frozen_lefts(&cols, g.frozen);
                let marked = g.marks.with(|m| m.len());
                let nf = g.frozen.max(1).min(cols.len());
                let frozen_w: f64 = cols.iter().take(nf).map(|c| px(c.width)).sum();
                let label = if marked > 0 { format!("{} rows · {} marked", ids.len(), marked) } else { format!("{} rows", ids.len()) };
                let mut out = vec![view! {
                    <div class="cell frz flabel2" style:width=format!("{frozen_w}px") style:left=format!("{GUTTER}px")>{label}</div>
                }.into_any()];
                for (ci, c) in cols.iter().enumerate().skip(nf) {
                    let _ = lefts[ci];
                    let w = px(c.width);
                    let txt = all.iter().position(|a| a.key == c.key).and_then(|i| sums[i]).map(fmt_total).unwrap_or_default();
                    out.push(view! { <div class="cell r" style:width=format!("{w}px")>{txt}</div> }.into_any());
                }
                out.collect_view()
            }}
        </div>
    }
}

/// Grids for read-only reasons show a lock in their title bar.
pub fn grid_title(app: App, g: Grid, title: &'static str) -> impl IntoView {
    let ro = move || !app.rep.with(|r| g.editable(r));
    view! {
        <div class="ptitle">
            <span>{title}</span>
            <Show when=ro><span class="lock" title="read only">"🔒 read-only"</span></Show>
            <span class="pcount">{move || format!("{}", g.ids.with(|i| i.len()))}</span>
        </div>
    }
}

/// Totals without "-0".
pub fn fmt_total(x: f64) -> String {
    grid::fmt_hours(if x.abs() < 0.005 { 0.0 } else { x })
}
