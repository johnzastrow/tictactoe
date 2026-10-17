//! One table drives key dispatch, the nano-style hint bar, menus and the help
//! screen (ui-spec §2), so they can never disagree.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Help,
    Menu,
    Palette,
    Filter,
    ClearFilter,
    SaveFilter,
    NextView,
    PrevView,
    View(u8),
    NextPane,
    PrevPane,
    Undo,
    Redo,
    New,
    NewProject,
    NewTask,
    EditRecord,
    Delete,
    ToggleDetail,
    Columns,
    SortColumn,
    SortDialog,
    GroupColumn,
    ClearGroup,
    Theme,
    Redraw,
    Quit,
    Logout,
    ContextMenu,
    Copy,
    Paste,
    FillDown,
    MarkAll,
    Mark,
    Clear,
    Edit,
    SetPassword,
    About,
    // board
    CardLeft,
    CardRight,
    Swimlanes,
    Scope,
    // timeline
    ZoomIn,
    ZoomOut,
    Today,
    TimeBack,
    TimeFwd,
    ShrinkDue,
    ExtendDue,
    BarLeft,
    BarRight,
    // dashboard
    Drill,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Help => "Help",
            Action::Menu => "Menu",
            Action::Palette => "Commands",
            Action::Filter => "Filter",
            Action::ClearFilter => "Clear filter",
            Action::SaveFilter => "Save filter…",
            Action::NextView => "Next view",
            Action::PrevView => "Previous view",
            Action::View(n) => VIEW_NAMES[(n as usize).min(8)],
            Action::NextPane => "Next pane",
            Action::PrevPane => "Previous pane",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::New => "New",
            Action::NewProject => "New project…",
            Action::NewTask => "New task…",
            Action::EditRecord => "Edit record",
            Action::Delete => "Delete",
            Action::ToggleDetail => "Detail pane",
            Action::Columns => "Columns…",
            Action::SortColumn => "Sort by column",
            Action::SortDialog => "Sort…",
            Action::GroupColumn => "Group by column",
            Action::ClearGroup => "Clear grouping",
            Action::Theme => "Theme",
            Action::Redraw => "Redraw",
            Action::Quit => "Quit",
            Action::Logout => "Log out",
            Action::ContextMenu => "Context menu",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::FillDown => "Fill down",
            Action::MarkAll => "Mark all",
            Action::Mark => "Mark row",
            Action::Clear => "Clear cell",
            Action::Edit => "Edit cell",
            Action::SetPassword => "Set password…",
            Action::About => "About",
            Action::CardLeft => "Move card ◂",
            Action::CardRight => "Move card ▸",
            Action::Swimlanes => "Swimlanes",
            Action::Scope => "Scope",
            Action::ZoomIn => "Zoom in",
            Action::ZoomOut => "Zoom out",
            Action::Today => "Today",
            Action::TimeBack => "Scroll ◂",
            Action::TimeFwd => "Scroll ▸",
            Action::ShrinkDue => "Due −",
            Action::ExtendDue => "Due +",
            Action::BarLeft => "Move ◂",
            Action::BarRight => "Move ▸",
            Action::Drill => "Drill down",
        }
    }
}

pub const VIEW_NAMES: [&str; 9] = ["Projects", "Tasks", "Board", "Timeline", "Dashboard", "Activity", "Templates", "Fields", "Users"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctx {
    Global,
    Grid,
    Board,
    Timeline,
    Dashboard,
}

#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

const fn k(code: KeyCode) -> Key {
    Key { code, mods: KeyModifiers::NONE }
}
const fn ctrl(c: char) -> Key {
    Key { code: KeyCode::Char(c), mods: KeyModifiers::CONTROL }
}
const fn alt(c: char) -> Key {
    Key { code: KeyCode::Char(c), mods: KeyModifiers::ALT }
}
const fn shift(code: KeyCode) -> Key {
    Key { code, mods: KeyModifiers::SHIFT }
}
const fn ctrlk(code: KeyCode) -> Key {
    Key { code, mods: KeyModifiers::CONTROL }
}

pub struct Bind {
    pub keys: &'static [Key],
    pub action: Action,
    pub ctx: Ctx,
    /// Priority in the hint bar (0 = not shown).
    pub hint: u8,
}

const fn b(keys: &'static [Key], action: Action, ctx: Ctx, hint: u8) -> Bind {
    Bind { keys, action, ctx, hint }
}

use Action as A;
use Ctx::*;
use KeyCode as C;

pub static BINDINGS: &[Bind] = &[
    // grid
    b(&[k(C::F(2))], A::Edit, Grid, 3),
    b(&[k(C::Insert), ctrl('n')], A::New, Grid, 4),
    b(&[k(C::F(8)), ctrlk(C::Delete)], A::Delete, Grid, 6),
    b(&[ctrl('e')], A::EditRecord, Grid, 5),
    b(&[ctrl('o')], A::SortColumn, Grid, 9),
    b(&[ctrl('r')], A::GroupColumn, Grid, 10),
    b(&[ctrl('b')], A::ToggleDetail, Grid, 11),
    b(&[k(C::F(4))], A::Columns, Grid, 12),
    b(&[ctrl('c')], A::Copy, Grid, 0),
    b(&[ctrl('v')], A::Paste, Grid, 0),
    b(&[ctrl('d')], A::FillDown, Grid, 13),
    b(&[ctrl('a')], A::MarkAll, Grid, 0),
    b(&[k(C::Char(' '))], A::Mark, Grid, 0),
    b(&[k(C::Delete)], A::Clear, Grid, 0),
    // board
    b(&[shift(C::Left), k(C::Char('<')), k(C::Char(','))], A::CardLeft, Board, 3),
    b(&[shift(C::Right), k(C::Char('>')), k(C::Char('.'))], A::CardRight, Board, 4),
    b(&[k(C::Enter), ctrl('e')], A::EditRecord, Board, 5),
    b(&[k(C::Insert), ctrl('n')], A::New, Board, 6),
    b(&[ctrl('r')], A::Swimlanes, Board, 7),
    b(&[k(C::Char('s')), k(C::Char('S'))], A::Scope, Board, 8),
    b(&[k(C::F(8)), ctrlk(C::Delete)], A::Delete, Board, 0),
    // timeline
    b(&[k(C::Char('+')), k(C::Char('='))], A::ZoomIn, Timeline, 3),
    b(&[k(C::Char('-'))], A::ZoomOut, Timeline, 4),
    b(&[k(C::Char('t')), k(C::Char('T'))], A::Today, Timeline, 5),
    b(&[shift(C::Left)], A::BarLeft, Timeline, 6),
    b(&[shift(C::Right)], A::BarRight, Timeline, 7),
    b(&[k(C::Char('['))], A::ShrinkDue, Timeline, 8),
    b(&[k(C::Char(']'))], A::ExtendDue, Timeline, 9),
    b(&[k(C::Char('<')), k(C::Char(','))], A::TimeBack, Timeline, 10),
    b(&[k(C::Char('>')), k(C::Char('.'))], A::TimeFwd, Timeline, 11),
    b(&[ctrl('e')], A::EditRecord, Timeline, 12),
    // dashboard
    b(&[k(C::Enter)], A::Drill, Dashboard, 3),
    // global
    b(&[ctrl('k'), ctrl('p')], A::Palette, Global, 1),
    b(&[ctrl('f'), k(C::F(3))], A::Filter, Global, 2),
    b(&[ctrl('z')], A::Undo, Global, 7),
    b(&[ctrl('y')], A::Redo, Global, 0),
    b(&[k(C::F(1)), ctrl('g')], A::Help, Global, 20),
    b(&[k(C::F(10))], A::Menu, Global, 21),
    b(&[ctrl('q')], A::Quit, Global, 22),
    b(&[ctrlk(C::PageDown)], A::NextView, Global, 0),
    b(&[ctrlk(C::PageUp)], A::PrevView, Global, 0),
    b(&[k(C::F(6))], A::NextPane, Global, 0),
    b(&[shift(C::F(6))], A::PrevPane, Global, 0),
    b(&[ctrl('t')], A::Theme, Global, 0),
    b(&[ctrl('l')], A::Redraw, Global, 0),
    b(&[shift(C::F(10)), k(C::Menu)], A::ContextMenu, Global, 0),
    b(&[alt('1')], A::View(0), Global, 0),
    b(&[alt('2')], A::View(1), Global, 0),
    b(&[alt('3')], A::View(2), Global, 0),
    b(&[alt('4')], A::View(3), Global, 0),
    b(&[alt('5')], A::View(4), Global, 0),
    b(&[alt('6')], A::View(5), Global, 0),
    b(&[alt('7')], A::View(6), Global, 0),
    b(&[alt('8')], A::View(7), Global, 0),
    b(&[alt('9')], A::View(8), Global, 0),
];

fn norm(e: &KeyEvent) -> (KeyCode, KeyModifiers) {
    let mut mods = e.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    let code = match e.code {
        // terminals report ^letters as lowercase; shifted chars carry SHIFT already
        // ^⇧K is ^K: terminals differ in whether they report the shift
        KeyCode::Char(c) if mods.contains(KeyModifiers::CONTROL) => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::Char(c.to_ascii_lowercase())
        }
        KeyCode::Char(c) => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::Char(c)
        }
        KeyCode::BackTab => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        c => c,
    };
    (code, mods)
}

pub fn matches(key: &Key, e: &KeyEvent) -> bool {
    let (code, mods) = norm(e);
    let want = if let KeyCode::Char(_) = key.code { key.mods - KeyModifiers::SHIFT } else { key.mods };
    key.code == code && want == mods
}

/// Find the action for a key in a context (context bindings win over global).
pub fn lookup(ctx: Ctx, e: &KeyEvent) -> Option<Action> {
    BINDINGS
        .iter()
        .filter(|b| b.ctx == ctx)
        .chain(BINDINGS.iter().filter(|b| b.ctx == Global))
        .find(|b| b.keys.iter().any(|k| matches(k, e)))
        .map(|b| b.action)
}

pub fn key_label(k: &Key) -> String {
    let base = match k.code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Insert => "Ins".into(),
        KeyCode::Delete => "Del".into(),
        KeyCode::PageUp => "PgUp".into(),
        KeyCode::PageDown => "PgDn".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Menu => "Menu".into(),
        other => format!("{other:?}"),
    };
    let mut s = String::new();
    if k.mods.contains(KeyModifiers::CONTROL) {
        s.push('^');
    }
    if k.mods.contains(KeyModifiers::ALT) {
        s.push_str("Alt+");
    }
    if k.mods.contains(KeyModifiers::SHIFT) {
        s.push('⇧');
    }
    if let KeyCode::Char(c) = k.code {
        if k.mods.is_empty() && c != ' ' {
            return c.to_string();
        }
    }
    s + &base
}

/// The first key for an action (in a context), for menus and the palette.
pub fn key_for(action: Action, ctx: Ctx) -> Option<String> {
    BINDINGS
        .iter()
        .filter(|b| b.action == action && (b.ctx == ctx || b.ctx == Global))
        .chain(BINDINGS.iter().filter(|b| b.action == action))
        .next()
        .and_then(|b| b.keys.first())
        .map(key_label)
}

/// Hint-bar entries for a context, most important first.
pub fn hints(ctx: Ctx) -> Vec<(String, Action)> {
    let mut v: Vec<&Bind> = BINDINGS
        .iter()
        .filter(|b| b.hint > 0 && (b.ctx == ctx || b.ctx == Global))
        .collect();
    v.sort_by_key(|b| b.hint);
    v.into_iter().map(|b| (key_label(&b.keys[0]), b.action)).collect()
}

/// Static hint lists for editors and dialogs.
pub fn editor_hints(kind: crate::widgets::editor::EdKind) -> Vec<(&'static str, &'static str)> {
    use crate::widgets::editor::EdKind as E;
    let mut v = vec![("Enter", "Save ↓"), ("Tab", "Save →"), ("Esc", "Cancel")];
    match kind {
        E::Date => v.extend([("↑↓", "±week"), ("PgUp/Dn", "±month"), ("^←→", "±day"), ("fri +3d eom", "type")]),
        E::Select | E::Combo => v.extend([("↑↓", "Choose"), ("type", "Filter")]),
        E::Suggest => v.extend([("↓", "Suggestions"), ("type", "Free text")]),
        E::Chips { .. } => v = vec![("Enter", "Add"), ("Bksp", "Remove last"), ("Enter on empty", "Save"), ("Esc", "Cancel")],
        E::Number | E::Int => v.extend([("↑↓", "±1"), ("⇧↑↓", "±0.25")]),
        E::Multiline => v = vec![("^S", "Save"), ("Tab", "Save →"), ("Esc", "Cancel"), ("Enter", "New line")],
        E::Bool => v.extend([("Space", "Toggle")]),
        _ => v.extend([("^U", "Clear"), ("^←→", "Word")]),
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn dispatch() {
        assert_eq!(lookup(Ctx::Grid, &ev(KeyCode::Char('k'), KeyModifiers::CONTROL)), Some(Action::Palette));
        assert_eq!(lookup(Ctx::Grid, &ev(KeyCode::Char('K'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)), Some(Action::Palette));
        assert_eq!(lookup(Ctx::Grid, &ev(KeyCode::F(2), KeyModifiers::NONE)), Some(Action::Edit));
        assert_eq!(lookup(Ctx::Board, &ev(KeyCode::Left, KeyModifiers::SHIFT)), Some(Action::CardLeft));
        assert_eq!(lookup(Ctx::Board, &ev(KeyCode::Char('>'), KeyModifiers::SHIFT)), Some(Action::CardRight));
        assert_eq!(lookup(Ctx::Grid, &ev(KeyCode::Char('3'), KeyModifiers::ALT)), Some(Action::View(2)));
        assert_eq!(lookup(Ctx::Grid, &ev(KeyCode::Char('x'), KeyModifiers::NONE)), None);
        assert_eq!(lookup(Ctx::Timeline, &ev(KeyCode::Char('+'), KeyModifiers::SHIFT)), Some(Action::ZoomIn));
    }

    #[test]
    fn labels() {
        assert_eq!(key_for(Action::Palette, Ctx::Grid).unwrap(), "^K");
        assert_eq!(key_for(Action::New, Ctx::Grid).unwrap(), "Ins");
        assert_eq!(key_for(Action::PrevPane, Ctx::Grid).unwrap(), "⇧F6");
        let h = hints(Ctx::Grid);
        assert_eq!(h[0].1, Action::Palette);
        assert!(h.iter().any(|(_, a)| *a == Action::Quit));
    }
}
