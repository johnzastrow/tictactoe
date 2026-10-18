//! Key lists: each tab's Keys panel, the searchable help (ui-spec §3) and the
//! nano-style hint bar for whatever has focus. Clicking a hint runs its
//! action or "presses" its key on the focused control.

use crate::app::{Act, App, Ctx};
use leptos::prelude::*;

pub type Keys = &'static [(&'static str, &'static str)];

/// The Keys panel of each tab, by tab id (ui-spec §4, web column).
pub fn tab_keys(id: &str) -> Keys {
    match id {
        "text" => &[
            ("←/→  Home/End", "move in the field"),
            ("^←/^→", "jump a word"),
            ("Backspace/Del", "delete"),
            ("^U", "clear the field"),
            ("Enter", "save"),
            ("Esc", "revert to the synced value"),
            ("^O  👁", "show / hide the password"),
            ("Tab  Shift+Tab", "next / previous field"),
        ],
        "number" => &[
            ("↑/↓", "±1"),
            ("Shift+↑/↓", "±0.25 (Hours)"),
            ("▲ ▼", "click to step"),
            ("Enter", "save the typed number"),
            ("Esc", "revert"),
            ("←/→", "slider ±1"),
            ("PgUp/PgDn", "slider ±10"),
            ("Home/End", "slider 0 / 100"),
            ("click / drag", "set the slider"),
        ],
        "date" => &[
            ("type", "fri  tom  +3d  -2w  eow  eom  23 oct  2026-11-02"),
            ("↑/↓", "calendar ±1 week"),
            ("^←/^→", "calendar ±1 day"),
            ("PgUp/PgDn", "calendar ±1 month"),
            ("Enter", "save the typed date, else the calendar day"),
            ("Today", "button: today"),
            ("Del", "clear (no date)"),
            ("Esc", "revert"),
        ],
        "select" => &[
            ("Enter  Space  ↓", "open the list"),
            ("↑/↓", "move"),
            ("type", "filter; the first match is selected"),
            ("Enter", "pick and save"),
            ("Esc", "close"),
            ("click", "open / pick"),
        ],
        "combo" => &[
            ("type", "fuzzy-search people"),
            ("↓  click", "open the list"),
            ("↑/↓", "move"),
            ("Enter", "pick and save"),
            ("Del", "clear to none"),
            ("Esc", "close the list / revert"),
        ],
        "tags" => &[
            ("type", "filter the suggestions"),
            ("↑/↓", "select a suggestion"),
            ("Enter", "add it, or the typed text as a new tag"),
            ("Backspace", "remove the last chip (empty input)"),
            ("←/→", "select a chip (empty input)"),
            ("Del", "remove the selected chip"),
            ("click ×", "remove a chip"),
        ],
        "textarea" => &[
            ("Enter", "new line"),
            ("arrows  Home/End  PgUp/PgDn", "move"),
            ("^Home/^End", "start / end"),
            ("^S", "save"),
            ("Esc", "revert (asks first if edited)"),
            ("Tab", "leave, keeping the draft (● unsaved)"),
        ],
        "choice" => &[
            ("Space", "toggle / select"),
            ("↑/↓  ←/→", "move within a group"),
            ("Tab", "next group"),
            ("click", "toggle / select"),
        ],
        "cascade" => &[
            ("Enter  Space  ↓", "open a list"),
            ("type", "filter"),
            ("Enter", "pick and save"),
            ("Del", "clear this level and below"),
            ("Tab", "next level"),
        ],
        "grid" => &[
            ("arrows  Tab  Home/End", "move"),
            ("Enter  F2  type", "edit the cell"),
            ("Enter", "save and move down"),
            ("Esc", "cancel the edit"),
            ("Del", "clear Due"),
            ("Ins", "add a row"),
            ("F8  ^Del", "delete the row"),
            ("^O", "sort this column ▲ ▼ off"),
            ("click  double-click", "select / edit"),
            ("click a header", "sort"),
        ],
        "menus" => &[
            ("Enter  ↓", "open the demo menu"),
            ("←/→", "between menus"),
            ("→", "open a submenu"),
            ("Enter", "pick"),
            ("↑/↓", "move in the file list"),
            ("Enter  Shift+F10  right-click", "file context menu"),
            ("Esc", "close"),
        ],
        "palette" => &[
            ("type", "filter the commands"),
            ("↑/↓  PgUp/PgDn", "move"),
            ("Enter", "run the command"),
            ("^K  ^P", "the same palette as a popup"),
        ],
        "dialogs" => &[
            ("Enter  Space", "open the focused button's popup"),
            ("←/→  Tab", "between buttons"),
            ("y / n", "confirm: Delete / Cancel"),
            ("Tab", "form: next field"),
            ("^S", "form: save"),
            ("Esc", "close"),
        ],
        "charts" => &[
            ("←/→", "select a month"),
            ("↑/↓", "±5"),
            ("PgUp/PgDn", "±20"),
            ("click a bar", "select it"),
            ("drag a bar", "set its value"),
        ],
        "kanban" => &[
            ("←/→", "column"),
            ("↑/↓", "card"),
            ("Shift+←/→  < >", "move the card"),
            ("Ins", "new card"),
            ("Enter", "rename"),
            ("F8  ^Del", "delete"),
            ("drag", "move a card to another column"),
        ],
        "gantt" => &[
            ("↑/↓", "select a bar"),
            ("Shift+←/→  < >", "move ±1 day"),
            ("[  ]", "shrink / extend 1 day"),
            ("+  -", "zoom: day ▸ week ▸ month"),
            ("t", "scroll to today"),
            ("←/→", "scroll the time axis"),
            ("drag  drag right edge", "move / resize"),
        ],
        _ => &[],
    }
}

pub const GETTING_AROUND: Keys = &[
    ("↑/↓", "sidebar: previous / next tab"),
    ("Home/End", "sidebar: first / last tab"),
    ("Enter  →  Tab", "sidebar: into the tab body"),
    (
        "Tab  Shift+Tab",
        "next / previous control (Shift+Tab from the first returns to the sidebar)",
    ),
    ("Esc", "close a popup; with none open, back to the sidebar"),
    ("F6", "sidebar ⇄ tab body"),
];

pub const GLOBAL: Keys = &[
    ("F1  ^G", "this help"),
    ("F10  Alt+F/V/H", "menu bar"),
    ("^K  ^P", "command palette"),
    ("Alt+↓  Alt+↑", "next / previous tab"),
    ("Alt+1…9  Alt+0", "go to tab 1–9, 10"),
    ("^R", "reset this tab to its defaults (asks first)"),
    ("View menu", "theme"),
    ("File ▸ Leave", "back to the join page"),
    ("Shift+F10  Menu key  right-click", "context menu"),
];

pub const HOW: &str = "demo is one server holding the current value of every control. Every client \
(TUI or browser, Rust or Go) sees every change live. Each key has a version: a change is sent with \
the version you last saw, and if someone changed it first the server refuses it as a conflict and \
you see their value. The Value panel shows who changed a key and when.";

// ---- hint bar --------------------------------------------------------------------------

/// What clicking a hint does.
#[derive(Clone, Copy)]
pub enum HA {
    None,
    Act(Act),
    /// Press this key (ctrl, shift) on the focused element.
    Key(&'static str, bool, bool),
}

pub type Hint = (&'static str, &'static str, HA);

const fn k(key: &'static str) -> HA {
    HA::Key(key, false, false)
}

/// The hint bar's lines for the current focus (tracked).
pub fn hints(app: App) -> (Vec<Hint>, Vec<Hint>) {
    use HA::Act as A;
    let tail: Vec<Hint> = vec![
        ("^K", "Commands", A(Act::Palette)),
        ("F1", "Help", A(Act::Help)),
        ("F10", "Menu", A(Act::MenuBar)),
    ];
    if let Some(m) = app.modal.get() {
        let v: Vec<Hint> = match m.kind() {
            "palette" => vec![
                ("↑↓", "Choose", HA::None),
                ("Enter", "Run", k("Enter")),
                ("Esc", "Close", k("Escape")),
            ],
            "help" => vec![
                ("type", "Search", HA::None),
                ("↑↓ PgUp/Dn", "Scroll", HA::None),
                ("Esc", "Close", k("Escape")),
            ],
            "context-menu" => {
                vec![
                    ("↑↓", "Choose", HA::None),
                    ("Enter", "Run", k("Enter")),
                    ("Esc", "Close", k("Escape")),
                ]
            }
            _ => vec![
                ("Tab", "Next", HA::None),
                ("Enter", "OK", k("Enter")),
                ("^S", "Save (form)", HA::Key("s", true, false)),
                ("Esc", "Cancel", k("Escape")),
            ],
        };
        return (v, vec![]);
    }
    if app.menu.get().is_some() {
        return (
            vec![
                ("←→", "Menus", HA::None),
                ("↑↓", "Items", HA::None),
                ("Enter", "Run", HA::None),
                ("Esc", "Close", HA::None),
            ],
            vec![],
        );
    }
    if let Some(p) = app.popup.get() {
        let v: Vec<Hint> = match p {
            "menu" => vec![
                ("←→", "Menus / submenu", HA::None),
                ("↑↓", "Items", k("ArrowDown")),
                ("Enter", "Pick", k("Enter")),
                ("Esc", "Close", k("Escape")),
            ],
            "tags" => vec![
                ("↑↓", "Choose", k("ArrowDown")),
                ("Enter", "Add", k("Enter")),
                ("type", "Filter", HA::None),
                ("Esc", "Close", k("Escape")),
            ],
            _ => vec![
                ("↑↓", "Choose", k("ArrowDown")),
                ("type", "Filter", HA::None),
                ("Enter", "Pick", k("Enter")),
                ("Esc", "Close", k("Escape")),
            ],
        };
        return (v, vec![]);
    }
    let body_tail = |mut v: Vec<Hint>| {
        v.push(("Tab", "Next", k("Tab")));
        v.push(("Esc", "Sidebar", A(Act::Sidebar)));
        v
    };
    let first: Vec<Hint> = match app.ctx.get() {
        Ctx::Sidebar => vec![
            ("↑↓", "Tab", HA::None),
            ("Enter", "Open", k("Enter")),
            ("Alt+↓↑", "Next tab", A(Act::NextTab)),
            ("^R", "Reset tab", A(Act::ResetTab)),
        ],
        Ctx::Body(c) => body_tail(match c {
            "text" => vec![
                ("Enter", "Save", k("Enter")),
                ("Esc", "Revert", HA::None),
                ("^U", "Clear", HA::Key("u", true, false)),
            ],
            "password" => vec![("^O", "Show / hide", HA::Key("o", true, false))],
            "spinner" => vec![
                ("↑↓", "±1", k("ArrowUp")),
                ("Shift+↑↓", "±0.25", HA::None),
                ("Enter", "Save", k("Enter")),
            ],
            "slider" => vec![
                ("←→", "±1", k("ArrowRight")),
                ("PgUp/Dn", "±10", k("PageUp")),
                ("Home/End", "0 / 100", HA::None),
            ],
            "date" => vec![
                ("Enter", "Save", k("Enter")),
                ("↑↓", "Week", k("ArrowDown")),
                ("^←→", "Day", HA::Key("ArrowRight", true, false)),
                ("PgUp/Dn", "Month", k("PageDown")),
                ("Del", "Clear", k("Delete")),
            ],
            "dropdown" => vec![
                ("Enter", "Open", k("Enter")),
                ("type", "Jump", HA::None),
                ("Del", "Clear", HA::None),
            ],
            "combo" => vec![
                ("type", "Search", HA::None),
                ("↓", "Open", k("ArrowDown")),
                ("Del", "None", k("Delete")),
            ],
            "tags" => vec![
                ("type", "Filter", HA::None),
                ("↓", "Suggest", k("ArrowDown")),
                ("Bksp", "Remove last", HA::None),
            ],
            "textarea" => vec![
                ("^S", "Save", HA::Key("s", true, false)),
                ("Esc", "Revert", HA::None),
            ],
            "check" | "toggle" => vec![("Space", "Toggle", k(" "))],
            "group" => vec![("↑↓", "Move", k("ArrowDown")), ("Space", "Toggle", k(" "))],
            "radio" => vec![("←→", "Select", k("ArrowRight"))],
            "grid" => vec![
                ("Enter", "Edit", k("Enter")),
                ("Ins", "Add row", k("Insert")),
                ("F8", "Delete row", k("F8")),
                ("^O", "Sort", HA::Key("o", true, false)),
                ("Del", "Clear due", k("Delete")),
            ],
            "cell" => vec![
                ("Enter", "Save ↓", k("Enter")),
                ("Esc", "Cancel", k("Escape")),
            ],
            "demo-menu" => vec![
                ("Enter ↓", "Open menu", k("Enter")),
                ("←→", "Menus", HA::None),
            ],
            "files" => vec![
                ("↑↓", "File", k("ArrowDown")),
                ("Enter", "Context menu", k("Enter")),
            ],
            "palette" => vec![
                ("type", "Filter", HA::None),
                ("↑↓", "Choose", k("ArrowDown")),
                ("Enter", "Run", k("Enter")),
            ],
            "button" => vec![
                ("Enter", "Open", k("Enter")),
                ("←→", "Buttons", k("ArrowRight")),
            ],
            "chart" => vec![
                ("←→", "Month", k("ArrowRight")),
                ("↑↓", "±5", k("ArrowUp")),
                ("PgUp/Dn", "±20", k("PageUp")),
            ],
            "kanban" => vec![
                ("←→↑↓", "Select", HA::None),
                ("Shift+←→", "Move card", HA::Key("ArrowRight", false, true)),
                ("Ins", "New", k("Insert")),
                ("Enter", "Rename", k("Enter")),
                ("F8", "Delete", k("F8")),
            ],
            "gantt" => vec![
                ("↑↓", "Bar", k("ArrowDown")),
                ("Shift+←→", "Move", HA::Key("ArrowRight", false, true)),
                ("[ ]", "Resize", k("]")),
                ("+ -", "Zoom", k("+")),
                ("t", "Today", k("t")),
            ],
            _ => vec![],
        }),
    };
    (first, tail)
}
