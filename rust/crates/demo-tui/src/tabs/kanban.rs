//! Tab 15 — Kanban: `kanban.cards` in four columns. Shift+←/→ (or < >)
//! moves the selected card; Ins adds, Enter renames, F8 deletes; cards can
//! be dragged with the mouse. Every change commits the whole list.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::dataset::KANBAN_COLUMNS;
use demo_core::keys::KanbanCard;
use demo_core::safe_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::json;

use super::{h, is_focused};
use crate::app::{App, Drag, Hint, Hit, hint};
use crate::dialogs::{Dialog, DialogAct};
use crate::widgets::draw::{fill, put};

pub const KEYS: &[(&str, &str)] = &[
    ("←→ ↑↓", "select"),
    ("Shift+←→ < >", "move card"),
    ("Ins", "new card"),
    ("Enter", "rename"),
    ("F8 Del", "delete"),
    ("drag", "move with the mouse"),
];

#[derive(Default)]
pub struct State {
    pub col: usize,
    pub idx: usize,
    /// Select this card once it shows up (after Ins).
    pub select_id: Option<u64>,
    /// Column under a dragged card.
    pub drag_over: Option<usize>,
    cols: [Rect; 4],
}

pub fn cards(app: &App) -> Vec<KanbanCard> {
    app.shown_get("kanban.cards").unwrap_or_default()
}

/// Cards of column `c` in list order.
pub fn column(cards: &[KanbanCard], c: usize) -> Vec<&KanbanCard> {
    cards
        .iter()
        .filter(|k| k.col == KANBAN_COLUMNS[c].id)
        .collect()
}

fn selected(app: &App) -> Option<KanbanCard> {
    let cs = cards(app);
    column(&cs, app.tabs.kanban.col)
        .get(app.tabs.kanban.idx)
        .map(|c| (*c).clone())
}

/// Move a card to column `to` and keep it selected.
fn move_card(app: &mut App, id: u64, to: usize) {
    let mut cs = cards(app);
    let Some(card) = cs.iter_mut().find(|c| c.id == id) else {
        return;
    };
    if card.col == KANBAN_COLUMNS[to].id {
        return;
    }
    card.col = KANBAN_COLUMNS[to].id.to_string();
    if app.commit("kanban.cards", json!(cs)) {
        app.tabs.kanban.select_id = Some(id);
    }
}

fn reselect(app: &mut App) {
    let cs = cards(app);
    let s = &mut app.tabs.kanban;
    if let Some(id) = s.select_id {
        for c in 0..4 {
            if let Some(i) = column(&cs, c).iter().position(|k| k.id == id) {
                s.col = c;
                s.idx = i;
                s.select_id = None;
                break;
            }
        }
    }
    let n = column(&cs, s.col).len();
    s.idx = s.idx.min(n.saturating_sub(1));
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    reselect(app);
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    let s = &mut app.tabs.kanban;
    let cur = s.col;
    match k.code {
        KeyCode::Left | KeyCode::Right if shift => {
            let to = if k.code == KeyCode::Left {
                cur.checked_sub(1)
            } else {
                (cur < 3).then_some(cur + 1)
            };
            if let (Some(to), Some(card)) = (to, selected(app)) {
                move_card(app, card.id, to);
            }
        }
        KeyCode::Char('<') | KeyCode::Char('>') => {
            let to = if k.code == KeyCode::Char('<') {
                cur.checked_sub(1)
            } else {
                (cur < 3).then_some(cur + 1)
            };
            if let (Some(to), Some(card)) = (to, selected(app)) {
                move_card(app, card.id, to);
            }
        }
        KeyCode::Left => s.col = (s.col + 3) % 4,
        KeyCode::Right => s.col = (s.col + 1) % 4,
        KeyCode::Up => s.idx = s.idx.saturating_sub(1),
        KeyCode::Down => s.idx += 1,
        KeyCode::Home => s.idx = 0,
        KeyCode::End => s.idx = usize::MAX,
        KeyCode::Insert => {
            let col = KANBAN_COLUMNS[cur];
            app.dialogs.push(Dialog::input(
                "New card",
                &format!("Title of the new card in {}:", col.label),
                "",
                DialogAct::NewCard(col.id.into()),
            ));
        }
        KeyCode::Enter => {
            if let Some(c) = selected(app) {
                app.dialogs.push(Dialog::input(
                    "Rename card",
                    &format!("New title for #{}:", c.id),
                    &c.title,
                    DialogAct::RenameCard(c.id),
                ));
            }
        }
        KeyCode::F(8) => delete(app),
        KeyCode::Delete => delete(app),
        _ => return false,
    }
    reselect(app);
    true
}

fn delete(app: &mut App) {
    if let Some(c) = selected(app) {
        app.dialogs.push(Dialog::confirm(
            "Delete card",
            &format!("Delete card #{} “{}”?", c.id, safe_text(&c.title)),
            "Delete",
            DialogAct::DeleteCard(c.id),
        ));
    }
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    match hit {
        Hit::Card(id) => {
            app.focus_ctrl(0);
            app.tabs.kanban.select_id = Some(*id);
            reselect(app);
            app.drag = Some(Drag::Card { id: *id });
            true
        }
        Hit::Column(c) => {
            app.focus_ctrl(0);
            app.tabs.kanban.col = *c;
            reselect(app);
            true
        }
        _ => false,
    }
}

pub fn drag(app: &mut App, x: u16, y: u16, end: bool) {
    let Some(Drag::Card { id }) = app.drag else {
        return;
    };
    let over = app
        .tabs
        .kanban
        .cols
        .iter()
        .position(|r| x >= r.x && x < r.right() && y >= r.y && y < r.bottom());
    app.tabs.kanban.drag_over = if end { None } else { over };
    if end {
        if let Some(to) = over {
            move_card(app, id, to);
        }
    }
}

pub fn hints(_app: &App) -> Vec<Hint> {
    vec![
        h("←→↑↓", "Select", KeyCode::Right),
        hint("⇧→", "Move", KeyCode::Right, KeyModifiers::SHIFT),
        h("Ins", "New", KeyCode::Insert),
        h("Enter", "Rename", KeyCode::Enter),
        h("F8", "Delete", KeyCode::F(8)),
    ]
}

#[allow(clippy::needless_range_loop)]
pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    reselect(app);
    let t = app.t.clone();
    let focused = is_focused(app, 0);
    let cs = cards(app);
    let gap = 1;
    let cw = (area.width.saturating_sub(2 + 3 * gap)) / 4;
    let cw = cw.min(26);
    let s = &app.tabs.kanban;
    let (scol, sidx, over) = (s.col, s.idx, s.drag_over);
    let dragging = match app.drag {
        Some(Drag::Card { id }) => Some(id),
        _ => None,
    };
    for c in 0..4 {
        let x = area.x + 1 + c as u16 * (cw + gap);
        let r = Rect::new(x, area.y, cw, area.height);
        app.tabs.kanban.cols[c] = r;
        let col = KANBAN_COLUMNS[c];
        let color = t.hex(col.color, t.fg);
        let list = column(&cs, c);
        let bg = if over == Some(c) { t.sel_bg } else { t.panel };
        fill(buf, r, Style::new().bg(bg));
        app.hits.push((r, Hit::Column(c)));
        let hs = Style::new().fg(color).bg(bg).add_modifier(Modifier::BOLD);
        put(
            buf,
            x,
            area.y,
            "▀".repeat(cw as usize).as_str(),
            Style::new().fg(color).bg(bg),
            cw as usize,
        );
        let head = format!("{} ({})", col.label, list.len());
        put(
            buf,
            x + 1,
            area.y + 1,
            &head,
            if c == scol && focused {
                hs.add_modifier(Modifier::UNDERLINED)
            } else {
                hs
            },
            cw as usize - 2,
        );
        // cards: 2 rows each plus a gap, scrolled to keep the selection visible
        let room = area.height.saturating_sub(3) as usize / 3;
        let off = if c == scol && sidx >= room {
            sidx + 1 - room
        } else {
            0
        };
        for (vi, (i, card)) in list.iter().enumerate().skip(off).take(room).enumerate() {
            let y = area.y + 3 + vi as u16 * 3;
            let sel = c == scol && i == sidx;
            let cbg = if dragging == Some(card.id) {
                t.flash
            } else if sel && focused {
                t.cur_bg
            } else if sel {
                t.sel_bg
            } else {
                t.bg
            };
            let cr = Rect::new(x + 1, y, cw - 2, 2);
            fill(buf, cr, Style::new().bg(cbg));
            put(buf, x + 1, y, "▌", Style::new().fg(color).bg(cbg), 1);
            put(buf, x + 1, y + 1, "▌", Style::new().fg(color).bg(cbg), 1);
            put(
                buf,
                x + 3,
                y,
                &safe_text(&card.title),
                Style::new().fg(t.fg).bg(cbg).add_modifier(Modifier::BOLD),
                cw as usize - 5,
            );
            put(
                buf,
                x + 3,
                y + 1,
                &format!("#{}", card.id),
                Style::new().fg(t.muted).bg(cbg),
                cw as usize - 5,
            );
            app.hits.push((cr, Hit::Card(card.id)));
        }
        let more = list.len().saturating_sub(off + room);
        if more > 0 {
            put(
                buf,
                x + 1,
                area.bottom() - 1,
                &format!("+{more} more ↓"),
                Style::new().fg(t.muted).bg(bg),
                cw as usize - 2,
            );
        }
    }
    app.key_rects.push((
        "kanban.cards",
        Rect::new(area.x + 1, area.y + 1, 4 * (cw + gap), 1),
    ));
}
