//! Tab 10, Grid: an editable table of `grid.rows` with typed cell editors,
//! a computed Total column, a totals row and a view-only sort.

use demo_core::dataset::{STATUSES, choice};
use demo_core::date::parse_natural;
use demo_core::keys::{GridRow, MAX_ITEM, MAX_PRICE, MAX_QTY, next_id, num_value, round2};
use leptos::prelude::*;
use serde_json::{Value, json};
use wasm_bindgen::JsCast;

use super::TabFrame;
use crate::app::{App, ConfirmAct, Ctx, Modal, use_app};
use crate::util;
use crate::widgets::{Dropdown, chip, opts};

const KEY: &str = "grid.rows";
const COLS: [(&str, &str); 7] = [
    ("id", "#"),
    ("item", "Item"),
    ("qty", "Qty"),
    ("price", "Price"),
    ("total", "Total"),
    ("status", "Status"),
    ("due", "Due"),
];

fn editable(col: usize) -> bool {
    matches!(col, 1 | 2 | 3 | 5 | 6)
}

fn rows_of(v: &Value) -> Vec<GridRow> {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

fn money(x: f64) -> String {
    format!("{x:.2}")
}

/// Replace one row (by id) in the latest list and send it.
fn update_row(app: App, id: u64, f: impl FnOnce(&mut GridRow)) {
    let mut rows = rows_of(&app.latest(KEY));
    if let Some(r) = rows.iter_mut().find(|r| r.id == id) {
        f(r);
        app.set_latest(KEY, json!(rows));
    }
}

pub fn delete_row(app: App, id: u64) {
    let mut rows = rows_of(&app.latest(KEY));
    rows.retain(|r| r.id != id);
    app.set_latest(KEY, json!(rows));
}

/// The cell's text as the editor starts with it.
fn cell_text(r: &GridRow, col: usize) -> String {
    match col {
        1 => r.item.clone(),
        2 => r.qty.to_string(),
        3 => serde_json::to_string(&num_value(r.price)).unwrap_or_default(),
        6 => r.due.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// A change to apply to one row.
type RowEdit = Box<dyn FnOnce(&mut GridRow)>;

/// Parse an edited cell into a change, or the reason it's invalid.
fn parse_cell(col: usize, text: &str) -> Result<RowEdit, String> {
    let t = text.trim().to_string();
    match col {
        1 if t.is_empty() => Err("item is required".into()),
        1 if t.chars().count() > MAX_ITEM => Err(format!("at most {MAX_ITEM} characters")),
        1 => Ok(Box::new(move |r| r.item = t)),
        2 => match t.parse::<i64>() {
            Ok(q) if (0..=MAX_QTY).contains(&q) => Ok(Box::new(move |r| r.qty = q as u64)),
            _ => Err(format!("a whole number 0–{MAX_QTY}")),
        },
        3 => match t.parse::<f64>() {
            Ok(p) if p.is_finite() && (0.0..=MAX_PRICE).contains(&p) => {
                Ok(Box::new(move |r| r.price = round2(p)))
            }
            _ => Err("a price 0–1,000,000".into()),
        },
        6 => match parse_natural(&t, util::today()) {
            Ok(d) => Ok(Box::new(move |r| r.due = d.map(|d| d.iso()))),
            Err(e) => Err(e),
        },
        _ => Err("read-only".into()),
    }
}

#[derive(Clone, PartialEq, Debug)]
struct Edit {
    id: u64,
    col: usize,
    text: String,
}

#[component]
pub fn GridTab() -> impl IntoView {
    let app = use_app();
    let raw = app.watch(KEY);
    let rows = Memo::new(move |_| raw.with(rows_of));
    // (column, ascending); a view only: the stored order never changes
    let sort = RwSignal::new(None::<(usize, bool)>);
    let order = Memo::new(move |_| {
        let rs = rows.get();
        let mut idx: Vec<usize> = (0..rs.len()).collect();
        if let Some((c, asc)) = sort.get() {
            idx.sort_by(|&a, &b| {
                let (x, y) = (&rs[a], &rs[b]);
                let o = match c {
                    0 => x.id.cmp(&y.id),
                    1 => x.item.to_lowercase().cmp(&y.item.to_lowercase()),
                    2 => x.qty.cmp(&y.qty),
                    3 => x.price.total_cmp(&y.price),
                    4 => x.total().total_cmp(&y.total()),
                    5 => STATUSES
                        .iter()
                        .position(|s| s.id == x.status)
                        .cmp(&STATUSES.iter().position(|s| s.id == y.status)),
                    // no date sorts last
                    _ => x
                        .due
                        .is_none()
                        .cmp(&y.due.is_none())
                        .then(x.due.cmp(&y.due)),
                };
                if asc { o } else { o.reverse() }
            });
        }
        idx
    });
    let cur = RwSignal::new((0usize, 1usize));
    let edit = RwSignal::new(None::<Edit>);
    let err = RwSignal::new(None::<String>);
    let pending_edit = RwSignal::new(None::<u64>);
    let n_rows = move || order.with(|o| o.len());
    let row_at = move |i: usize| -> Option<GridRow> {
        let o = order.with_untracked(|o| o.get(i).copied())?;
        rows.with_untracked(|r| r.get(o).cloned())
    };
    let focus_grid = move || util::focus_sel(".grid[data-control='grid.rows']");
    let start_edit = move |seed: Option<String>| {
        let (ri, c) = cur.get_untracked();
        let Some(r) = row_at(ri) else { return };
        if !editable(c) {
            return;
        }
        err.set(None);
        edit.set(Some(Edit {
            id: r.id,
            col: c,
            text: seed.unwrap_or_else(|| cell_text(&r, c)),
        }));
        app.ctx.set(Ctx::Body("cell"));
        queue_microtask(|| {
            if let Some(i) = util::query(".grid .ed")
                .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
            {
                let _ = i.focus();
            } else {
                util::focus_sel(".grid .dd");
            }
        });
    };
    // after Ins, edit the new row's Item once it arrives
    Effect::new(move |_| {
        let Some(id) = pending_edit.get() else { return };
        let rs = rows.get();
        if let Some(pos) = rs.iter().position(|r| r.id == id) {
            let vi = order
                .with_untracked(|o| o.iter().position(|&x| x == pos))
                .unwrap_or(0);
            pending_edit.set(None);
            cur.set((vi, 1));
            start_edit(None);
        }
    });
    // keep the cursor inside the table as rows come and go
    Effect::new(move |_| {
        let n = n_rows();
        cur.update(|(r, _)| *r = (*r).min(n.saturating_sub(1)));
    });
    let finish = move |down: bool| {
        edit.set(None);
        err.set(None);
        if down {
            cur.update(|(r, _)| *r = (*r + 1).min(n_rows().saturating_sub(1)));
        }
        app.ctx.set(Ctx::Body("grid"));
        focus_grid();
    };
    let commit_text = move || {
        let Some(ed) = edit.get_untracked() else {
            return;
        };
        match parse_cell(ed.col, &ed.text) {
            Ok(f) => {
                update_row(app, ed.id, f);
                finish(true);
            }
            Err(e) => err.set(Some(e)),
        }
    };
    let add_row = move || {
        let rs = rows_of(&app.latest(KEY));
        let id = next_id(rs.iter().map(|r| r.id));
        let mut list = rs;
        list.push(GridRow {
            id,
            item: "New item".into(),
            qty: 1,
            price: 0.0,
            status: "todo".into(),
            due: None,
        });
        app.set_latest(KEY, json!(list));
        pending_edit.set(Some(id));
    };
    let cycle_sort = move |c: usize| {
        sort.update(|s| {
            *s = match *s {
                Some((sc, true)) if sc == c => Some((c, false)),
                Some((sc, false)) if sc == c => None,
                _ => Some((c, true)),
            }
        })
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        if e.default_prevented() || edit.with_untracked(|x| x.is_some()) {
            return;
        }
        let n = n_rows();
        let ctrl = util::ctrl(&e);
        let (r, c) = cur.get_untracked();
        let last_col = COLS.len() - 1;
        match e.key().as_str() {
            "ArrowDown" => cur.set(((r + 1).min(n.saturating_sub(1)), c)),
            "ArrowUp" => cur.set((r.saturating_sub(1), c)),
            "ArrowRight" => cur.set((r, (c + 1).min(last_col))),
            "ArrowLeft" => cur.set((r, c.saturating_sub(1))),
            "Home" if ctrl => cur.set((0, c)),
            "End" if ctrl => cur.set((n.saturating_sub(1), c)),
            "Home" => cur.set((r, 0)),
            "End" => cur.set((r, last_col)),
            "Tab" if e.shift_key() => {
                if c > 0 {
                    cur.set((r, c - 1));
                } else if r > 0 {
                    cur.set((r - 1, last_col));
                } else {
                    return; // to the sidebar
                }
            }
            "Tab" => {
                if c < last_col {
                    cur.set((r, c + 1));
                } else if r + 1 < n {
                    cur.set((r + 1, 0));
                }
            }
            "Enter" | "F2" => start_edit(None),
            "Insert" => add_row(),
            "F8" => confirm_delete(app, row_at(r)),
            "Delete" if ctrl => confirm_delete(app, row_at(r)),
            "Delete" if c == 6 => {
                if let Some(row) = row_at(r) {
                    update_row(app, row.id, |x| x.due = None);
                }
            }
            "o" | "O" if ctrl => cycle_sort(c),
            _ => match util::typed_char(&e) {
                Some(ch) if editable(c) => start_edit((c != 5).then(|| ch.to_string())),
                _ => return,
            },
        }
        e.prevent_default();
    };
    // keep the current cell in view
    Effect::new(move |_| {
        let (r, c) = cur.get();
        request_animation_frame(move || {
            if let Some(el) = util::query(&format!(
                ".grid tbody tr:nth-child({}) td:nth-child({})",
                r + 1,
                c + 1
            )) {
                util::scroll_into_view(&el);
            }
        });
    });
    let editor = move |col: usize| -> AnyView {
        if col == 5 {
            let id = edit
                .with_untracked(|e| e.as_ref().map(|e| e.id))
                .unwrap_or(0);
            let cur_status =
                rows.with_untracked(|rs| rs.iter().find(|r| r.id == id).map(|r| r.status.clone()));
            return view! {
                <Dropdown ctl="grid.status" options=Signal::derive(|| opts(&STATUSES)) value=Signal::derive(move || cur_status.clone())
                    start_open=true
                    on_pick=Callback::new(move |s: String| { update_row(app, id, |r| r.status = s); finish(true); })
                    on_cancel=Callback::new(move |_| { if edit.try_get_untracked().flatten().is_some() { finish(false); } }) />
            }
            .into_any();
        }
        view! {
            <input class="ed" class:bad=move || err.get().is_some() spellcheck="false" autocomplete="off"
                title=move || err.get().unwrap_or_default()
                prop:value=move || edit.with(|e| e.as_ref().map(|e| e.text.clone()).unwrap_or_default())
                on:input=move |e| { let v = event_target_value(&e); edit.update(|x| if let Some(x) = x { x.text = v; }); err.set(None); }
                on:keydown=move |e: web_sys::KeyboardEvent| {
                    match e.key().as_str() {
                        "Enter" => commit_text(),
                        "Escape" => finish(false),
                        "Tab" => commit_text(),
                        _ => return,
                    }
                    e.prevent_default();
                }
                on:blur=move |_| { if edit.try_get_untracked().flatten().is_some() { let _ = edit.try_set(None); } } />
        }
        .into_any()
    };
    let today = util::today();
    view! {
        <TabFrame id="grid">
            <div class="gridwrap" class:flash=move || app.flash.with(|f| f.contains(KEY))>
                <div class="grid" tabindex="0" data-control=KEY on:keydown=on_key on:focus=move |_| app.ctx.set(Ctx::Body("grid"))>
                    <table>
                        <thead><tr>
                            {COLS.iter().enumerate().map(|(i, (name, title))| view! {
                                <th data-col=*name class:num=matches!(i, 0 | 2 | 3 | 4) on:mousedown=|e| e.prevent_default()
                                    on:click=move |_| { cycle_sort(i); focus_grid(); }>
                                    {*title}
                                    <span class="sarrow">{move || match sort.get() { Some((c, true)) if c == i => "▲", Some((c, false)) if c == i => "▼", _ => "" }}</span>
                                </th>
                            }).collect_view()}
                        </tr></thead>
                        <tbody>
                            {move || {
                                let rs = rows.get();
                                order.get().into_iter().enumerate().map(|(vi, ri)| {
                                    let r = rs[ri].clone();
                                    let id = r.id;
                                    let cells = (0..COLS.len()).map(|c| {
                                        let is_cur = move || cur.get() == (vi, c);
                                        let is_edit = move || edit.with(|e| e.as_ref().is_some_and(|e| e.id == id && e.col == c));
                                        let r = r.clone();
                                        view! {
                                            <td data-col=COLS[c].0 class:cur=is_cur class:num=matches!(c, 0 | 2 | 3 | 4) class:ro=!editable(c)
                                                class:editing=is_edit
                                                on:mousedown=move |_| cur.set((vi, c))
                                                on:dblclick=move |_| { cur.set((vi, c)); start_edit(None); }>
                                                {move || if is_edit() { editor(c) } else { cell_view(&r, c, today) }}
                                            </td>
                                        }
                                    }).collect_view();
                                    view! { <tr data-id=id class:currow=move || cur.get().0 == vi>{cells}</tr> }
                                }).collect_view()
                            }}
                        </tbody>
                        <tfoot><tr>
                            <td></td><td class="muted">{move || util::plural(rows.with(|r| r.len()), "row", "rows")}</td>
                            <td class="num">{move || rows.with(|r| r.iter().map(|x| x.qty).sum::<u64>())}</td>
                            <td></td>
                            <td class="num">{move || money(rows.with(|r| r.iter().map(|x| x.total()).sum::<f64>()))}</td>
                            <td></td><td></td>
                        </tr></tfoot>
                    </table>
                </div>
                <div class="gridfoot">
                    {move || err.get().map(|e| view! { <span class="ferr">"✗ "{e}</span> })}
                    <span class="fhint">{move || match sort.get() {
                        Some((c, asc)) => format!("sorted by {} {} (view only; ^O again to change)", COLS[c].1, if asc { "▲" } else { "▼" }),
                        None => "stored order · Total = Qty × Price (read-only)".into(),
                    }}</span>
                </div>
            </div>
        </TabFrame>
    }
}

/// A cell's display content.
fn cell_view(r: &GridRow, c: usize, today: demo_core::Date) -> AnyView {
    match c {
        0 => view! { <span class="muted">{r.id}</span> }.into_any(),
        1 => view! { <span>{r.item.clone()}</span> }.into_any(),
        2 => view! { <span>{r.qty}</span> }.into_any(),
        3 => view! { <span>{money(r.price)}</span> }.into_any(),
        4 => view! { <span class="muted2">{money(r.total())}</span> }.into_any(),
        5 => {
            let s = choice(&STATUSES, &r.status);
            chip(
                s.map_or(r.status.as_str(), |s| s.label),
                s.map_or("", |s| s.color),
            )
            .into_any()
        }
        _ => {
            let d = r.due.as_deref().and_then(demo_core::Date::parse_iso);
            let late = d.is_some_and(|d| d < today) && r.status != "done";
            view! { <span class:late=late>{d.map(|d| d.short(today.year())).unwrap_or_else(|| "—".into())}</span> }.into_any()
        }
    }
}

fn confirm_delete(app: App, row: Option<GridRow>) {
    let Some(r) = row else { return };
    app.open(Modal::Confirm {
        title: "Delete row?".into(),
        body: format!("Delete row {} \"{}\" for everyone?", r.id, r.item),
        ok: "Delete".into(),
        act: ConfirmAct::DeleteRow(r.id),
    });
}
