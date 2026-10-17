//! Application state, the event loop, network handling and the actions that
//! every view shares (editing, clipboard, undo, presence). View-specific
//! input lives in `views/`, dialogs in `dialogs.rs`, drawing in `ui.rs`.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use futures_util::StreamExt;
use pt_core::grid::{self as cg, Column, Editor, GridRow};
use pt_core::model::*;
use pt_core::protocol::{ClientMsg, LoginResponse, Op, ResultMsg, ServerMsg};
use pt_core::query::{self, Query, Target};
use pt_core::replica::Replica;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::dialogs::{self, Dialog};
use crate::grid::{GridKind, Hit, RowFilter};
use crate::keymap::{self, Action, Ctx};
use crate::net::{self, ConnState, Connection, NetEvent, ServerUrl};
use crate::theme::{self, Theme};
use crate::ui;
use crate::views::{self, GridSel, Views};
use crate::widgets::draw::rect_contains;
use crate::widgets::editor::{EdKind, EdOut, EdValue, FieldEditor, Nav};
use crate::widgets::input::TextInput;

pub type CellKey = (Entity, i64, String);
/// (view, focused row and field) as last sent to the server.
type FocusKey = (String, Option<(Entity, i64, Option<String>)>);

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

#[derive(Clone, Debug)]
pub struct Conflict {
    pub current: Value,
    pub by: Option<i64>,
    pub mine: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendKind {
    Cells,
    Undo,
    Redo,
    Form,
    Password,
}

pub struct Pending {
    pub kind: PendKind,
    pub cells: Vec<CellKey>,
    pub ops: Vec<Op>,
    pub label: String,
    /// Select this created row (by ref) once it arrives.
    pub select: Option<(Entity, String)>,
}

/// An open inline cell editor.
pub struct CellEdit {
    pub grid: GridSel,
    pub entity: Entity,
    pub id: i64,
    pub field: String,
    pub base: Value,
    pub row: usize,
    pub vcol: usize,
    pub ed: FieldEditor,
}

/// The filter bar for one view, with autocomplete.
#[derive(Default)]
pub struct QueryBar {
    pub input: TextInput,
    pub query: Query,
    pub compiled_text: String,
    pub compiled_version: u64,
    pub sugg: Vec<String>,
    pub sel: usize,
}

pub enum Drag {
    ResizeCol { grid: GridSel, vcol: usize, x0: u16, w0: u16 },
    Card { task: i64 },
    Bar { task_or_project: (Entity, i64), x0: u16, resize: bool },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SavedFilter {
    pub name: String,
    pub view: usize,
    pub query: String,
}

pub enum Internal {
    Login(Result<LoginResponse, String>, bool),
    LoggedOut,
}

#[derive(Default)]
pub struct Hits {
    pub menus: Vec<(Rect, usize)>,
    pub tabs: Vec<(Rect, usize)>,
    pub query: Rect,
    pub hints: Vec<(Rect, Action)>,
    pub main: Rect,
    pub toasts: Rect,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Login,
    Main,
}

pub struct App {
    pub r: Replica,
    pub theme_idx: usize,
    pub t: Theme,
    pub view: usize,
    pub focus_query: bool,
    pub queries: Vec<QueryBar>,
    pub views: Views,
    pub editor: Option<CellEdit>,
    pub dialogs: Vec<Dialog>,
    pub menu: Option<ui::MenuState>,
    pub toasts: Vec<Toast>,
    pub status: Option<(String, Level, Instant)>,
    pub phase: Phase,
    pub url: ServerUrl,
    pub tls: Arc<rustls::ClientConfig>,
    pub token: Option<String>,
    pub username: String,
    pub conn: Option<Connection>,
    pub conn_state: ConnState,
    pub req: u64,
    pub pending: HashMap<u64, Pending>,
    pub pending_cells: HashSet<CellKey>,
    pub conflicts: HashMap<CellKey, Conflict>,
    pub flashes: HashMap<(Entity, i64), Instant>,
    pub select_after: Option<(Entity, i64)>,
    pub clipboard: String,
    focus_sent: Option<FocusKey>,
    focus_dirty: Option<Instant>,
    pub quit: bool,
    pub hits: Hits,
    last_click: Option<(Instant, u16, u16)>,
    pub drag: Option<Drag>,
    pub saved_filters: Vec<SavedFilter>,
    pub internal_tx: mpsc::UnboundedSender<Internal>,
    pub dirty: bool,
    pub hover: Option<(u16, u16)>,
    pub start_time: Instant,
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn today() -> pt_core::Date {
    pt_core::date::civil_from_unix(now_secs()).0
}

pub const VIEW_KEYS: [&str; 9] = ["projects", "tasks", "board", "timeline", "dashboard", "activity", "templates", "fields", "users"];

impl App {
    pub fn new(url: ServerUrl, tls: Arc<rustls::ClientConfig>, theme_idx: usize, internal_tx: mpsc::UnboundedSender<Internal>) -> App {
        let r = Replica::new(today());
        let views = Views::new(&r);
        App {
            views,
            r,
            theme_idx,
            t: Theme::new(theme_idx),
            view: views::PROJECTS,
            focus_query: false,
            queries: (0..9).map(|_| QueryBar::default()).collect(),
            editor: None,
            dialogs: vec![],
            menu: None,
            toasts: vec![],
            status: None,
            phase: Phase::Login,
            url,
            tls,
            token: None,
            username: String::new(),
            conn: None,
            conn_state: ConnState::Connecting,
            req: 0,
            pending: HashMap::new(),
            pending_cells: HashSet::new(),
            conflicts: HashMap::new(),
            flashes: HashMap::new(),
            select_after: None,
            clipboard: String::new(),
            focus_sent: None,
            focus_dirty: None,
            quit: false,
            hits: Hits::default(),
            last_click: None,
            drag: None,
            saved_filters: load_filters(),
            internal_tx,
            dirty: true,
            hover: None,
            start_time: Instant::now(),
        }
    }

    pub fn online(&self) -> bool {
        self.conn_state == ConnState::Online && self.r.is_loaded()
    }

    // ---- event loop -----------------------------------------------------------------

    pub async fn run(mut self, term: &mut DefaultTerminal, mut internal_rx: mpsc::UnboundedReceiver<Internal>) -> anyhow::Result<()> {
        let mut events = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            if self.dirty {
                term.draw(|f| ui::draw(&mut self, f))?;
                self.dirty = false;
            }
            if self.quit {
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
                    self.dirty = true;
                }
                ie = internal_rx.recv() => if let Some(ie) = ie { self.on_internal(ie); self.dirty = true; },
                _ = tick.tick() => self.on_tick(),
            }
            if term.size().is_ok_and(|s| s.width == 0) {
                continue;
            }
        }
    }

    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Mouse(m) => self.on_mouse(m),
            Event::Paste(s) => self.on_paste(&s),
            Event::Resize(..) | Event::FocusGained => self.dirty = true,
            _ => {}
        }
        self.after_input();
    }

    fn on_tick(&mut self) {
        let now = Instant::now();
        let before = (self.toasts.len(), self.flashes.len(), self.status.is_some());
        self.toasts.retain(|t| now.duration_since(t.at) < Duration::from_secs(4));
        self.flashes.retain(|_, t| now.duration_since(*t) < Duration::from_millis(1300));
        if self.status.as_ref().is_some_and(|s| now.duration_since(s.2) > Duration::from_secs(8)) {
            self.status = None;
        }
        if before != (self.toasts.len(), self.flashes.len(), self.status.is_some()) || !self.flashes.is_empty() {
            self.dirty = true;
        }
        if let Some(at) = self.focus_dirty {
            if now.duration_since(at) >= Duration::from_millis(150) {
                self.focus_dirty = None;
                self.send_focus();
            }
        }
        // date rolls over at midnight UTC
        let t = today();
        if t != self.r.today {
            self.r.set_today(t);
            self.r.version += 1;
            self.dirty = true;
        }
        if self.conn_state != ConnState::Online {
            self.dirty = true; // animate the reconnect indicator
        }
    }

    // ---- network ----------------------------------------------------------------------

    pub fn connect(&mut self, token: String) {
        self.token = Some(token.clone());
        self.conn_state = ConnState::Connecting;
        self.conn = Some(net::connect(self.url.clone(), self.tls.clone(), token));
        self.phase = Phase::Main;
    }

    fn on_internal(&mut self, ie: Internal) {
        match ie {
            Internal::Login(Ok(resp), remember) => {
                if remember {
                    if let Err(e) = net::remember(&self.url, &resp.user.username, &resp.token) {
                        self.toast(format!("Couldn't save the session: {e}"), Level::Warn);
                    }
                }
                self.username = resp.user.username.clone();
                self.dialogs.retain(|d| !matches!(d, Dialog::Form(f) if f.kind == dialogs::FormKind::Login));
                self.connect(resp.token);
            }
            Internal::Login(Err(e), _) => {
                if let Some(Dialog::Form(f)) = self.dialogs.iter_mut().find(|d| matches!(d, Dialog::Form(f) if f.kind == dialogs::FormKind::Login)) {
                    f.busy = false;
                    f.message = Some((e, true));
                }
            }
            Internal::LoggedOut => {}
        }
    }

    pub(crate) fn on_net(&mut self, ne: NetEvent) {
        match ne {
            NetEvent::State(s) => {
                let was = self.conn_state;
                self.conn_state = s;
                if s == ConnState::Online && was == ConnState::Reconnecting {
                    self.toast("Reconnected".into(), Level::Ok);
                } else if s == ConnState::Reconnecting && was == ConnState::Online {
                    self.toast("Connection lost — reconnecting…".into(), Level::Warn);
                    self.pending.clear();
                    self.pending_cells.clear();
                }
            }
            NetEvent::Msg(m) => self.on_server(*m),
            NetEvent::AuthFailed => {
                self.conn = None;
                self.token = None;
                let _ = net::forget(&self.url);
                self.r = Replica::new(today());
                self.views = Views::new(&self.r);
                self.clear_session_state();
                self.show_login(Some("Your session has ended. Please log in again.".into()));
            }
            NetEvent::Error(e) => self.set_status(format!("Connection problem: {e}"), Level::Err),
        }
    }

    fn on_server(&mut self, m: ServerMsg) {
        match &m {
            ServerMsg::Result(res) => {
                let res = res.clone();
                self.on_result(res);
                return;
            }
            ServerMsg::Error { message, .. } => {
                self.toast(format!("Server: {message}"), Level::Err);
                return;
            }
            _ => {}
        }
        let first_load = !self.r.is_loaded();
        let applied = self.r.apply(&m);
        if let ServerMsg::Welcome { me, .. } = &m {
            self.username = me.username.clone();
        }
        if applied.snapshot {
            self.pending.clear();
            self.pending_cells.clear();
            if first_load {
                self.views.timeline.goto_today(self.r.today);
                let me = self.r.me.as_ref().map(|u| u.display_name.clone()).unwrap_or_default();
                let srv = self.r.server.as_ref().map(|s| format!("{} {}", s.implementation, s.version)).unwrap_or_default();
                self.set_status(format!("Welcome, {me} — connected to pt ({srv})"), Level::Ok);
            }
        }
        let me = self.r.me_id();
        if applied.by.is_some_and(|b| b != me) {
            let now = Instant::now();
            for key in &applied.upserted {
                self.flashes.insert(*key, now);
            }
            let fresh: Vec<&Activity> = applied.activity.iter().filter(|a| a.user_id != Some(me) && a.state == ActState::Done).collect();
            if let Some(a) = fresh.first() {
                let who = self.r.username(a.user_id).to_string();
                let more = if fresh.len() > 1 { format!(" (+{} more)", fresh.len() - 1) } else { String::new() };
                self.toast(format!("{who}: {}{more}", a.label), Level::Info);
            }
        }
        // a row the editor is on was deleted
        if let Some(ed) = &self.editor {
            if !self.r.exists(ed.entity, ed.id) {
                self.editor = None;
                self.toast("That row was deleted by someone else.".into(), Level::Warn);
            }
        }
        if let Some((e, id)) = self.select_after {
            if self.r.exists(e, id) {
                self.select_after = None;
                self.prepare();
                self.reveal(e, id);
            }
        }
    }

    fn on_result(&mut self, res: ResultMsg) {
        let Some(p) = self.pending.remove(&res.req) else { return };
        for c in &p.cells {
            self.pending_cells.remove(c);
        }
        if p.kind == PendKind::Form || p.kind == PendKind::Password {
            self.form_result(res.req, &res);
            if let (true, Some((e, r))) = (res.ok, &p.select) {
                if let Some(id) = res.created.get(r) {
                    self.select_after = Some((*e, *id));
                }
            }
            return;
        }
        if !res.ok {
            let msg = res.error.as_ref().map_or("failed".to_string(), |e| e.message.clone());
            match p.kind {
                PendKind::Undo | PendKind::Redo if !res.conflicts.is_empty() => {
                    let c = &res.conflicts[0];
                    let who = c.by.map(|b| self.r.username(Some(b)).to_string()).unwrap_or_else(|| "someone".into());
                    let what = self.r.row_label(c.entity, c.id);
                    self.toast(format!("Can't {}: {who} changed {what} since.", if p.kind == PendKind::Undo { "undo" } else { "redo" }), Level::Warn);
                }
                PendKind::Undo | PendKind::Redo => self.toast(msg, Level::Info),
                _ => self.toast(format!("✗ {msg}"), Level::Err),
            }
            return;
        }
        for c in &res.conflicts {
            if let Some(Op::Set { entity, id, field, value, .. }) = p.ops.get(c.index) {
                let key = (*entity, *id, field.clone());
                let who = c.by.map(|b| self.r.user_name(Some(b)).to_string()).unwrap_or_else(|| "Someone".into());
                let label = self.field_label(*entity, field);
                let cur = cg::display_value(&self.r, *entity, field, &c.current);
                self.toast(format!("⚠ {who} changed {label} to {cur}; your edit wasn't applied (Enter: edit again)"), Level::Warn);
                self.conflicts.insert(key, Conflict { current: c.current.clone(), by: c.by, mine: value.clone() });
            }
        }
        if res.group.is_some() {
            match p.kind {
                PendKind::Undo => self.set_status(format!("↶ Undone{}", if p.label.is_empty() { String::new() } else { format!(": {}", p.label) }), Level::Ok),
                PendKind::Redo => self.set_status("↷ Redone".into(), Level::Ok),
                _ => self.set_status(format!("✓ Saved {}", p.label), Level::Ok),
            }
            for c in &p.cells {
                if !res.conflicts.iter().any(|x| p.ops.get(x.index).is_some_and(|o| matches!(o, Op::Set { entity, id, field, .. } if (*entity, *id, field.clone()) == *c))) {
                    self.conflicts.remove(c);
                }
            }
        }
        if let Some((e, r)) = &p.select {
            if let Some(id) = res.created.get(r) {
                self.select_after = Some((*e, *id));
            }
        }
    }

    pub fn field_label(&self, e: Entity, field: &str) -> String {
        if let Some(id) = pt_core::fields::cf_id(field) {
            return self.r.custom_fields.get(&id).map_or(field.to_string(), |c| c.name.clone());
        }
        pt_core::fields::field(e, field).map_or(field.to_string(), |m| m.label.to_string())
    }

    pub fn send(&mut self, msg: ClientMsg) -> bool {
        match &self.conn {
            Some(c) if self.conn_state == ConnState::Online => c.tx.send(msg).is_ok(),
            _ => false,
        }
    }

    fn next_req(&mut self) -> u64 {
        self.req += 1;
        self.req
    }

    /// Send a mutate; cells show as pending until the result arrives.
    pub fn mutate(&mut self, ops: Vec<Op>, label: impl Into<String>, kind: PendKind, select: Option<(Entity, String)>) -> Option<u64> {
        if ops.is_empty() {
            return None;
        }
        if !self.online() {
            self.toast("You're offline — changes are disabled until the connection is back.".into(), Level::Err);
            return None;
        }
        if !self.r.can_edit() {
            self.toast("Your account is read-only (viewer).".into(), Level::Err);
            return None;
        }
        let req = self.next_req();
        let cells: Vec<CellKey> = ops
            .iter()
            .filter_map(|o| match o {
                Op::Set { entity, id, field, .. } => Some((*entity, *id, field.clone())),
                _ => None,
            })
            .collect();
        for c in &cells {
            self.pending_cells.insert(c.clone());
        }
        self.pending.insert(req, Pending { kind, cells, ops: ops.clone(), label: label.into(), select });
        if !self.send(ClientMsg::Mutate { req, ops }) {
            self.toast("Not connected.".into(), Level::Err);
        }
        Some(req)
    }

    pub fn undo(&mut self, redo: bool) {
        if !self.online() {
            return;
        }
        let req = self.next_req();
        let label = if redo {
            String::new()
        } else {
            let me = self.r.me_id();
            self.r
                .activity
                .values()
                .rev()
                .find(|a| a.user_id == Some(me) && a.state == ActState::Done)
                .map(|a| a.label.clone())
                .unwrap_or_default()
        };
        self.pending.insert(req, Pending { kind: if redo { PendKind::Redo } else { PendKind::Undo }, cells: vec![], ops: vec![], label, select: None });
        self.send(if redo { ClientMsg::Redo { req } } else { ClientMsg::Undo { req } });
    }

    pub fn toast(&mut self, text: String, level: Level) {
        self.toasts.push(Toast { text, level, at: Instant::now() });
        while self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
        self.dirty = true;
    }

    pub fn set_status(&mut self, text: String, level: Level) {
        self.status = Some((text, level, Instant::now()));
    }

    // ---- presence ----------------------------------------------------------------------

    fn focus_key(&self) -> FocusKey {
        let view = VIEW_KEYS[self.view].to_string();
        if let Some(gs) = self.views.active_grid(self.view) {
            let g = self.views.grid_ref(gs);
            if let (Some(e), Some(id)) = (g.entity(), g.current_id()) {
                return (view, Some((e, id, g.current_col().and_then(|c| c.field.clone()))));
            }
        }
        match self.view {
            views::BOARD => (view, self.views.board.current.map(|id| (Entity::Task, id, None))),
            views::TIMELINE => (view, self.views.timeline.current_entity().map(|(e, id)| (e, id, None))),
            _ => (view, None),
        }
    }

    fn after_input(&mut self) {
        let k = self.focus_key();
        if self.focus_sent.as_ref() != Some(&k) {
            self.focus_dirty = Some(Instant::now());
        }
    }

    fn send_focus(&mut self) {
        let k = self.focus_key();
        if self.focus_sent.as_ref() == Some(&k) {
            return;
        }
        let (view, f) = k.clone();
        let msg = ClientMsg::Focus {
            view: Some(view),
            entity: f.as_ref().map(|x| x.0),
            id: f.as_ref().map(|x| x.1),
            field: f.and_then(|x| x.2),
        };
        if self.send(msg) {
            self.focus_sent = Some(k);
        }
    }

    /// Other users' focus by row, for gutter and cell markers.
    pub fn presence_map(&self) -> crate::grid::Presence {
        let mut m = crate::grid::Presence::new();
        for p in self.r.other_peers() {
            if let Some(f) = &p.focus {
                let initial = self.r.users.get(&p.user_id).map(|u| u.initials().chars().next().unwrap_or('?').to_string()).unwrap_or("?".into());
                m.entry((f.entity, f.id)).or_default().push((p.user_id, initial, f.field.clone()));
            }
        }
        m
    }

    // ---- view preparation ------------------------------------------------------------

    fn recompile_queries(&mut self) {
        for (i, qb) in self.queries.iter_mut().enumerate() {
            let text = qb.input.text();
            if text == qb.compiled_text && qb.compiled_version == self.r.version {
                continue;
            }
            let target = match i {
                views::PROJECTS | views::TIMELINE => Some(Target::Projects),
                views::TASKS | views::BOARD => Some(Target::Tasks),
                views::ACTIVITY => Some(Target::Activity),
                _ => None,
            };
            let changed = text != qb.compiled_text;
            qb.query = match target {
                Some(t) => Query::compile(&text, t, &self.r),
                None => Query::default(),
            };
            qb.compiled_text = text;
            qb.compiled_version = self.r.version;
            if changed {
                match i {
                    views::PROJECTS => self.views.projects.main.touch(),
                    views::TASKS => self.views.tasks.touch(),
                    views::ACTIVITY => self.views.activity.touch(),
                    views::TEMPLATES => self.views.templates.main.touch(),
                    views::FIELDS => self.views.fields.touch(),
                    views::USERS => self.views.users.main.touch(),
                    _ => {}
                }
            }
        }
    }

    /// Refresh every grid's rows (cheap when nothing changed) and detail parents.
    pub fn prepare(&mut self) {
        self.recompile_queries();
        let r = &self.r;
        let q = &self.queries;
        let v = &mut self.views;
        v.projects.main.refresh(r, RowFilter::Query(&q[views::PROJECTS].query));
        let pid = v.projects.main.current_id();
        if v.projects.detail.parent != pid {
            v.projects.detail.parent = pid;
            v.projects.detail.touch();
            v.projects.detail.cur = 0;
        }
        v.projects.detail.refresh(r, RowFilter::None);
        v.tasks.refresh(r, RowFilter::Query(&q[views::TASKS].query));
        v.activity.refresh(r, RowFilter::Query(&q[views::ACTIVITY].query));
        v.templates.main.refresh(r, RowFilter::Text(&q[views::TEMPLATES].compiled_text));
        let tid = v.templates.main.current_id();
        if v.templates.detail.parent != tid {
            v.templates.detail.parent = tid;
            v.templates.detail.touch();
            v.templates.detail.cur = 0;
        }
        v.templates.detail.refresh(r, RowFilter::None);
        v.fields.refresh(r, RowFilter::Text(&q[views::FIELDS].compiled_text));
        v.users.main.refresh(r, RowFilter::Text(&q[views::USERS].compiled_text));
        v.users.detail.refresh(r, RowFilter::Text(&q[views::USERS].compiled_text));
    }

    /// Switch to the best view for a row and put the cursor on it.
    pub fn reveal(&mut self, e: Entity, id: i64) {
        self.prepare();
        match e {
            Entity::Project => {
                if self.view != views::PROJECTS && self.view != views::TIMELINE {
                    self.view = views::PROJECTS;
                }
                if self.view == views::PROJECTS {
                    if !self.views.projects.main.select_id(id) {
                        self.queries[views::PROJECTS].input.clear();
                        self.prepare();
                        self.views.projects.main.select_id(id);
                    }
                    self.views.projects.pane = 0;
                }
            }
            Entity::Task => {
                match self.view {
                    views::PROJECTS => {
                        let pid = self.r.tasks.get(&id).map(|t| t.project_id);
                        if let Some(pid) = pid {
                            if !self.views.projects.main.select_id(pid) {
                                self.queries[views::PROJECTS].input.clear();
                                self.prepare();
                                self.views.projects.main.select_id(pid);
                            }
                            self.prepare();
                            self.views.projects.detail.select_id(id);
                            self.views.projects.pane = 1;
                            self.views.projects.show_detail = true;
                        }
                        return;
                    }
                    views::BOARD | views::TIMELINE => {
                        self.views.board.current = Some(id);
                        return;
                    }
                    _ => {}
                }
                self.view = views::TASKS;
                // expand its group if collapsed
                if let Some(t) = self.r.tasks.get(&id) {
                    let code = self.r.project_code(t.project_id).to_lowercase();
                    self.views.tasks.collapsed.remove(&code);
                    self.views.tasks.touch();
                }
                if !self.views.tasks.select_id(id) {
                    self.queries[views::TASKS].input.clear();
                    self.prepare();
                    self.views.tasks.select_id(id);
                }
            }
            Entity::Template => {
                self.view = views::TEMPLATES;
                self.views.templates.main.select_id(id);
                self.views.templates.pane = 0;
            }
            Entity::TemplateTask => {
                self.view = views::TEMPLATES;
                if let Some(tid) = self.r.template_tasks.get(&id).map(|t| t.template_id) {
                    self.views.templates.main.select_id(tid);
                    self.prepare();
                    self.views.templates.detail.select_id(id);
                    self.views.templates.pane = 1;
                }
            }
            Entity::CustomField => {
                self.view = views::FIELDS;
                self.views.fields.select_id(id);
            }
            Entity::User => {
                self.view = views::USERS;
                self.views.users.pane = 0;
                self.views.users.main.select_id(id);
            }
            Entity::Program => {
                self.view = views::USERS;
                self.views.users.pane = 1;
                self.views.users.detail.select_id(id);
            }
        }
        self.prepare();
    }

    // ---- keyboard ------------------------------------------------------------------------

    pub fn ctx(&self) -> Ctx {
        match self.view {
            views::BOARD => Ctx::Board,
            views::TIMELINE => Ctx::Timeline,
            views::DASHBOARD => Ctx::Dashboard,
            _ => Ctx::Grid,
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if !self.dialogs.is_empty() {
            self.dialog_key(k);
            return;
        }
        if self.phase == Phase::Login {
            return;
        }
        if self.menu.is_some() {
            self.menu_key(k);
            return;
        }
        if self.editor.is_some() {
            self.editor_key(k);
            return;
        }
        // Alt+letter opens a menu
        if k.modifiers == KeyModifiers::ALT {
            if let KeyCode::Char(c) = k.code {
                if let Some(i) = ui::menu_for_letter(c) {
                    self.menu = Some(ui::MenuState { open: i, sel: 0 });
                    return;
                }
            }
        }
        if self.focus_query {
            self.query_key(k);
            return;
        }
        if let Some(a) = keymap::lookup(self.ctx(), &k) {
            self.action(a);
            return;
        }
        match self.view {
            views::BOARD => self.board_key(k),
            views::TIMELINE => self.timeline_key(k),
            views::DASHBOARD => self.dashboard_key(k),
            _ => self.grid_key(k),
        }
    }

    pub fn action(&mut self, a: Action) {
        match a {
            Action::Help => self.open_help(),
            Action::Menu => self.menu = Some(ui::MenuState { open: 0, sel: 0 }),
            Action::Palette => self.open_palette(),
            Action::Filter => {
                if ui::has_query_bar(self.view) {
                    self.focus_query = true;
                    self.update_suggestions();
                }
            }
            Action::ClearFilter => {
                self.queries[self.view].input.clear();
                self.focus_query = false;
            }
            Action::SaveFilter => self.open_save_filter(),
            Action::NextView => self.switch_view((self.view + 1) % 9),
            Action::PrevView => self.switch_view((self.view + 8) % 9),
            Action::View(n) => self.switch_view(n as usize),
            Action::NextPane | Action::PrevPane => self.next_pane(a == Action::PrevPane),
            Action::Undo => self.undo(false),
            Action::Redo => self.undo(true),
            Action::New => self.new_record(),
            Action::NewProject => self.open_new_project(),
            Action::NewTask => self.open_new_task(),
            Action::EditRecord => self.edit_record(),
            Action::Delete => self.delete_selected(),
            Action::ToggleDetail => match self.view {
                views::PROJECTS => {
                    self.views.projects.show_detail = !self.views.projects.show_detail;
                    self.views.projects.pane = 0;
                }
                views::TASKS => self.views.tasks_card = !self.views.tasks_card,
                views::TEMPLATES => self.views.templates.show_detail = !self.views.templates.show_detail,
                _ => {}
            },
            Action::Columns => {
                if let Some(g) = self.views.active_grid(self.view) {
                    self.open_columns(g);
                }
            }
            Action::SortColumn => {
                if let Some(gs) = self.views.active_grid(self.view) {
                    let g = self.views.grid(gs);
                    if let Some(key) = g.current_col().map(|c| c.key.clone()) {
                        g.cycle_sort(&key, false);
                    }
                }
            }
            Action::SortDialog => {
                if let Some(g) = self.views.active_grid(self.view) {
                    self.open_sort(g);
                }
            }
            Action::GroupColumn => {
                if let Some(gs) = self.views.active_grid(self.view) {
                    let g = self.views.grid(gs);
                    if let Some(key) = g.current_col().map(|c| c.key.clone()) {
                        g.toggle_group_by(&key);
                    }
                }
            }
            Action::ClearGroup => {
                if let Some(gs) = self.views.active_grid(self.view) {
                    let g = self.views.grid(gs);
                    g.group = None;
                    g.touch();
                }
            }
            Action::Theme => {
                self.theme_idx = (self.theme_idx + 1) % theme::PALETTES.len();
                self.t = Theme::new(self.theme_idx);
                self.set_status(format!("Theme: {}", self.t.name), Level::Info);
            }
            Action::Redraw => self.dirty = true,
            Action::Quit => self.quit = true,
            Action::Logout => self.logout(),
            Action::ContextMenu => self.open_context_here(),
            Action::Copy => self.copy(),
            Action::Paste => {
                let c = self.clipboard.clone();
                if c.is_empty() {
                    self.toast("The pt clipboard is empty. Use your terminal's paste (e.g. ⌘V / ⇧^V) for text from other apps.".into(), Level::Info);
                } else {
                    self.on_paste(&c);
                }
            }
            Action::FillDown => self.fill_down(),
            Action::MarkAll => {
                if let Some(gs) = self.views.active_grid(self.view) {
                    self.views.grid(gs).mark_all();
                }
            }
            Action::Mark => {
                if let Some(gs) = self.views.active_grid(self.view) {
                    let g = self.views.grid(gs);
                    if matches!(g.current_row(), Some(GridRow::Group { .. })) {
                        g.toggle_group(None);
                    } else {
                        g.toggle_mark();
                        g.move_row(1);
                    }
                }
            }
            Action::Clear => self.clear_cell(),
            Action::Edit => self.start_edit(None),
            Action::SetPassword => {
                let uid = if self.view == views::USERS && self.views.users.pane == 0 { self.views.users.main.current_id() } else { None };
                let me = self.r.me_id();
                self.open_set_password(uid.unwrap_or(me));
            }
            Action::About => self.open_about(),
            Action::CardLeft | Action::CardRight | Action::Swimlanes | Action::Scope => self.board_action(a),
            Action::ZoomIn
            | Action::ZoomOut
            | Action::Today
            | Action::TimeBack
            | Action::TimeFwd
            | Action::ShrinkDue
            | Action::ExtendDue
            | Action::BarLeft
            | Action::BarRight => self.timeline_action(a),
            Action::Drill => self.dashboard_drill(),
        }
    }

    pub fn switch_view(&mut self, v: usize) {
        self.editor = None;
        self.focus_query = false;
        self.view = v.min(8);
    }

    fn next_pane(&mut self, back: bool) {
        let tg = match self.view {
            views::PROJECTS => {
                if !self.views.projects.show_detail {
                    self.views.projects.show_detail = true;
                }
                Some(&mut self.views.projects)
            }
            views::TEMPLATES => Some(&mut self.views.templates),
            views::USERS => Some(&mut self.views.users),
            _ => None,
        };
        match tg {
            Some(tg) => {
                // grid ⇄ detail ⇄ filter (ui-spec)
                let order = [0usize, 1, 2];
                let cur = if self.focus_query { 2 } else { tg.pane };
                let next = if back { order[(cur + 2) % 3] } else { order[(cur + 1) % 3] };
                if next == 2 {
                    self.focus_query = true;
                } else {
                    self.focus_query = false;
                    tg.pane = next;
                }
            }
            None => {
                if ui::has_query_bar(self.view) {
                    self.focus_query = !self.focus_query;
                }
            }
        }
    }

    // ---- query bar -------------------------------------------------------------------------

    fn query_key(&mut self, k: KeyEvent) {
        let qb = &mut self.queries[self.view];
        match k.code {
            KeyCode::Esc => {
                if qb.sugg.is_empty() {
                    self.focus_query = false;
                } else {
                    qb.sugg.clear();
                }
            }
            KeyCode::Enter => {
                if !qb.sugg.is_empty() && qb.sel > 0 {
                    self.accept_suggestion();
                } else {
                    qb.sugg.clear();
                    self.focus_query = false;
                }
            }
            KeyCode::Tab => {
                if qb.sugg.is_empty() {
                    self.focus_query = false;
                } else {
                    self.accept_suggestion();
                }
            }
            KeyCode::Down => {
                if !qb.sugg.is_empty() {
                    qb.sel = (qb.sel + 1) % (qb.sugg.len() + 1);
                } else {
                    self.focus_query = false;
                }
            }
            KeyCode::Up if !qb.sugg.is_empty() => qb.sel = (qb.sel + qb.sugg.len()) % (qb.sugg.len() + 1),
            KeyCode::F(6) => self.focus_query = false,
            _ => {
                if qb.input.handle(&k) {
                    self.update_suggestions();
                } else if let Some(a) = keymap::lookup(Ctx::Global, &k) {
                    self.focus_query = false;
                    self.action(a);
                }
            }
        }
    }

    /// The token being typed at the cursor.
    fn query_token(&self) -> (usize, String) {
        let qb = &self.queries[self.view];
        let text: Vec<char> = qb.input.text().chars().collect();
        let end = qb.input.cursor.min(text.len());
        let mut start = end;
        while start > 0 && !text[start - 1].is_whitespace() {
            start -= 1;
        }
        (start, text[start..end].iter().collect())
    }

    pub fn update_suggestions(&mut self) {
        let (_, tok) = self.query_token();
        let tok_l = tok.to_lowercase().trim_start_matches('-').to_string();
        let target = match self.view {
            views::PROJECTS | views::TIMELINE => Some(Target::Projects),
            views::TASKS | views::BOARD => Some(Target::Tasks),
            views::ACTIVITY => Some(Target::Activity),
            _ => None,
        };
        let mut out: Vec<String> = Vec::new();
        if let Some(target) = target {
            if let Some((field, val)) = tok_l.split_once(':') {
                let last = val.rsplit(',').next().unwrap_or("");
                let mut vals: Vec<String> = match field {
                    "status" if target == Target::Projects => ProjectStatus::NAMES.iter().map(|s| s.to_string()).collect(),
                    "status" => TaskStatus::NAMES.iter().map(|s| s.to_string()).collect(),
                    "priority" => Priority::NAMES.iter().map(|s| s.to_string()).collect(),
                    "health" => Health::NAMES.iter().map(|s| s.to_string()).collect(),
                    "owner" | "assignee" | "user" => {
                        let mut v: Vec<String> = vec!["me".into(), "none".into()];
                        v.extend(self.r.users.values().map(|u| u.username.clone()));
                        v
                    }
                    "program" => self.r.programs.values().map(|p| p.prefix.to_lowercase()).collect(),
                    "project" => self.r.projects.values().map(|p| p.code.to_lowercase()).collect(),
                    "tag" => self.r.tag_counts(if target == Target::Projects { Entity::Project } else { Entity::Task }).into_iter().map(|t| t.0).collect(),
                    "is" => query::is_values(target).iter().map(|s| s.to_string()).collect(),
                    "has" => query::has_values(target).iter().map(|s| s.to_string()).collect(),
                    "due" | "start" => ["today", "tom", "eow", "eom", "+7d", "-7d", "none"].iter().map(|s| s.to_string()).collect(),
                    "action" => vec!["set".into(), "create".into(), "delete".into()],
                    other => {
                        let ent = if target == Target::Projects { CfEntity::Project } else { CfEntity::Task };
                        self.r
                            .custom_fields_for(ent)
                            .into_iter()
                            .find(|c| c.query_name() == other)
                            .map(|c| c.options.iter().map(|o| o.to_lowercase()).collect())
                            .unwrap_or_default()
                    }
                };
                vals.retain(|v| v.starts_with(last) && v != last);
                let prefix = &tok[..tok.len() - last.len()];
                out = vals.into_iter().take(12).map(|v| format!("{prefix}{v}")).collect();
            } else if let Some(u) = tok_l.strip_prefix('@') {
                out = std::iter::once("me".to_string())
                    .chain(self.r.users.values().map(|x| x.username.clone()))
                    .filter(|n| n.starts_with(u) && n != u)
                    .map(|n| format!("@{n}"))
                    .take(10)
                    .collect();
            } else if let Some(tg) = tok_l.strip_prefix('#') {
                out = self
                    .r
                    .tag_counts(if target == Target::Projects { Entity::Project } else { Entity::Task })
                    .into_iter()
                    .filter(|(t, _)| t.starts_with(tg) && t != tg)
                    .map(|(t, _)| format!("#{t}"))
                    .take(10)
                    .collect();
            } else if !tok_l.is_empty() {
                let ent = if target == Target::Projects { CfEntity::Project } else { CfEntity::Task };
                let mut fields: Vec<String> = query::field_names(target).iter().map(|s| s.to_string()).collect();
                if target != Target::Activity {
                    fields.extend(self.r.custom_fields_for(ent).iter().map(|c| c.query_name()));
                }
                out = fields.into_iter().filter(|f| f.starts_with(&tok_l)).map(|f| format!("{f}:")).take(10).collect();
            }
        }
        let qb = &mut self.queries[self.view];
        qb.sugg = out;
        qb.sel = 0;
    }

    fn accept_suggestion(&mut self) {
        let (start, _) = self.query_token();
        let qb = &mut self.queries[self.view];
        let idx = if qb.sel == 0 { 0 } else { qb.sel - 1 };
        let Some(s) = qb.sugg.get(idx).cloned() else { return };
        let text: Vec<char> = qb.input.text().chars().collect();
        let end = qb.input.cursor.min(text.len());
        let neg = text.get(start) == Some(&'-');
        let mut new: String = text[..start].iter().collect();
        if neg {
            new.push('-');
        }
        new.push_str(&s);
        let rest: String = text[end..].iter().collect();
        let cursor_at = new.chars().count();
        if !s.ends_with(':') && !rest.starts_with(' ') {
            new.push(' ');
        }
        new.push_str(&rest);
        qb.input.set(&new);
        qb.input.cursor = cursor_at + usize::from(!s.ends_with(':'));
        self.update_suggestions();
    }

    // ---- grid editing ------------------------------------------------------------------------

    fn grid_key(&mut self, k: KeyEvent) {
        let Some(gs) = self.views.active_grid(self.view) else { return };
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let g = self.views.grid(gs);
        match k.code {
            KeyCode::Up if shift => g.extend_marks(-1),
            KeyCode::Down if shift => g.extend_marks(1),
            KeyCode::Up => g.move_row(-1),
            KeyCode::Down => g.move_row(1),
            KeyCode::Left => {
                if !g.toggle_group(Some(false)) {
                    g.move_col(-1);
                }
            }
            KeyCode::Right => {
                if !g.toggle_group(Some(true)) {
                    g.move_col(1);
                }
            }
            KeyCode::Tab => g.tab(true),
            KeyCode::BackTab => g.tab(false),
            KeyCode::Home if ctrl => g.set_row(0),
            KeyCode::End if ctrl => g.set_row(usize::MAX),
            KeyCode::Home => g.ccol = 0,
            KeyCode::End => g.ccol = g.vis_cols().len().saturating_sub(1),
            KeyCode::PageUp => {
                let p = g.page();
                g.move_row(-p);
            }
            KeyCode::PageDown => {
                let p = g.page();
                g.move_row(p);
            }
            KeyCode::Enter => {
                if matches!(g.current_row(), Some(GridRow::Group { .. })) {
                    g.toggle_group(None);
                } else if gs == GridSel::Activity {
                    self.activity_jump();
                } else {
                    self.start_edit(None);
                }
            }
            KeyCode::Esc => {
                let key = self.current_cell_key();
                let g = self.views.grid(gs);
                if let Some(key) = key.filter(|k| self.conflicts.contains_key(k)) {
                    self.conflicts.remove(&key);
                } else if !g.marks.is_empty() {
                    g.marks.clear();
                }
            }
            KeyCode::Backspace => self.start_edit(Some('\u{8}')),
            KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => {
                if c == '+' && matches!(g.current_row(), Some(GridRow::Group { .. })) {
                    g.set_all_collapsed(false);
                } else if c == '-' && matches!(g.current_row(), Some(GridRow::Group { .. })) {
                    g.set_all_collapsed(true);
                } else {
                    self.start_edit(Some(c));
                }
            }
            _ => {}
        }
    }

    pub fn current_cell_key(&self) -> Option<CellKey> {
        let gs = self.views.active_grid(self.view)?;
        let g = self.views.grid_ref(gs);
        Some((g.entity()?, g.current_id()?, g.current_col()?.field.clone()?))
    }

    /// Build the right editor for a column.
    pub fn make_editor(&self, e: Entity, id: i64, col: &Column, project_hint: Option<i64>) -> FieldEditor {
        let field = col.field.clone().unwrap_or_default();
        let r = &self.r;
        let today = r.today;
        let text = cg::edit_text(r, e, &field, id);
        let cur = r.field_value(e, id, &field);
        match col.editor {
            Editor::Text => FieldEditor::text(EdKind::Text, &text, today),
            Editor::Number => FieldEditor::text(EdKind::Number, &text, today),
            Editor::Int => FieldEditor::text(EdKind::Int, &text, today),
            Editor::Date => FieldEditor::text(EdKind::Date, &text, today),
            Editor::Multiline => FieldEditor::text(EdKind::Multiline, &text, today),
            Editor::Select => FieldEditor::list(EdKind::Select, cg::choices(r, e, &field, id, project_hint), cur, "", today).open(),
            Editor::Combo => FieldEditor::list(EdKind::Combo, cg::choices(r, e, &field, id, project_hint), cur, "", today).open(),
            Editor::Suggest => FieldEditor::list(EdKind::Suggest, cg::choices(r, e, &field, id, project_hint), Value::Null, &text, today).open(),
            Editor::Chips => {
                let vals = cur.as_array().cloned().unwrap_or_default();
                FieldEditor::chips(cg::choices(r, e, &field, id, project_hint), &vals, field != "deps", today).open()
            }
            Editor::Bool => FieldEditor::boolean(cur.as_bool().unwrap_or(false), today),
            Editor::None => FieldEditor::text(EdKind::Text, &text, today),
        }
    }

    /// Open the inline editor on the current cell (optionally seeded by a typed char).
    pub fn start_edit(&mut self, seed: Option<char>) {
        let Some(gs) = self.views.active_grid(self.view) else { return };
        let g = self.views.grid_ref(gs);
        let (Some(e), Some(id), Some(col)) = (g.entity(), g.current_id(), g.current_col().cloned()) else { return };
        let (row, vcol) = (g.cur, g.ccol);
        if !col.editable() {
            if seed.is_none() {
                self.set_status(format!("{} is calculated and can't be edited.", col.title), Level::Info);
            }
            return;
        }
        if !self.r.can_edit() {
            self.toast("Your account is read-only (viewer).".into(), Level::Err);
            return;
        }
        if !matches!(e, Entity::Project | Entity::Task) && !self.r.is_admin() {
            self.toast("Only admins can change this.".into(), Level::Err);
            return;
        }
        let field = col.field.clone().unwrap_or_default();
        if col.editor == Editor::Multiline {
            self.open_record(e, id, Some(field));
            return;
        }
        let mut ed = self.make_editor(e, id, &col, None);
        match seed {
            Some('\u{8}') => {
                if !ed.kind.is_list() {
                    ed.input.clear();
                }
            }
            Some(c) => ed.seed(c),
            None => {}
        }
        // a conflict is resolved by editing again from their (now current) value
        self.conflicts.remove(&(e, id, field.clone()));
        let base = self.r.field_value(e, id, &field);
        self.editor = Some(CellEdit { grid: gs, entity: e, id, field, base, row, vcol, ed });
    }

    fn editor_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Some(ce) = self.editor.as_mut() else { return };
        // ^S in an editor: save, forcing over a conflicting value
        if ctrl && k.code == KeyCode::Char('s') && ce.ed.kind != EdKind::Multiline {
            self.commit_edit(Nav::Stay, true);
            return;
        }
        match ce.ed.handle_key(&k) {
            EdOut::Commit(nav) => self.commit_edit(nav, false),
            EdOut::Cancel => self.editor = None,
            EdOut::Consumed => {}
            EdOut::Ignored => {
                // let global shortcuts (F1, ^Q…) through
                if let Some(a) = keymap::lookup(Ctx::Global, &k) {
                    if matches!(a, Action::Help | Action::Quit | Action::Undo | Action::Redo) {
                        self.editor = None;
                        self.action(a);
                    }
                }
            }
        }
    }

    pub fn commit_edit(&mut self, nav: Nav, force: bool) {
        let Some(ce) = self.editor.take() else { return };
        let value = match ce.ed.value() {
            EdValue::Value(v) => Ok(v),
            EdValue::Text(s) => cg::parse_input(&self.r, ce.entity, &ce.field, ce.id, &s),
        };
        match value {
            Err(msg) => {
                let mut ce = ce;
                ce.ed.error = Some(msg);
                self.editor = Some(ce);
                return;
            }
            Ok(v) => {
                if !pt_core::fields::values_equal(&v, &ce.base) || force {
                    let label = format!("{} {}", self.r.row_label(ce.entity, ce.id), self.field_label(ce.entity, &ce.field).to_lowercase());
                    let op = Op::Set { entity: ce.entity, id: ce.id, field: ce.field.clone(), base: ce.base.clone(), value: v, force };
                    self.mutate(vec![op], label, PendKind::Cells, None);
                }
            }
        }
        let g = self.views.grid(ce.grid);
        match nav {
            Nav::Down => g.move_row(1),
            Nav::Right => g.tab(true),
            Nav::Left => g.tab(false),
            Nav::Stay => {}
        }
    }

    fn clear_cell(&mut self) {
        let Some(gs) = self.views.active_grid(self.view) else { return };
        let g = self.views.grid_ref(gs);
        let (Some(e), Some(col)) = (g.entity(), g.current_col().cloned()) else { return };
        if !col.editable() {
            return;
        }
        let field = col.field.clone().unwrap_or_default();
        let ids = g.selection();
        let mut ops = vec![];
        for id in ids {
            let base = self.r.field_value(e, id, &field);
            match cg::parse_input(&self.r, e, &field, id, "") {
                Ok(v) if !pt_core::fields::values_equal(&v, &base) => ops.push(Op::set(e, id, &field, base, v)),
                Ok(_) => {}
                Err(_) => {
                    self.toast(format!("{} can't be empty.", col.title), Level::Warn);
                    return;
                }
            }
        }
        if let Err(msg) = self.check_meta(e, &field) {
            self.toast(msg, Level::Err);
            return;
        }
        let n = ops.len();
        self.mutate(ops, format!("cleared {} {}", n, col.title), PendKind::Cells, None);
    }

    fn check_meta(&self, e: Entity, field: &str) -> Result<(), String> {
        if let Some(m) = pt_core::fields::field(e, field) {
            if !m.nullable && matches!(m.kind, pt_core::fields::Kind::Text { min: 1.., .. }) {
                return Err(format!("{} can't be empty.", m.label));
            }
        }
        Ok(())
    }

    fn copy(&mut self) {
        let text = match self.views.active_grid(self.view) {
            Some(gs) => self.views.grid_ref(gs).copy_text(&self.r),
            None => return,
        };
        if text.is_empty() {
            return;
        }
        let lines = text.lines().count();
        self.clipboard = text.clone();
        osc52(&text);
        self.set_status(format!("Copied {} {} (also to the system clipboard where the terminal allows)", lines, if lines == 1 { "cell/row" } else { "rows" }), Level::Ok);
    }

    /// Paste TSV at the cursor, filling right and down; one mutate.
    pub fn on_paste(&mut self, s: &str) {
        // terminals send pasted newlines as CR (or CRLF)
        let normalized = s.replace("\r\n", "\n").replace('\r', "\n");
        let s = normalized.as_str();
        if let Some(d) = self.dialogs.last_mut() {
            d.paste(s);
            return;
        }
        if let Some(ce) = self.editor.as_mut() {
            ce.ed.paste(s);
            return;
        }
        if self.focus_query {
            self.queries[self.view].input.insert_str(s.lines().next().unwrap_or(""));
            self.update_suggestions();
            return;
        }
        let Some(gs) = self.views.active_grid(self.view) else { return };
        let g = self.views.grid_ref(gs);
        let Some(e) = g.entity() else { return };
        let rows = g.data_rows_from_cursor();
        let cols: Vec<Column> = g.vis_cols().into_iter().skip(g.ccol).cloned().collect();
        let lines: Vec<&str> = s.trim_end_matches('\n').split('\n').collect();
        if lines.len() == 1 && !lines[0].contains('\t') {
            // a single value into one cell
            if let (Some(id), Some(col)) = (rows.first(), cols.first()) {
                if col.editable() {
                    let field = col.field.clone().unwrap_or_default();
                    match cg::parse_input(&self.r, e, &field, *id, lines[0]) {
                        Ok(v) => {
                            let base = self.r.field_value(e, *id, &field);
                            let label = format!("paste into {}", col.title);
                            self.mutate(vec![Op::set(e, *id, &field, base, v)], label, PendKind::Cells, None);
                        }
                        Err(m) => self.toast(format!("✗ {m}"), Level::Err),
                    }
                }
            }
            return;
        }
        let mut ops = vec![];
        let mut errors = vec![];
        for (line, id) in lines.iter().zip(rows.iter()) {
            for (val, col) in line.split('\t').zip(cols.iter()) {
                if !col.editable() {
                    continue;
                }
                let field = col.field.clone().unwrap_or_default();
                match cg::parse_input(&self.r, e, &field, *id, val) {
                    Ok(v) => {
                        let base = self.r.field_value(e, *id, &field);
                        if !pt_core::fields::values_equal(&v, &base) {
                            ops.push(Op::set(e, *id, &field, base, v));
                        }
                    }
                    Err(m) => errors.push(m),
                }
            }
        }
        if !errors.is_empty() {
            self.toast(format!("✗ {} value(s) skipped: {}", errors.len(), errors[0]), Level::Warn);
        }
        let n = ops.len();
        if n == 0 && errors.is_empty() {
            self.set_status("Nothing to change.".into(), Level::Info);
        }
        self.mutate(ops, format!("paste ({n} cells)"), PendKind::Cells, None);
    }

    fn fill_down(&mut self) {
        let Some(gs) = self.views.active_grid(self.view) else { return };
        let g = self.views.grid_ref(gs);
        let (Some(e), Some(col)) = (g.entity(), g.current_col().cloned()) else { return };
        if !col.editable() {
            self.set_status(format!("{} is calculated.", col.title), Level::Info);
            return;
        }
        let field = col.field.clone().unwrap_or_default();
        let (src, targets): (Option<i64>, Vec<i64>) = if g.marks.len() >= 2 {
            let sel = g.selection();
            (sel.first().copied(), sel[1..].to_vec())
        } else {
            let prev = g.rows[..g.cur.min(g.rows.len())].iter().rev().find_map(GridRow::id);
            (prev, g.current_id().into_iter().collect())
        };
        let Some(src) = src else { return };
        let v = self.r.field_value(e, src, &field);
        let ops: Vec<Op> = targets
            .iter()
            .filter(|id| **id != src)
            .map(|id| Op::set(e, *id, &field, self.r.field_value(e, *id, &field), v.clone()))
            .filter(|o| matches!(o, Op::Set { base, value, .. } if !pt_core::fields::values_equal(base, value)))
            .collect();
        let n = ops.len();
        self.mutate(ops, format!("fill down {} ({n} rows)", col.title), PendKind::Cells, None);
    }

    fn delete_selected(&mut self) {
        let (e, ids) = match self.view {
            views::BOARD => (Entity::Task, self.views.board.current.into_iter().collect::<Vec<_>>()),
            _ => {
                let Some(gs) = self.views.active_grid(self.view) else { return };
                let g = self.views.grid_ref(gs);
                match g.entity() {
                    Some(e) if g.kind != GridKind::Activity => (e, g.selection()),
                    _ => return,
                }
            }
        };
        if ids.is_empty() {
            return;
        }
        if e == Entity::User {
            self.toast("Users can't be deleted — set Active to no instead.".into(), Level::Info);
            return;
        }
        let what = if ids.len() == 1 { self.r.row_label(e, ids[0]) } else { format!("{} {}s", ids.len(), e.label()) };
        let extra = match e {
            Entity::Project => {
                let n: usize = ids.iter().map(|id| self.r.project_tasks(*id).len()).sum();
                format!(" and its {n} task(s)")
            }
            Entity::Program | Entity::Template | Entity::CustomField => " (this can't be undone)".into(),
            _ => String::new(),
        };
        let ops: Vec<Op> = ids.iter().map(|id| Op::Delete { entity: e, id: *id }).collect();
        self.open_confirm(format!("Delete {what}{extra}?"), dialogs::ConfirmAction::Mutate(ops, format!("deleted {what}")));
    }

    /// ^E: record form for the current row.
    fn edit_record(&mut self) {
        let target = match self.view {
            views::BOARD => self.views.board.current.map(|id| (Entity::Task, id)),
            views::TIMELINE => self.views.timeline.current_entity(),
            _ => self.views.active_grid(self.view).and_then(|gs| {
                let g = self.views.grid_ref(gs);
                Some((g.entity()?, g.current_id()?))
            }),
        };
        if let Some((e, id)) = target {
            self.open_record(e, id, None);
        }
    }

    fn new_record(&mut self) {
        match self.view {
            views::PROJECTS if self.views.projects.pane == 1 => self.open_new_task(),
            views::PROJECTS | views::TIMELINE => self.open_new_project(),
            views::TASKS | views::BOARD => self.open_new_task(),
            views::TEMPLATES if self.views.templates.pane == 1 => {
                if let Some(t) = self.views.templates.main.current_id() {
                    self.open_create(Entity::TemplateTask, Some(t));
                }
            }
            views::TEMPLATES => self.open_create(Entity::Template, None),
            views::FIELDS => self.open_create(Entity::CustomField, None),
            views::USERS if self.views.users.pane == 1 => self.open_create(Entity::Program, None),
            views::USERS => self.open_create(Entity::User, None),
            _ => {}
        }
    }

    fn activity_jump(&mut self) {
        let Some(id) = self.views.activity.current_id() else { return };
        let Some(a) = self.r.activity.get(&id) else { return };
        let Some(e) = Entity::parse(&a.entity) else { return };
        if self.r.exists(e, a.entity_id) {
            let field = a.field.clone();
            self.reveal(e, a.entity_id);
            if let Some(gs) = self.views.active_grid(self.view) {
                if !field.is_empty() {
                    self.views.grid(gs).select_col_key(&field);
                }
            }
        } else {
            self.toast("That record no longer exists.".into(), Level::Info);
        }
    }

    /// Forget everything tied to the previous session.
    fn clear_session_state(&mut self) {
        self.editor = None;
        self.menu = None;
        self.focus_query = false;
        self.pending.clear();
        self.pending_cells.clear();
        self.conflicts.clear();
        self.flashes.clear();
        self.select_after = None;
        self.focus_sent = None;
        self.status = None;
        for q in &mut self.queries {
            *q = QueryBar::default();
        }
    }

    pub fn logout(&mut self) {
        if let Some(token) = self.token.take() {
            let (url, tls, tx) = (self.url.clone(), self.tls.clone(), self.internal_tx.clone());
            tokio::spawn(async move {
                let _ = net::logout(&url, &tls, &token).await;
                let _ = tx.send(Internal::LoggedOut);
            });
        }
        let _ = net::forget(&self.url);
        self.conn = None;
        self.r = Replica::new(today());
        self.views = Views::new(&self.r);
        self.clear_session_state();
        self.show_login(Some("Logged out.".into()));
    }

    // ---- mouse -----------------------------------------------------------------------------

    pub fn is_double(&mut self, x: u16, y: u16) -> bool {
        let now = Instant::now();
        let dbl = self.last_click.is_some_and(|(t, lx, ly)| now.duration_since(t) < Duration::from_millis(450) && lx == x && ly == y);
        self.last_click = if dbl { None } else { Some((now, x, y)) };
        dbl
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        if let MouseEventKind::Moved = m.kind {
            let changed = self.hover != Some((x, y));
            self.hover = Some((x, y));
            if !changed || self.menu.is_none() && self.dialogs.is_empty() {
                // only menus and dialogs react to hover
                return;
            }
        }
        if !self.dialogs.is_empty() {
            self.dialog_mouse(m);
            return;
        }
        if self.phase == Phase::Login {
            return;
        }
        if self.menu.is_some() {
            self.menu_mouse(m);
            return;
        }
        // drags in progress
        if let Some(drag) = &self.drag {
            match m.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Drag::ResizeCol { grid, vcol, x0, w0 } = *drag {
                        let w = (w0 as i32 + x as i32 - x0 as i32).max(3) as u16;
                        self.views.grid(grid).resize_col(vcol, w);
                    } else {
                        self.drag_move(x, y);
                    }
                    return;
                }
                MouseEventKind::Up(_) => {
                    self.drag_end(x, y);
                    self.drag = None;
                    return;
                }
                _ => {}
            }
        }
        if let Some(ce) = self.editor.as_mut() {
            match m.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(out) = ce.ed.click(x, y) {
                        if let EdOut::Commit(nav) = out {
                            self.commit_edit(nav, false);
                        }
                        return;
                    }
                    // click outside: commit and continue with the click
                    self.commit_edit(Nav::Stay, false);
                }
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    ce.ed.scroll_list(matches!(m.kind, MouseEventKind::ScrollDown));
                    return;
                }
                _ => return,
            }
        }
        if let MouseEventKind::Down(MouseButton::Left) = m.kind {
            // chrome
            if let Some(&(_, i)) = self.hits.menus.iter().find(|(r, _)| rect_contains(*r, x, y)) {
                self.menu = Some(ui::MenuState { open: i, sel: 0 });
                return;
            }
            if let Some(&(_, v)) = self.hits.tabs.iter().find(|(r, _)| rect_contains(*r, x, y)) {
                self.switch_view(v);
                return;
            }
            if let Some(&(_, a)) = self.hits.hints.iter().find(|(r, _)| rect_contains(*r, x, y)) {
                self.action(a);
                return;
            }
            if rect_contains(self.hits.query, x, y) && ui::has_query_bar(self.view) {
                self.focus_query = true;
                self.update_suggestions();
                return;
            }
            if rect_contains(self.hits.toasts, x, y) {
                self.toasts.clear();
                return;
            }
        }
        if rect_contains(self.hits.main, x, y) || matches!(m.kind, MouseEventKind::ScrollDown | MouseEventKind::ScrollUp) {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                self.focus_query = false;
            }
            match self.view {
                views::BOARD => self.board_mouse(m),
                views::TIMELINE => self.timeline_mouse(m),
                views::DASHBOARD => self.dashboard_mouse(m),
                _ => self.grid_mouse(m),
            }
        }
    }

    /// Which grid (and pane) is under the mouse.
    fn grid_at(&self, x: u16, y: u16) -> Option<(GridSel, usize)> {
        let candidates: &[(GridSel, usize)] = match self.view {
            views::PROJECTS => &[(GridSel::Projects, 0), (GridSel::ProjectTasks, 1)],
            views::TASKS => &[(GridSel::Tasks, 0)],
            views::ACTIVITY => &[(GridSel::Activity, 0)],
            views::TEMPLATES => &[(GridSel::Templates, 0), (GridSel::TemplateTasks, 1)],
            views::FIELDS => &[(GridSel::Fields, 0)],
            views::USERS => &[(GridSel::Users, 0), (GridSel::Programs, 1)],
            _ => &[],
        };
        candidates.iter().copied().find(|(g, _)| rect_contains(self.views.grid_ref(*g).layout.area, x, y))
    }

    fn set_pane(&mut self, pane: usize) {
        match self.view {
            views::PROJECTS => self.views.projects.pane = pane,
            views::TEMPLATES => self.views.templates.pane = pane,
            views::USERS => self.views.users.pane = pane,
            _ => {}
        }
    }

    fn grid_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        let Some((gs, pane)) = self.grid_at(x, y) else { return };
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        match m.kind {
            MouseEventKind::ScrollDown => self.views.grid(gs).scroll(3),
            MouseEventKind::ScrollUp => self.views.grid(gs).scroll(-3),
            MouseEventKind::Down(btn) => {
                self.set_pane(pane);
                let hit = self.views.grid_ref(gs).hit(x, y);
                let dbl = btn == MouseButton::Left && self.is_double(x, y);
                let g = self.views.grid(gs);
                match hit {
                    Hit::Header(c) if btn == MouseButton::Left => {
                        if let Some(key) = g.vis_cols().get(c).map(|c| c.key.clone()) {
                            g.cycle_sort(&key, shift);
                        }
                    }
                    Hit::Border(c) if btn == MouseButton::Left => {
                        let w0 = g.vis_cols().get(c).map_or(10, |c| c.width);
                        self.drag = Some(Drag::ResizeCol { grid: gs, vcol: c, x0: x, w0 });
                    }
                    Hit::Gutter(row) => {
                        g.set_row(row);
                        if matches!(g.current_row(), Some(GridRow::Group { .. })) {
                            g.toggle_group(None);
                        } else {
                            g.toggle_mark();
                        }
                    }
                    Hit::Cell(row, c) => {
                        g.set_row(row);
                        g.ccol = c;
                        if matches!(g.current_row(), Some(GridRow::Group { .. })) {
                            if btn == MouseButton::Left {
                                g.toggle_group(None);
                            }
                        } else if btn == MouseButton::Right {
                            self.open_context_at(x, y);
                        } else if dbl {
                            if gs == GridSel::Activity {
                                self.activity_jump();
                            } else {
                                self.start_edit(None);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn drag_move(&mut self, x: u16, y: u16) {
        match self.view {
            views::BOARD => self.board_drag(x, y),
            views::TIMELINE => self.timeline_drag(x, y),
            _ => {}
        }
    }

    fn drag_end(&mut self, x: u16, y: u16) {
        match self.view {
            views::BOARD => self.board_drop(x, y),
            views::TIMELINE => self.timeline_drop(x, y),
            _ => {}
        }
    }

    // ---- saved filters ------------------------------------------------------------------------

    pub fn save_filter(&mut self, name: String) {
        let q = self.queries[self.view].input.text();
        self.saved_filters.retain(|f| !(f.name == name && f.view == self.view));
        self.saved_filters.push(SavedFilter { name: name.clone(), view: self.view, query: q });
        if let Err(e) = store_filters(&self.saved_filters) {
            self.toast(format!("Couldn't save filters: {e}"), Level::Err);
        } else {
            self.set_status(format!("Saved filter “{name}”"), Level::Ok);
        }
    }

    pub fn apply_filter(&mut self, i: usize) {
        if let Some(f) = self.saved_filters.get(i).cloned() {
            self.switch_view(f.view);
            self.queries[f.view].input.set(&f.query);
        }
    }
}

async fn recv_net(conn: &mut Option<Connection>) -> Option<NetEvent> {
    match conn {
        Some(c) => c.rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Copy to the system clipboard via OSC 52 (Ghostty, Windows Terminal, iTerm2, kitty…).
pub fn osc52(text: &str) {
    use base64::Engine;
    if cfg!(test) {
        return;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{b64}\x07");
    let _ = out.flush();
}

fn filters_path() -> Option<std::path::PathBuf> {
    net::sessions_path().map(|p| p.with_file_name("filters.json"))
}

fn load_filters() -> Vec<SavedFilter> {
    filters_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn store_filters(f: &[SavedFilter]) -> anyhow::Result<()> {
    let p = filters_path().ok_or_else(|| anyhow::anyhow!("no config directory"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    net::write_private(&p, serde_json::to_string_pretty(f)?.as_bytes())
}
