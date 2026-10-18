//! Tab 10 — Grid: `grid.rows` as an editable table with typed cell editors
//! (text, number, status dropdown, natural date), a computed Total column, a
//! totals row and a view-only sort. Every edit commits the whole list.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::dataset::{STATUSES, choice};
use demo_core::date::{parse_natural, preview};
use demo_core::keys::{GridRow, MAX_GRID_ROWS, next_id, round2};
use demo_core::{Date, safe_text};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::json;

use super::{h, hc, is_focused, muted};
use crate::app::{App, Hint, Hit, Level};
use crate::dialogs::{Dialog, DialogAct};
use crate::widgets::draw::{fill, pad, put, width};
use crate::widgets::input::TextInput;
use crate::widgets::list::{self, Drop, DropOut};

pub const KEYS: &[(&str, &str)] = &[
    ("arrows Tab", "move"),
    ("Enter F2 type", "edit cell"),
    ("Enter", "save, move down"),
    ("Esc", "cancel edit"),
    ("Del", "clear Due"),
    ("Ins", "add a row"),
    ("F8 ^Del", "delete row"),
    ("^O", "sort ▲ ▼ off"),
];

/// (title, width, editable)
pub const COLS: [(&str, u16, bool); 7] = [
    ("#", 4, false),
    ("Item", 18, true),
    ("Qty", 6, true),
    ("Price", 11, true),
    ("Total", 12, false),
    ("Status", 15, true),
    ("Due", 10, true),
];

/// A validated change to one row.
type Apply = Box<dyn FnOnce(&mut GridRow)>;

pub struct Editor {
    pub id: u64,
    pub col: usize,
    pub input: TextInput,
    pub drop: Drop,
}

pub struct State {
    pub row: usize,
    pub col: usize,
    /// (column, descending)
    pub sort: Option<(usize, bool)>,
    pub editor: Option<Editor>,
    scroll: usize,
    cell: Rect,
    body_rows: usize,
}

impl Default for State {
    /// Start on the Item column: the first editable one (as the Go TUI does).
    fn default() -> Self {
        State { row: 0, col: 1, sort: None, editor: None, scroll: 0, cell: Rect::default(), body_rows: 0 }
    }
}

pub fn rows(app: &App) -> Vec<GridRow> {
    app.shown_get("grid.rows").unwrap_or_default()
}

/// Row indices in display order (the sort is a view only).
pub fn view(rows: &[GridRow], sort: Option<(usize, bool)>) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    if let Some((col, desc)) = sort {
        let st = |s: &str| STATUSES.iter().position(|c| c.id == s).unwrap_or(9);
        idx.sort_by(|&a, &b| {
            let (x, y) = (&rows[a], &rows[b]);
            let o = match col {
                0 => x.id.cmp(&y.id),
                1 => x.item.to_lowercase().cmp(&y.item.to_lowercase()),
                2 => x.qty.cmp(&y.qty),
                3 => x.price.total_cmp(&y.price),
                4 => x.total().total_cmp(&y.total()),
                5 => st(&x.status).cmp(&st(&y.status)),
                // empty dates sort last either way
                _ => match (&x.due, &y.due) {
                    (Some(a), Some(b)) => a.cmp(b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                },
            };
            let nulls = col == 6 && (x.due.is_none() || y.due.is_none());
            if desc && !nulls { o.reverse() } else { o }
        });
    }
    idx
}

fn money(x: f64) -> String {
    format!("{x:.2}")
}

fn cell_text(r: &GridRow, col: usize, this_year: i32) -> String {
    match col {
        0 => r.id.to_string(),
        1 => safe_text(&r.item),
        2 => r.qty.to_string(),
        3 => money(r.price),
        4 => money(r.total()),
        5 => choice(&STATUSES, &r.status).map_or(r.status.clone(), |c| c.label.to_string()),
        _ => r
            .due
            .as_deref()
            .and_then(Date::parse_iso)
            .map_or("—".into(), |d| d.short(this_year).trim().to_string()),
    }
}

/// The raw editable text of a cell (what the editor starts with).
fn edit_text(r: &GridRow, col: usize) -> String {
    match col {
        1 => r.item.clone(),
        2 => r.qty.to_string(),
        3 => demo_core::keys::num_value(r.price).to_string(),
        6 => r.due.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

fn current(app: &App) -> Option<GridRow> {
    let rs = rows(app);
    let v = view(&rs, app.tabs.grid.sort);
    v.get(app.tabs.grid.row).map(|&i| rs[i].clone())
}

fn update(app: &mut App, id: u64, f: impl FnOnce(&mut GridRow)) -> bool {
    let mut rs = rows(app);
    let Some(r) = rs.iter_mut().find(|r| r.id == id) else {
        return false;
    };
    f(r);
    app.commit("grid.rows", json!(rs))
}

fn start_edit(app: &mut App, seed: Option<char>) {
    let Some(r) = current(app) else { return };
    let col = app.tabs.grid.col;
    if !COLS[col].2 {
        app.info(format!(
            "{} is {}",
            COLS[col].0,
            if col == 0 {
                "the row id (read-only)"
            } else {
                "calculated: qty × price"
            }
        ));
        return;
    }
    let mut ed = Editor {
        id: r.id,
        col,
        input: TextInput::new(&edit_text(&r, col)),
        drop: Drop::default(),
    };
    if col == 5 {
        let cur = STATUSES.iter().position(|s| s.id == r.status);
        ed.drop.open_at(cur);
        if let Some(c) = seed {
            let ls: Vec<&str> = STATUSES.iter().map(|s| s.label).collect();
            ed.drop
                .key(&ls, &KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    } else if let Some(c) = seed {
        ed.input.set(&c.to_string());
    }
    app.tabs.grid.editor = Some(ed);
}

/// Validate the editor's text and commit it. Returns true when done.
fn save_edit(app: &mut App) -> bool {
    let Some(ed) = app.tabs.grid.editor.as_ref() else {
        return true;
    };
    let (id, col, text) = (ed.id, ed.col, ed.input.text());
    let today = app.today;
    let res: Result<Apply, String> = match col {
        1 => {
            let t = text.trim().to_string();
            let n = t.chars().count();
            if n == 0 || n > 40 {
                Err("item is 1–40 characters".into())
            } else {
                Ok(Box::new(move |r| r.item = t))
            }
        }
        2 => match text.trim().parse::<u64>() {
            Ok(q) if q <= 9999 => Ok(Box::new(move |r| r.qty = q)),
            _ => Err("qty is a whole number 0–9999".into()),
        },
        3 => match text.trim().parse::<f64>() {
            Ok(p) if p.is_finite() && (0.0..=1_000_000.0).contains(&p) => {
                Ok(Box::new(move |r| r.price = round2(p)))
            }
            _ => Err("price is 0–1,000,000".into()),
        },
        6 => match parse_natural(&text, today) {
            Ok(d) => Ok(Box::new(move |r| r.due = d.map(|d| d.iso()))),
            Err(e) => Err(e),
        },
        _ => return true,
    };
    match res {
        Ok(f) => {
            app.tabs.grid.editor = None;
            update(app, id, f);
            true
        }
        Err(e) => {
            app.set_status(format!("✗ {e}"), Level::Err);
            false
        }
    }
}

fn move_to(app: &mut App, row: isize, col: isize) {
    let n = rows(app).len();
    let s = &mut app.tabs.grid;
    s.row = row.clamp(0, n.saturating_sub(1) as isize) as usize;
    s.col = col.clamp(0, COLS.len() as isize - 1) as usize;
}

fn add_row(app: &mut App) {
    let mut rs = rows(app);
    if rs.len() >= MAX_GRID_ROWS {
        app.set_status(format!("✗ at most {MAX_GRID_ROWS} rows"), Level::Err);
        return;
    }
    let id = next_id(rs.iter().map(|r| r.id));
    rs.push(GridRow {
        id,
        item: "New item".into(),
        qty: 1,
        price: 0.0,
        status: "todo".into(),
        due: None,
    });
    if app.commit("grid.rows", json!(rs)) {
        // select the new row (in view order) and edit its Item
        let rs = rows(app);
        let v = view(&rs, app.tabs.grid.sort);
        let pos = v.iter().position(|&i| rs[i].id == id).unwrap_or(0);
        app.tabs.grid.row = pos;
        app.tabs.grid.col = 1;
        start_edit(app, None);
    }
}

fn cycle_sort(app: &mut App, col: usize) {
    let s = &mut app.tabs.grid;
    s.sort = match s.sort {
        Some((c, false)) if c == col => Some((col, true)),
        Some((c, true)) if c == col => None,
        _ => Some((col, false)),
    };
    let what = match s.sort {
        Some((c, d)) => format!(
            "Sorted by {} {} (view only; the stored order is unchanged)",
            COLS[c].0,
            if d { "▼" } else { "▲" }
        ),
        None => "Sort off: stored order".into(),
    };
    app.info(what);
}

fn editor_key(app: &mut App, k: KeyEvent) -> bool {
    let Some(ed) = app.tabs.grid.editor.as_mut() else {
        return false;
    };
    if ed.col == 5 {
        let ls: Vec<&str> = STATUSES.iter().map(|s| s.label).collect();
        match ed.drop.key(&ls, &k) {
            DropOut::Pick(i) => {
                let id = ed.id;
                app.tabs.grid.editor = None;
                update(app, id, |r| r.status = STATUSES[i].id.to_string());
            }
            DropOut::Closed => app.tabs.grid.editor = None,
            DropOut::Used => {}
            DropOut::Ignored => {
                if matches!(k.code, KeyCode::Tab | KeyCode::BackTab) {
                    app.tabs.grid.editor = None;
                }
            }
        }
        return true;
    }
    match k.code {
        KeyCode::Esc => app.tabs.grid.editor = None,
        KeyCode::Enter => {
            if save_edit(app) {
                let (r, c) = (app.tabs.grid.row as isize, app.tabs.grid.col as isize);
                move_to(app, r + 1, c);
            }
        }
        KeyCode::Tab | KeyCode::BackTab => {
            if save_edit(app) {
                let c = app.tabs.grid.col as isize + if k.code == KeyCode::Tab { 1 } else { -1 };
                let r = app.tabs.grid.row as isize;
                move_to(app, r, c);
            }
        }
        _ => {
            ed.input.handle(&k);
        }
    }
    true
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    if app.tabs.grid.editor.is_some() {
        return editor_key(app, k);
    }
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let (r, c) = (app.tabs.grid.row as isize, app.tabs.grid.col as isize);
    let n = rows(app).len() as isize;
    let page = app.tabs.grid.body_rows.max(2) as isize - 1;
    match k.code {
        KeyCode::Up => move_to(app, r - 1, c),
        KeyCode::Down => move_to(app, r + 1, c),
        KeyCode::Left => move_to(app, r, c - 1),
        KeyCode::Right => move_to(app, r, c + 1),
        KeyCode::Home if ctrl => move_to(app, 0, 0),
        KeyCode::End if ctrl => move_to(app, n - 1, 6),
        KeyCode::Home => move_to(app, r, 0),
        KeyCode::End => move_to(app, r, 6),
        KeyCode::PageUp => move_to(app, r - page, c),
        KeyCode::PageDown => move_to(app, r + page, c),
        KeyCode::Tab => {
            if c < 6 {
                move_to(app, r, c + 1);
            } else {
                move_to(app, if r + 1 >= n { 0 } else { r + 1 }, 0);
            }
        }
        KeyCode::BackTab => {
            if r == 0 && c == 0 {
                return false;
            }
            if c > 0 {
                move_to(app, r, c - 1)
            } else {
                move_to(app, r - 1, 6)
            }
        }
        KeyCode::Enter | KeyCode::F(2) => start_edit(app, None),
        KeyCode::Insert => add_row(app),
        KeyCode::Delete if ctrl => delete_row(app),
        KeyCode::F(8) => delete_row(app),
        KeyCode::Delete => {
            if let Some(row) = current(app) {
                if row.due.is_some() {
                    update(app, row.id, |r| r.due = None);
                } else {
                    app.info("Due is already empty");
                }
            }
        }
        KeyCode::Char('o') if ctrl => cycle_sort(app, c as usize),
        KeyCode::Char(ch) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => {
            start_edit(app, Some(ch))
        }
        _ => return false,
    }
    true
}

fn delete_row(app: &mut App) {
    if let Some(r) = current(app) {
        app.dialogs.push(Dialog::confirm(
            "Delete row",
            &format!("Delete row {} “{}”?", r.id, safe_text(&r.item)),
            "Delete",
            DialogAct::DeleteRow(r.id),
        ));
    }
}

pub fn click(app: &mut App, hit: &Hit, x: u16, y: u16) -> bool {
    match hit {
        Hit::Cell(r, c) => {
            let dbl = app.is_double(x, y);
            if app.tabs.grid.editor.is_some() && !save_edit(app) {
                return true;
            }
            app.focus_ctrl(0);
            app.tabs.grid.row = *r;
            app.tabs.grid.col = *c;
            if dbl {
                start_edit(app, None);
            }
            true
        }
        Hit::Header(c) => {
            app.focus_ctrl(0);
            app.tabs.grid.col = *c;
            cycle_sort(app, *c);
            true
        }
        Hit::Popup(0, j) => {
            if let Some(ed) = app.tabs.grid.editor.take() {
                let st = STATUSES[*j].id.to_string();
                update(app, ed.id, |r| r.status = st);
            }
            true
        }
        _ => false,
    }
}

pub fn hints(app: &App) -> Vec<Hint> {
    match &app.tabs.grid.editor {
        Some(e) if e.col == 5 => super::select::drop_hints(true),
        Some(_) => vec![
            h("Enter", "Save ↓", KeyCode::Enter),
            h("Tab", "Save →", KeyCode::Tab),
            h("Esc", "Cancel", KeyCode::Esc),
        ],
        None => vec![
            h("Enter", "Edit", KeyCode::Enter),
            h("Ins", "Add row", KeyCode::Insert),
            h("Del", "Clear due", KeyCode::Delete),
            h("F8", "Delete row", KeyCode::F(8)),
            hc("^O", "Sort", 'o'),
        ],
    }
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let focused = is_focused(app, 0);
    let rs = rows(app);
    let v = view(&rs, app.tabs.grid.sort);
    let s = &mut app.tabs.grid;
    s.row = s.row.min(v.len().saturating_sub(1));
    let x0 = area.x + 1;
    let total_w: u16 = COLS.iter().map(|c| c.1 + 1).sum();
    // header
    let hs = Style::new()
        .bg(t.header_bg)
        .fg(t.fg)
        .add_modifier(Modifier::BOLD);
    fill(buf, Rect::new(x0, area.y, total_w, 1), hs);
    let mut x = x0;
    for (i, (title, w, _)) in COLS.iter().enumerate() {
        let arrow = match s.sort {
            Some((c, d)) if c == i => {
                if d {
                    " ▼"
                } else {
                    " ▲"
                }
            }
            _ => "",
        };
        let right = matches!(i, 0 | 2 | 3 | 4);
        put(
            buf,
            x,
            area.y,
            &pad(&format!("{title}{arrow}"), *w as usize, right),
            hs,
            *w as usize,
        );
        app.hits.push((Rect::new(x, area.y, *w, 1), Hit::Header(i)));
        x += w + 1;
    }
    // body
    let body_h = (area.height.saturating_sub(3) as usize).max(1);
    let s = &mut app.tabs.grid;
    s.body_rows = body_h;
    if s.row < s.scroll {
        s.scroll = s.row;
    } else if s.row >= s.scroll + body_h {
        s.scroll = s.row + 1 - body_h;
    }
    let (cur_row, cur_col, scroll) = (s.row, s.col, s.scroll);
    let this_year = app.today.year();
    for (vi, &ri) in v.iter().enumerate().skip(scroll).take(body_h) {
        let y = area.y + 1 + (vi - scroll) as u16;
        let r = &rs[ri];
        let zebra = if vi % 2 == 1 { t.zebra } else { t.bg };
        let row_bg = if vi == cur_row && focused {
            t.sel_bg
        } else {
            zebra
        };
        let mut x = x0;
        for (ci, (_, w, editable)) in COLS.iter().enumerate() {
            let cur = vi == cur_row && ci == cur_col;
            let bg = if cur && focused {
                t.cur_bg
            } else if cur {
                t.sel_bg
            } else {
                row_bg
            };
            let mut st = Style::new()
                .bg(bg)
                .fg(if *editable { t.fg } else { t.muted });
            fill(buf, Rect::new(x, y, w + 1, 1), Style::new().bg(row_bg));
            let text = cell_text(r, ci, this_year);
            if ci == 5 {
                let col = choice(&STATUSES, &r.status).map_or(t.fg, |c| t.hex(c.color, t.fg));
                fill(buf, Rect::new(x, y, *w, 1), Style::new().bg(bg));
                put(buf, x, y, "●", Style::new().fg(col).bg(bg), 1);
                put(buf, x + 2, y, &text, st, *w as usize - 2);
            } else {
                if ci == 6 && r.due.is_none() {
                    st = st.fg(t.muted);
                }
                put(
                    buf,
                    x,
                    y,
                    &pad(&text, *w as usize, matches!(ci, 0 | 2 | 3 | 4)),
                    st,
                    *w as usize,
                );
            }
            if cur {
                app.tabs.grid.cell = Rect::new(x, y, *w, 1);
            }
            app.hits
                .push((Rect::new(x, y, *w + 1, 1), Hit::Cell(vi, ci)));
            x += w + 1;
        }
    }
    // totals
    let ty = area.y + 1 + (v.len().saturating_sub(scroll).min(body_h)) as u16;
    let ts = Style::new()
        .bg(t.header_bg)
        .fg(t.fg)
        .add_modifier(Modifier::BOLD);
    fill(buf, Rect::new(x0, ty, total_w, 1), ts);
    let qty: u64 = rs.iter().map(|r| r.qty).sum();
    let total: f64 = round2(rs.iter().map(GridRow::total).sum());
    let xs: Vec<u16> = COLS
        .iter()
        .scan(x0, |acc, c| {
            let here = *acc;
            *acc += c.1 + 1;
            Some(here)
        })
        .collect();
    put(
        buf,
        xs[1],
        ty,
        &format!("Totals · {} rows", rs.len()),
        ts,
        18,
    );
    put(buf, xs[2], ty, &pad(&qty.to_string(), 6, true), ts, 6);
    put(buf, xs[4], ty, &pad(&money(total), 12, true), ts, 12);
    app.key_rects.push((
        "grid.rows",
        Rect::new(x0, area.y + 1, total_w, ty - area.y - 1),
    ));
    if v.len() > body_h {
        put(
            buf,
            x0 + total_w + 1,
            area.y + 1,
            &format!("{}/{}", cur_row + 1, v.len()),
            muted(&t),
            8,
        );
    }
    let note = match app.tabs.grid.sort {
        Some(_) => "sorted view · ^O again to change · the stored order is unchanged",
        None => "Total = Qty × Price (calculated) · # is read-only",
    };
    if ty + 1 < area.bottom() {
        put(buf, x0, ty + 1, note, muted(&t), area.width as usize - 2);
    }
}

pub fn popup(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let cell = app.tabs.grid.cell;
    let today = app.today;
    let Some(ed) = app.tabs.grid.editor.as_mut() else {
        return;
    };
    if ed.col == 5 {
        let rows = super::select::rows(&STATUSES, &ed.drop, &t);
        let r = Rect::new(cell.x, cell.y + 1, 20, rows.len() as u16 + 2);
        let sel = ed.drop.sel;
        for (rect, j) in list::render(buf, r, "Status", &rows, Some(sel), &t) {
            // map the filtered row back to the status index
            let ls: Vec<&str> = STATUSES.iter().map(|s| s.label).collect();
            let idx = ed.drop.visible(&ls).get(j).copied().unwrap_or(j);
            app.hits.push((rect, Hit::Popup(0, idx)));
        }
        return;
    }
    let w = cell.width.max(if ed.col == 6 { 18 } else { 14 });
    let r = Rect::new(cell.x.min(area.right().saturating_sub(w)), cell.y, w, 1);
    ed.input.render(
        buf,
        r,
        Style::new()
            .bg(t.sel_bg)
            .fg(t.fg)
            .add_modifier(Modifier::BOLD),
        true,
    );
    if ed.col == 6 {
        let p = match preview(&ed.input.text(), today) {
            Ok(p) => (p, Style::new().fg(t.info).bg(t.panel)),
            Err(e) => (format!("✗ {e}"), Style::new().fg(t.err).bg(t.panel)),
        };
        let pw = (width(&p.0) as u16 + 2).max(w);
        let pr = Rect::new(r.x.min(area.right().saturating_sub(pw)), r.y + 1, pw, 1);
        fill(buf, pr, Style::new().bg(t.panel));
        put(buf, pr.x + 1, pr.y, &p.0, p.1, pw as usize - 1);
    }
}
