//! Tab 15, Kanban: `kanban.cards` in four columns; keys move the selection
//! and cards, cards drag between columns (pointer events), and every change
//! commits the whole list.

use demo_core::dataset::KANBAN_COLUMNS;
use demo_core::keys::{KanbanCard, MAX_CARD_TITLE, MAX_CARDS, next_id};
use leptos::prelude::*;
use serde_json::json;

use super::TabFrame;
use crate::app::{App, ConfirmAct, Ctx, InputAct, Modal, use_app};
use crate::util;

const KEY: &str = "kanban.cards";

fn cards(app: App) -> Vec<KanbanCard> {
    serde_json::from_value(app.latest(KEY)).unwrap_or_default()
}

fn col_index(id: &str) -> usize {
    KANBAN_COLUMNS.iter().position(|c| c.id == id).unwrap_or(0)
}

fn check_title(t: &str) -> Result<String, String> {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > MAX_CARD_TITLE {
        return Err(format!("1–{MAX_CARD_TITLE} characters"));
    }
    Ok(t.to_string())
}

pub fn new_card(app: App, col: &'static str, title: &str) -> Result<(), String> {
    let title = check_title(title)?;
    let mut list = cards(app);
    if list.len() >= MAX_CARDS {
        return Err(format!("at most {MAX_CARDS} cards"));
    }
    let id = next_id(list.iter().map(|c| c.id));
    list.push(KanbanCard {
        id,
        title,
        col: col.into(),
    });
    app.set_latest(KEY, json!(list));
    Ok(())
}

pub fn rename_card(app: App, id: u64, title: &str) -> Result<(), String> {
    let title = check_title(title)?;
    let mut list = cards(app);
    if let Some(c) = list.iter_mut().find(|c| c.id == id) {
        c.title = title;
    }
    app.set_latest(KEY, json!(list));
    Ok(())
}

pub fn delete_card(app: App, id: u64) {
    let mut list = cards(app);
    list.retain(|c| c.id != id);
    app.set_latest(KEY, json!(list));
}

/// Move card `id` to column `col`, before the `pos`-th card of that column
/// (or to its end).
fn move_card(app: App, id: u64, col: usize, pos: Option<usize>) {
    let mut list = cards(app);
    let Some(from) = list.iter().position(|c| c.id == id) else {
        return;
    };
    let mut card = list.remove(from);
    card.col = KANBAN_COLUMNS[col].id.into();
    let in_col: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, c)| col_index(&c.col) == col)
        .map(|(i, _)| i)
        .collect();
    let at = match pos.and_then(|p| in_col.get(p)) {
        Some(&i) => i,
        None => in_col.last().map_or(list.len(), |&i| i + 1),
    };
    list.insert(at, card);
    app.set_latest(KEY, json!(list));
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Drag {
    id: u64,
    x0: f64,
    y0: f64,
    dx: f64,
    dy: f64,
}

#[component]
pub fn KanbanTab() -> impl IntoView {
    let app = use_app();
    let raw = app.watch(KEY);
    let list = Memo::new(move |_| {
        raw.with(|v| serde_json::from_value::<Vec<KanbanCard>>(v.clone()).unwrap_or_default())
    });
    let by_col = Memo::new(move |_| {
        let l = list.get();
        KANBAN_COLUMNS
            .iter()
            .map(|c| {
                l.iter()
                    .filter(|k| k.col == c.id)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });
    let cur_col = RwSignal::new(0usize);
    let sel_id = RwSignal::new(None::<u64>);
    let drag = RwSignal::new(None::<Drag>);
    let drop_col = RwSignal::new(None::<usize>);
    // the selected card follows its column; a vanished card selects the column's first
    Effect::new(move |_| {
        let cols = by_col.get();
        let found = sel_id
            .get_untracked()
            .and_then(|id| cols.iter().position(|c| c.iter().any(|k| k.id == id)));
        match found {
            Some(c) => cur_col.set(c),
            None => sel_id.set(cols[cur_col.get_untracked()].first().map(|k| k.id)),
        }
    });
    let sel_pos = move || {
        let id = sel_id.get_untracked()?;
        by_col.with_untracked(|c| c[cur_col.get_untracked()].iter().position(|k| k.id == id))
    };
    let select = move |col: usize, idx: usize| {
        cur_col.set(col);
        let id = by_col.with_untracked(|c| {
            c[col]
                .get(idx.min(c[col].len().saturating_sub(1)))
                .map(|k| k.id)
        });
        sel_id.set(id);
    };
    let selected = move || {
        sel_id
            .get_untracked()
            .and_then(|id| list.with_untracked(|l| l.iter().find(|k| k.id == id).cloned()))
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let col = cur_col.get_untracked();
        let pos = sel_pos().unwrap_or(0);
        let shift = e.shift_key();
        let k = e.key();
        let ctrl = util::ctrl(&e);
        match k.as_str() {
            "ArrowRight" | ">" if shift || k == ">" => {
                if let Some(id) = sel_id.get_untracked() {
                    if col + 1 < KANBAN_COLUMNS.len() {
                        move_card(app, id, col + 1, None);
                        cur_col.set(col + 1);
                    }
                }
            }
            "ArrowLeft" | "<" if shift || k == "<" => {
                if let Some(id) = sel_id.get_untracked() {
                    if col > 0 {
                        move_card(app, id, col - 1, None);
                        cur_col.set(col - 1);
                    }
                }
            }
            "ArrowRight" => select((col + 1).min(KANBAN_COLUMNS.len() - 1), pos),
            "ArrowLeft" => select(col.saturating_sub(1), pos),
            "ArrowDown" => select(col, pos + 1),
            "ArrowUp" => select(col, pos.saturating_sub(1)),
            "Insert" => app.open(Modal::Input {
                title: format!("New card in {}", KANBAN_COLUMNS[col].label),
                label: "Title".into(),
                value: String::new(),
                act: InputAct::NewCard(KANBAN_COLUMNS[col].id),
            }),
            "Enter" | "F2" => {
                if let Some(c) = selected() {
                    app.open(Modal::Input {
                        title: "Rename card".into(),
                        label: "Title".into(),
                        value: c.title,
                        act: InputAct::RenameCard(c.id),
                    });
                }
            }
            "F8" => confirm_delete(app, selected()),
            "Delete" if ctrl => confirm_delete(app, selected()),
            _ => return,
        }
        e.prevent_default();
    };
    // which column (and position in it) a pointer is over
    let target = move |x: f64, y: f64, id: u64| -> Option<(usize, usize)> {
        let cols = util::query_all(".kanban .kcol");
        let ci = cols.iter().position(|c| {
            let (l, _, w, _) = util::rect_of(c);
            x >= l && x < l + w
        })?;
        let cards = util::query_all(&format!(
            ".kanban .kcol[data-col='{}'] .kcard",
            KANBAN_COLUMNS[ci].id
        ));
        let pos = cards
            .iter()
            .filter(|c| c.get_attribute("data-id").as_deref() != Some(&id.to_string()))
            .position(|c| {
                let (_, t, _, h) = util::rect_of(c);
                y < t + h / 2.0
            });
        Some((ci, pos.unwrap_or(usize::MAX)))
    };
    let on_move = move |e: web_sys::PointerEvent| {
        if let Some(mut d) = drag.get_untracked() {
            d.dx = e.client_x() as f64 - d.x0;
            d.dy = e.client_y() as f64 - d.y0;
            drag.set(Some(d));
            drop_col.set(target(e.client_x() as f64, e.client_y() as f64, d.id).map(|t| t.0));
        }
    };
    let on_up = move |e: web_sys::PointerEvent| {
        let Some(d) = drag.get_untracked() else {
            return;
        };
        drag.set(None);
        drop_col.set(None);
        if d.dx.abs() + d.dy.abs() < 5.0 {
            return; // a click, not a drag
        }
        if let Some((col, pos)) = target(e.client_x() as f64, e.client_y() as f64, d.id) {
            move_card(app, d.id, col, (pos != usize::MAX).then_some(pos));
            cur_col.set(col);
        }
    };
    let board = NodeRef::<leptos::html::Div>::new();
    view! {
        <TabFrame id="kanban">
            <div class="kanban" tabindex="0" data-control=KEY node_ref=board class:flash=move || app.flash.with(|f| f.contains(KEY))
                on:keydown=on_key on:focus=move |_| app.ctx.set(Ctx::Body("kanban"))
                on:pointermove=on_move on:pointerup=on_up>
                {KANBAN_COLUMNS.iter().enumerate().map(|(ci, col)| view! {
                    <div class="kcol" data-col=col.id class:cur=move || cur_col.get() == ci class:over=move || drop_col.get() == Some(ci)
                        style=format!("--c:{}", col.color)>
                        <div class="khead"><span>{col.label}</span><span class="kcount">{move || by_col.with(|c| c[ci].len())}</span></div>
                        <div class="kcards">
                            {move || by_col.with(|c| c[ci].clone()).into_iter().map(|k| {
                                let id = k.id;
                                let style = move || drag.get().filter(|d| d.id == id).map(|d| format!("transform:translate({}px,{}px) rotate(1.5deg)", d.dx, d.dy)).unwrap_or_default();
                                view! {
                                    <div class="kcard" data-id=id class:sel=move || sel_id.get() == Some(id) class:dragging=move || drag.get().is_some_and(|d| d.id == id)
                                        style=style
                                        on:pointerdown=move |e: web_sys::PointerEvent| {
                                            e.prevent_default();
                                            cur_col.set(ci);
                                            sel_id.set(Some(id));
                                            if let Some(b) = board.get_untracked() {
                                                let _ = b.focus();
                                                let _ = b.set_pointer_capture(e.pointer_id());
                                            }
                                            drag.set(Some(Drag { id, x0: e.client_x() as f64, y0: e.client_y() as f64, dx: 0.0, dy: 0.0 }));
                                        }
                                        on:dblclick=move |_| app.open(Modal::Input { title: "Rename card".into(), label: "Title".into(), value: k.title.clone(), act: InputAct::RenameCard(id) })>
                                        <span class="kid">{format!("#{id}")}</span>
                                        <span class="ktitle">{k.title.clone()}</span>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    </div>
                }).collect_view()}
            </div>
        </TabFrame>
    }
}

fn confirm_delete(app: App, card: Option<KanbanCard>) {
    let Some(c) = card else { return };
    app.open(Modal::Confirm {
        title: "Delete card?".into(),
        body: format!("Delete card #{} \"{}\" for everyone?", c.id, c.title),
        ok: "Delete".into(),
        act: ConfirmAct::DeleteCard(c.id),
    });
}
