//! Tab 9 — Cascade: Program ▸ Project ▸ Phase, each list depending on the
//! one before. Changing a level clears the levels after it; each pick
//! commits the whole `cascade.pick` object.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use demo_core::dataset::{Choice, PROGRAMS, phases_of, projects_of};
use demo_core::keys::CascadePick;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::json;

use super::select::{closed, labels, open_list};
use super::{h, is_focused, label, muted};
use crate::app::{App, Hint, Hit};
use crate::widgets::draw::put;
use crate::widgets::list::{Drop, DropOut};

pub const KEYS: &[(&str, &str)] = &[
    ("Enter Space ↓", "open"),
    ("type", "filter / jump"),
    ("Enter", "pick & save"),
    ("Del", "clear this level"),
    ("Esc", "close"),
    ("Tab", "next level"),
];

const NAMES: [&str; 3] = ["Program", "Project", "Phase"];

#[derive(Default)]
pub struct State {
    pub drop: Drop,
    anchor: Rect,
}

fn pick_value(app: &App) -> CascadePick {
    app.shown_get("cascade.pick").unwrap_or_default()
}

fn ch(name: &'static str) -> Choice {
    Choice {
        id: name,
        label: name,
        color: "",
    }
}

/// The choices of a level for the current pick (empty when disabled).
pub fn choices(p: &CascadePick, level: usize) -> Vec<Choice> {
    match level {
        0 => PROGRAMS.iter().map(|p| ch(p.name)).collect(),
        1 => p
            .program
            .as_deref()
            .map(|pr| projects_of(pr).iter().map(|j| ch(j.name)).collect())
            .unwrap_or_default(),
        _ => match (&p.program, &p.project) {
            (Some(pr), Some(j)) => phases_of(pr, j).iter().map(|h| ch(h)).collect(),
            _ => vec![],
        },
    }
}

fn selected(p: &CascadePick, level: usize) -> Option<&str> {
    match level {
        0 => p.program.as_deref(),
        1 => p.project.as_deref(),
        _ => p.phase.as_deref(),
    }
}

/// Set one level (None clears it); later levels clear when it changes.
pub fn with_level(p: &CascadePick, level: usize, v: Option<&str>) -> CascadePick {
    let mut n = p.clone();
    let changed = selected(p, level) != v;
    let v = v.map(String::from);
    match level {
        0 => {
            n.program = v;
            if changed {
                n.project = None;
                n.phase = None;
            }
        }
        1 => {
            n.project = v;
            if changed {
                n.phase = None;
            }
        }
        _ => n.phase = v,
    }
    n
}

fn set(app: &mut App, level: usize, v: Option<&str>) {
    let n = with_level(&pick_value(app), level, v);
    app.commit("cascade.pick", json!(n));
}

fn open(app: &mut App, level: usize) {
    let p = pick_value(app);
    let list = choices(&p, level);
    if list.is_empty() {
        app.info(format!("Pick a {} first", NAMES[level - 1].to_lowercase()));
        return;
    }
    let cur = selected(&p, level).and_then(|s| list.iter().position(|c| c.id == s));
    app.tabs.cascade.drop.open_at(cur);
}

pub fn key(app: &mut App, k: KeyEvent) -> bool {
    let c = app.ctrl;
    let p = pick_value(app);
    let list = choices(&p, c);
    let ls = labels(&list);
    if app.tabs.cascade.drop.open {
        return match app.tabs.cascade.drop.key(&ls, &k) {
            DropOut::Pick(i) => {
                set(app, c, Some(list[i].id));
                true
            }
            DropOut::Used | DropOut::Closed => true,
            DropOut::Ignored => false,
        };
    }
    match k.code {
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Down => open(app, c),
        KeyCode::Delete | KeyCode::Backspace => {
            if selected(&p, c).is_some() {
                set(app, c, None);
            }
        }
        KeyCode::Char(ch)
            if !k
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            open(app, c);
            if app.tabs.cascade.drop.open {
                app.tabs
                    .cascade
                    .drop
                    .key(&ls, &KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            }
        }
        KeyCode::Up if c > 0 => app.next_ctrl(true),
        _ => return false,
    }
    true
}

pub fn click(app: &mut App, hit: &Hit) -> bool {
    match hit {
        Hit::Ctrl(c) => {
            let was = app.ctrl == *c && app.tabs.cascade.drop.open;
            app.focus_ctrl(*c);
            if was {
                app.tabs.cascade.drop.close();
            } else {
                open(app, *c);
            }
            true
        }
        Hit::Popup(c, j) => {
            let list = choices(&pick_value(app), *c);
            app.tabs.cascade.drop.close();
            if let Some(ch) = list.get(*j) {
                set(app, *c, Some(ch.id));
            }
            true
        }
        _ => false,
    }
}

pub fn hints(app: &App) -> Vec<Hint> {
    let mut v = super::select::drop_hints(app.tabs.cascade.drop.open);
    if !app.tabs.cascade.drop.open {
        v.push(h("Del", "Clear", KeyCode::Delete));
    }
    v
}

pub fn render(app: &mut App, buf: &mut Buffer, area: Rect) {
    let t = app.t.clone();
    let p = pick_value(app);
    let x = area.x + 1;
    let fx = x + 11;
    for level in 0..3 {
        let y = area.y + level as u16 * 2;
        let focused = is_focused(app, level);
        label(buf, x, y, NAMES[level], focused, &t, 10);
        let list = choices(&p, level);
        let enabled = !list.is_empty();
        let cur = selected(&p, level)
            .and_then(|s| list.iter().find(|c| c.id == s))
            .copied();
        let ph = if enabled {
            "choose…"
        } else {
            "pick above first"
        };
        closed(buf, fx, y, 30, cur.as_ref(), ph, focused, enabled, &t);
        app.hits.push((Rect::new(fx, y, 30, 1), Hit::Ctrl(level)));
        if focused {
            app.tabs.cascade.anchor = Rect::new(fx, y, 30, 1);
        }
        let n = list.len();
        let what = ["programs", "projects", "phases"][level];
        let count = if enabled {
            format!("{n} {what}")
        } else {
            "disabled".into()
        };
        put(buf, fx + 32, y, &count, muted(&t), 20);
    }
    app.key_rects
        .push(("cascade.pick", Rect::new(fx, area.y, 30, 5)));
    // breadcrumb
    let y = area.y + 7;
    let crumbs: Vec<String> = [&p.program, &p.project, &p.phase]
        .iter()
        .filter_map(|s| s.as_deref().map(demo_core::safe_text))
        .collect();
    put(buf, x + 1, y, "Path", muted(&t), 5);
    let text = if crumbs.is_empty() {
        "— nothing picked —".to_string()
    } else {
        crumbs.join(" ▸ ")
    };
    put(
        buf,
        fx,
        y,
        &text,
        Style::new()
            .fg(t.accent)
            .bg(t.bg)
            .add_modifier(Modifier::BOLD),
        area.right().saturating_sub(fx) as usize,
    );
}

pub fn popup(app: &mut App, buf: &mut Buffer, _area: Rect) {
    if app.tabs.cascade.drop.open {
        let c = app.ctrl;
        let list = choices(&pick_value(app), c);
        let a = app.tabs.cascade.anchor;
        open_list(app, buf, a, &list, c, NAMES[c]);
    }
}
