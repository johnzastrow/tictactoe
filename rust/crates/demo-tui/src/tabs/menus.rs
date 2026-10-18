//! Tab 11 — Menus: (a) a demo menu bar with submenus, separators, a
//! disabled item and checkable items (picking only reports what was
//! picked); (b) the `menus.files` list, whose context menu (Enter,
//! Shift+F10, Menu key, right-click) opens, renames, duplicates, stars and
//! deletes files.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::keys::{FileEntry, MAX_FILES};
use demo_core::safe_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::json;

use super::{h, is_focused, muted};
use crate::app::{App, Hint, Hit, Level};
use crate::dialogs::{Dialog, DialogAct};
use crate::widgets::draw::{fill, frame, put, width};
use crate::widgets::list::{self, Row};
use crate::widgets::menu::{self, Item, MenuState};

pub const KEYS: &[(&str, &str)] = &[
    ("←→", "menus / submenus"),
    ("Enter ↓", "open a menu"),
    ("↑↓", "items / files"),
    ("Enter", "run / file menu"),
    ("Shift+F10", "file context menu"),
    ("o r d s e", "open/rename/dup/star/del"),
    ("Del", "delete file"),
    ("Esc", "close"),
];

const TITLES: [&str; 3] = ["Document", "Format", "Arrange"];
const CTX: [&str; 5] = ["Open", "Rename…", "Duplicate", "Star", "Delete…"];
const CTX_KEYS: [char; 5] = ['o', 'r', 'd', 's', 'e'];

/// What a demo menu item does: report it, or flip a checkable item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoAct {
    Report,
    Toggle(usize),
}

#[derive(Default)]
pub struct State {
    pub title: usize,
    pub menu: Option<MenuState>,
    pub file: usize,
    /// The open context menu's selection.
    pub ctx: Option<usize>,
    pub checks: [bool; 2],
    pub last: String,
    title_x: [u16; 3],
    title_y: u16,
    file_y: u16,
    file_x: u16,
}

pub fn demo_menus(checks: [bool; 2]) -> Vec<(&'static str, Vec<Item<DemoAct>>)> {
    use DemoAct::*;
    let r = |l: &str, k: &str| Item::act(l, k, Report);
    vec![
        (
            TITLES[0],
            vec![
                r("New", "^N"),
                r("Open…", "^O"),
                r("Save", "^S"),
                Item::sep(),
                Item::sub(
                    "Export",
                    vec![r("PDF", ""), r("HTML", ""), r("Markdown", "")],
                ),
                Item::sep(),
                r("Print…", "^P").disabled(),
                r("Close", "^W"),
            ],
        ),
        (
            TITLES[1],
            vec![
                Item::sub(
                    "Text",
                    vec![
                        r("Bold", "^B"),
                        r("Italic", "^I"),
                        r("Underline", "^U"),
                        r("Strikethrough", ""),
                    ],
                ),
                Item::sub(
                    "Paragraph",
                    vec![
                        r("Align left", ""),
                        r("Center", ""),
                        r("Align right", ""),
                        r("Justify", ""),
                    ],
                ),
                Item::sep(),
                Item::act("Show ruler", "", Toggle(0)).checked(checks[0]),
                r("Clear formatting", "").disabled(),
            ],
        ),
        (
            TITLES[2],
            vec![
                r("Bring to front", ""),
                r("Send to back", ""),
                Item::sep(),
                r("Group", "^G"),
                r("Ungroup", "").disabled(),
                Item::sep(),
                Item::act("Snap to grid", "", Toggle(1)).checked(checks[1]),
            ],
        ),
    ]
}

fn files(app: &App) -> Vec<FileEntry> {
    app.shown_get("menus.files").unwrap_or_default()
}

fn picked(app: &mut App, act: DemoAct, labels: Vec<String>) {
    let s = &mut app.tabs.menus;
    s.menu = None;
    let mut path = labels.join(" ▸ ");
    if let DemoAct::Toggle(i) = act {
        s.checks[i] = !s.checks[i];
        path.push_str(if s.checks[i] { " ✓" } else { " (off)" });
    }
    s.last = path.clone();
    app.info(format!("Picked {path}"));
}

/// Unique "Name copy.ext" for Duplicate.
pub fn copy_name(name: &str, taken: &[FileEntry]) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut n = 1;
    loop {
        let cand = if n == 1 {
            format!("{stem} copy{ext}")
        } else {
            format!("{stem} copy {n}{ext}")
        };
        if !taken.iter().any(|f| f.name == cand) {
            return cand.chars().take(40).collect();
        }
        n += 1;
    }
}

fn run_ctx(app: &mut App, i: usize) {
    app.tabs.menus.ctx = None;
    let fs = files(app);
    let Some(f) = fs.get(app.tabs.menus.file).cloned() else {
        return;
    };
    let name = safe_text(&f.name);
    match i {
        0 => app.info(format!("Opened {name}")),
        1 => app.dialogs.push(Dialog::input(
            "Rename",
            &format!("New name for {name}:"),
            &f.name,
            DialogAct::RenameFile(f.name.clone()),
        )),
        2 => {
            if fs.len() >= MAX_FILES {
                app.set_status(format!("✗ at most {MAX_FILES} files"), Level::Err);
                return;
            }
            let mut fs = fs;
            let pos = app.tabs.menus.file;
            let copy = FileEntry {
                name: copy_name(&f.name, &fs),
                starred: false,
            };
            fs.insert(pos + 1, copy);
            if app.commit("menus.files", json!(fs)) {
                app.tabs.menus.file = pos + 1;
            }
        }
        3 => {
            let mut fs = fs;
            let pos = app.tabs.menus.file;
            fs[pos].starred = !fs[pos].starred;
            app.commit("menus.files", json!(fs));
        }
        _ => app.dialogs.push(Dialog::confirm(
            "Delete file",
            &format!("Delete {name}?"),
            "Delete",
            DialogAct::DeleteFile(f.name.clone()),
        )),
    }
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let s = &mut app.tabs.menus;
    if let Some(st) = s.menu.as_mut() {
        let menus = demo_menus(s.checks);
        match menu::key(&menus, st, &k) {
            menu::Out::Close => s.menu = None,
            menu::Out::Picked(a, labels) => picked(app, a, labels),
            menu::Out::Disabled(l) => app.info(format!("{l} is disabled")),
            menu::Out::None => s.title = st.open,
        }
        return true;
    }
    if let Some(sel) = s.ctx.as_mut() {
        match k.code {
            KeyCode::Up => *sel = (*sel + CTX.len() - 1) % CTX.len(),
            KeyCode::Down => *sel = (*sel + 1) % CTX.len(),
            KeyCode::Esc => s.ctx = None,
            KeyCode::Enter | KeyCode::Char(' ') => {
                let i = *sel;
                run_ctx(app, i);
            }
            KeyCode::Char(c) => {
                // accelerators: o open, r rename, d duplicate, s star, e delete
                if let Some(i) = CTX_KEYS.iter().position(|k| *k == c.to_ascii_lowercase()) {
                    run_ctx(app, i);
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                s.ctx = None;
                return false;
            }
            _ => {}
        }
        return true;
    }
    let n = files(app).len();
    let s = &mut app.tabs.menus;
    match (app.ctrl, k.code) {
        (0, KeyCode::Left) => s.title = (s.title + 2) % 3,
        (0, KeyCode::Right) => s.title = (s.title + 1) % 3,
        (0, KeyCode::Enter | KeyCode::Down | KeyCode::Char(' ')) => {
            let menus = demo_menus(s.checks);
            s.menu = Some(MenuState::new(&menus, s.title));
        }
        (1, KeyCode::Up) if s.file > 0 => s.file -= 1,
        (1, KeyCode::Up) => app.next_ctrl(true),
        (1, KeyCode::Down) => s.file = (s.file + 1).min(n.saturating_sub(1)),
        (1, KeyCode::Home) => s.file = 0,
        (1, KeyCode::End) => s.file = n.saturating_sub(1),
        (1, KeyCode::Enter | KeyCode::Menu) => s.ctx = Some(0),
        (1, KeyCode::Delete) => run_ctx(app, 4),
        (1, KeyCode::F(10)) if k.modifiers.contains(KeyModifiers::SHIFT) => s.ctx = Some(0),
        _ => return false,
    }
    true
}

pub fn click(app: &mut App, hit: &Hit, right: bool) -> bool {
    match hit {
        Hit::DemoTitle(i) => {
            app.focus_ctrl(0);
            let s = &mut app.tabs.menus;
            let same = s.menu.as_ref().is_some_and(|m| m.open == *i);
            s.title = *i;
            s.menu = if same {
                None
            } else {
                Some(MenuState::new(&demo_menus(s.checks), *i))
            };
            true
        }
        Hit::DemoItem(path) => {
            let s = &mut app.tabs.menus;
            let menus = demo_menus(s.checks);
            if let Some(st) = s.menu.as_mut() {
                st.path = path.clone();
                match menu::activate(&menus, st) {
                    menu::Out::Picked(a, labels) => picked(app, a, labels),
                    menu::Out::Disabled(l) => app.info(format!("{l} is disabled")),
                    _ => {}
                }
            }
            true
        }
        Hit::Item(1, j) => {
            app.focus_ctrl(1);
            let s = &mut app.tabs.menus;
            s.menu = None;
            s.file = *j;
            s.ctx = if right { Some(0) } else { None };
            true
        }
        Hit::CtxItem(i) => {
            run_ctx(app, *i);
            true
        }
        _ => {
            // a click elsewhere closes the open menus
            let s = &mut app.tabs.menus;
            let open = s.menu.is_some() || s.ctx.is_some();
            s.menu = None;
            s.ctx = None;
            open
        }
    }
}

pub fn hints(app: &App) -> Vec<Hint> {
    let s = &app.tabs.menus;
    if s.menu.is_some() {
        return vec![
            h("←→", "Menu/Sub", KeyCode::Right),
            h("↑↓", "Item", KeyCode::Down),
            h("Enter", "Pick", KeyCode::Enter),
            h("Esc", "Close", KeyCode::Esc),
        ];
    }
    if s.ctx.is_some() {
        return vec![
            h("↑↓", "Item", KeyCode::Down),
            h("Enter", "Run", KeyCode::Enter),
            h("Esc", "Close", KeyCode::Esc),
        ];
    }
    if app.ctrl == 0 {
        vec![
            h("←→", "Menu", KeyCode::Right),
            h("Enter", "Open", KeyCode::Enter),
        ]
    } else {
        vec![
            h("↑↓", "File", KeyCode::Down),
            h("Enter", "File menu", KeyCode::Enter),
        ]
    }
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let x = area.x + 1;
    let f0 = is_focused(app, 0);
    // (a) demo menu bar
    put(
        buf,
        x,
        area.y,
        "Demo menu bar",
        Style::new()
            .fg(if f0 { t.accent } else { t.fg })
            .bg(t.bg)
            .add_modifier(Modifier::BOLD),
        20,
    );
    let by = area.y + 1;
    let bar = Rect::new(x, by, 40.min(area.width - 2), 1);
    let mb = Style::new().bg(t.header_bg).fg(t.fg);
    fill(buf, bar, mb);
    let mut tx = x + 1;
    let s = &mut app.tabs.menus;
    s.title_y = by;
    for (i, title) in TITLES.iter().enumerate() {
        let open = s.menu.as_ref().is_some_and(|m| m.open == i);
        let st = if open || (f0 && s.title == i) {
            Style::new().bg(t.accent).fg(t.bg)
        } else {
            mb
        };
        let w = width(title) as u16 + 2;
        fill(buf, Rect::new(tx, by, w, 1), st);
        put(
            buf,
            tx + 1,
            by,
            &title[..1],
            st.add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            1,
        );
        put(buf, tx + 2, by, &title[1..], st, w as usize);
        s.title_x[i] = tx;
        app.hits.push((Rect::new(tx, by, w, 1), Hit::DemoTitle(i)));
        tx += w + 1;
    }
    let s = &app.tabs.menus;
    let last = if s.last.is_empty() {
        "Pick something: Enter or ↓ opens the focused menu".to_string()
    } else {
        format!("Last picked: {}", s.last)
    };
    put(buf, x, by + 1, &last, muted(&t), area.width as usize - 2);
    // (b) the file list
    let f1 = is_focused(app, 1);
    let fy = by + 3;
    let fs = files(app);
    let h = (fs.len() as u16 + 2)
        .min(area.bottom().saturating_sub(fy))
        .max(3);
    let r = Rect::new(x, fy, 40.min(area.width - 2), h);
    let inner = frame(buf, r, "Files (menus.files)", &t, f1);
    app.key_rects.push(("menus.files", inner));
    let vis = inner.height as usize;
    let sel = app.tabs.menus.file.min(fs.len().saturating_sub(1));
    app.tabs.menus.file = sel;
    let off = if sel >= vis { sel + 1 - vis } else { 0 };
    for (row, (j, f)) in fs.iter().enumerate().skip(off).take(vis).enumerate() {
        let y = inner.y + row as u16;
        let cur = j == sel;
        let st = if cur && f1 {
            Style::new().bg(t.cur_bg).fg(t.fg)
        } else if cur {
            Style::new().bg(t.sel_bg).fg(t.fg)
        } else {
            t.panel()
        };
        fill(buf, Rect::new(inner.x, y, inner.width, 1), st);
        put(
            buf,
            inner.x + 1,
            y,
            if f.starred { "★" } else { " " },
            st.fg(t.warn),
            1,
        );
        put(
            buf,
            inner.x + 3,
            y,
            &safe_text(&f.name),
            st,
            inner.width as usize - 4,
        );
        app.hits
            .push((Rect::new(inner.x, y, inner.width, 1), Hit::Item(1, j)));
        if cur {
            app.tabs.menus.file_y = y;
            app.tabs.menus.file_x = inner.x + 3;
        }
    }
    put(
        buf,
        r.right() + 2,
        fy + 1,
        "Enter / Shift+F10 / right-click:",
        muted(&t),
        area.right().saturating_sub(r.right() + 2) as usize,
    );
    put(
        buf,
        r.right() + 2,
        fy + 2,
        "Open · Rename… · Duplicate · Star · Delete…",
        muted(&t),
        area.right().saturating_sub(r.right() + 2) as usize,
    );
}

pub fn popup(app: &mut App, buf: &mut Buffer, _area: Rect) {
    let t = app.t.clone();
    let s = &app.tabs.menus;
    if let Some(st) = &s.menu {
        let menus = demo_menus(s.checks);
        let x = s.title_x[st.open];
        let y = s.title_y + 1;
        for (r, path) in menu::render(buf, &menus, st, x, y, &t) {
            app.hits.push((r, Hit::DemoItem(path)));
        }
    }
    let s = &app.tabs.menus;
    if let Some(sel) = s.ctx {
        let starred = files(app).get(s.file).is_some_and(|f| f.starred);
        let rows: Vec<Row> = CTX
            .iter()
            .enumerate()
            .map(|(i, l)| Row::new(if i == 3 && starred { "Unstar" } else { l }))
            .collect();
        let r = Rect::new(s.file_x + 10, s.file_y + 1, 18, CTX.len() as u16 + 2);
        for (rect, i) in list::render(buf, r, "", &rows, Some(sel), &t) {
            app.hits.push((rect, Hit::CtxItem(i)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies() {
        let fs = vec![FileEntry {
            name: "a copy.txt".into(),
            starred: false,
        }];
        assert_eq!(copy_name("a.txt", &fs), "a copy 2.txt");
        assert_eq!(copy_name("Logo", &[]), "Logo copy");
    }
}
