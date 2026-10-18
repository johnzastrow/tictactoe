//! Screen layout (ui-spec §1): menu bar, sidebar, tab body with its Keys and
//! Value panels, status line, nano-style hint bar, toasts and overlays.
//! Drawing records hit rectangles in `app.hits` for the mouse.

use crossterm::event::{KeyCode, KeyModifiers};
use demo_core::dataset::{TABS, initials};
use demo_core::safe_text;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, Focus, Hint, Hit, Level, Phase, hint};
use crate::help::{self, HelpState};
use crate::net::ConnState;
use crate::palette::Cmd;
use crate::tabs;
use crate::theme::PALETTES;
use crate::widgets::draw::{centered, fill, frame, modal, put, restyle, shadow, width};
use crate::widgets::menu::{self, Item};

pub const MENUS: [(&str, char); 3] = [("File", 'f'), ("View", 'v'), ("Help", 'h')];
pub const SIDEBAR_W: u16 = 17;

pub fn menu_for_letter(c: char) -> Option<usize> {
    MENUS.iter().position(|(_, l)| *l == c.to_ascii_lowercase())
}

/// The menu bar's contents (ui-spec §3 Menus).
pub fn menus(app: &App) -> Vec<(&'static str, Vec<Item<Cmd>>)> {
    let a = |c: Cmd| Item::act(c.label(), c.key(), c);
    let mut view: Vec<Item<Cmd>> = (0..TABS.len())
        .map(|i| {
            Item::act(
                format!("{:>2} {}", i + 1, TABS[i].title),
                Cmd::GoTo(i).key(),
                Cmd::GoTo(i),
            )
        })
        .collect();
    view.push(Item::sep());
    view.push(a(Cmd::Next));
    view.push(a(Cmd::Prev));
    view.push(Item::sub(
        "Theme",
        PALETTES
            .iter()
            .enumerate()
            .map(|(i, p)| Item::act(p.name, "", Cmd::Theme(i)).checked(i == app.theme_idx))
            .collect(),
    ));
    view.push(Item::act("Toggle Keys panel", "", Cmd::ToggleKeys).checked(app.show_keys));
    view.push(Item::act("Toggle Value panel", "", Cmd::ToggleValue).checked(app.show_value));
    vec![
        (
            "File",
            vec![
                a(Cmd::ResetTab),
                Item::act("Reset all tabs…", "", Cmd::ResetAll),
                Item::sep(),
                a(Cmd::Quit),
            ],
        ),
        ("View", view),
        (
            "Help",
            vec![Item::act("Keyboard help", "F1", Cmd::Help), a(Cmd::About)],
        ),
    ]
}

pub fn draw(app: &mut App, f: &mut Frame) {
    let area = f.area();
    let t = app.t.clone();
    let buf = f.buffer_mut();
    fill(buf, area, t.base());
    app.hits.clear();
    app.key_rects.clear();
    if area.width < 80 || area.height < 24 {
        let msg = format!(
            "demo needs at least 80×24 — this window is {}×{}. Make the window bigger, or ^Q to quit.",
            area.width, area.height
        );
        for (i, l) in help::wrap(&msg, area.width.saturating_sub(2).max(10) as usize)
            .iter()
            .enumerate()
        {
            put(
                buf,
                area.x + 1,
                area.y + area.height / 2 + i as u16,
                l,
                Style::new().fg(t.warn).bg(t.bg),
                area.width.saturating_sub(2) as usize,
            );
        }
        return;
    }
    if app.phase == Phase::Join {
        draw_join(app, buf, area);
        return;
    }
    tabs::sync(app);
    if app.help.is_some() {
        let hl = 1;
        help::render(
            app,
            buf,
            Rect::new(area.x, area.y, area.width, area.height - hl),
        );
        let hints = HelpState::hints();
        draw_hints(
            app,
            buf,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
            &hints,
        );
        return;
    }
    let hints = current_hints(app);
    let hint_lines = if hints_width(&hints) > area.width as usize {
        2
    } else {
        1
    };
    let status_y = area.bottom() - 1 - hint_lines;
    draw_menubar(app, buf, area);
    let main = Rect::new(area.x, area.y + 1, area.width, status_y - area.y - 1);
    draw_sidebar(app, buf, Rect::new(main.x, main.y, SIDEBAR_W, main.height));
    draw_body(
        app,
        buf,
        Rect::new(
            main.x + SIDEBAR_W + 1,
            main.y,
            main.width - SIDEBAR_W - 2,
            main.height,
        ),
    );
    draw_status(app, buf, Rect::new(area.x, status_y, area.width, 1));
    draw_hints(
        app,
        buf,
        Rect::new(area.x, status_y + 1, area.width, hint_lines),
        &hints,
    );
    // overlays
    if let Some(p) = app.palette.as_mut() {
        let r = centered(area, 64.min(area.width - 4), 20.min(area.height - 4));
        let r = Rect::new(r.x, area.y + 2, r.width, r.height);
        let inner = modal(buf, r, "Commands (^K)", &t);
        let mut hits = vec![];
        p.render(buf, inner, &t, true, &mut hits);
        app.hits.extend(hits);
    }
    if let Some(st) = &app.menu {
        let ms = menus(app);
        let x = app
            .hits
            .iter()
            .find(|(_, h)| *h == Hit::MenuTitle(st.open))
            .map_or(0, |(r, _)| r.x);
        let hits = menu::render(buf, &ms, st, x, area.y + 1, &t);
        app.hits
            .extend(hits.into_iter().map(|(r, p)| (r, Hit::MenuItem(p))));
    }
    draw_toasts(app, buf, area);
    let mut ds = std::mem::take(&mut app.dialogs);
    if let Some(d) = ds.last_mut() {
        // only the top dialog takes clicks
        let mut hits = vec![];
        crate::dialogs::render(d, buf, area, &t, &mut hits);
        app.hits.extend(hits);
    }
    app.dialogs = ds;
}

fn draw_join(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let logo = [
        r"     _                     ",
        r"  __| | ___ _ __ ___   ___  ",
        r" / _` |/ _ \ '_ ` _ \ / _ \ ",
        r"| (_| |  __/ | | | | | (_) |",
        r" \__,_|\___|_| |_| |_|\___/ ",
    ];
    let r = centered(area, 64, 18);
    for (i, l) in logo.iter().enumerate() {
        put(
            buf,
            r.x + 2,
            r.y + i as u16,
            l,
            Style::new()
                .fg(t.accent)
                .bg(t.bg)
                .add_modifier(Modifier::BOLD),
            40,
        );
    }
    put(
        buf,
        r.x + 2,
        r.y + 5,
        "a control gallery — every control on its own tab, shared live",
        Style::new().fg(t.muted).bg(t.bg),
        62,
    );
    let box_r = Rect::new(r.x, r.y + 7, 64, 9);
    shadow(buf, box_r);
    let inner = frame(buf, box_r, "Join", &t, true);
    let fields: [(&str, usize); 2] = [("Server URL", 0), ("Name", 1)];
    for (label, i) in fields {
        let y = inner.y + 1 + i as u16 * 2;
        let focused = app.join.field == i;
        put(
            buf,
            inner.x + 2,
            y,
            label,
            Style::new()
                .fg(if focused { t.accent } else { t.fg })
                .bg(t.panel)
                .add_modifier(Modifier::BOLD),
            12,
        );
        let ir = Rect::new(inner.x + 14, y, inner.width - 16, 1);
        let input = if i == 0 {
            &mut app.join.server
        } else {
            &mut app.join.name
        };
        input.render(
            buf,
            ir,
            Style::new()
                .bg(if focused { t.sel_bg } else { t.bg })
                .fg(t.fg),
            focused,
        );
        app.hits.push((ir, Hit::JoinField(i)));
    }
    let y = inner.y + 5;
    put(
        buf,
        inner.right() - 10,
        y,
        "[ Join ]",
        Style::new()
            .bg(t.accent)
            .fg(t.bg)
            .add_modifier(Modifier::BOLD),
        8,
    );
    app.hits
        .push((Rect::new(inner.right() - 10, y, 8, 1), Hit::JoinButton));
    match &app.join.error {
        Some(e) => {
            put(
                buf,
                inner.x + 2,
                y + 1,
                &format!("✗ {}", safe_text(e)),
                Style::new().fg(t.err).bg(t.panel),
                inner.width as usize - 3,
            );
        }
        None => {
            put(
                buf,
                inner.x + 2,
                y,
                "Your name is a label others see, not an account.",
                Style::new().fg(t.muted).bg(t.panel),
                50,
            );
        }
    }
    let hints = vec![
        hint("Tab", "Next field", KeyCode::Tab, KeyModifiers::NONE),
        hint("Enter", "Join", KeyCode::Enter, KeyModifiers::NONE),
        hint("^S", "Join", KeyCode::Char('s'), KeyModifiers::CONTROL),
        hint("^T", "Theme", KeyCode::Char('t'), KeyModifiers::CONTROL),
        hint("^Q", "Quit", KeyCode::Char('q'), KeyModifiers::CONTROL),
    ];
    draw_hints(
        app,
        buf,
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
        &hints,
    );
}

fn draw_menubar(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let mb = Style::new().bg(t.header_bg).fg(t.fg);
    fill(buf, Rect::new(area.x, area.y, area.width, 1), mb);
    let mut x = area.x + 1;
    for (i, (name, _)) in MENUS.iter().enumerate() {
        let open = app.menu.as_ref().is_some_and(|m| m.open == i);
        let st = if open {
            Style::new().bg(t.accent).fg(t.bg)
        } else {
            mb
        };
        let r = Rect::new(x, area.y, width(name) as u16 + 2, 1);
        fill(buf, r, st);
        put(
            buf,
            x + 1,
            area.y,
            &name[..1],
            st.add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            1,
        );
        put(buf, x + 2, area.y, &name[1..], st, 10);
        app.hits.push((r, Hit::MenuTitle(i)));
        x += r.width + 1;
    }
    // right side: demo · name · ● impl version
    let (dot_c, state) = match app.conn_state {
        ConnState::Online if app.welcomed => (t.ok, String::new()),
        ConnState::Online | ConnState::Connecting => (t.warn, " connecting…".to_string()),
        ConnState::Reconnecting => (
            if (app.start.elapsed().as_millis() / 500) % 2 == 0 {
                t.warn
            } else {
                t.err
            },
            " reconnecting…".into(),
        ),
    };
    let dot_c = if app.conn.is_none() && app.welcomes == 0 && app.fixed_now.is_none() {
        t.err
    } else {
        dot_c
    };
    let srv = app
        .v
        .server()
        .map(|s| format!("{} {}", s.implementation, s.version))
        .unwrap_or_else(|| "offline".into());
    let left = format!("demo · {} · ", safe_text(&app.name));
    let right = format!("{srv}{state} ");
    let total = width(&left) + 2 + width(&right);
    let rx = area.right().saturating_sub(total as u16);
    let w = put(buf, rx, area.y, &left, mb.fg(t.muted), width(&left));
    put(buf, rx + w, area.y, "●", mb.fg(dot_c), 1);
    put(
        buf,
        rx + w + 2,
        area.y,
        &right,
        mb.fg(t.muted),
        width(&right),
    );
}

fn draw_sidebar(app: &mut App, buf: &mut Buffer, r: Rect) {
    let t = app.t.clone();
    let focused = app.focus == Focus::Sidebar && app.menu.is_none() && app.palette.is_none();
    fill(buf, r, Style::new().bg(t.panel));
    for y in r.y..r.bottom() {
        put(
            buf,
            r.right(),
            y,
            "│",
            Style::new().fg(t.border).bg(t.bg),
            1,
        );
    }
    for (i, tab) in TABS.iter().enumerate() {
        let y = r.y + i as u16;
        if y >= r.bottom() {
            break;
        }
        let cur = i == app.tab;
        let st = if cur && focused {
            Style::new()
                .bg(t.cur_bg)
                .fg(t.fg)
                .add_modifier(Modifier::BOLD)
        } else if cur {
            Style::new()
                .bg(t.sel_bg)
                .fg(t.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().bg(t.panel).fg(t.fg)
        };
        let row = Rect::new(r.x, y, r.width, 1);
        fill(buf, row, st);
        put(buf, r.x, y, if cur { "▸" } else { " " }, st.fg(t.accent), 1);
        put(
            buf,
            r.x + 1,
            y,
            &format!("{:>2}", tab.number),
            st.fg(if cur { st.fg.unwrap_or(t.fg) } else { t.muted }),
            2,
        );
        let w = put(buf, r.x + 4, y, tab.title, st, r.width as usize - 5);
        // a dot per other peer on this tab
        for (dx, p) in (r.x + 5 + w..r.right()).zip(app.v.others_on(tab.id)) {
            put(buf, dx, y, "●", st.fg(t.hex(&p.color, t.accent)), 1);
        }
        app.hits.push((row, Hit::Sidebar(i)));
    }
    // who is online, under the tab list
    let mut y = r.y + TABS.len() as u16 + 1;
    if y + 1 < r.bottom() {
        put(
            buf,
            r.x + 1,
            y,
            "Online",
            Style::new()
                .fg(t.muted)
                .bg(t.panel)
                .add_modifier(Modifier::BOLD),
            10,
        );
        y += 1;
        for p in app.v.peers() {
            if y >= r.bottom() {
                break;
            }
            let me = Some(p.conn) == app.v.my_conn();
            let name = format!("{}{}", safe_text(&p.name), if me { " (you)" } else { "" });
            put(
                buf,
                r.x + 1,
                y,
                "●",
                Style::new().fg(t.hex(&p.color, t.accent)).bg(t.panel),
                1,
            );
            put(
                buf,
                r.x + 3,
                y,
                &name,
                Style::new().fg(t.fg).bg(t.panel),
                r.width as usize - 4,
            );
            y += 1;
        }
    }
}

/// Height of the Keys/Value panels for the current tab at body width `w`.
fn panel_layout(app: &App, w: u16) -> (u16, u16, u16) {
    let keys = tabs::keys_panel(app.tab);
    let (show_k, show_v) = (app.show_keys, app.show_value);
    let (kw, vw) = match (show_k, show_v) {
        (true, true) => {
            let kw = w * 11 / 20;
            (kw, w - kw - 1)
        }
        (true, false) => (w, 0),
        (false, true) => (0, w),
        _ => (0, 0),
    };
    let key_rows = if show_k { key_columns(keys, kw).0 } else { 0 };
    let nkeys = app.cur_tab().keys.len() as u16;
    let val_rows = if !show_v {
        0
    } else if nkeys <= 4 {
        nkeys * 2
    } else {
        nkeys
    };
    let h = key_rows.max(val_rows);
    (if h == 0 { 0 } else { (h + 2).min(10) }, kw, vw)
}

/// Flow the key entries into as many columns as fit a panel `w` wide:
/// (rows, and per column its key-column width and total width).
fn key_columns(keys: &[(&str, &str)], w: u16) -> (u16, Vec<(u16, u16)>) {
    let n = keys.len().max(1);
    for ncols in (1..=3usize).rev() {
        let rows = n.div_ceil(ncols);
        let cols: Vec<(u16, u16)> = keys
            .chunks(rows)
            .map(|c| {
                let kw = c.iter().map(|(k, _)| width(k)).max().unwrap_or(1) as u16 + 2;
                let lw = c.iter().map(|(_, l)| width(l)).max().unwrap_or(1) as u16;
                (kw, kw + lw + 2)
            })
            .collect();
        let total: u16 = cols.iter().map(|c| c.1).sum();
        if ncols == 1 || total <= w.saturating_sub(2) {
            return (rows as u16, cols);
        }
    }
    (n as u16, vec![])
}

fn draw_body(app: &mut App, buf: &mut Buffer, r: Rect) {
    let t = app.t.clone();
    let tab = app.cur_tab();
    let x = r.x + 1;
    put(
        buf,
        x,
        r.y,
        &format!("{} {}", tab.number, tab.heading),
        Style::new()
            .fg(t.accent)
            .bg(t.bg)
            .add_modifier(Modifier::BOLD),
        r.width as usize - 2,
    );
    let focus_note = if app.focus == Focus::Body {
        ""
    } else {
        "  ·  Enter / → / Tab to use it"
    };
    let w = put(
        buf,
        x,
        r.y + 1,
        tab.blurb,
        Style::new().fg(t.fg).bg(t.bg),
        r.width as usize - 2,
    );
    put(
        buf,
        x + w,
        r.y + 1,
        focus_note,
        Style::new()
            .fg(t.muted)
            .bg(t.bg)
            .add_modifier(Modifier::ITALIC),
        (r.width as usize).saturating_sub(w as usize + 2),
    );
    let (ph, kw, vw) = panel_layout(app, r.width - 1);
    let py = r.bottom().saturating_sub(ph);
    let ctrl_area = Rect::new(x, r.y + 3, r.width - 1, py.saturating_sub(r.y + 4));
    tabs::render(app, buf, ctrl_area);
    // flash controls whose value another peer just changed
    for (key, rect) in app.key_rects.clone() {
        if app.flashes.contains_key(key) {
            restyle(buf, rect, Style::new().bg(t.flash));
        }
    }
    if ph > 0 {
        if kw > 0 {
            draw_keys(app, buf, Rect::new(x, py, kw, ph));
        }
        if vw > 0 {
            draw_values(
                app,
                buf,
                Rect::new(x + if kw > 0 { kw + 1 } else { 0 }, py, vw, ph),
            );
        }
    }
    tabs::popup(app, buf, ctrl_area);
}

fn draw_keys(app: &App, buf: &mut Buffer, r: Rect) {
    let t = &app.t;
    let inner = frame(buf, r, "Keys", t, false);
    let keys = tabs::keys_panel(app.tab);
    let (rows, cols) = key_columns(keys, r.width);
    let rows = rows.max(1) as usize;
    for (i, (k, l)) in keys.iter().enumerate() {
        let (c, row) = (i / rows, i % rows);
        let Some(&(kcol, _)) = cols.get(c) else {
            continue;
        };
        let x = inner.x + 1 + cols[..c].iter().map(|c| c.1).sum::<u16>();
        let y = inner.y + row as u16;
        if y >= inner.bottom() || x >= inner.right() {
            continue;
        }
        let room = inner.right().saturating_sub(x) as usize;
        put(
            buf,
            x,
            y,
            k,
            Style::new()
                .fg(t.warn)
                .bg(t.panel)
                .add_modifier(Modifier::BOLD),
            room,
        );
        put(
            buf,
            x + kcol,
            y,
            l,
            Style::new().fg(t.fg).bg(t.panel),
            room.saturating_sub(kcol as usize),
        );
    }
}

fn draw_values(app: &App, buf: &mut Buffer, r: Rect) {
    let t = &app.t;
    let inner = frame(buf, r, "Value (shared)", t, false);
    let keys = app.cur_tab().keys;
    let two = keys.len() <= 4;
    let now = app.now();
    let mut y = inner.y;
    for key in keys {
        if y >= inner.bottom() {
            break;
        }
        let flash = app.flashes.contains_key(*key);
        let bg = if flash { t.flash } else { t.panel };
        let val = safe_text(&app.v.value_text(key));
        let desc = app.v.describe(key, now);
        let w = inner.width as usize - 1;
        fill(
            buf,
            Rect::new(inner.x, y, inner.width, if two { 2 } else { 1 }).intersection(inner),
            Style::new().bg(bg),
        );
        let kw = put(
            buf,
            inner.x + 1,
            y,
            key,
            Style::new().fg(t.accent).bg(bg),
            w,
        );
        let rest = format!(" = {val}");
        if two {
            put(
                buf,
                inner.x + 1 + kw,
                y,
                &rest,
                Style::new().fg(t.fg).bg(bg),
                w.saturating_sub(kw as usize),
            );
            y += 1;
            if y < inner.bottom() {
                put(
                    buf,
                    inner.x + 3,
                    y,
                    &desc,
                    Style::new().fg(t.muted).bg(bg),
                    w - 2,
                );
            }
        } else {
            // one line: value left, version info right
            let dw = width(&desc).min(w / 2);
            put(
                buf,
                inner.x + 1 + kw,
                y,
                &rest,
                Style::new().fg(t.fg).bg(bg),
                w.saturating_sub(kw as usize + dw + 2),
            );
            put(
                buf,
                inner.right().saturating_sub(dw as u16 + 1),
                y,
                &desc,
                Style::new().fg(t.muted).bg(bg),
                dw,
            );
        }
        y += 1;
    }
}

fn draw_status(app: &mut App, buf: &mut Buffer, r: Rect) {
    let t = app.t.clone();
    let st = Style::new().bg(t.header_bg).fg(t.fg);
    fill(buf, r, st);
    let mut parts: Vec<(String, Style)> = vec![];
    for p in app.v.peers() {
        let c = t.hex(&p.color, t.accent);
        parts.push((
            format!(" {} ", initials(&safe_text(&p.name))),
            Style::new()
                .bg(c)
                .fg(ratatui::style::Color::Black)
                .add_modifier(Modifier::BOLD),
        ));
        parts.push((" ".into(), st));
    }
    parts.push((format!(" online: {} ", app.v.peers().len()), st.fg(t.muted)));
    if !app.inflight.is_empty() {
        parts.push((format!("⟳ {} ", app.inflight.len()), st.fg(t.warn)));
    }
    let rw: usize = parts.iter().map(|(s, _)| width(s)).sum();
    let mut x = r.right().saturating_sub(rw as u16);
    for (s, sst) in &parts {
        x += put(buf, x, r.y, s, *sst, width(s));
    }
    let (msg, lvl) = match &app.status {
        Some((m, l, _)) => (m.clone(), *l),
        None => match app.conn_state {
            ConnState::Online if app.welcomed => (
                format!("{} · {}", app.cur_tab().title, app.cur_tab().blurb),
                Level::Info,
            ),
            ConnState::Reconnecting => (
                format!("⚠ Can't reach {} — retrying…", app.url.base()),
                Level::Warn,
            ),
            _ => (format!("Connecting to {}…", app.url.base()), Level::Info),
        },
    };
    let color = match lvl {
        Level::Ok => t.ok,
        Level::Warn => t.warn,
        Level::Err => t.err,
        Level::Info => t.fg,
    };
    put(
        buf,
        r.x + 1,
        r.y,
        &msg,
        st.fg(color),
        (r.width as usize).saturating_sub(rw + 3),
    );
}

/// Hints for whatever has focus (ui-spec §1 hint bar).
pub fn current_hints(app: &App) -> Vec<Hint> {
    let n = KeyModifiers::NONE;
    if let Some(d) = app.dialogs.last() {
        return d.hints();
    }
    if app.menu.is_some() {
        return vec![
            hint("←→", "Menus", KeyCode::Right, n),
            hint("↑↓", "Items", KeyCode::Down, n),
            hint("Enter", "Run", KeyCode::Enter, n),
            hint("letter", "Jump", KeyCode::Null, n),
            hint("Esc", "Close", KeyCode::Esc, n),
        ];
    }
    if app.palette.is_some() {
        return vec![
            hint("type", "Filter", KeyCode::Null, n),
            hint("↑↓", "Move", KeyCode::Down, n),
            hint("Enter", "Run", KeyCode::Enter, n),
            hint("Esc", "Close", KeyCode::Esc, n),
        ];
    }
    let mut v = match app.focus {
        Focus::Sidebar => vec![
            hint("↑↓", "Tab", KeyCode::Down, n),
            hint("Enter", "Open", KeyCode::Enter, n),
        ],
        Focus::Body => tabs::hints(app),
    };
    v.push(hint(
        "^K",
        "Commands",
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    ));
    v.push(hint("F1", "Help", KeyCode::F(1), n));
    v.push(hint("F10", "Menu", KeyCode::F(10), n));
    if app.focus == Focus::Sidebar {
        v.push(hint(
            "^R",
            "Reset tab",
            KeyCode::Char('r'),
            KeyModifiers::CONTROL,
        ));
        v.push(hint(
            "^T",
            "Theme",
            KeyCode::Char('t'),
            KeyModifiers::CONTROL,
        ));
    }
    v.push(hint(
        "^Q",
        "Quit",
        KeyCode::Char('q'),
        KeyModifiers::CONTROL,
    ));
    v
}

fn hints_width(h: &[Hint]) -> usize {
    h.iter().map(|e| width(&e.key) + width(&e.label) + 3).sum()
}

fn draw_hints(app: &mut App, buf: &mut Buffer, r: Rect, entries: &[Hint]) {
    let t = app.t.clone();
    fill(buf, r, Style::new().bg(t.bg));
    let (mut x, mut y) = (r.x, r.y);
    for e in entries {
        let kw = width(&e.key) as u16;
        let lw = width(&e.label) as u16;
        if x + kw + lw + 2 > r.right() {
            if y + 1 < r.bottom() {
                x = r.x;
                y += 1;
            } else {
                break;
            }
        }
        put(buf, x, y, &e.key, t.key(), kw as usize);
        put(
            buf,
            x + kw,
            y,
            &format!(" {}", e.label),
            Style::new().fg(t.fg).bg(t.bg),
            lw as usize + 1,
        );
        if e.ev.code != KeyCode::Null {
            app.hits
                .push((Rect::new(x, y, kw + lw + 1, 1), Hit::Hint(e.ev)));
        }
        x += kw + lw + 3;
    }
}

fn draw_toasts(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let mut y = area.y + 2;
    for toast in app.toasts.iter().rev() {
        let w = (width(&toast.text) as u16 + 4).min(60).min(area.width - 2);
        let per = w.saturating_sub(4).max(1) as usize;
        let lines = help::wrap(&toast.text, per);
        let r = Rect::new(
            area.right().saturating_sub(w + 2),
            y,
            w,
            lines.len().clamp(1, 3) as u16 + 2,
        );
        let color = match toast.level {
            Level::Ok => t.ok,
            Level::Warn => t.warn,
            Level::Err => t.err,
            Level::Info => t.info,
        };
        shadow(buf, r);
        fill(buf, r, Style::new().bg(t.panel));
        for yy in r.y..r.bottom() {
            put(buf, r.x, yy, "▌", Style::new().fg(color).bg(t.panel), 1);
        }
        for (i, l) in lines.iter().take(3).enumerate() {
            put(
                buf,
                r.x + 2,
                r.y + 1 + i as u16,
                l,
                Style::new().fg(t.fg).bg(t.panel),
                per,
            );
        }
        y += r.height + 1;
        if y > area.bottom().saturating_sub(6) {
            break;
        }
    }
}
