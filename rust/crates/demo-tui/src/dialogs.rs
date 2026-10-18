//! Modal popups: confirm, message, one-line input and the profile form
//! (tab 13), also used by ^R, deletes, renames and new cards. A dialog
//! carries a [`DialogAct`] saying what to do with the answer.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::dataset::TABS;
use demo_core::keys::{FileEntry, GridRow, KanbanCard, Profile, valid_email};
use demo_core::safe_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::{Value, json};

use crate::app::{App, Hint, Hit, Level, hint};
use crate::widgets::draw::{centered, modal, put, width};
use crate::widgets::input::TextInput;

#[derive(Clone, Debug, PartialEq)]
pub enum DialogAct {
    ResetTab(usize),
    ResetAll,
    /// Tab 13's demo confirm.
    DemoConfirm,
    DeleteRow(u64),
    DeleteCard(u64),
    DeleteFile(String),
    DiscardNotes,
    RenameFile(String),
    NewCard(String),
    RenameCard(u64),
    Broadcast,
}

pub struct Confirm {
    pub title: String,
    pub text: String,
    pub yes: String,
    /// 0 = Cancel (the default), 1 = yes.
    pub sel: usize,
    pub act: DialogAct,
}

pub struct InputDlg {
    pub title: String,
    pub prompt: String,
    pub input: TextInput,
    pub error: Option<String>,
    pub act: DialogAct,
}

pub struct ProfileForm {
    pub name: TextInput,
    pub email: TextInput,
    /// 0 name, 1 email, 2 [Save], 3 [Cancel]
    pub field: usize,
    pub base: u64,
    pub tried: bool,
}

pub enum Dialog {
    Confirm(Confirm),
    Message { title: String, text: String },
    Input(InputDlg),
    Form(ProfileForm),
}

impl Dialog {
    pub fn confirm(title: &str, text: &str, yes: &str, act: DialogAct) -> Dialog {
        Dialog::Confirm(Confirm {
            title: title.into(),
            text: text.into(),
            yes: yes.into(),
            sel: 0,
            act,
        })
    }
    pub fn message(title: &str, text: &str) -> Dialog {
        Dialog::Message {
            title: title.into(),
            text: text.into(),
        }
    }
    pub fn input(title: &str, prompt: &str, initial: &str, act: DialogAct) -> Dialog {
        Dialog::Input(InputDlg {
            title: title.into(),
            prompt: prompt.into(),
            input: TextInput::new(initial),
            error: None,
            act,
        })
    }

    pub fn hints(&self) -> Vec<Hint> {
        let n = KeyModifiers::NONE;
        match self {
            Dialog::Confirm(_) => vec![
                hint("←→", "Choose", KeyCode::Right, n),
                hint("Enter", "OK", KeyCode::Enter, n),
                hint("y", "Yes", KeyCode::Char('y'), n),
                hint("n", "No", KeyCode::Char('n'), n),
                hint("Esc", "Cancel", KeyCode::Esc, n),
            ],
            Dialog::Message { .. } => vec![
                hint("Enter", "OK", KeyCode::Enter, n),
                hint("Esc", "Close", KeyCode::Esc, n),
            ],
            Dialog::Input(_) => vec![
                hint("Enter", "OK", KeyCode::Enter, n),
                hint("Esc", "Cancel", KeyCode::Esc, n),
                hint("^U", "Clear", KeyCode::Char('u'), KeyModifiers::CONTROL),
            ],
            Dialog::Form(_) => vec![
                hint("Tab", "Next field", KeyCode::Tab, n),
                hint("^S", "Save", KeyCode::Char('s'), KeyModifiers::CONTROL),
                hint("Enter", "Next/Save", KeyCode::Enter, n),
                hint("Esc", "Cancel", KeyCode::Esc, n),
            ],
        }
    }
}

impl ProfileForm {
    pub fn new(p: &Profile, base: u64) -> ProfileForm {
        ProfileForm {
            name: TextInput::new(&p.name),
            email: TextInput::new(&p.email),
            field: 0,
            base,
            tried: false,
        }
    }

    /// Per-field error messages (name, email).
    pub fn errors(&self) -> (Option<String>, Option<String>) {
        let name = self.name.text();
        let n = name.trim().chars().count();
        let ne = if n == 0 {
            Some("name is required".to_string())
        } else if n > 80 {
            Some("at most 80 characters".to_string())
        } else {
            None
        };
        let email = self.email.text().trim().to_lowercase();
        let ee = if email.is_empty() {
            Some("email is required".to_string())
        } else if !valid_email(&email) || email.chars().count() > 120 {
            Some("must look like name@example.com".to_string())
        } else {
            None
        };
        (ne, ee)
    }

    pub fn value(&self) -> Value {
        json!({"name": self.name.text().trim(), "email": self.email.text().trim().to_lowercase()})
    }
}

/// Keys for the top dialog.
pub fn key(app: &mut App, k: KeyEvent) {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let Some(d) = app.dialogs.last_mut() else {
        return;
    };
    match d {
        Dialog::Message { .. } => {
            if matches!(k.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ')) {
                app.dialogs.pop();
            }
        }
        Dialog::Confirm(c) => match k.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => c.sel = 1 - c.sel,
            KeyCode::Enter | KeyCode::Char(' ') => {
                let ok = c.sel == 1;
                finish_confirm(app, ok);
            }
            KeyCode::Char('y' | 'Y') => finish_confirm(app, true),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => finish_confirm(app, false),
            _ => {}
        },
        Dialog::Input(d) => match k.code {
            KeyCode::Esc => {
                app.dialogs.pop();
            }
            KeyCode::Enter => {
                let text = d.input.text();
                let act = d.act.clone();
                if let Err(e) = apply_result(app, act, true, Some(text)) {
                    if let Some(Dialog::Input(d)) = app.dialogs.last_mut() {
                        d.error = Some(e);
                    }
                } else {
                    app.dialogs.pop();
                }
            }
            _ => {
                if d.input.handle(&k) {
                    d.error = None;
                }
            }
        },
        Dialog::Form(f) => match k.code {
            KeyCode::Esc => {
                app.dialogs.pop();
                app.info("Form cancelled");
            }
            KeyCode::Tab | KeyCode::Down => f.field = (f.field + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => f.field = (f.field + 3) % 4,
            KeyCode::Left | KeyCode::Right if f.field >= 2 => {
                f.field = if f.field == 2 { 3 } else { 2 }
            }
            KeyCode::Char('s') if ctrl => save_form(app),
            KeyCode::Enter => match f.field {
                0 | 1 => f.field += 1,
                2 => save_form(app),
                _ => {
                    app.dialogs.pop();
                    app.info("Form cancelled");
                }
            },
            _ => {
                let input = if f.field == 0 {
                    &mut f.name
                } else {
                    &mut f.email
                };
                if f.field < 2 {
                    input.handle(&k);
                }
            }
        },
    }
}

fn finish_confirm(app: &mut App, ok: bool) {
    if let Some(Dialog::Confirm(c)) = app.dialogs.pop() {
        apply(app, c.act, ok, None);
    }
}

fn save_form(app: &mut App) {
    let Some(Dialog::Form(f)) = app.dialogs.last_mut() else {
        return;
    };
    f.tried = true;
    let (ne, ee) = f.errors();
    if ne.is_some() || ee.is_some() {
        f.field = if ne.is_some() { 0 } else { 1 };
        return;
    }
    let (value, base) = (f.value(), f.base);
    if app.commit_from("dialog.profile", value, Some(base)) {
        app.dialogs.pop();
    }
}

/// Mouse on an open dialog.
pub fn click(app: &mut App, hit: Option<Hit>) {
    let Some(d) = app.dialogs.last_mut() else {
        return;
    };
    match (d, hit) {
        (Dialog::Message { .. }, Some(Hit::DialogBtn(_))) => {
            app.dialogs.pop();
        }
        (Dialog::Confirm(_), Some(Hit::DialogBtn(i))) => finish_confirm(app, i == 1),
        (Dialog::Input(_), Some(Hit::DialogBtn(i))) => {
            let ev = if i == 0 { KeyCode::Enter } else { KeyCode::Esc };
            key(app, KeyEvent::new(ev, KeyModifiers::NONE));
        }
        (Dialog::Form(f), Some(Hit::DialogField(i))) => f.field = i,
        (Dialog::Form(f), Some(Hit::DialogBtn(i))) => {
            f.field = 2 + i;
            key(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        _ => {}
    }
}

/// Apply a dialog's answer, reporting problems on the status line.
pub fn apply(app: &mut App, act: DialogAct, ok: bool, text: Option<String>) {
    if let Err(e) = apply_result(app, act, ok, text) {
        app.set_status(format!("✗ {e}"), Level::Err);
    }
}

/// Apply a dialog's answer. `Err` keeps an input dialog open with the message.
fn apply_result(
    app: &mut App,
    act: DialogAct,
    ok: bool,
    text: Option<String>,
) -> Result<(), String> {
    let text = text.unwrap_or_default();
    match act {
        DialogAct::DemoConfirm => app.info(if ok {
            "Confirm: you chose Delete"
        } else {
            "Confirm: you chose Cancel"
        }),
        _ if !ok => app.info("Cancelled"),
        DialogAct::ResetTab(i) => {
            if !app.online() {
                return Err("Offline — nothing was reset".into());
            }
            let msg = app.v.reset_tab(TABS[i].id).map_err(|e| e.to_string())?;
            app.outbox.push(msg);
        }
        DialogAct::ResetAll => {
            if !app.online() {
                return Err("Offline — nothing was reset".into());
            }
            let msg = app.v.reset(&[]).map_err(|e| e.to_string())?;
            app.outbox.push(msg);
        }
        DialogAct::DeleteRow(id) => {
            let mut rows: Vec<GridRow> = app.shown_get("grid.rows").unwrap_or_default();
            rows.retain(|r| r.id != id);
            app.commit("grid.rows", json!(rows));
        }
        DialogAct::DeleteCard(id) => {
            let mut cards: Vec<KanbanCard> = app.shown_get("kanban.cards").unwrap_or_default();
            cards.retain(|c| c.id != id);
            app.commit("kanban.cards", json!(cards));
        }
        DialogAct::DeleteFile(name) => {
            let mut files: Vec<FileEntry> = app.shown_get("menus.files").unwrap_or_default();
            files.retain(|f| f.name != name);
            app.commit("menus.files", json!(files));
        }
        DialogAct::DiscardNotes => crate::tabs::textarea::discard(app),
        DialogAct::RenameFile(old) => {
            let new = text.trim().to_string();
            let n = new.chars().count();
            if n == 0 || n > 40 {
                return Err("a name is 1–40 characters".into());
            }
            let mut files: Vec<FileEntry> = app.shown_get("menus.files").unwrap_or_default();
            if new != old && files.iter().any(|f| f.name == new) {
                return Err(format!("{new} already exists"));
            }
            if let Some(f) = files.iter_mut().find(|f| f.name == old) {
                f.name = new;
            }
            if !app.commit("menus.files", json!(files)) {
                return Err(app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default());
            }
        }
        DialogAct::NewCard(col) => {
            let title = card_title(&text)?;
            let mut cards: Vec<KanbanCard> = app.shown_get("kanban.cards").unwrap_or_default();
            if cards.len() >= 40 {
                return Err("the board holds at most 40 cards".into());
            }
            let id = demo_core::keys::next_id(cards.iter().map(|c| c.id));
            cards.push(KanbanCard { id, title, col });
            app.tabs.kanban.select_id = Some(id);
            app.commit("kanban.cards", json!(cards));
        }
        DialogAct::RenameCard(id) => {
            let title = card_title(&text)?;
            let mut cards: Vec<KanbanCard> = app.shown_get("kanban.cards").unwrap_or_default();
            if let Some(c) = cards.iter_mut().find(|c| c.id == id) {
                c.title = title;
            }
            app.commit("kanban.cards", json!(cards));
        }
        DialogAct::Broadcast => {
            if !app.online() {
                return Err("Offline — not sent".into());
            }
            match app.v.toast(&text) {
                Some(m) => app.outbox.push(m),
                None => return Err("a message is 1–120 characters".into()),
            }
        }
    }
    Ok(())
}

fn card_title(text: &str) -> Result<String, String> {
    let t = text.trim();
    let n = t.chars().count();
    if n == 0 || n > 60 {
        Err("a title is 1–60 characters".into())
    } else {
        Ok(t.to_string())
    }
}

/// Draw one dialog centred on `area`, recording button/field hits.
pub fn render(
    d: &mut Dialog,
    buf: &mut Buffer,
    area: Rect,
    app_t: &crate::theme::Theme,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let t = app_t;
    let panel = t.panel();
    let button = |buf: &mut Buffer,
                  x: u16,
                  y: u16,
                  label: &str,
                  focused: bool,
                  hits: &mut Vec<(Rect, Hit)>,
                  i: usize|
     -> u16 {
        let s = format!("[ {label} ]");
        let st = if focused {
            Style::new()
                .bg(t.accent)
                .fg(t.bg)
                .add_modifier(Modifier::BOLD)
        } else {
            panel.fg(t.accent)
        };
        let w = width(&s) as u16;
        put(buf, x, y, &s, st, w as usize);
        hits.push((Rect::new(x, y, w, 1), Hit::DialogBtn(i)));
        w
    };
    match d {
        Dialog::Message { title, text } => {
            let lines: Vec<&str> = text.lines().collect();
            let w = lines.iter().map(|l| width(l)).max().unwrap_or(20).max(30) as u16 + 6;
            let r = centered(area, w, lines.len() as u16 + 5);
            let inner = modal(buf, r, title, t);
            for (i, l) in lines.iter().enumerate() {
                put(
                    buf,
                    inner.x + 2,
                    inner.y + 1 + i as u16,
                    l,
                    panel,
                    inner.width as usize - 3,
                );
            }
            let bx = inner.x + (inner.width.saturating_sub(6)) / 2;
            button(buf, bx, inner.bottom() - 1, "OK", true, hits, 0);
        }
        Dialog::Confirm(c) => {
            let w = (width(&c.text) as u16 + 6).clamp(36, area.width.saturating_sub(4));
            let r = centered(area, w, 7);
            let inner = modal(buf, r, &c.title, t);
            put(
                buf,
                inner.x + 2,
                inner.y + 1,
                &c.text,
                panel,
                inner.width as usize - 3,
            );
            let y = inner.bottom() - 1;
            let cw = width("[ Cancel ]") as u16;
            let yw = width(&c.yes) as u16 + 4;
            let x = inner.right().saturating_sub(cw + yw + 3);
            button(buf, x, y, "Cancel", c.sel == 0, hits, 0);
            let yes = c.yes.clone();
            button(buf, x + cw + 2, y, &yes, c.sel == 1, hits, 1);
        }
        Dialog::Input(d) => {
            let w = 56.min(area.width.saturating_sub(4));
            let r = centered(area, w, 8);
            let inner = modal(buf, r, &d.title, t);
            put(
                buf,
                inner.x + 2,
                inner.y + 1,
                &d.prompt,
                panel,
                inner.width as usize - 3,
            );
            let ir = Rect::new(inner.x + 2, inner.y + 2, inner.width - 4, 1);
            d.input
                .render(buf, ir, Style::new().bg(t.sel_bg).fg(t.fg), true);
            if let Some(e) = &d.error {
                put(
                    buf,
                    inner.x + 2,
                    inner.y + 3,
                    &format!("✗ {e}"),
                    panel.fg(t.err),
                    inner.width as usize - 3,
                );
            }
            let y = inner.bottom() - 1;
            let x = inner.right().saturating_sub(22);
            let w1 = button(buf, x, y, "OK", true, hits, 0);
            button(buf, x + w1 + 2, y, "Cancel", false, hits, 1);
        }
        Dialog::Form(f) => {
            let w = 60.min(area.width.saturating_sub(4));
            let r = centered(area, w, 11);
            let inner = modal(buf, r, "Edit profile (dialog.profile)", t);
            let (ne, ee) = f.errors();
            let fields: [(&str, &mut TextInput, Option<String>); 2] =
                [("Name", &mut f.name, ne), ("Email", &mut f.email, ee)];
            for (i, (label, input, err)) in fields.into_iter().enumerate() {
                let y = inner.y + 1 + i as u16 * 3;
                let focused = f.field == i;
                put(
                    buf,
                    inner.x + 2,
                    y,
                    label,
                    panel
                        .fg(if focused { t.accent } else { t.fg })
                        .add_modifier(Modifier::BOLD),
                    6,
                );
                let ir = Rect::new(inner.x + 9, y, inner.width - 11, 1);
                let bad = err.is_some() && (f.tried || !input.is_empty());
                let st = Style::new()
                    .bg(if focused { t.sel_bg } else { t.bg })
                    .fg(if bad { t.err } else { t.fg });
                input.render(buf, ir, st, focused);
                hits.push((ir, Hit::DialogField(i)));
                if let Some(e) = err.filter(|_| bad) {
                    put(
                        buf,
                        ir.x,
                        y + 1,
                        &format!("✗ {e}"),
                        panel.fg(t.err),
                        ir.width as usize,
                    );
                }
            }
            let y = inner.bottom() - 1;
            let x = inner.right().saturating_sub(24);
            let w1 = button(buf, x, y, "Save", f.field == 2, hits, 0);
            button(buf, x + w1 + 2, y, "Cancel", f.field == 3, hits, 1);
            put(buf, inner.x + 2, y, "^S saves", panel.fg(t.muted), 10);
        }
    }
}

/// Profile text for display (peer-supplied).
pub fn profile_line(p: &Profile) -> String {
    format!("{} <{}>", safe_text(&p.name), safe_text(&p.email))
}
