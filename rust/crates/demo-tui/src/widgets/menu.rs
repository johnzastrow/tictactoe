//! Drop-down menus with submenus, separators, disabled and checkable items.
//! Used by the app's menu bar (File/View/Help) and by tab 11's demo menu
//! bar. The caller owns the menu definitions; [`MenuState`] is the open
//! menu and the selected item at each level.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::draw::{fill, frame, put, shadow, width};
use crate::theme::Theme;

#[derive(Clone, Debug)]
pub enum Kind<A> {
    Act(A),
    Sub(Vec<Item<A>>),
    Sep,
}

#[derive(Clone, Debug)]
pub struct Item<A> {
    pub label: String,
    pub key: String,
    pub kind: Kind<A>,
    pub disabled: bool,
    /// `Some` for a checkable item (drawn with ✓ when true).
    pub checked: Option<bool>,
}

impl<A> Item<A> {
    pub fn act(label: impl Into<String>, key: impl Into<String>, a: A) -> Item<A> {
        Item {
            label: label.into(),
            key: key.into(),
            kind: Kind::Act(a),
            disabled: false,
            checked: None,
        }
    }
    pub fn sub(label: impl Into<String>, items: Vec<Item<A>>) -> Item<A> {
        Item {
            label: label.into(),
            key: String::new(),
            kind: Kind::Sub(items),
            disabled: false,
            checked: None,
        }
    }
    pub fn sep() -> Item<A> {
        Item {
            label: String::new(),
            key: String::new(),
            kind: Kind::Sep,
            disabled: false,
            checked: None,
        }
    }
    pub fn disabled(mut self) -> Item<A> {
        self.disabled = true;
        self
    }
    pub fn checked(mut self, on: bool) -> Item<A> {
        self.checked = Some(on);
        self
    }
    fn selectable(&self) -> bool {
        !matches!(self.kind, Kind::Sep)
    }
}

/// An open menu: which top-level menu, and the selected index per level
/// (`path[0]` in the drop-down, `path[1]` in its open submenu, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuState {
    pub open: usize,
    pub path: Vec<usize>,
}

impl MenuState {
    pub fn new<A>(menus: &[(&str, Vec<Item<A>>)], open: usize) -> MenuState {
        let first = menus.get(open).map_or(0, |m| first_selectable(&m.1));
        MenuState {
            open,
            path: vec![first],
        }
    }
}

pub enum Out<A> {
    None,
    Close,
    /// An action was picked; the labels along the way (`["Format", "Text", "Bold"]`).
    Picked(A, Vec<String>),
    /// Enter on a disabled item.
    Disabled(String),
}

fn first_selectable<A>(items: &[Item<A>]) -> usize {
    items.iter().position(Item::selectable).unwrap_or(0)
}

/// The item list at `depth` (0 = the drop-down) along the state's path.
fn level<'a, A>(menus: &'a [(&str, Vec<Item<A>>)], st: &MenuState, depth: usize) -> &'a [Item<A>] {
    let mut items: &[Item<A>] = &menus[st.open].1;
    for &i in &st.path[..depth] {
        match items.get(i).map(|it| &it.kind) {
            Some(Kind::Sub(s)) => items = s,
            _ => return &[],
        }
    }
    items
}

/// Handle a key in an open menu.
pub fn key<A: Clone>(menus: &[(&str, Vec<Item<A>>)], st: &mut MenuState, k: &KeyEvent) -> Out<A> {
    let depth = st.path.len() - 1;
    let items = level(menus, st, depth);
    let n = items.len();
    let step = |from: usize, down: bool| -> usize {
        let mut i = from;
        for _ in 0..n {
            i = if down { (i + 1) % n } else { (i + n - 1) % n };
            if items[i].selectable() {
                break;
            }
        }
        i
    };
    let cur = st.path[depth];
    match k.code {
        KeyCode::Esc => {
            if depth > 0 {
                st.path.pop();
            } else {
                return Out::Close;
            }
        }
        KeyCode::Up => st.path[depth] = step(cur, false),
        KeyCode::Down => st.path[depth] = step(cur, true),
        KeyCode::Home => st.path[depth] = first_selectable(items),
        KeyCode::Left => {
            if depth > 0 {
                st.path.pop();
            } else {
                *st = MenuState::new(menus, (st.open + menus.len() - 1) % menus.len());
            }
        }
        KeyCode::Right => match items.get(cur).map(|i| (&i.kind, i.disabled)) {
            Some((Kind::Sub(s), false)) => st.path.push(first_selectable(s)),
            _ => *st = MenuState::new(menus, (st.open + 1) % menus.len()),
        },
        KeyCode::Enter | KeyCode::Char(' ') => return activate(menus, st),
        KeyCode::Char(c) => {
            // the underlined first letter runs (or opens) the first match
            let c = c.to_ascii_lowercase();
            if let Some(i) = items
                .iter()
                .position(|it| it.selectable() && it.label.to_lowercase().starts_with(c))
            {
                st.path[depth] = i;
                return activate(menus, st);
            }
        }
        _ => {}
    }
    Out::None
}

/// Run (or open) the selected item of the deepest level.
pub fn activate<A: Clone>(menus: &[(&str, Vec<Item<A>>)], st: &mut MenuState) -> Out<A> {
    let depth = st.path.len() - 1;
    let items = level(menus, st, depth);
    let Some(it) = items.get(st.path[depth]) else {
        return Out::None;
    };
    if it.disabled {
        return Out::Disabled(it.label.clone());
    }
    match &it.kind {
        Kind::Sub(s) => {
            let f = first_selectable(s);
            st.path.push(f);
            Out::None
        }
        Kind::Act(a) => {
            let mut labels = vec![menus[st.open].0.to_string()];
            for d in 0..=depth {
                labels.push(level(menus, st, d)[st.path[d]].label.clone());
            }
            Out::Picked(a.clone(), labels)
        }
        Kind::Sep => Out::None,
    }
}

/// Draw the open drop-down (and submenus) under the title at `x`, top at
/// `y`. Returns the hit rectangle of every selectable row with its path.
pub fn render<A>(
    buf: &mut Buffer,
    menus: &[(&str, Vec<Item<A>>)],
    st: &MenuState,
    x: u16,
    y: u16,
    t: &Theme,
) -> Vec<(Rect, Vec<usize>)> {
    let mut hits = vec![];
    let area = buf.area;
    let (mut bx, mut by) = (x, y);
    for depth in 0..st.path.len() {
        let items = level(menus, st, depth);
        if items.is_empty() {
            break;
        }
        let w = items
            .iter()
            .map(|i| width(&i.label) + width(&i.key) + 8)
            .max()
            .unwrap_or(20)
            .max(18) as u16;
        let w = w.min(area.width);
        // fit on screen: scroll when taller than the space below
        let avail = area.bottom().saturating_sub(by).max(3);
        let h = (items.len() as u16 + 2).min(avail);
        let vis = h.saturating_sub(2) as usize;
        let sel = st.path[depth];
        let off = if sel >= vis { sel + 1 - vis } else { 0 };
        let bx2 = bx.min(area.right().saturating_sub(w + 2));
        let r = Rect::new(bx2, by, w, h);
        shadow(buf, r);
        let inner = frame(buf, r, "", t, true);
        for (row, (i, it)) in items.iter().enumerate().skip(off).take(vis).enumerate() {
            let yy = inner.y + row as u16;
            if !it.selectable() {
                put(
                    buf,
                    inner.x,
                    yy,
                    &"─".repeat(inner.width as usize),
                    Style::new().fg(t.border).bg(t.panel),
                    inner.width as usize,
                );
                continue;
            }
            let on_path = i == sel;
            let mut st_ = if on_path {
                Style::new().bg(t.cur_bg).fg(t.fg)
            } else {
                t.panel()
            };
            if it.disabled {
                st_ = st_.fg(t.muted);
            }
            fill(buf, Rect::new(inner.x, yy, inner.width, 1), st_);
            let mark = match it.checked {
                Some(true) => "✓",
                _ => " ",
            };
            put(buf, inner.x, yy, mark, st_.fg(t.ok), 1);
            put(
                buf,
                inner.x + 2,
                yy,
                &it.label,
                st_,
                inner.width as usize - 3,
            );
            if !it.disabled {
                if let Some(c) = it.label.chars().next() {
                    put(
                        buf,
                        inner.x + 2,
                        yy,
                        &c.to_string(),
                        st_.add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
                        1,
                    );
                }
            }
            let right = if matches!(it.kind, Kind::Sub(_)) {
                "▸".to_string()
            } else {
                it.key.clone()
            };
            put(
                buf,
                inner.right().saturating_sub(width(&right) as u16 + 1),
                yy,
                &right,
                st_.fg(t.muted),
                12,
            );
            let mut p = st.path[..depth].to_vec();
            p.push(i);
            hits.push((Rect::new(inner.x, yy, inner.width, 1), p));
        }
        // the next level opens to the right of the selected row
        bx = r.right();
        by = inner.y + sel.saturating_sub(off) as u16;
        if bx + 10 > area.right() {
            bx = r.x + 4;
            by += 1;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn navigate_and_pick() {
        let menus = vec![
            (
                "A",
                vec![
                    Item::act("One", "", 1),
                    Item::sep(),
                    Item::sub("More", vec![Item::act("Deep", "", 2)]),
                ],
            ),
            ("B", vec![Item::act("Off", "", 3).disabled()]),
        ];
        let mut st = MenuState::new(&menus, 0);
        assert!(matches!(key(&menus, &mut st, &k(KeyCode::Down)), Out::None));
        assert_eq!(st.path, vec![2]); // skipped the separator
        key(&menus, &mut st, &k(KeyCode::Right));
        assert_eq!(st.path, vec![2, 0]);
        let Out::Picked(a, labels) = key(&menus, &mut st, &k(KeyCode::Enter)) else {
            panic!()
        };
        assert_eq!(
            (a, labels),
            (2, vec!["A".to_string(), "More".into(), "Deep".into()])
        );
        key(&menus, &mut st, &k(KeyCode::Left));
        key(&menus, &mut st, &k(KeyCode::Right)); // opens the submenu again
        key(&menus, &mut st, &k(KeyCode::Esc));
        key(&menus, &mut st, &k(KeyCode::Up));
        key(&menus, &mut st, &k(KeyCode::Right)); // not a submenu: next menu
        assert_eq!(st.open, 1);
        assert!(matches!(
            key(&menus, &mut st, &k(KeyCode::Enter)),
            Out::Disabled(_)
        ));
        assert!(matches!(key(&menus, &mut st, &k(KeyCode::Esc)), Out::Close));
    }
}
