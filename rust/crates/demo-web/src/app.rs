//! Application state (one `App`, provided as context), the action catalogue
//! shared by keys, menus, the palette and the hint bar, and the glue between
//! the WebSocket and demo-core's value store.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use demo_core::dataset::TABS;
use demo_core::{ClientMsg, Event, Values, safe_text};
use leptos::prelude::*;
use serde_json::Value;

use crate::{net, util};

/// (data-theme id, label): pt's four themes.
pub const THEMES: [(&str, &str); 4] = [
    ("midnight", "Midnight"),
    ("daylight", "Daylight"),
    ("solarized", "Solarized Dark"),
    ("gruvbox", "Gruvbox"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnState {
    Connecting,
    Online,
    Offline,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Ok,
    Info,
    Warn,
    Err,
}

impl Level {
    pub fn icon(self) -> &'static str {
        match self {
            Level::Ok => "✓",
            Level::Info => "ℹ",
            Level::Warn => "⚠",
            Level::Err => "✗",
        }
    }
    pub fn class(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Err => "err",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Toast {
    pub id: u64,
    pub level: Level,
    pub text: String,
}

/// What has keyboard focus; drives the hint bar. `Body` names the kind of
/// control focused ("text", "dropdown", "grid", …; see `help::hints`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ctx {
    Sidebar,
    Body(&'static str),
}

/// What a confirm dialog does on "yes".
#[derive(Clone, PartialEq, Debug)]
pub enum ConfirmAct {
    ResetTab(usize),
    ResetAll,
    /// The Dialogs tab's demo confirm: only reports the answer.
    Demo,
    DeleteFile(String),
    DeleteRow(u64),
    DeleteCard(u64),
    RevertNotes,
}

/// What a one-line input dialog does with its text.
#[derive(Clone, PartialEq, Debug)]
pub enum InputAct {
    RenameFile(String),
    NewCard(&'static str),
    RenameCard(u64),
    Broadcast,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Modal {
    Help,
    Palette,
    About,
    Confirm {
        title: String,
        body: String,
        ok: String,
        act: ConfirmAct,
    },
    Message {
        title: String,
        body: String,
    },
    Input {
        title: String,
        label: String,
        value: String,
        act: InputAct,
    },
    Profile,
    /// The file list's context menu at a viewport position.
    Context {
        x: f64,
        y: f64,
        file: String,
    },
}

impl Modal {
    /// The `data-popup` kind of this modal.
    pub fn kind(&self) -> &'static str {
        match self {
            Modal::Help => "help",
            Modal::Palette => "palette",
            Modal::Context { .. } => "context-menu",
            _ => "dialog",
        }
    }
}

// ---- actions -----------------------------------------------------------------------

/// Every app-level command: menus, the palette, hint clicks and global keys
/// all run these through [`run`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    GoTab(usize),
    NextTab,
    PrevTab,
    ResetTab,
    ResetAll,
    Leave,
    Theme(usize),
    ToggleKeys,
    ToggleValue,
    Help,
    About,
    Palette,
    MenuBar,
    Sidebar,
    /// The Menus tab's checkable demo item.
    DemoWrap,
}

impl Act {
    /// Label in menus.
    pub fn label(self) -> String {
        match self {
            Act::GoTab(i) => TABS[i].title.to_string(),
            Act::NextTab => "Next tab".into(),
            Act::PrevTab => "Previous tab".into(),
            Act::ResetTab => "Reset this tab".into(),
            Act::ResetAll => "Reset all tabs…".into(),
            Act::Leave => "Leave".into(),
            Act::Theme(i) => THEMES[i].1.into(),
            Act::ToggleKeys => "Keys panel".into(),
            Act::ToggleValue => "Value panel".into(),
            Act::Help => "Keyboard help".into(),
            Act::About => "About".into(),
            Act::Palette => "Command palette".into(),
            Act::MenuBar => "Open menu bar".into(),
            Act::Sidebar => "Back to the sidebar".into(),
            Act::DemoWrap => "Word wrap".into(),
        }
    }

    /// Label in the command palette (ui-spec §3); also what `palette.last` records.
    pub fn palette_label(self) -> String {
        match self {
            Act::GoTab(i) => format!("Go to: {}", TABS[i].title),
            Act::ResetAll => "Reset all tabs".into(),
            Act::Theme(i) => format!("Theme: {}", THEMES[i].1),
            Act::Help => "Help".into(),
            Act::ToggleKeys => "Toggle Keys panel".into(),
            Act::ToggleValue => "Toggle Value panel".into(),
            a => a.label(),
        }
    }

    pub fn key(self) -> String {
        match self {
            Act::GoTab(i) if i < 9 => format!("Alt+{}", i + 1),
            Act::GoTab(9) => "Alt+0".into(),
            Act::NextTab => "Alt+↓".into(),
            Act::PrevTab => "Alt+↑".into(),
            Act::ResetTab => "^R".into(),
            Act::Help => "F1".into(),
            Act::Palette => "^K".into(),
            Act::MenuBar => "F10".into(),
            Act::Sidebar => "Esc".into(),
            _ => String::new(),
        }
    }
}

// ---- state -------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct App {
    /// Our display name once joined; `None` shows the join page.
    pub name: RwSignal<Option<String>>,
    pub join_error: RwSignal<Option<String>>,
    pub vals: RwSignal<Values>,
    pub conn: RwSignal<ConnState>,
    pub tab: RwSignal<usize>,
    pub ctx: RwSignal<Ctx>,
    /// An open inline popup (dropdown list, combobox list, tag suggestions, demo menu).
    pub popup: RwSignal<Option<&'static str>>,
    pub modal: RwSignal<Option<Modal>>,
    /// The app menu bar: open menu and the selected item at each level.
    pub menu: RwSignal<Option<usize>>,
    pub menu_path: RwSignal<Vec<usize>>,
    pub toasts: RwSignal<Vec<Toast>>,
    pub status: RwSignal<(Level, String)>,
    pub theme: RwSignal<usize>,
    pub show_keys: RwSignal<bool>,
    pub show_value: RwSignal<bool>,
    /// Keys changed by someone else a moment ago (flash the Value panel and control).
    pub flash: RwSignal<HashSet<String>>,
    /// (key, n): controls drop their draft of `key` when this changes (conflict, reset).
    pub revert: RwSignal<(String, u64)>,
    /// Seconds since the epoch, ticking, for "2m ago".
    pub now: RwSignal<i64>,
    pub demo_wrap: RwSignal<bool>,
    backoff: StoredValue<u32>,
    toast_id: StoredValue<u64>,
    revert_n: StoredValue<u64>,
    /// Keys with a `set` on the wire: (value sent, newer value queued behind it).
    inflight: StoredValue<HashMap<String, (Value, Option<Value>)>>,
}

pub fn use_app() -> App {
    expect_context::<App>()
}

impl App {
    pub fn new() -> App {
        let flag = |k: &str, def: bool| util::load(k).map_or(def, |v| v == "1");
        let theme = util::load("demo.theme")
            .and_then(|t| THEMES.iter().position(|(id, _)| *id == t))
            .unwrap_or(0);
        let app = App {
            name: RwSignal::new(None),
            join_error: RwSignal::new(None),
            vals: RwSignal::new(Values::new()),
            conn: RwSignal::new(ConnState::Connecting),
            tab: RwSignal::new(0),
            ctx: RwSignal::new(Ctx::Sidebar),
            popup: RwSignal::new(None),
            modal: RwSignal::new(None),
            menu: RwSignal::new(None),
            menu_path: RwSignal::new(vec![0]),
            toasts: RwSignal::new(vec![]),
            status: RwSignal::new((Level::Info, "Connecting…".into())),
            theme: RwSignal::new(theme),
            show_keys: RwSignal::new(flag("demo.keys", true)),
            show_value: RwSignal::new(flag("demo.value", true)),
            flash: RwSignal::new(HashSet::new()),
            revert: RwSignal::new((String::new(), 0)),
            now: RwSignal::new(util::now_secs()),
            demo_wrap: RwSignal::new(true),
            backoff: StoredValue::new(500),
            toast_id: StoredValue::new(0),
            revert_n: StoredValue::new(0),
            inflight: StoredValue::new(HashMap::new()),
        };
        Effect::new(move |_| util::save("demo.theme", THEMES[app.theme.get()].0));
        Effect::new(move |_| util::save("demo.keys", if app.show_keys.get() { "1" } else { "0" }));
        Effect::new(move |_| {
            util::save("demo.value", if app.show_value.get() { "1" } else { "0" })
        });
        // tell the server which tab we are on (presence dots in everyone's sidebar)
        Effect::new(move |_| {
            let t = TABS[app.tab.get()].id;
            net::send(&Values::focus(t));
        });
        app
    }

    pub fn tab_id(&self) -> &'static str {
        TABS[self.tab.get_untracked()].id
    }

    // ---- messages to the user ----

    pub fn toast(&self, level: Level, text: impl Into<String>) {
        let text = text.into();
        let id = self.toast_id.get_value() + 1;
        self.toast_id.set_value(id);
        self.toasts.update(|t| {
            t.push(Toast { id, level, text });
            while t.len() > 3 {
                t.remove(0);
            }
        });
        let toasts = self.toasts;
        set_timeout(
            move || _ = toasts.try_update(|t| t.retain(|x| x.id != id)),
            Duration::from_millis(4000),
        );
    }

    pub fn say(&self, level: Level, text: impl Into<String>) {
        let _ = self.status.try_set((level, text.into()));
    }

    fn flash_key(&self, key: &str) {
        let k = key.to_string();
        self.flash.update(|f| {
            f.insert(k.clone());
        });
        let flash = self.flash;
        set_timeout(
            move || {
                _ = flash.try_update(|f| {
                    f.remove(&k);
                })
            },
            Duration::from_millis(1200),
        );
    }

    fn revert_key(&self, key: &str) {
        let n = self.revert_n.get_value() + 1;
        self.revert_n.set_value(n);
        self.revert.set((key.to_string(), n));
    }

    // ---- reading values (tracked) ----

    /// A key's value, re-running only when that key changes.
    pub fn watch(self, key: &'static str) -> Memo<Value> {
        Memo::new(move |_| {
            self.vals
                .with(|v| v.value(key).cloned().unwrap_or(Value::Null))
        })
    }

    pub fn version(&self, key: &str) -> u64 {
        self.vals.with_untracked(|v| v.version(key))
    }

    /// Who last set `key` (empty for a default).
    pub fn by(&self, key: &str) -> String {
        self.vals
            .with(|v| v.entry(key).map(|e| e.by.clone()).unwrap_or_default())
    }

    // ---- writing values ----

    /// Validate and send a `set`. `base` overrides the version sent (the
    /// version shown when editing began); `None` uses the version we hold.
    /// `Err` carries the message to show; nothing was sent.
    pub fn commit(self, key: &str, value: Value, base: Option<u64>) -> Result<(), String> {
        let mut out = None;
        self.vals
            .update_untracked(|v| out = Some(v.set(key, value)));
        let res = match out {
            Some(Ok(mut msg)) => {
                if let (Some(b), ClientMsg::Set { base, .. }) = (base, &mut msg) {
                    *base = b;
                }
                if net::send(&msg) {
                    Ok(())
                } else {
                    Err("offline: change not saved".to_string())
                }
            }
            Some(Err(e)) => Err(e.to_string()),
            None => Err("not ready".into()),
        };
        if let Err(e) = &res {
            self.say(Level::Err, format!("{key}: {e}"));
        }
        res
    }

    /// `commit` with the version we hold, errors only shown in the status line.
    pub fn set(self, key: &str, value: Value) {
        let _ = self.commit(key, value, None);
    }

    /// For rapid changes (slider, charts, list moves): one `set` per key on
    /// the wire; a newer value waits for the ack and only the latest is sent,
    /// so we never conflict with our own previous change.
    pub fn set_latest(self, key: &str, value: Value) {
        if self.inflight.with_value(|m| m.contains_key(key)) {
            self.inflight.update_value(|m| {
                if let Some(e) = m.get_mut(key) {
                    e.1 = Some(value);
                }
            });
            return;
        }
        if self.commit(key, value.clone(), None).is_ok() {
            self.inflight.update_value(|m| {
                m.insert(key.to_string(), (value, None));
            });
        }
    }

    /// The value our next change should build on: the newest one we sent or
    /// queued, else the store's.
    pub fn latest(&self, key: &str) -> Value {
        self.inflight
            .with_value(|m| {
                m.get(key)
                    .map(|(s, q)| q.clone().unwrap_or_else(|| s.clone()))
            })
            .unwrap_or_else(|| {
                self.vals
                    .with_untracked(|v| v.value(key).cloned().unwrap_or(Value::Null))
            })
    }

    /// The ack for `key` arrived: send what was queued behind it.
    fn settle(self, key: &str) {
        let queued = self.inflight.try_update_value(|m| m.remove(key)).flatten();
        if let Some((_, Some(next))) = queued {
            self.set_latest(key, next);
        }
    }

    pub fn reset_keys(self, which: &[&str]) {
        let mut out = None;
        self.vals.update_untracked(|v| out = Some(v.reset(which)));
        if let Some(Ok(msg)) = out {
            if !net::send(&msg) {
                self.say(Level::Err, "offline: reset not sent");
            }
        }
    }

    pub fn broadcast(self, text: &str) -> Result<(), String> {
        let mut out = None;
        self.vals.update_untracked(|v| out = v.toast(text));
        match out {
            Some(msg) if net::send(&msg) => Ok(()),
            Some(_) => Err("offline: not sent".into()),
            None => Err("1–120 characters, no control characters".into()),
        }
    }

    // ---- navigation ----

    pub fn go_tab(self, i: usize) {
        if i >= TABS.len() {
            return;
        }
        let was_in_body = util::active_element().is_some_and(|e| util::within(&e, ".tabbody"));
        self.popup.set(None);
        self.tab.set(i);
        if was_in_body {
            focus_sidebar();
        }
    }

    pub fn open(self, m: Modal) {
        // a dialog opened from another one (context menu ▸ Rename…) keeps
        // the original return target
        let from_dialog = util::active_element().is_some_and(|e| util::within(&e, ".modal-back"));
        if self.modal.get_untracked().is_none() && !from_dialog {
            util::remember_focus();
        }
        self.menu.set(None);
        self.modal.set(Some(m));
    }

    pub fn close_modal(self) {
        let _ = self.modal.try_set(None);
        queue_microtask(move || {
            // another dialog may have opened meanwhile; leave its focus alone
            if self.modal.try_get_untracked().flatten().is_none() && !util::restore_focus() {
                focus_sidebar();
            }
        });
    }

    // ---- connection ----

    pub fn join(self, name: String) {
        util::save("demo.name", &name);
        self.join_error.set(None);
        self.vals.set(Values::new());
        self.name.set(Some(name));
        self.conn.set(ConnState::Connecting);
        self.say(Level::Info, "Connecting…");
        self.open_socket();
    }

    pub fn leave(self) {
        net::close();
        util::clear_query();
        self.modal.set(None);
        self.menu.set(None);
        self.popup.set(None);
        self.name.set(None);
        self.vals.set(Values::new());
        self.conn.set(ConnState::Connecting);
    }

    fn open_socket(self) {
        let Some(name) = self.name.get_untracked() else {
            return;
        };
        self.conn.set(ConnState::Connecting);
        net::connect(
            &name,
            || {},
            move |t| self.on_text(&t),
            move |_| self.on_disconnect(),
        );
    }

    fn on_disconnect(self) {
        if self.name.try_get_untracked().flatten().is_none() {
            return;
        }
        self.vals.update(|v| v.disconnected());
        self.inflight.update_value(|m| m.clear());
        self.conn.set(ConnState::Offline);
        let delay = self.backoff.get_value();
        self.backoff.set_value((delay * 2).min(5000));
        self.say(
            Level::Warn,
            format!(
                "Connection lost. Reconnecting in {:.1} s…",
                delay as f64 / 1000.0
            ),
        );
        set_timeout(
            move || {
                if self.name.try_get_untracked().flatten().is_some() {
                    self.open_socket();
                }
            },
            Duration::from_millis(delay as u64),
        );
    }

    fn on_text(self, text: &str) {
        let mut ev = Event::Ignored;
        self.vals.update(|v| ev = v.handle_text(text));
        match ev {
            Event::Welcome => {
                self.conn.set(ConnState::Online);
                self.backoff.set_value(500);
                net::send(&Values::focus(self.tab_id()));
                let (srv, me) = self.vals.with_untracked(|v| {
                    (
                        v.server()
                            .map(|s| format!("{} {}", s.implementation, s.version))
                            .unwrap_or_default(),
                        v.me().map(|m| m.name.clone()).unwrap_or_default(),
                    )
                });
                self.say(
                    Level::Ok,
                    format!("Connected to {srv} as {}", safe_text(&me)),
                );
            }
            Event::Changed {
                key,
                by,
                mine: false,
                ..
            } => {
                self.say(Level::Info, format!("{} changed {key}", safe_text(&by)));
                self.flash_key(&key);
            }
            Event::Saved { key, .. } => {
                self.say(Level::Ok, format!("Saved {key}"));
                self.settle(&key);
            }
            Event::Conflict { key, by } => {
                self.settle(&key);
                self.say(
                    Level::Warn,
                    format!("{} changed {key} first, showing theirs", safe_text(&by)),
                );
                self.flash_key(&key);
                self.revert_key(&key);
            }
            Event::ResetDone { keys } => {
                for k in &keys {
                    self.revert_key(k);
                }
                self.say(
                    Level::Ok,
                    format!("Reset {}", util::plural(keys.len(), "control", "controls")),
                );
            }
            Event::Rejected { key, error } => {
                if let Some(k) = &key {
                    self.settle(k);
                }
                let what = key.map(|k| format!("{k}: ")).unwrap_or_default();
                self.say(Level::Err, format!("{what}{error}"));
            }
            Event::Toast { text, by, .. } => self.toast(
                Level::Info,
                format!("{}: {}", safe_text(&by), safe_text(&text)),
            ),
            Event::ServerError { message } => self.say(Level::Err, message),
            _ => {}
        }
    }
}

// ---- focus helpers -------------------------------------------------------------------

/// The focus stops of the current tab body, in order.
pub fn stops() -> Vec<web_sys::Element> {
    util::query_all(".tabbody [data-control]")
}

/// Focus the sidebar now, or once mounted (right after joining).
pub fn focus_sidebar() {
    if util::query("#sidebar").is_some() {
        util::focus_sel("#sidebar");
    } else {
        util::focus_later("#sidebar");
    }
}

/// Focus the first control of the tab body.
pub fn focus_body() {
    if let Some(first) = stops().first() {
        util::focus_stop(first);
    }
}

// ---- running actions -----------------------------------------------------------------

pub fn run(app: App, act: Act) {
    app.menu.set(None);
    match act {
        Act::GoTab(i) => app.go_tab(i),
        Act::NextTab => app.go_tab((app.tab.get_untracked() + 1) % TABS.len()),
        Act::PrevTab => app.go_tab((app.tab.get_untracked() + TABS.len() - 1) % TABS.len()),
        Act::ResetTab => {
            let i = app.tab.get_untracked();
            let t = &TABS[i];
            app.open(Modal::Confirm {
                title: "Reset this tab?".into(),
                body: format!(
                    "Set {} ({}) back to its defaults, for everyone connected?",
                    t.title,
                    t.keys.join(", ")
                ),
                ok: "Reset".into(),
                act: ConfirmAct::ResetTab(i),
            });
        }
        Act::ResetAll => app.open(Modal::Confirm {
            title: "Reset all tabs?".into(),
            body: "Set every control on every tab back to its default, for everyone connected?"
                .into(),
            ok: "Reset all".into(),
            act: ConfirmAct::ResetAll,
        }),
        Act::Leave => app.leave(),
        Act::Theme(i) => app.theme.set(i),
        Act::ToggleKeys => app.show_keys.update(|v| *v = !*v),
        Act::ToggleValue => app.show_value.update(|v| *v = !*v),
        Act::Help => app.open(Modal::Help),
        Act::About => app.open(Modal::About),
        Act::Palette => app.open(Modal::Palette),
        Act::MenuBar => {
            app.menu_path.set(vec![0]);
            app.menu.set(Some(0));
        }
        Act::Sidebar => focus_sidebar(),
        Act::DemoWrap => app.demo_wrap.update(|v| *v = !*v),
    }
}

/// Run a confirmed action.
pub fn confirm(app: App, act: ConfirmAct) {
    match act {
        ConfirmAct::ResetTab(i) => app.reset_keys(TABS[i].keys),
        ConfirmAct::ResetAll => app.reset_keys(&[]),
        ConfirmAct::Demo => app.say(
            Level::Info,
            "Confirm: you chose Delete (nothing was deleted)",
        ),
        ConfirmAct::DeleteFile(name) => crate::tabs::menus::delete_file(app, &name),
        ConfirmAct::DeleteRow(id) => crate::tabs::grid::delete_row(app, id),
        ConfirmAct::DeleteCard(id) => crate::tabs::kanban::delete_card(app, id),
        ConfirmAct::RevertNotes => app.revert_key("textarea.notes"),
    }
}

/// Apply an input dialog's text; `Err` keeps the dialog open with the message.
pub fn input_done(app: App, act: InputAct, text: String) -> Result<(), String> {
    match act {
        InputAct::RenameFile(old) => crate::tabs::menus::rename_file(app, &old, &text),
        InputAct::NewCard(col) => crate::tabs::kanban::new_card(app, col, &text),
        InputAct::RenameCard(id) => crate::tabs::kanban::rename_card(app, id, &text),
        InputAct::Broadcast => app.broadcast(&text),
    }
}
