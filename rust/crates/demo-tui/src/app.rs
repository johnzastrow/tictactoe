//! Application state, the event loop, network handling and the global keys
//! (ui-spec §2–3). Each tab's control lives in `tabs/`, popups in
//! `dialogs.rs`, the palette in `palette.rs`, help in `help.rs`, the shell's
//! drawing in `ui.rs`.
//!
//! Values: the `demo_core::Values` store is the synced truth. Controls draw
//! [`App::shown`], which is the store overlaid with our own commits still in
//! flight, so rapid changes (slider, kanban moves) don't flicker or race:
//! while a `set` for a key awaits its ack, the next one is queued and sent
//! after it, based on the new version.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use demo_core::dataset::{TABS, Tab};
use demo_core::protocol::clean_name;
use demo_core::{ClientMsg, Date, Event as VEvent, ServerMsg, Values, keys};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::dialogs::{self, Dialog, DialogAct};
use crate::help::HelpState;
use crate::net::{self, ConnState, Connection, NetEvent, ServerUrl};
use crate::palette::{Cmd, PaletteState};
use crate::tabs::{self, Tabs};
use crate::theme::{PALETTES, Theme};
use crate::ui;
use crate::widgets::draw::rect_contains;
use crate::widgets::input::TextInput;
use crate::widgets::menu::{self, MenuState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Err,
}

pub struct Toast {
    pub text: String,
    pub level: Level,
    pub at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Join,
    Main,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Body,
}

/// Something clickable, recorded while drawing (last pushed = topmost).
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    MenuTitle(usize),
    MenuItem(Vec<usize>),
    Sidebar(usize),
    Hint(KeyEvent),
    JoinField(usize),
    JoinButton,
    /// Focus control `n` of the current tab.
    Ctrl(usize),
    /// Item `j` of control `n` (option, chip, list row, file, button…).
    Item(usize, usize),
    /// A small button of control `n` (spinner −/+ = -1/+1, clear, show…).
    Btn(usize, i8),
    /// A slider track of control `n`.
    Track(usize, Rect),
    /// Row `j` of the open popup list of control `n`.
    Popup(usize, usize),
    Day(Date),
    ChipX(usize),
    Cell(usize, usize),
    Header(usize),
    Card(u64),
    Column(usize),
    Bar(u64, bool),
    Month(usize),
    DemoTitle(usize),
    DemoItem(Vec<usize>),
    CtxItem(usize),
    PaletteRow(usize),
    DialogBtn(usize),
    DialogField(usize),
}

/// A hint-bar entry; clicking it replays `ev`.
#[derive(Clone, Debug)]
pub struct Hint {
    pub key: String,
    pub label: String,
    pub ev: KeyEvent,
}

pub fn hint(key: &str, label: &str, code: KeyCode, mods: KeyModifiers) -> Hint {
    Hint {
        key: key.into(),
        label: label.into(),
        ev: KeyEvent::new(code, mods),
    }
}

pub enum Drag {
    Card {
        id: u64,
    },
    Bar {
        id: u64,
        resize: bool,
        x0: u16,
        start0: Date,
        days0: u32,
    },
    Slider {
        track: Rect,
    },
}

pub struct JoinForm {
    pub server: TextInput,
    pub name: TextInput,
    pub field: usize,
    pub error: Option<String>,
}

pub struct App {
    pub v: Values,
    pub phase: Phase,
    pub join: JoinForm,
    pub theme_idx: usize,
    pub t: Theme,
    pub tab: usize,
    pub focus: Focus,
    /// Focused control of the current tab (when `focus` is the body).
    pub ctrl: usize,
    pub tabs: Tabs,
    pub dialogs: Vec<Dialog>,
    pub menu: Option<MenuState>,
    pub palette: Option<PaletteState>,
    pub help: Option<HelpState>,
    pub toasts: Vec<Toast>,
    pub status: Option<(String, Level, Instant)>,
    pub flashes: HashMap<String, Instant>,
    /// Our sets awaiting an ack: key → the normalised value sent.
    pub inflight: HashMap<String, Value>,
    /// The next value for a key whose previous set is still in flight.
    pub queued: HashMap<String, Value>,
    /// Messages for the server (drained into the connection; tests read it).
    pub outbox: Vec<ClientMsg>,
    pub url: ServerUrl,
    pub tls: Arc<rustls::ClientConfig>,
    pub name: String,
    pub conn: Option<Connection>,
    pub conn_state: ConnState,
    pub welcomed: bool,
    pub welcomes: u32,
    pub ever_online: bool,
    pub today: Date,
    /// Fixed clock for tests (Unix seconds).
    pub fixed_now: Option<i64>,
    pub show_keys: bool,
    pub show_value: bool,
    pub hits: Vec<(Rect, Hit)>,
    /// Where each key's control was drawn (for the change flash).
    pub key_rects: Vec<(&'static str, Rect)>,
    pub drag: Option<Drag>,
    last_click: Option<(Instant, u16, u16)>,
    pub quit: bool,
    pub dirty: bool,
    pub clear_screen: bool,
    pub start: Instant,
    /// A slider drag value waiting for the 150 ms debounce.
    pub slider_pending: Option<(i64, Instant)>,
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Today in the local time zone (natural dates are relative to it).
pub fn local_today() -> Date {
    use chrono::Datelike;
    let d = chrono::Local::now().date_naive();
    Date::from_ymd(d.year(), d.month(), d.day())
        .unwrap_or_else(|| demo_core::date::civil_from_unix(now_secs()).0)
}

impl App {
    pub fn new(url: ServerUrl, tls: Arc<rustls::ClientConfig>, theme_idx: usize) -> App {
        let today = local_today();
        let v = Values::new();
        let tabs = Tabs::new(today);
        App {
            v,
            phase: Phase::Join,
            join: JoinForm {
                server: TextInput::new(&url.base()),
                name: TextInput::new(""),
                field: 1,
                error: None,
            },
            theme_idx,
            t: Theme::new(theme_idx),
            tab: 0,
            focus: Focus::Sidebar,
            ctrl: 0,
            tabs,
            dialogs: vec![],
            menu: None,
            palette: None,
            help: None,
            toasts: vec![],
            status: None,
            flashes: HashMap::new(),
            inflight: HashMap::new(),
            queued: HashMap::new(),
            outbox: vec![],
            url,
            tls,
            name: String::new(),
            conn: None,
            conn_state: ConnState::Connecting,
            welcomed: false,
            welcomes: 0,
            ever_online: false,
            today,
            fixed_now: None,
            show_keys: true,
            show_value: true,
            hits: vec![],
            key_rects: vec![],
            drag: None,
            last_click: None,
            quit: false,
            dirty: true,
            clear_screen: false,
            start: Instant::now(),
            slider_pending: None,
        }
    }

    pub fn now(&self) -> i64 {
        self.fixed_now.unwrap_or_else(now_secs)
    }

    pub fn online(&self) -> bool {
        self.conn_state == ConnState::Online && self.welcomed
    }

    pub fn cur_tab(&self) -> &'static Tab {
        &TABS[self.tab]
    }

    // ---- event loop -----------------------------------------------------------------

    pub async fn run(mut self, term: &mut DefaultTerminal) -> anyhow::Result<()> {
        let mut events = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            if self.clear_screen {
                self.clear_screen = false;
                term.clear()?;
            }
            if self.dirty {
                term.draw(|f| ui::draw(&mut self, f))?;
                self.dirty = false;
            }
            if self.quit {
                // give the last messages (palette.last, a toast) a moment to leave
                self.flush();
                tokio::time::sleep(Duration::from_millis(50)).await;
                return Ok(());
            }
            tokio::select! {
                ev = events.next() => match ev {
                    Some(Ok(ev)) => { self.on_event(ev); self.dirty = true; }
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                },
                ne = recv_net(&mut self.conn) => {
                    match ne {
                        Some(ne) => self.on_net(ne),
                        None => self.conn = None,
                    }
                    self.flush();
                    self.dirty = true;
                }
                _ = tick.tick() => self.on_tick(),
            }
        }
    }

    /// Hand queued messages to the connection (kept for inspection in tests).
    fn flush(&mut self) {
        if let Some(c) = &self.conn {
            for m in self.outbox.drain(..) {
                let _ = c.tx.send(m);
            }
        }
    }

    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Mouse(m) => self.on_mouse(m),
            Event::Paste(s) => self.on_paste(&s),
            _ => {}
        }
        self.flush();
    }

    pub fn on_tick(&mut self) {
        let now = Instant::now();
        let before = (self.toasts.len(), self.flashes.len(), self.status.is_some());
        self.toasts
            .retain(|t| now.duration_since(t.at) < Duration::from_secs(4));
        self.flashes
            .retain(|_, t| now.duration_since(*t) < Duration::from_millis(1300));
        if self
            .status
            .as_ref()
            .is_some_and(|s| now.duration_since(s.2) > Duration::from_secs(10))
        {
            self.status = None;
        }
        if before != (self.toasts.len(), self.flashes.len(), self.status.is_some())
            || !self.flashes.is_empty()
        {
            self.dirty = true;
        }
        if let Some((v, at)) = self.slider_pending {
            if now.duration_since(at) >= Duration::from_millis(150) {
                self.slider_pending = None;
                self.commit("number.percent", Value::from(v));
                self.flush();
            }
        }
        if self.fixed_now.is_none() {
            let t = local_today();
            if t != self.today {
                self.today = t;
                self.dirty = true;
            }
        }
        if self.conn_state != ConnState::Online {
            self.dirty = true; // animate the reconnect indicator
        }
        // relative times in the Value panel ("2m ago") age
        if (self.start.elapsed().as_millis() / 100) % 50 == 0 {
            self.dirty = true;
        }
    }

    // ---- network ----------------------------------------------------------------------

    /// Join as `name` (already validated).
    pub fn connect(&mut self, name: String) {
        self.name = name.clone();
        self.conn_state = ConnState::Connecting;
        self.welcomed = false;
        self.conn = Some(net::connect(self.url.clone(), self.tls.clone(), name));
        self.phase = Phase::Main;
    }

    pub(crate) fn on_net(&mut self, ne: NetEvent) {
        match ne {
            NetEvent::State(s) => {
                let was = self.conn_state;
                self.conn_state = s;
                if s == ConnState::Online {
                    self.ever_online = true;
                }
                if s == ConnState::Reconnecting && was == ConnState::Online {
                    self.welcomed = false;
                    self.v.disconnected();
                    self.inflight.clear();
                    self.queued.clear();
                    self.toast("Connection lost — reconnecting…".into(), Level::Warn);
                }
            }
            NetEvent::Msg(m) => self.on_server(*m),
            NetEvent::Refused(e) => {
                self.conn = None;
                self.back_to_join(e);
            }
            NetEvent::Error(e) => {
                if !self.ever_online && self.join.error.is_none() && self.came_from_join() {
                    self.conn = None;
                    self.back_to_join(e);
                } else {
                    self.set_status(format!("✗ {e}"), Level::Err);
                }
            }
        }
    }

    fn came_from_join(&self) -> bool {
        !self.join.name.is_empty()
    }

    fn back_to_join(&mut self, e: String) {
        self.phase = Phase::Join;
        self.join.error = Some(e);
        self.welcomed = false;
    }

    pub fn on_server(&mut self, m: ServerMsg) {
        let ev = self.v.handle(m);
        match ev {
            VEvent::Welcome => {
                let first = self.welcomes == 0;
                self.welcomes += 1;
                self.welcomed = true;
                self.inflight.clear();
                self.queued.clear();
                tabs::reset_all(self);
                let srv = self
                    .v
                    .server()
                    .map(|s| format!("{} {}", s.implementation, s.version))
                    .unwrap_or_default();
                if first {
                    self.set_status(
                        format!("✓ Connected to demo ({srv}) as {}", self.name),
                        Level::Ok,
                    );
                } else {
                    self.toast("Reconnected — values refreshed".into(), Level::Ok);
                }
                let id = self.cur_tab().id;
                self.outbox.push(Values::focus(id));
            }
            VEvent::Changed { key, by, mine, .. } => {
                if !mine {
                    self.flashes.insert(key.clone(), Instant::now());
                    self.set_status(
                        format!("{} changed {key}", demo_core::safe_text(&by)),
                        Level::Info,
                    );
                }
            }
            VEvent::Saved { key, .. } => {
                self.inflight.remove(&key);
                if let Some(v) = self.queued.remove(&key) {
                    self.commit(&key, v);
                } else {
                    self.set_status(format!("✓ Saved {key}"), Level::Ok);
                }
            }
            VEvent::Conflict { key, by } => {
                self.inflight.remove(&key);
                self.queued.remove(&key);
                self.flashes.insert(key.clone(), Instant::now());
                let who = if by.is_empty() {
                    "someone".to_string()
                } else {
                    demo_core::safe_text(&by)
                };
                self.set_status(
                    format!("⚠ {who} changed {key} first, showing theirs"),
                    Level::Warn,
                );
                tabs::drop_drafts(self, &key);
            }
            VEvent::Rejected { key, error } => {
                if let Some(k) = &key {
                    self.inflight.remove(k);
                    self.queued.remove(k);
                    tabs::drop_drafts(self, k);
                }
                let what = key.map(|k| format!("{k}: ")).unwrap_or_default();
                self.set_status(
                    format!("✗ {what}{}", demo_core::safe_text(&error)),
                    Level::Err,
                );
            }
            VEvent::ResetDone { keys } => {
                for k in &keys {
                    tabs::drop_drafts(self, k);
                }
                self.set_status(
                    format!(
                        "✓ Reset {} control{} to defaults",
                        keys.len(),
                        if keys.len() == 1 { "" } else { "s" }
                    ),
                    Level::Ok,
                );
            }
            VEvent::ToastSent => {}
            VEvent::Toast { text, by, mine, .. } => {
                let who = if mine {
                    "You".to_string()
                } else {
                    demo_core::safe_text(&by)
                };
                self.toast(
                    format!("» {who}: {}", demo_core::safe_text(&text)),
                    Level::Info,
                );
            }
            VEvent::Presence | VEvent::Pong | VEvent::Ignored => {}
            VEvent::ServerError { message } => self.set_status(
                format!("✗ Server: {}", demo_core::safe_text(&message)),
                Level::Err,
            ),
        }
    }

    // ---- values ---------------------------------------------------------------------

    /// What a control should show for `key`: our queued or in-flight value,
    /// else the synced one.
    pub fn shown(&self, key: &str) -> Value {
        self.queued
            .get(key)
            .or_else(|| self.inflight.get(key))
            .cloned()
            .or_else(|| self.v.value(key).cloned())
            .unwrap_or(Value::Null)
    }

    pub fn shown_get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_value(self.shown(key)).ok()
    }

    pub fn shown_str(&self, key: &str) -> String {
        self.shown(key).as_str().unwrap_or("").to_string()
    }

    /// Commit a value (a `set` based on the version we hold).
    pub fn commit(&mut self, key: &str, value: Value) -> bool {
        self.commit_from(key, value, None)
    }

    /// Commit with `base` = the version shown when editing began (text
    /// fields): if someone else changed the key meanwhile, the server
    /// answers with a conflict (ui-spec §6).
    pub fn commit_from(&mut self, key: &str, value: Value, base: Option<u64>) -> bool {
        if !self.online() {
            self.set_status("✗ Offline — the change was not sent".into(), Level::Err);
            return false;
        }
        if self.inflight.contains_key(key) {
            return match keys::validate_with(key, &value, |k| Some(self.shown(k))) {
                Ok(norm) => {
                    self.queued.insert(key.to_string(), norm);
                    true
                }
                Err(e) => {
                    self.set_status(format!("✗ {e}"), Level::Err);
                    false
                }
            };
        }
        match self.v.set(key, value) {
            Ok(mut msg) => {
                if let ClientMsg::Set { base: b, value, .. } = &mut msg {
                    if let Some(base) = base {
                        *b = base;
                    }
                    self.inflight.insert(key.to_string(), value.clone());
                }
                self.outbox.push(msg);
                true
            }
            Err(e) => {
                self.set_status(format!("✗ {e}"), Level::Err);
                false
            }
        }
    }

    pub fn toast(&mut self, text: String, level: Level) {
        self.toasts.push(Toast {
            text,
            level,
            at: Instant::now(),
        });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    pub fn set_status(&mut self, text: String, level: Level) {
        self.status = Some((text, level, Instant::now()));
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.set_status(format!("ℹ {}", text.into()), Level::Info);
    }

    // ---- navigation -----------------------------------------------------------------

    pub fn switch_tab(&mut self, i: usize) {
        let i = i.min(TABS.len() - 1);
        if i != self.tab {
            tabs::leave(self);
            self.tab = i;
            self.ctrl = 0;
            if self.online() {
                self.outbox.push(Values::focus(TABS[i].id));
            }
        }
    }

    /// Move focus into the tab body, on its first control.
    pub fn enter_body(&mut self) {
        self.focus = Focus::Body;
        self.ctrl = 0;
        tabs::enter(self);
    }

    pub fn focus_sidebar(&mut self) {
        tabs::leave(self);
        self.focus = Focus::Sidebar;
    }

    /// Tab / Shift+Tab between the tab's controls; Shift+Tab from the first
    /// and Tab from the last return to the sidebar.
    pub fn next_ctrl(&mut self, back: bool) {
        let n = tabs::controls(self.tab);
        tabs::leave_ctrl(self);
        if back {
            if self.ctrl == 0 {
                self.focus_sidebar();
                return;
            }
            self.ctrl -= 1;
        } else {
            if self.ctrl + 1 >= n {
                self.focus_sidebar();
                return;
            }
            self.ctrl += 1;
        }
        tabs::enter_ctrl(self);
    }

    pub fn focus_ctrl(&mut self, c: usize) {
        if self.focus != Focus::Body || self.ctrl != c {
            if self.focus == Focus::Body {
                tabs::leave_ctrl(self);
            }
            self.focus = Focus::Body;
            self.ctrl = c.min(tabs::controls(self.tab) - 1);
            tabs::enter_ctrl(self);
        }
    }

    pub fn cycle_theme(&mut self) {
        self.set_theme((self.theme_idx + 1) % PALETTES.len());
    }

    pub fn set_theme(&mut self, i: usize) {
        self.theme_idx = i % PALETTES.len();
        self.t = Theme::new(self.theme_idx);
        self.info(format!("Theme: {}", self.t.name));
    }

    /// Run a command (menu, palette, hint). `from_palette` also records it
    /// in `palette.last` (tab 12).
    pub fn run_cmd(&mut self, c: Cmd, from_palette: bool) {
        if from_palette && self.online() {
            self.commit("palette.last", Value::from(c.label()));
        }
        match c {
            Cmd::GoTo(i) => {
                self.switch_tab(i);
                self.focus = Focus::Sidebar;
            }
            Cmd::Next => self.switch_tab((self.tab + 1) % TABS.len()),
            Cmd::Prev => self.switch_tab((self.tab + TABS.len() - 1) % TABS.len()),
            Cmd::ResetTab => {
                let t = self.cur_tab();
                self.dialogs.push(Dialog::confirm(
                    "Reset tab",
                    &format!(
                        "Reset every control on {} to its default for everyone?",
                        t.title
                    ),
                    "Reset",
                    DialogAct::ResetTab(self.tab),
                ));
            }
            Cmd::ResetAll => self.dialogs.push(Dialog::confirm(
                "Reset all tabs",
                "Reset all 16 tabs to their defaults for everyone?",
                "Reset all",
                DialogAct::ResetAll,
            )),
            Cmd::Theme(i) => self.set_theme(i),
            Cmd::ToggleKeys => self.show_keys = !self.show_keys,
            Cmd::ToggleValue => self.show_value = !self.show_value,
            Cmd::Help => self.help = Some(HelpState::default()),
            Cmd::About => {
                let srv = self
                    .v
                    .server()
                    .map(|s| format!("{} {}", s.implementation, s.version))
                    .unwrap_or_else(|| "not connected".into());
                let text = format!(
                    "demo — a control gallery\n\nServer:   {srv} ({})\nClient:   rust tui-rust {}\nProtocol: {}\nYou:      {}\n\nEvery control on its own tab, shared live.",
                    self.url.base(),
                    demo_core::VERSION,
                    demo_core::protocol::PROTOCOL_VERSION,
                    self.name,
                );
                self.dialogs.push(Dialog::message("About demo", &text));
            }
            Cmd::MenuBar => self.menu = Some(MenuState::new(&ui::menus(self), 0)),
            Cmd::Quit => self.quit = true,
        }
    }

    // ---- keys -----------------------------------------------------------------------

    pub fn on_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        if self.phase == Phase::Join {
            if ctrl && k.code == KeyCode::Char('q') {
                self.quit = true;
            } else {
                self.join_key(k);
            }
            return;
        }
        if ctrl && k.code == KeyCode::Char('q') {
            self.quit = true;
            return;
        }
        if !self.dialogs.is_empty() {
            dialogs::key(self, k);
            return;
        }
        if let Some(h) = self.help.as_mut() {
            if h.key(&k) {
                self.help = None;
            }
            return;
        }
        if self.menu.is_some() {
            self.menu_key(k);
            return;
        }
        if let Some(p) = self.palette.as_mut() {
            match p.key(&k) {
                crate::palette::Out::Run(c) => {
                    self.palette = None;
                    self.run_cmd(c, true);
                }
                crate::palette::Out::Close => self.palette = None,
                _ => {}
            }
            return;
        }
        // global keys
        match k.code {
            KeyCode::F(1) => return self.run_cmd(Cmd::Help, false),
            KeyCode::Char('g') if ctrl => return self.run_cmd(Cmd::Help, false),
            KeyCode::F(10) if k.modifiers.contains(KeyModifiers::SHIFT) => {}
            KeyCode::F(10) => return self.run_cmd(Cmd::MenuBar, false),
            KeyCode::Char(c) if alt && !ctrl && ui::menu_for_letter(c).is_some() => {
                let i = ui::menu_for_letter(c).unwrap_or(0);
                self.menu = Some(MenuState::new(&ui::menus(self), i));
                return;
            }
            KeyCode::Char('k' | 'p') if ctrl => {
                self.palette = Some(PaletteState::default());
                return;
            }
            KeyCode::PageDown if ctrl => return self.run_cmd(Cmd::Next, false),
            KeyCode::PageUp if ctrl => return self.run_cmd(Cmd::Prev, false),
            KeyCode::Char(c @ '0'..='9') if alt && !ctrl => {
                let i = if c == '0' {
                    9
                } else {
                    c as usize - '1' as usize
                };
                self.switch_tab(i);
                return;
            }
            KeyCode::Char('r') if ctrl => return self.run_cmd(Cmd::ResetTab, false),
            KeyCode::Char('l') if ctrl => {
                self.clear_screen = true;
                return;
            }
            KeyCode::F(6) => {
                if self.focus == Focus::Sidebar {
                    self.enter_body();
                } else {
                    self.focus_sidebar();
                }
                return;
            }
            _ => {}
        }
        match self.focus {
            Focus::Sidebar => self.sidebar_key(k),
            Focus::Body => {
                if tabs::key(self, k) {
                    return;
                }
                match k.code {
                    KeyCode::Tab => self.next_ctrl(false),
                    KeyCode::BackTab => self.next_ctrl(true),
                    KeyCode::Esc => self.focus_sidebar(),
                    KeyCode::Char('t') if ctrl => self.cycle_theme(),
                    _ => {}
                }
            }
        }
    }

    fn sidebar_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Up => self.switch_tab((self.tab + TABS.len() - 1) % TABS.len()),
            KeyCode::Down => self.switch_tab((self.tab + 1) % TABS.len()),
            KeyCode::Home => self.switch_tab(0),
            KeyCode::End => self.switch_tab(TABS.len() - 1),
            KeyCode::Enter | KeyCode::Right | KeyCode::Tab => self.enter_body(),
            KeyCode::Char('t') if ctrl => self.cycle_theme(),
            KeyCode::Char(c) if !ctrl && c.is_ascii_digit() => {
                // digits jump too (1–9; 0 = 10)
                let i = if c == '0' {
                    9
                } else {
                    c as usize - '1' as usize
                };
                self.switch_tab(i);
            }
            _ => {}
        }
    }

    fn menu_key(&mut self, k: KeyEvent) {
        let menus = ui::menus(self);
        let Some(st) = self.menu.as_mut() else { return };
        if k.code == KeyCode::F(10) {
            self.menu = None;
            return;
        }
        match menu::key(&menus, st, &k) {
            menu::Out::Close => self.menu = None,
            menu::Out::Picked(c, _) => {
                self.menu = None;
                self.run_cmd(c, false);
            }
            menu::Out::Disabled(l) => self.info(format!("{l} is not available")),
            menu::Out::None => {}
        }
    }

    fn join_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Tab | KeyCode::Down => self.join.field = (self.join.field + 1) % 2,
            KeyCode::BackTab | KeyCode::Up => self.join.field = (self.join.field + 1) % 2,
            KeyCode::Enter => self.try_join(),
            KeyCode::Char('s') if ctrl => self.try_join(),
            KeyCode::Char('t') if ctrl => self.cycle_theme(),
            _ => {
                let f = if self.join.field == 0 {
                    &mut self.join.server
                } else {
                    &mut self.join.name
                };
                if f.handle(&k) {
                    self.join.error = None;
                }
            }
        }
    }

    pub fn try_join(&mut self) {
        let url = match ServerUrl::parse(&self.join.server.text()) {
            Ok(u) => u,
            Err(e) => {
                self.join.error = Some(format!("Server URL: {e}"));
                self.join.field = 0;
                return;
            }
        };
        let Some(name) = clean_name(&self.join.name.text()) else {
            self.join.error =
                Some("Name: 1–24 letters, digits, space, _ . - starting with a letter".into());
            self.join.field = 1;
            return;
        };
        self.join.error = None;
        self.url = url;
        self.ever_online = false;
        self.connect(name);
    }

    pub fn on_paste(&mut self, s: &str) {
        if self.phase == Phase::Join {
            let f = if self.join.field == 0 {
                &mut self.join.server
            } else {
                &mut self.join.name
            };
            f.insert_str(s);
            return;
        }
        if let Some(Dialog::Input(d)) = self.dialogs.last_mut() {
            d.input.insert_str(s);
            return;
        }
        if self.focus == Focus::Body && self.dialogs.is_empty() {
            tabs::paste(self, s);
        }
    }

    // ---- mouse ----------------------------------------------------------------------

    pub fn hit_at(&self, x: u16, y: u16) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| rect_contains(*r, x, y))
            .map(|(_, h)| h.clone())
    }

    pub fn is_double(&mut self, x: u16, y: u16) -> bool {
        let now = Instant::now();
        let dbl = self.last_click.is_some_and(|(t, lx, ly)| {
            now.duration_since(t) < Duration::from_millis(400) && lx == x && ly == y
        });
        self.last_click = if dbl { None } else { Some((now, x, y)) };
        dbl
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        match m.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                tabs::drag(self, x, y, false);
                return;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.drag.is_some() {
                    tabs::drag(self, x, y, true);
                    self.drag = None;
                }
                return;
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = matches!(m.kind, MouseEventKind::ScrollDown);
                if let Some(h) = self.help.as_mut() {
                    h.scroll_by(if down { 3 } else { -3 });
                } else if self.dialogs.is_empty() && self.menu.is_none() && self.palette.is_none() {
                    tabs::scroll(self, down);
                }
                return;
            }
            MouseEventKind::Down(_) => {}
            _ => return,
        }
        let right = matches!(m.kind, MouseEventKind::Down(MouseButton::Right));
        let hit = self.hit_at(x, y);
        if self.phase == Phase::Join {
            match hit {
                Some(Hit::JoinField(i)) => self.join.field = i,
                Some(Hit::JoinButton) => self.try_join(),
                _ => {}
            }
            return;
        }
        if let Some(Hit::Hint(ev)) = hit {
            self.on_key(ev);
            return;
        }
        if !self.dialogs.is_empty() {
            dialogs::click(self, hit);
            return;
        }
        if self.help.is_some() {
            return;
        }
        if self.menu.is_some() {
            match hit {
                Some(Hit::MenuTitle(i)) => {
                    let same = self.menu.as_ref().is_some_and(|s| s.open == i);
                    self.menu = if same {
                        None
                    } else {
                        Some(MenuState::new(&ui::menus(self), i))
                    };
                }
                Some(Hit::MenuItem(path)) => {
                    let menus = ui::menus(self);
                    if let Some(st) = self.menu.as_mut() {
                        st.path = path;
                        match menu::activate(&menus, st) {
                            menu::Out::Picked(c, _) => {
                                self.menu = None;
                                self.run_cmd(c, false);
                            }
                            menu::Out::Disabled(l) => self.info(format!("{l} is not available")),
                            _ => {}
                        }
                    }
                }
                _ => self.menu = None,
            }
            return;
        }
        if self.palette.is_some() {
            match hit {
                Some(Hit::PaletteRow(i)) => {
                    let c = self.palette.as_ref().and_then(|p| p.command_at(i));
                    self.palette = None;
                    if let Some(c) = c {
                        self.run_cmd(c, true);
                    }
                }
                _ => self.palette = None,
            }
            return;
        }
        match hit {
            Some(Hit::MenuTitle(i)) => self.menu = Some(MenuState::new(&ui::menus(self), i)),
            Some(Hit::Sidebar(i)) => {
                if self.focus == Focus::Body {
                    self.focus_sidebar();
                }
                self.switch_tab(i);
            }
            Some(h) => tabs::click(self, h, x, y, right),
            None => {}
        }
    }
}

async fn recv_net(conn: &mut Option<Connection>) -> Option<NetEvent> {
    match conn {
        Some(c) => c.rx.recv().await,
        None => std::future::pending().await,
    }
}
