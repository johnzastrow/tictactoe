//! Grid behaviour independent of rendering: cursor movement, editing,
//! clipboard, fill-down, delete, sort and group (ui-spec §3).

use leptos::prelude::*;
use pt_core::fields::values_equal;
use pt_core::grid::{self, GridRow, SortSpec};
use pt_core::model::*;
use pt_core::protocol::Op;
use serde_json::Value;

use crate::app::{Act, App, ConfirmAct, EditState, Grid, GridKey, Level, Mark, Modal, ViewId};
use crate::util;

/// After committing an edit, where the cursor goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Stay,
    Down,
    Right,
    Left,
}

pub fn row_h(app: &App) -> f64 {
    if app.compact.get_untracked() {
        21.0
    } else {
        25.0
    }
}

pub fn set_cursor(g: Grid, row: usize, col: usize) {
    let (n, id) = g
        .rows
        .with_untracked(|rows| (rows.len(), rows.get(row).and_then(GridRow::id)));
    if n == 0 {
        return;
    }
    let row = row.min(n - 1);
    let ncols = g.cols.with_untracked(|c| c.len());
    g.cur_row.set(row);
    g.cur_col.set(col.min(ncols.saturating_sub(1)));
    g.cur_id.set(id);
}

pub fn move_by(g: Grid, dr: i64, dc: i64) {
    let n = g.rows.with_untracked(|r| r.len()) as i64;
    let nc = g.cols.with_untracked(|c| c.len()) as i64;
    let r = (g.cur_row.get_untracked() as i64 + dr).clamp(0, (n - 1).max(0));
    let c = (g.cur_col.get_untracked() as i64 + dc).clamp(0, (nc - 1).max(0));
    set_cursor(g, r as usize, c as usize);
}

/// Tab order: next/previous cell, wrapping across rows.
pub fn tab(g: Grid, back: bool) {
    let nc = g.cols.with_untracked(|c| c.len());
    let c = g.cur_col.get_untracked();
    if back {
        if c == 0 {
            if g.cur_row.get_untracked() > 0 {
                set_cursor(g, g.cur_row.get_untracked() - 1, nc - 1);
            }
        } else {
            g.cur_col.set(c - 1);
        }
    } else if c + 1 >= nc {
        set_cursor(g, g.cur_row.get_untracked() + 1, 0);
    } else {
        g.cur_col.set(c + 1);
    }
}

fn page_rows(app: &App, g: Grid) -> i64 {
    let h = util::by_id(g.key.dom_id()).map_or(500.0, |e| e.client_height() as f64);
    ((h / row_h(app)) as i64 - 3).max(1)
}

/// Data row ids in display order that are marked, or the current row.
pub fn target_ids(g: Grid) -> Vec<i64> {
    let marks = g.marks.get_untracked();
    if marks.is_empty() {
        return g.cur_id.get_untracked().into_iter().collect();
    }
    g.rows.with_untracked(|rows| {
        rows.iter()
            .filter_map(GridRow::id)
            .filter(|id| marks.contains(id))
            .collect()
    })
}

pub fn focus_grid(g: Grid) {
    util::focus_id(g.key.dom_id());
}

// ---- editing ---------------------------------------------------------------------

/// Open the editor on the current cell. `seed` = first typed character.
pub fn start_edit(app: App, g: Grid, seed: Option<String>) {
    let Some(col) = g.cur_column() else { return };
    let Some(id) = g.cur_id.get_untracked() else {
        return;
    };
    let editable = app.rep.with_untracked(|r| g.editable(r));
    if !editable || !col.editable() {
        if g.key == GridKey::Activity {
            open_activity(app, id);
            return;
        }
        let why = if !app.can_edit() {
            "You have read-only access."
        } else if g.admin_only && !app.is_admin() {
            "Only admins can change this."
        } else {
            "This column is calculated and can't be edited."
        };
        app.say(Level::Info, why);
        return;
    }
    let field = col.field.clone().unwrap_or_default();
    app.clear_conflict(g.entity, id, &field);
    let base = app
        .rep
        .with_untracked(|r| r.field_value(g.entity, id, &field));
    let rect = util::document()
        .query_selector(&format!("#{} .cell.cur", g.key.dom_id()))
        .ok()
        .flatten()
        .map(|e| util::rect_of(&e))
        .unwrap_or((100.0, 100.0, 120.0, 24.0));
    // A typed character replaces text; lists treat it as a filter.
    g.edit.set(Some(EditState {
        id,
        col: col.key.clone(),
        field,
        editor: col.editor,
        seed,
        base,
        rect,
    }));
    app.ctx.set(crate::app::Ctx::Edit);
}

pub fn cancel_edit(app: App, g: Grid) {
    g.edit.set(None);
    app.ctx.set(crate::app::Ctx::Grid);
    focus_grid(g);
}

/// Commit a value from the open editor and move.
pub fn commit(app: App, g: Grid, value: Value, nav: Nav) {
    let Some(e) = g.edit.get_untracked() else {
        return;
    };
    g.edit.set(None);
    app.ctx.set(crate::app::Ctx::Grid);
    if !values_equal(&e.base, &value) {
        app.mutate(
            vec![Op::set(g.entity, e.id, &e.field, e.base.clone(), value)],
            |_| {},
        );
    }
    match nav {
        Nav::Stay => {}
        Nav::Down => move_by(g, 1, 0),
        Nav::Right => tab(g, false),
        Nav::Left => tab(g, true),
    }
    focus_grid(g);
}

fn open_activity(app: App, id: i64) {
    let target = app.rep.with_untracked(|r| {
        r.activity
            .get(&id)
            .and_then(|a| Entity::parse(&a.entity).map(|e| (e, a.entity_id)))
    });
    if let Some((e, eid)) = target {
        if app.rep.with_untracked(|r| r.exists(e, eid)) {
            app.goto(e, eid);
        } else {
            app.say(Level::Info, "That record no longer exists.");
        }
    }
}

// ---- actions -----------------------------------------------------------------------

pub fn act(app: App, g: Grid, act: Act) {
    match act {
        Act::Edit => start_edit(app, g, None),
        Act::EditRecord => {
            if let Some(id) = g.cur_id.get_untracked() {
                if g.key == GridKey::Activity {
                    open_activity(app, id);
                } else {
                    app.modal.set(Some(Modal::Record {
                        entity: g.entity,
                        id,
                    }));
                }
            }
        }
        Act::New => new_row(app, g),
        Act::Delete => delete_rows(app, g),
        Act::Columns => app.modal.set(Some(Modal::Columns(g.key))),
        Act::SortDialog => app.modal.set(Some(Modal::Sort(g.key))),
        Act::SortCol => sort_col(app, g),
        Act::GroupCol => group_col(app, g),
        Act::ClearGroup => g.group.set(None),
        Act::Copy => copy(app, g),
        Act::Paste => paste_from_clipboard(app, g),
        Act::FillDown => fill_down(app, g),
        Act::ClearCell => clear_cell(app, g),
        Act::MarkRow => mark_row(g),
        Act::MarkAll => mark_all(g),
        _ => app.say(
            Level::Info,
            format!("{} isn't available here.", act.label()),
        ),
    }
}

pub fn new_row(app: App, g: Grid) {
    if !app.can_edit() {
        app.say(Level::Warn, "You have read-only access.");
        return;
    }
    let cur_project = |g: Grid| {
        g.cur_id.get_untracked().and_then(|t| {
            app.rep
                .with_untracked(|r| r.tasks.get(&t).map(|t| t.project_id))
        })
    };
    match g.key {
        GridKey::Projects => crate::app::run(app, Act::NewProject),
        GridKey::ProjTasks => app.modal.set(Some(Modal::NewTask {
            project: app.grids.projects.cur_id.get_untracked(),
            status: None,
        })),
        GridKey::Tasks => app.modal.set(Some(Modal::NewTask {
            project: cur_project(g),
            status: None,
        })),
        GridKey::Activity => app.say(Level::Info, "The activity log is read-only."),
        _ if !app.is_admin() => app.say(Level::Warn, "Only admins can add these."),
        GridKey::TemplateTasks => match app.grids.templates.cur_id.get_untracked() {
            Some(t) => app.modal.set(Some(Modal::Create {
                entity: Entity::TemplateTask,
                preset: vec![("template_id".into(), Value::from(t))],
            })),
            None => app.say(Level::Info, "Select a template first."),
        },
        _ => app.modal.set(Some(Modal::Create {
            entity: g.entity,
            preset: vec![],
        })),
    }
}

pub fn delete_rows(app: App, g: Grid) {
    if !app.rep.with_untracked(|r| g.editable(r)) {
        app.say(Level::Warn, "You can't delete rows here.");
        return;
    }
    if g.entity == Entity::User {
        app.say(
            Level::Info,
            "Users can't be deleted; set Active to no instead.",
        );
        return;
    }
    let ids = target_ids(g);
    if ids.is_empty() {
        return;
    }
    let (title, body) = app.rep.with_untracked(|r| {
        let names: Vec<String> = ids
            .iter()
            .take(5)
            .map(|id| r.row_label(g.entity, *id))
            .collect();
        let more = if ids.len() > 5 {
            format!(" and {} more", ids.len() - 5)
        } else {
            String::new()
        };
        let extra = if g.entity == Entity::Project {
            let n: usize = ids.iter().map(|id| r.project_tasks(*id).len()).sum();
            format!(" Their {n} tasks will be deleted too.")
        } else {
            String::new()
        };
        let undo = if matches!(g.entity, Entity::Project | Entity::Task) {
            " You can undo this with ^Z."
        } else {
            " This can't be undone."
        };
        (
            format!(
                "Delete {} {}{}?",
                ids.len(),
                g.entity.label(),
                if ids.len() == 1 { "" } else { "s" }
            ),
            format!("{}{more}.{extra}{undo}", names.join(", ")),
        )
    });
    app.modal.set(Some(Modal::Confirm {
        title,
        body,
        act: ConfirmAct::Delete {
            entity: g.entity,
            ids,
        },
    }));
}

pub fn sort_col(app: App, g: Grid) {
    let Some(col) = g.cur_column() else { return };
    toggle_sort(g, &col.key, false);
    let s = g.sort.get_untracked();
    let txt = match s.iter().find(|s| s.key == col.key) {
        Some(s) if s.desc => format!("Sorted by {} ▼", col.title),
        Some(_) => format!("Sorted by {} ▲", col.title),
        None => "Sort cleared".into(),
    };
    app.say(Level::Info, txt);
}

/// Cycle ▲ → ▼ → off. `add` keeps other keys (Shift+click).
pub fn toggle_sort(g: Grid, key: &str, add: bool) {
    g.sort.update(|s| {
        let cur = s.iter().position(|x| x.key == key);
        match cur {
            Some(i) if !s[i].desc => s[i].desc = true,
            Some(i) => {
                s.remove(i);
            }
            None => {
                if !add {
                    s.clear();
                }
                s.push(SortSpec {
                    key: key.to_string(),
                    desc: false,
                });
            }
        }
    });
}

pub fn group_col(app: App, g: Grid) {
    let Some(col) = g.cur_column() else { return };
    if g.group.get_untracked().as_deref() == Some(col.key.as_str()) {
        g.group.set(None);
        app.say(Level::Info, "Grouping cleared");
    } else {
        g.collapsed.set(Default::default());
        g.group.set(Some(col.key.clone()));
        app.say(Level::Info, format!("Grouped by {}", col.title));
    }
}

pub fn mark_row(g: Grid) {
    if let Some(id) = g.cur_id.get_untracked() {
        g.marks.update(|m| {
            if !m.remove(&id) {
                m.insert(id);
            }
        });
        g.anchor.set(Some(g.cur_row.get_untracked()));
    }
}

pub fn mark_all(g: Grid) {
    let all: Vec<i64> = g
        .rows
        .with_untracked(|r| r.iter().filter_map(GridRow::id).collect());
    g.marks.update(|m| {
        if m.len() == all.len() {
            m.clear();
        } else {
            m.extend(all);
        }
    });
}

/// Shift+↑/↓: extend the marked range from the anchor.
pub fn extend_marks(g: Grid, dr: i64) {
    let start = g
        .anchor
        .get_untracked()
        .unwrap_or(g.cur_row.get_untracked());
    if g.anchor.get_untracked().is_none() {
        g.anchor.set(Some(start));
    }
    move_by(g, dr, 0);
    let end = g.cur_row.get_untracked();
    let (a, b) = (start.min(end), start.max(end));
    let ids: Vec<i64> = g.rows.with_untracked(|r| {
        r[a..=b.min(r.len().saturating_sub(1))]
            .iter()
            .filter_map(GridRow::id)
            .collect()
    });
    g.marks.set(ids.into_iter().collect());
}

fn clear_cell(app: App, g: Grid) {
    let Some(col) = g.cur_column() else { return };
    let Some(id) = g.cur_id.get_untracked() else {
        return;
    };
    if !col.editable() || !app.rep.with_untracked(|r| g.editable(r)) {
        return;
    }
    let field = col.field.clone().unwrap_or_default();
    let (base, empty) = app.rep.with_untracked(|r| {
        (
            r.field_value(g.entity, id, &field),
            grid::parse_input(r, g.entity, &field, id, ""),
        )
    });
    match empty {
        Ok(v) => {
            let required =
                pt_core::fields::field(g.entity, &field).is_some_and(|m| m.required && !m.nullable);
            if required {
                app.say(Level::Warn, format!("{} is required.", col.title));
            } else {
                app.mutate(vec![Op::set(g.entity, id, &field, base, v)], |_| {});
            }
        }
        Err(e) => app.say(Level::Warn, e),
    }
}

/// Visible text of a cell for copying: editable cells copy their edit text
/// so a paste round-trips.
fn copy_text(app: &App, g: Grid, col: &grid::Column, id: i64) -> String {
    app.rep
        .with_untracked(|r| match &col.field {
            Some(f) => grid::edit_text(r, g.entity, f, id),
            None => grid::cell(r, col, id).text,
        })
        .replace(['\t', '\n'], " ")
}

pub fn copy(app: App, g: Grid) {
    let cols = g.cols.get_untracked();
    let marks = g.marks.get_untracked();
    let text = if marks.is_empty() {
        match (g.cur_column(), g.cur_id.get_untracked()) {
            (Some(c), Some(id)) => copy_text(&app, g, &c, id),
            _ => return,
        }
    } else {
        target_ids(g)
            .iter()
            .map(|id| {
                cols.iter()
                    .map(|c| copy_text(&app, g, c, *id))
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let n = text.lines().count();
    app.clip.set_value(text.clone());
    if util::clipboard_write(&text) {
        app.say(
            Level::Ok,
            format!("Copied {n} row{}.", if n == 1 { "" } else { "s" }),
        );
    } else {
        app.say(
            Level::Info,
            "Copied inside pt (the system clipboard needs https).",
        );
    }
}

fn paste_from_clipboard(app: App, g: Grid) {
    if !util::window().is_secure_context() {
        let t = app.clip.get_value();
        paste(app, g, &t);
        return;
    }
    let p = util::window().navigator().clipboard().read_text();
    leptos::task::spawn_local(async move {
        let text = wasm_bindgen_futures::JsFuture::from(p)
            .await
            .ok()
            .and_then(|v| v.as_string());
        let text = text.unwrap_or_else(|| app.clip.get_value());
        paste(app, g, &text);
    });
}

/// Paste TSV at the cursor, filling right and down (one undo step).
pub fn paste(app: App, g: Grid, text: &str) {
    if text.is_empty() {
        return;
    }
    if !app.rep.with_untracked(|r| g.editable(r)) {
        app.say(Level::Warn, "You can't edit here.");
        return;
    }
    let cells = util::parse_tsv(text);
    let cols = g.cols.get_untracked();
    let start_col = g.cur_col.get_untracked();
    let row_ids: Vec<i64> = g.rows.with_untracked(|rows| {
        rows.iter()
            .skip(g.cur_row.get_untracked())
            .filter_map(GridRow::id)
            .take(cells.len())
            .collect()
    });
    let mut ops = Vec::new();
    let mut errors = Vec::new();
    app.rep.with_untracked(|r| {
        for (ri, row) in cells.iter().enumerate() {
            let Some(id) = row_ids.get(ri) else { break };
            for (ci, txt) in row.iter().enumerate() {
                let Some(col) = cols.get(start_col + ci) else {
                    break;
                };
                let Some(field) = &col.field else { continue };
                if !col.editable() {
                    continue;
                }
                match grid::parse_input(r, g.entity, field, *id, txt) {
                    Ok(v) => {
                        let base = r.field_value(g.entity, *id, field);
                        if !values_equal(&base, &v) {
                            ops.push(Op::set(g.entity, *id, field, base, v));
                        }
                    }
                    Err(e) => errors.push(format!("{}: {e}", col.title)),
                }
            }
        }
    });
    if let Some(e) = errors.first() {
        app.toast(
            Level::Warn,
            format!("Skipped {} value(s) — {e}", errors.len()),
        );
    }
    let n = ops.len();
    if n == 0 {
        app.say(Level::Info, "Nothing to paste.");
        return;
    }
    app.mutate(ops, move |r| {
        if r.ok && r.error.is_none() {
            let _ = n;
        }
    });
}

pub fn fill_down(app: App, g: Grid) {
    let Some(col) = g.cur_column() else { return };
    let Some(field) = col.field.clone().filter(|_| col.editable()) else {
        app.say(Level::Info, "This column can't be filled.");
        return;
    };
    if !app.rep.with_untracked(|r| g.editable(r)) {
        return;
    }
    let ids = target_ids(g);
    let (source, targets): (i64, Vec<i64>) = if ids.len() >= 2 {
        (ids[0], ids[1..].to_vec())
    } else {
        // no marks: copy from the data row above
        let above = g.rows.with_untracked(|rows| {
            rows[..g.cur_row.get_untracked()]
                .iter()
                .rev()
                .find_map(GridRow::id)
        });
        match (above, g.cur_id.get_untracked()) {
            (Some(a), Some(cur)) => (a, vec![cur]),
            _ => return,
        }
    };
    let ops: Vec<Op> = app.rep.with_untracked(|r| {
        let v = r.field_value(g.entity, source, &field);
        targets
            .iter()
            .filter_map(|id| {
                let base = r.field_value(g.entity, *id, &field);
                (!values_equal(&base, &v)).then(|| Op::set(g.entity, *id, &field, base, v.clone()))
            })
            .collect()
    });
    if ops.is_empty() {
        app.say(Level::Info, "Nothing to fill.");
    } else {
        app.mutate(ops, |_| {});
    }
}

// ---- keyboard --------------------------------------------------------------------

/// Grid keydown. Returns true if handled (caller prevents default).
pub fn key(app: App, g: Grid, e: &web_sys::KeyboardEvent) -> bool {
    if g.edit.get_untracked().is_some() {
        return false; // the editor has focus and handles its keys
    }
    let k = e.key();
    let ctrl = util::ctrl(e);
    let shift = e.shift_key();
    if e.alt_key() {
        return false;
    }
    let on_group = g.rows.with_untracked(|rows| {
        matches!(
            rows.get(g.cur_row.get_untracked()),
            Some(GridRow::Group { .. })
        )
    });
    match (k.as_str(), ctrl) {
        ("ArrowUp", false) if shift => extend_marks(g, -1),
        ("ArrowDown", false) if shift => extend_marks(g, 1),
        ("ArrowUp", false) => move_by(g, -1, 0),
        ("ArrowDown", false) => move_by(g, 1, 0),
        ("ArrowLeft", false) if on_group => toggle_group(g, Some(true)),
        ("ArrowRight", false) if on_group => toggle_group(g, Some(false)),
        ("ArrowLeft", false) => move_by(g, 0, -1),
        ("ArrowRight", false) => move_by(g, 0, 1),
        ("Tab", _) => tab(g, shift),
        ("Home", false) => g.cur_col.set(0),
        ("End", false) => g
            .cur_col
            .set(g.cols.with_untracked(|c| c.len().saturating_sub(1))),
        ("Home", true) => set_cursor(g, 0, g.cur_col.get_untracked()),
        ("End", true) => set_cursor(g, usize::MAX, g.cur_col.get_untracked()),
        ("PageUp", false) => move_by(g, -page_rows(&app, g), 0),
        ("PageDown", false) => move_by(g, page_rows(&app, g), 0),
        ("Enter", false) if on_group => toggle_group(g, None),
        ("Enter", false) | ("F2", _) => {
            let conflict = conflict_here(app, g);
            if conflict {
                app.say(
                    Level::Info,
                    "Editing from their value; your earlier edit was discarded.",
                );
            }
            start_edit(app, g, None)
        }
        ("Escape", _) => {
            if conflict_here(app, g) {
                if let (Some(id), Some(c)) = (g.cur_id.get_untracked(), g.cur_column()) {
                    app.clear_conflict(g.entity, id, c.field.as_deref().unwrap_or(""));
                }
            } else if !g.marks.get_untracked().is_empty() {
                g.marks.set(Default::default());
                g.anchor.set(None);
            } else {
                return false;
            }
        }
        ("Delete", false) => clear_cell(app, g),
        ("Delete", true) | ("F8", _) => delete_rows(app, g),
        ("Insert", _) => new_row(app, g),
        (" ", false) => mark_row(g),
        ("a", true) | ("A", true) => mark_all(g),
        ("c", true) | ("C", true) => copy(app, g),
        ("v", true) | ("V", true) => return false, // let the browser fire `paste`
        ("d", true) | ("D", true) => fill_down(app, g),
        ("e", true) | ("E", true) => act(app, g, Act::EditRecord),
        ("o", true) | ("O", true) => sort_col(app, g),
        ("r", true) | ("R", true) => group_col(app, g),
        ("F4", _) => act(app, g, Act::Columns),
        ("Backspace", false) => start_edit(app, g, Some(String::new())),
        (s, false) if s.chars().count() == 1 && !s.starts_with(char::is_control) => {
            start_edit(app, g, Some(s.to_string()))
        }
        _ => return false,
    }
    true
}

fn conflict_here(app: App, g: Grid) -> bool {
    match (g.cur_id.get_untracked(), g.cur_column()) {
        (Some(id), Some(c)) => {
            matches!(
                app.mark_of(g.entity, id, c.field.as_deref().unwrap_or("")),
                Some(Mark::Conflict(_))
            )
        }
        _ => false,
    }
}

/// Collapse (Some(true)), expand (Some(false)) or toggle the group under the cursor.
pub fn toggle_group(g: Grid, collapse: Option<bool>) {
    let key = g
        .rows
        .with_untracked(|rows| match rows.get(g.cur_row.get_untracked()) {
            Some(GridRow::Group { key, .. }) => Some(key.clone()),
            _ => None,
        });
    if let Some(k) = key {
        g.collapsed.update(|c| {
            let is = c.contains(&k);
            let want = collapse.unwrap_or(!is);
            if want {
                c.insert(k);
            } else {
                c.remove(&k);
            }
        });
    }
}

/// Views switch the "last grid" when one of their grids gets focus.
pub fn on_focus(app: App, g: Grid) {
    app.last_grid.set(g.key);
    if g.edit.get_untracked().is_none() {
        app.ctx.set(crate::app::Ctx::Grid);
    }
    let _ = ViewId::Projects;
}
