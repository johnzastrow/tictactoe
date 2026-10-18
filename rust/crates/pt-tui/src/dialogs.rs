//! Dialogs (ui-spec §5): forms (login, cascading New project / New task,
//! record form with per-field conflicts, admin create forms, set password,
//! save filter), confirm, columns, sort, command palette, help and the
//! context menu. Dialogs stack; the top one gets all input.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use pt_core::fields::{self, Kind};
use pt_core::fuzzy;
use pt_core::grid::{self as cg, Column, Editor, SortSpec};
use pt_core::model::*;
use pt_core::protocol::{ClientMsg, Op, ResultMsg};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::{Map, Value, json};

use crate::app::{App, Internal, Level, PendKind};
use crate::keymap::{self, Action, Ctx};
use crate::net::{self, ServerUrl};
use crate::theme::{self, Theme};
use crate::views::{self, GridSel};
use crate::widgets::draw::{centered, fill, frame, modal, pad, put, put_highlighted, rect_contains, shadow, width};
use crate::widgets::editor::{EdKind, EdOut, EdValue, FieldEditor, Nav};
use crate::widgets::input::TextInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    Login,
    NewProject,
    NewTask,
    Record(Entity, i64),
    Create(Entity),
    SetPassword(i64, bool),
    SaveFilter,
    BoardScope,
}

pub struct FormField {
    pub key: String,
    pub label: String,
    pub ed: FieldEditor,
    pub required: bool,
    pub error: Option<String>,
    pub base: Value,
    pub conflict: Option<(Value, Option<i64>)>,
    pub note: String,
    pub height: u16,
    pub rect: Rect,
}

impl FormField {
    fn new(key: &str, label: &str, ed: FieldEditor) -> FormField {
        let height = if ed.kind == EdKind::Multiline { 5 } else { 1 };
        FormField { key: key.into(), label: label.into(), ed, required: false, error: None, base: Value::Null, conflict: None, note: String::new(), height, rect: Rect::default() }
    }
    fn req(mut self) -> Self {
        self.required = true;
        self
    }
}

pub struct Form {
    pub kind: FormKind,
    pub title: String,
    pub fields: Vec<FormField>,
    pub focus: usize,
    pub message: Option<(String, bool)>,
    pub busy: bool,
    pub awaiting: Option<u64>,
    pub footer: Vec<String>,
    /// Values we filled in automatically (so cascades don't overwrite user edits).
    pub autos: HashMap<String, String>,
    pub extra: Option<i64>,
    scroll: usize,
    buttons: [Rect; 2],
    box_rect: Rect,
}

impl Form {
    fn new(kind: FormKind, title: &str, fields: Vec<FormField>) -> Form {
        Form {
            kind,
            title: title.into(),
            fields,
            focus: 0,
            message: None,
            busy: false,
            awaiting: None,
            footer: vec![],
            autos: HashMap::new(),
            extra: None,
            scroll: 0,
            buttons: [Rect::default(); 2],
            box_rect: Rect::default(),
        }
    }

    fn field(&mut self, key: &str) -> Option<&mut FormField> {
        self.fields.iter_mut().find(|f| f.key == key)
    }

    fn field_ref(&self, key: &str) -> Option<&FormField> {
        self.fields.iter().find(|f| f.key == key)
    }

    fn text(&self, key: &str) -> String {
        self.field_ref(key).map(|f| f.ed.input.text()).unwrap_or_default()
    }

    fn value(&self, key: &str) -> Value {
        self.field_ref(key).map_or(Value::Null, |f| match f.ed.value() {
            EdValue::Value(v) => v,
            EdValue::Text(s) => Value::String(s),
        })
    }

    /// Set a text field unless the user changed it from our last automatic value.
    fn set_auto(&mut self, key: &str, val: &str) {
        let current = self.text(key);
        let auto = self.autos.get(key).cloned().unwrap_or_default();
        if current.is_empty() || current == auto {
            if let Some(f) = self.field(key) {
                f.ed.input.set(val);
            }
            self.autos.insert(key.into(), val.into());
        }
    }

    fn n(&self) -> usize {
        self.fields.len()
    }
}

#[derive(Clone, Debug)]
pub enum ConfirmAction {
    Mutate(Vec<Op>, String),
}

pub struct Confirm {
    pub text: String,
    pub action: ConfirmAction,
    pub sel: usize, // 0 = Cancel (default), 1 = OK
    buttons: [Rect; 2],
}

#[derive(Clone, Debug)]
pub enum PalAct {
    Action(Action),
    Project(i64),
    Task(i64),
    Theme(usize),
    Filter(usize),
}

pub struct Palette {
    pub input: TextInput,
    pub items: Vec<(String, String, PalAct)>,
    pub filtered: Vec<(usize, Vec<usize>)>,
    pub sel: usize,
    scroll: usize,
    rows: Rect,
}

pub struct Help {
    pub input: TextInput,
    pub scroll: usize,
    lines: Vec<(String, bool)>,
}

#[derive(Clone, Debug)]
pub enum CtxAct {
    Action(Action),
    FilterBy(bool),
    Sort(bool),
    HideColumn,
}

pub struct ContextMenu {
    pub x: u16,
    pub y: u16,
    pub items: Vec<(String, String, CtxAct)>,
    pub sel: usize,
    rect: Rect,
}

pub struct Columns {
    pub grid: GridSel,
    pub sel: usize,
    rows: Rect,
}

pub struct Sort {
    pub grid: GridSel,
    pub keys: Vec<(usize, bool)>, // (column index, desc)
    pub sel: usize,
}

pub struct Message {
    pub title: String,
    pub lines: Vec<String>,
}

pub enum Dialog {
    Form(Box<Form>),
    Confirm(Confirm),
    Columns(Columns),
    Sort(Sort),
    Palette(Palette),
    Help(Help),
    Context(ContextMenu),
    Message(Message),
}

impl Dialog {
    pub fn paste(&mut self, s: &str) {
        match self {
            Dialog::Form(f) => {
                if let Some(fl) = f.fields.get_mut(f.focus) {
                    fl.ed.paste(s);
                }
            }
            Dialog::Palette(p) => p.input.insert_str(s.lines().next().unwrap_or("")),
            Dialog::Help(h) => h.input.insert_str(s.lines().next().unwrap_or("")),
            _ => {}
        }
    }

    /// Hints for the bottom bar while this dialog is on top.
    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            Dialog::Form(f) => {
                if let Some(fl) = f.fields.get(f.focus) {
                    let mut v = vec![("Tab", "Next field"), ("^S", "Save"), ("Esc", "Cancel")];
                    match fl.ed.kind {
                        EdKind::Select | EdKind::Combo if !fl.ed.list_open => v.extend([("Enter", "Open list"), ("↑↓", "Change")]),
                        k => v.extend(keymap::editor_hints(k).into_iter().filter(|(k, _)| !matches!(*k, "Esc" | "Tab"))),
                    }
                    if fl.conflict.is_some() {
                        v.insert(0, ("Enter", "Keep theirs"));
                    }
                    v
                } else {
                    vec![("Enter", "Press button"), ("Tab", "Next"), ("Esc", "Cancel")]
                }
            }
            Dialog::Confirm(_) => vec![("Y", "Yes"), ("N/Esc", "No"), ("←→", "Choose"), ("Enter", "OK")],
            Dialog::Columns(_) => vec![("Space", "Show/hide"), ("⇧↑↓ [ ]", "Move"), ("+ -", "Width"), ("R", "Reset"), ("Esc", "Done")],
            Dialog::Sort(_) => vec![("↑↓", "Key"), ("←→", "Column"), ("Space", "▲/▼"), ("Del", "Remove"), ("Enter", "Apply"), ("Esc", "Cancel")],
            Dialog::Palette(_) => vec![("type", "Search"), ("↑↓", "Choose"), ("Enter", "Run"), ("Esc", "Close")],
            Dialog::Help(_) => vec![("type", "Search keys"), ("↑↓ PgUp PgDn", "Scroll"), ("Esc", "Close")],
            Dialog::Context(_) => vec![("↑↓", "Choose"), ("Enter", "Run"), ("Esc", "Close")],
            Dialog::Message(_) => vec![("Esc/Enter", "Close")],
        }
    }
}

// ---- building dialogs -------------------------------------------------------------------

impl App {
    pub fn show_login(&mut self, message: Option<String>) {
        self.phase = crate::app::Phase::Login;
        self.dialogs.clear();
        let today = self.r.today;
        let user = if self.username.is_empty() { String::new() } else { self.username.clone() };
        let mut f = Form::new(
            FormKind::Login,
            "Log in to pt",
            vec![
                FormField::new("server", "Server", FieldEditor::text(EdKind::Text, &self.url.base(), today)).req(),
                FormField::new("username", "Username", FieldEditor::text(EdKind::Text, &user, today)).req(),
                FormField::new("password", "Password", FieldEditor::text(EdKind::Password, "", today)).req(),
                FormField::new("remember", "Remember me", FieldEditor::boolean(true, today)),
            ],
        );
        f.focus = if user.is_empty() { 1 } else { 2 };
        f.message = message.map(|m| (m, false));
        f.footer = vec!["Demo data: user admin / alice / bob … with the password printed by `pt seed`.".into()];
        self.dialogs.push(Dialog::Form(Box::new(f)));
    }

    /// Board scope: cascading Program ▸ Project.
    pub fn open_board_scope(&mut self) {
        let r = &self.r;
        let today = r.today;
        let mut programs = cg::choices(r, Entity::Project, "program_id", 0, None);
        if let Some(first) = programs.first_mut() {
            first.label = "All programs".into();
        }
        let b = &self.views.board;
        let mut f = Form::new(
            FormKind::BoardScope,
            "Board scope",
            vec![
                FormField::new("program", "Program", FieldEditor::list(EdKind::Combo, programs, b.program.map_or(Value::Null, Value::from), "", today)),
                FormField::new("project", "Project", FieldEditor::list(EdKind::Combo, vec![], b.project.map_or(Value::Null, Value::from), "", today)),
            ],
        );
        f.footer = vec!["Picking a program narrows the project list. Esc on the board clears the scope.".into()];
        self.cascade(&mut f, None);
        self.dialogs.push(Dialog::Form(Box::new(f)));
    }

    pub fn open_help(&mut self) {
        let mut h = Help { input: TextInput::default(), scroll: 0, lines: vec![] };
        h.lines = help_lines();
        self.dialogs.push(Dialog::Help(h));
    }

    pub fn open_about(&mut self) {
        let srv = self.r.server.as_ref().map(|s| format!("{} {}", s.implementation, s.version)).unwrap_or_else(|| "?".into());
        let me = self.r.me.as_ref().map(|u| format!("{} ({}, {})", u.display_name, u.username, u.role.label())).unwrap_or_default();
        self.dialogs.push(Dialog::Message(Message {
            title: "About pt".into(),
            lines: vec![
                "pt — projects & tasks".into(),
                String::new(),
                format!("Client:   Rust TUI (ratatui) {}", pt_core::VERSION),
                format!("Server:   {}  at {}", srv, self.url.base()),
                format!("Protocol: {}", pt_core::protocol::PROTOCOL_VERSION),
                format!("You:      {me}"),
                format!("Colour:   {} · theme {}", if theme::is_truecolor() { "truecolor" } else { "256 colours" }, self.t.name),
                format!("Data:     {} projects, {} tasks, {} users", self.r.projects.len(), self.r.tasks.len(), self.r.users.len()),
            ],
        }));
    }

    pub fn open_confirm(&mut self, text: String, action: ConfirmAction) {
        self.dialogs.push(Dialog::Confirm(Confirm { text, action, sel: 0, buttons: [Rect::default(); 2] }));
    }

    pub fn open_columns(&mut self, grid: GridSel) {
        self.dialogs.push(Dialog::Columns(Columns { grid, sel: 0, rows: Rect::default() }));
    }

    pub fn open_sort(&mut self, grid: GridSel) {
        let g = self.views.grid_ref(grid);
        let keys = g.sort.iter().filter_map(|s| g.cols.iter().position(|c| c.key == s.key).map(|i| (i, s.desc))).collect();
        self.dialogs.push(Dialog::Sort(Sort { grid, keys, sel: 0 }));
    }

    pub fn open_save_filter(&mut self) {
        if !crate::ui::has_query_bar(self.view) {
            return;
        }
        let f = Form::new(
            FormKind::SaveFilter,
            "Save filter",
            vec![FormField::new("name", "Name", FieldEditor::text(EdKind::Text, "", self.r.today)).req()],
        );
        let mut f = f;
        f.footer = vec![format!("Filter: {}", self.queries[self.view].input.text())];
        self.dialogs.push(Dialog::Form(Box::new(f)));
    }

    pub fn open_palette(&mut self) {
        let mut items: Vec<(String, String, PalAct)> = Vec::new();
        let ctx = self.ctx();
        let acts = [
            Action::NewProject,
            Action::NewTask,
            Action::Undo,
            Action::Redo,
            Action::Filter,
            Action::ClearFilter,
            Action::SaveFilter,
            Action::Columns,
            Action::ContextMenu,
            Action::SortDialog,
            Action::SortColumn,
            Action::GroupColumn,
            Action::ClearGroup,
            Action::ToggleDetail,
            Action::Copy,
            Action::FillDown,
            Action::EditRecord,
            Action::Delete,
            Action::Theme,
            Action::SetPassword,
            Action::Help,
            Action::About,
            Action::Logout,
            Action::Quit,
        ];
        for (i, name) in keymap::VIEW_NAMES.iter().enumerate() {
            items.push((format!("Go to {name}"), format!("Alt+{}", i + 1), PalAct::Action(Action::View(i as u8))));
        }
        for a in acts {
            items.push((a.label().trim_end_matches('…').to_string(), keymap::key_for(a, ctx).unwrap_or_default(), PalAct::Action(a)));
        }
        for (i, p) in theme::PALETTES.iter().enumerate() {
            items.push((format!("Theme: {}", p.name), String::new(), PalAct::Theme(i)));
        }
        for (i, f) in self.saved_filters.iter().enumerate() {
            items.push((format!("Filter: {} ({})", f.name, keymap::VIEW_NAMES[f.view]), f.query.clone(), PalAct::Filter(i)));
        }
        for p in self.r.projects.values() {
            items.push((format!("{}  {}", p.code, p.name), format!("project · {}", self.r.program_name(p.program_id)), PalAct::Project(p.id)));
        }
        for t in self.r.tasks.values() {
            items.push((format!("{}  {}", self.r.task_ref(t), t.title), format!("task · {}", t.status.label()), PalAct::Task(t.id)));
        }
        let mut p = Palette { input: TextInput::default(), items, filtered: vec![], sel: 0, scroll: 0, rows: Rect::default() };
        refilter_palette(&mut p);
        self.dialogs.push(Dialog::Palette(p));
    }

    pub fn open_context_here(&mut self) {
        let pos = self.views.active_grid(self.view).and_then(|gs| {
            let g = self.views.grid_ref(gs);
            g.cell_rect(g.cur, g.ccol).map(|r| (r.x + 2, r.y + 1))
        });
        let (x, y) = pos.unwrap_or((self.hits.main.x + 4, self.hits.main.y + 2));
        self.open_context_at(x, y);
    }

    pub fn open_context_at(&mut self, x: u16, y: u16) {
        let ctx = self.ctx();
        let k = |a: Action| keymap::key_for(a, ctx).unwrap_or_default();
        let mut items: Vec<(String, String, CtxAct)> = Vec::new();
        let grid = self.views.active_grid(self.view);
        let col_title = grid.and_then(|g| self.views.grid_ref(g).current_col().map(|c| c.title.clone())).unwrap_or_default();
        match self.view {
            views::BOARD => {
                items.push(("Record form".into(), k(Action::EditRecord), CtxAct::Action(Action::EditRecord)));
                items.push(("Move card ◂".into(), k(Action::CardLeft), CtxAct::Action(Action::CardLeft)));
                items.push(("Move card ▸".into(), k(Action::CardRight), CtxAct::Action(Action::CardRight)));
                items.push(("New task".into(), k(Action::New), CtxAct::Action(Action::New)));
                items.push(("Delete task".into(), k(Action::Delete), CtxAct::Action(Action::Delete)));
            }
            views::TIMELINE => {
                items.push(("Record form".into(), k(Action::EditRecord), CtxAct::Action(Action::EditRecord)));
                items.push(("Move ◂".into(), k(Action::BarLeft), CtxAct::Action(Action::BarLeft)));
                items.push(("Move ▸".into(), k(Action::BarRight), CtxAct::Action(Action::BarRight)));
                items.push(("Jump to today".into(), k(Action::Today), CtxAct::Action(Action::Today)));
            }
            _ if grid == Some(GridSel::Activity) => {
                items.push(("Go to record".into(), "Enter".into(), CtxAct::Action(Action::EditRecord)));
                items.push(("Copy".into(), k(Action::Copy), CtxAct::Action(Action::Copy)));
                items.push(("Filter by this value".into(), String::new(), CtxAct::FilterBy(false)));
                items.push(("Undo my last change".into(), k(Action::Undo), CtxAct::Action(Action::Undo)));
            }
            _ => {
                items.push(("Edit".into(), k(Action::Edit), CtxAct::Action(Action::Edit)));
                items.push(("Record form".into(), k(Action::EditRecord), CtxAct::Action(Action::EditRecord)));
                items.push(("Copy".into(), k(Action::Copy), CtxAct::Action(Action::Copy)));
                items.push(("Paste".into(), k(Action::Paste), CtxAct::Action(Action::Paste)));
                items.push(("Fill down".into(), k(Action::FillDown), CtxAct::Action(Action::FillDown)));
                items.push(("Clear".into(), "Del".into(), CtxAct::Action(Action::Clear)));
                if crate::ui::has_query_bar(self.view) {
                    items.push(("Filter by this value".into(), String::new(), CtxAct::FilterBy(false)));
                    items.push(("Exclude this value".into(), String::new(), CtxAct::FilterBy(true)));
                }
                items.push(("Sort ▲".into(), String::new(), CtxAct::Sort(false)));
                items.push(("Sort ▼".into(), String::new(), CtxAct::Sort(true)));
                items.push((format!("Group by {col_title}"), k(Action::GroupColumn), CtxAct::Action(Action::GroupColumn)));
                items.push((format!("Hide column {col_title}"), String::new(), CtxAct::HideColumn));
                if self.view == views::USERS && self.views.users.pane == 0 {
                    items.push(("Set password…".into(), String::new(), CtxAct::Action(Action::SetPassword)));
                }
                items.push(("Delete row(s)".into(), k(Action::Delete), CtxAct::Action(Action::Delete)));
            }
        }
        self.dialogs.push(Dialog::Context(ContextMenu { x, y, items, sel: 0, rect: Rect::default() }));
    }

    pub fn open_set_password(&mut self, user_id: i64) {
        let me = self.r.me_id();
        let need_old = user_id == me && !self.r.is_admin();
        if user_id != me && !self.r.is_admin() {
            self.toast("Only admins can set other people's passwords.".into(), Level::Err);
            return;
        }
        let today = self.r.today;
        let mut fields = vec![];
        if need_old {
            fields.push(FormField::new("old", "Current password", FieldEditor::text(EdKind::Password, "", today)).req());
        }
        fields.push(FormField::new("new", "New password", FieldEditor::text(EdKind::Password, "", today)).req());
        fields.push(FormField::new("again", "Repeat", FieldEditor::text(EdKind::Password, "", today)).req());
        let who = self.r.users.get(&user_id).map_or("?".to_string(), |u| u.display_name.clone());
        let mut f = Form::new(FormKind::SetPassword(user_id, need_old), &format!("Set password — {who}"), fields);
        f.footer = vec!["8–256 characters. Other sessions of this user are logged out.".into()];
        self.dialogs.push(Dialog::Form(Box::new(f)));
    }

    /// Editor for an entity field (forms), using the grid column definition when there is one.
    fn form_editor(&self, e: Entity, id: i64, field: &str, project_hint: Option<i64>) -> FieldEditor {
        let r = &self.r;
        let today = r.today;
        if let Some(meta) = fields::field(e, field) {
            let text = if id > 0 { cg::edit_text(r, e, field, id) } else { String::new() };
            let cur = if id > 0 { r.field_value(e, id, field) } else { Value::Null };
            let choices = || cg::choices(r, e, field, id, project_hint);
            return match meta.kind {
                Kind::Text { max, .. } if max > 2000 => FieldEditor::text(EdKind::Multiline, &text, today),
                Kind::Text { .. } if field == "phase" => FieldEditor::list(EdKind::Suggest, choices(), Value::Null, &text, today),
                Kind::Text { .. } | Kind::Code | Kind::Username | Kind::Prefix | Kind::Color => FieldEditor::text(EdKind::Text, &text, today),
                Kind::Password => FieldEditor::text(EdKind::Password, "", today),
                Kind::Enum(_) => FieldEditor::list(EdKind::Select, choices(), cur, "", today),
                Kind::Ref(_) => FieldEditor::list(EdKind::Combo, choices(), cur, "", today),
                Kind::Date => FieldEditor::text(EdKind::Date, &text, today),
                Kind::Hours => FieldEditor::text(EdKind::Number, &text, today),
                Kind::Int { .. } => FieldEditor::text(EdKind::Int, &text, today),
                Kind::Tags | Kind::Options => FieldEditor::chips(choices(), cur.as_array().map(Vec::as_slice).unwrap_or(&[]), true, today),
                Kind::Deps => FieldEditor::chips(choices(), cur.as_array().map(Vec::as_slice).unwrap_or(&[]), false, today),
                Kind::Bool => FieldEditor::boolean(cur.as_bool().unwrap_or(true), today),
            };
        }
        // custom field
        let col = Column {
            key: field.into(),
            title: String::new(),
            width: 10,
            align: cg::Align::Left,
            entity: e,
            field: Some(field.into()),
            editor: match fields::cf_id(field).and_then(|c| r.custom_fields.get(&c)).map(|c| c.kind) {
                Some(CfKind::Number) => Editor::Number,
                Some(CfKind::Date) => Editor::Date,
                Some(CfKind::Enum) => Editor::Select,
                _ => Editor::Text,
            },
            sum: false,
            hidden: false,
        };
        let mut ed = self.make_editor(e, id, &col, project_hint);
        ed.list_open = false;
        ed
    }

    pub fn open_new_project(&mut self) {
        if !self.can_write(Entity::Project) {
            return;
        }
        let r = &self.r;
        let today = r.today;
        let program = match self.view {
            views::PROJECTS => self.views.projects.main.current_id().and_then(|id| r.projects.get(&id)).and_then(|p| p.program_id),
            _ => None,
        }
        .or_else(|| r.programs.keys().next().copied());
        let tpl = r.templates.values().find(|t| t.name == "Standard Project").or_else(|| r.templates.values().next()).map(|t| t.id);
        let mut f = Form::new(
            FormKind::NewProject,
            "New project",
            vec![
                FormField::new("program_id", "Program", FieldEditor::list(EdKind::Combo, cg::choices(r, Entity::Project, "program_id", 0, None), program.map_or(Value::Null, Value::from), "", today)),
                FormField::new("code", "Code", FieldEditor::text(EdKind::Text, "", today)).req(),
                FormField::new("name", "Name", FieldEditor::text(EdKind::Text, "", today)).req(),
                FormField::new("template_id", "Template", FieldEditor::list(EdKind::Select, cg::choices(r, Entity::Project, "template_id", 0, None), tpl.map_or(Value::Null, Value::from), "", today)),
                FormField::new("owner_id", "Owner", FieldEditor::list(EdKind::Combo, cg::choices(r, Entity::Project, "owner_id", 0, None), Value::from(r.me_id()), "", today)),
                FormField::new("start_date", "Start", FieldEditor::text(EdKind::Date, &today.iso(), today)),
                FormField::new("due_date", "Due", FieldEditor::text(EdKind::Date, "", today)),
                FormField::new("budget_hours", "Budget h", FieldEditor::text(EdKind::Number, "", today)),
            ],
        );
        f.focus = 2;
        self.cascade(&mut f, None);
        self.dialogs.push(Dialog::Form(Box::new(f)));
    }

    pub fn open_new_task(&mut self) {
        if !self.can_write(Entity::Task) {
            return;
        }
        let r = &self.r;
        let today = r.today;
        // preset the project from context
        let project = match self.view {
            views::PROJECTS => self.views.projects.main.current_id(),
            views::TASKS => self.views.tasks.current_id().and_then(|t| r.tasks.get(&t)).map(|t| t.project_id),
            views::BOARD => self.views.board.project.or_else(|| self.views.board.current.and_then(|t| r.tasks.get(&t)).map(|t| t.project_id)),
            views::TIMELINE => self.views.timeline.current_entity().and_then(|(e, id)| match e {
                Entity::Project => Some(id),
                _ => r.tasks.get(&id).map(|t| t.project_id),
            }),
            _ => None,
        };
        let program = project.and_then(|p| r.projects.get(&p)).and_then(|p| p.program_id);
        let status = if self.view == views::BOARD { TaskStatus::ALL[self.views.board.col.min(4)].as_str() } else { "todo" };
        let mut programs = cg::choices(r, Entity::Project, "program_id", 0, None);
        if let Some(first) = programs.first_mut() {
            first.label = "All programs".into();
        }
        let mut f = Form::new(
            FormKind::NewTask,
            "New task",
            vec![
                FormField::new("program", "Program", FieldEditor::list(EdKind::Combo, programs, program.map_or(Value::Null, Value::from), "", today)),
                FormField::new("project_id", "Project", FieldEditor::list(EdKind::Combo, vec![], project.map_or(Value::Null, Value::from), "", today)).req(),
                FormField::new("phase", "Phase", FieldEditor::list(EdKind::Suggest, vec![], Value::Null, "", today)),
                FormField::new("title", "Title", FieldEditor::text(EdKind::Text, "", today)).req(),
                FormField::new("assignee_id", "Assignee", FieldEditor::list(EdKind::Combo, cg::choices(r, Entity::Task, "assignee_id", 0, None), Value::Null, "", today)),
                FormField::new("status", "Status", FieldEditor::list(EdKind::Select, cg::choices(r, Entity::Task, "status", 0, None), json!(status), "", today)),
                FormField::new("start_date", "Start", FieldEditor::text(EdKind::Date, "", today)),
                FormField::new("due_date", "Due", FieldEditor::text(EdKind::Date, "", today)),
                FormField::new("est_hours", "Est h", FieldEditor::text(EdKind::Number, "", today)),
            ],
        );
        f.focus = if project.is_some() { 3 } else { 0 };
        self.cascade(&mut f, None);
        self.dialogs.push(Dialog::Form(Box::new(f)));
    }

    pub fn open_record(&mut self, e: Entity, id: i64, focus_field: Option<String>) {
        if !self.r.exists(e, id) {
            return;
        }
        let r = &self.r;
        let cols: Vec<Column> = match e {
            Entity::Project => cg::project_columns(r),
            Entity::Task => cg::task_columns(r, false),
            Entity::Template => cg::template_columns(),
            Entity::TemplateTask => cg::template_task_columns(),
            Entity::CustomField => cg::custom_field_columns(),
            Entity::User => cg::user_columns(),
            Entity::Program => cg::program_columns(),
        };
        let project_hint = if e == Entity::Task { r.tasks.get(&id).map(|t| t.project_id) } else { None };
        let mut fields_v = vec![];
        for c in cols.iter().filter(|c| c.editable()) {
            let key = c.field.clone().unwrap_or_default();
            let mut ed = self.form_editor(e, id, &key, project_hint);
            ed.list_open = false;
            let mut ff = FormField::new(&key, &c.title, ed);
            ff.base = r.field_value(e, id, &key);
            if fields::field(e, &key).is_some_and(|m| m.required) {
                ff.required = true;
            }
            fields_v.push(ff);
        }
        let label = r.row_label(e, id);
        let title = match e {
            Entity::Task => format!("Task {label} — {}", r.tasks.get(&id).map_or("", |t| t.title.as_str())),
            Entity::Project => format!("Project {label} — {}", r.projects.get(&id).map_or("", |p| p.name.as_str())),
            _ => format!("{} {label}", e.label()),
        };
        let mut f = Form::new(FormKind::Record(e, id), &title, fields_v);
        // footer: meta + recent activity on this record
        let (created, updated, by) = match e {
            Entity::Project => r.projects.get(&id).map(|p| (p.created_at.clone(), p.updated_at.clone(), p.updated_by)).unwrap_or_default(),
            Entity::Task => r.tasks.get(&id).map(|t| (t.created_at.clone(), t.updated_at.clone(), t.updated_by)).unwrap_or_default(),
            _ => Default::default(),
        };
        if !created.is_empty() {
            f.footer.push(format!("Created {} · updated {} by {}", &created[..10.min(created.len())], updated[..16.min(updated.len())].replace('T', " "), r.username(by)));
        }
        if e == Entity::Task {
            if let Some(t) = r.tasks.get(&id) {
                let deps: Vec<String> = t.deps.iter().filter_map(|d| r.tasks.get(d)).map(|d| format!("#{} {} ({})", d.seq, d.title, d.status.label())).collect();
                if !deps.is_empty() {
                    f.footer.push(format!("Waits for: {}", deps.join(", ")));
                }
            }
        }
        if e == Entity::Project {
            let m = r.metrics(id);
            f.footer.push(format!("{}/{} tasks done · {} h estimated · {} h actual · {} overdue", m.tasks_done, m.tasks_total, cg::fmt_hours(m.est_total), cg::fmt_hours(m.actual_total), m.tasks_overdue));
        }
        let recent: Vec<String> = r
            .activity
            .values()
            .rev()
            .filter(|a| a.entity == e.as_str() && a.entity_id == id)
            .take(4)
            .map(|a| format!("  {} {} — {}", a.ts.get(5..16).unwrap_or("").replace('T', " "), r.username(a.user_id), a.label))
            .collect();
        if !recent.is_empty() {
            f.footer.push("Recent changes:".into());
            f.footer.extend(recent);
        }
        if let Some(ff) = focus_field {
            if let Some(i) = f.fields.iter().position(|x| x.key == ff) {
                f.focus = i;
            }
        }
        if !self.r.can_edit() || (!matches!(e, Entity::Project | Entity::Task) && !self.r.is_admin()) {
            f.message = Some(("Read-only: you can look but not change this.".into(), false));
        }
        self.dialogs.push(Dialog::Form(Box::new(f)));
    }

    pub fn open_create(&mut self, e: Entity, parent: Option<i64>) {
        if !self.can_write(e) {
            return;
        }
        let today = self.r.today;
        let mut fields_v = vec![];
        for m in fields::fields(e) {
            if e == Entity::TemplateTask && m.name == "template_id" {
                continue;
            }
            let mut ed = self.form_editor(e, 0, m.name, None);
            if m.name == "duration_days" {
                ed.input.set("1");
            }
            if m.name == "role" {
                ed.current = json!("member");
            }
            if m.name == "kind" {
                ed.current = json!("text");
            }
            if m.name == "entity" {
                ed.current = json!("task");
            }
            if m.name == "color" {
                ed.input.set("#4f9dde");
            }
            let mut ff = FormField::new(m.name, m.label, ed);
            ff.required = m.required;
            fields_v.push(ff);
        }
        if e == Entity::User {
            fields_v.push(FormField::new("again", "Repeat password", FieldEditor::text(EdKind::Password, "", today)).req());
        }
        let mut f = Form::new(FormKind::Create(e), &format!("New {}", e.label()), fields_v);
        f.extra = parent;
        self.dialogs.push(Dialog::Form(Box::new(f)));
    }

    fn can_write(&mut self, e: Entity) -> bool {
        if !self.r.can_edit() {
            self.toast("Your account is read-only (viewer).".into(), Level::Err);
            return false;
        }
        if !matches!(e, Entity::Project | Entity::Task) && !self.r.is_admin() {
            self.toast(format!("Only admins can add {}s.", e.label()), Level::Err);
            return false;
        }
        true
    }

    /// Cascading selects and auto-filled defaults (ui-spec §5).
    fn cascade(&self, f: &mut Form, changed: Option<&str>) {
        let r = &self.r;
        match f.kind {
            FormKind::NewProject => {
                let program = f.value("program_id").as_i64();
                if changed.is_none() || changed == Some("program_id") {
                    let code = r.next_code(program);
                    f.set_auto("code", &code);
                }
                let tpl = f.value("template_id").as_i64();
                let start = pt_core::date::parse_natural(&f.text("start_date"), r.today).ok().flatten().unwrap_or(r.today);
                let (n, hours, span) = tpl.map(|t| r.template_stats(t)).unwrap_or((0, 0.0, 30));
                if matches!(changed, None | Some("template_id") | Some("start_date")) {
                    f.set_auto("due_date", &start.add_days(span.max(1) as i32).iso());
                    if tpl.is_some() {
                        f.set_auto("budget_hours", &cg::fmt_hours((hours / 5.0).ceil() * 5.0));
                    }
                }
                if let Some(fl) = f.field("template_id") {
                    fl.note = if tpl.is_some() { format!("{n} tasks, {} h, {span} days will be created", cg::fmt_hours(hours)) } else { "a blank project with no tasks".into() };
                }
            }
            FormKind::BoardScope => {
                let program = f.value("program").as_i64();
                let mut projects = vec![cg::Choice { value: Value::Null, label: "All projects".into(), detail: String::new(), style: cg::Style::Muted }];
                projects.extend(
                    cg::choices(r, Entity::Task, "project_id", 0, None)
                        .into_iter()
                        .filter(|c| program.is_none() || c.value.as_i64().and_then(|id| r.projects.get(&id)).is_some_and(|p| p.program_id == program)),
                );
                if let Some(fl) = f.field("project") {
                    let mut cur = fl.ed.current.clone();
                    if !projects.iter().any(|c| c.value == cur) {
                        cur = Value::Null;
                    }
                    fl.ed = FieldEditor::list(EdKind::Combo, projects, cur, "", r.today);
                    fl.note = format!("{} project(s)", fl.ed.choices.len() - 1);
                }
            }
            FormKind::NewTask => {
                // picking a project while "all programs" is shown also sets the program
                let mut program = f.value("program").as_i64();
                let mut project = f.value("project_id").as_i64();
                if program.is_none() {
                    if let Some(pg) = project.and_then(|p| r.projects.get(&p)).and_then(|p| p.program_id) {
                        program = Some(pg);
                        if let Some(fl) = f.field("program") {
                            fl.ed.current = Value::from(pg);
                        }
                    }
                }
                let projects: Vec<cg::Choice> = cg::choices(r, Entity::Task, "project_id", 0, None)
                    .into_iter()
                    .filter(|c| program.is_none() || c.value.as_i64().and_then(|id| r.projects.get(&id)).is_some_and(|p| p.program_id == program))
                    .collect();
                if let Some(fl) = f.field("project_id") {
                    if !projects.iter().any(|c| c.value.as_i64() == project) {
                        project = if program.is_some() { projects.first().and_then(|c| c.value.as_i64()) } else { None };
                        fl.ed.current = project.map_or(Value::Null, Value::from);
                    }
                    let current = fl.ed.current.clone();
                    let open = fl.ed.list_open;
                    fl.ed = FieldEditor::list(EdKind::Combo, projects, current, "", r.today);
                    fl.ed.list_open = open && changed == Some("project_id");
                    fl.note = match program.and_then(|p| r.programs.get(&p)) {
                        Some(pg) => format!("{} project(s) in {}", fl.ed.choices.len(), pg.name),
                        None => format!("{} projects — pick a program first to narrow the list", fl.ed.choices.len()),
                    };
                }
                if let Some(pid) = project {
                    if matches!(changed, None | Some("project_id") | Some("program")) {
                        let phases = cg::choices(r, Entity::Task, "phase", 0, Some(pid));
                        if let Some(fl) = f.field("phase") {
                            let text = fl.ed.input.text();
                            fl.ed = FieldEditor::list(EdKind::Suggest, phases, Value::Null, &text, r.today);
                            fl.note = format!("phases of {}", r.project_code(pid));
                        }
                        let owner = r.projects.get(&pid).and_then(|p| p.owner_id);
                        let auto = f.autos.get("assignee_id").cloned();
                        let mut set_auto = None;
                        if let Some(fl) = f.field("assignee_id") {
                            let cur = fl.ed.current.to_string();
                            if fl.ed.current.is_null() || auto.as_deref() == Some(cur.as_str()) {
                                fl.ed.current = owner.map_or(Value::Null, Value::from);
                                set_auto = Some(fl.ed.current.to_string());
                            }
                        }
                        if let Some(a) = set_auto {
                            f.autos.insert("assignee_id".into(), a);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ---- input ----------------------------------------------------------------------------

    /// Put a popped dialog back under anything opened while it was handled.
    fn restore_dialog(&mut self, d: Dialog, base_len: usize) {
        let opened = self.dialogs.split_off(base_len.min(self.dialogs.len()));
        self.dialogs.push(d);
        self.dialogs.extend(opened);
    }

    pub fn dialog_key(&mut self, k: KeyEvent) {
        let Some(mut d) = self.dialogs.pop() else { return };
        let base_len = self.dialogs.len();
        let keep = match &mut d {
            Dialog::Form(f) => self.form_key(f, k),
            Dialog::Confirm(c) => self.confirm_key(c, k),
            Dialog::Columns(c) => self.columns_key(c, k),
            Dialog::Sort(s) => self.sort_key(s, k),
            Dialog::Palette(p) => self.palette_key(p, k),
            Dialog::Help(h) => help_key(h, k),
            Dialog::Context(m) => self.context_key(m, k),
            Dialog::Message(_) => !matches!(k.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')),
        };
        if keep {
            self.restore_dialog(d, base_len);
        }
    }

    /// Returns whether the form stays open.
    fn form_key(&mut self, f: &mut Form, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if f.busy && k.code != KeyCode::Esc {
            return true;
        }
        if ctrl && k.code == KeyCode::Char('q') && f.kind == FormKind::Login {
            self.quit = true;
            return false;
        }
        if ctrl && k.code == KeyCode::Char('s') {
            self.submit_form(f, true);
            return f.awaiting.is_some() || f.busy || f.message.as_ref().is_some_and(|m| m.1) || matches!(f.kind, FormKind::Login);
        }
        let n = f.n();
        if f.focus >= n {
            // buttons
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if f.focus == n {
                        self.submit_form(f, false);
                        return f.awaiting.is_some() || f.busy || f.message.as_ref().is_some_and(|m| m.1) || f.kind == FormKind::Login;
                    }
                    return f.kind == FormKind::Login;
                }
                KeyCode::Esc => return f.kind == FormKind::Login,
                KeyCode::Tab | KeyCode::Right | KeyCode::Down => f.focus = (f.focus + 1) % (n + 2),
                KeyCode::BackTab | KeyCode::Left | KeyCode::Up => f.focus = (f.focus + n + 1) % (n + 2),
                _ => {}
            }
            return true;
        }
        // conflict on this field: Enter takes their value
        if k.code == KeyCode::Enter && f.fields[f.focus].conflict.is_some() {
            let fl = &mut f.fields[f.focus];
            let (theirs, _) = fl.conflict.take().unwrap_or((Value::Null, None));
            if let FormKind::Record(e, id) = f.kind {
                let mut ed = self.form_editor(e, id, &fl.key, None);
                ed.list_open = false;
                fl.ed = ed;
                fl.base = theirs;
            }
            return true;
        }
        // Login: Enter anywhere submits
        if f.kind == FormKind::Login && k.code == KeyCode::Enter && !f.fields[f.focus].ed.list_open {
            self.submit_form(f, false);
            return true;
        }
        let before = f.fields[f.focus].ed.value();
        let out = f.fields[f.focus].ed.handle_key(&k);
        let changed = f.fields[f.focus].ed.value() != before;
        if changed {
            f.fields[f.focus].error = None;
            let key = f.fields[f.focus].key.clone();
            self.cascade(f, Some(&key));
        }
        match out {
            EdOut::Commit(nav) => {
                f.fields[f.focus].ed.list_open = false;
                if !changed {
                    let key = f.fields[f.focus].key.clone();
                    self.cascade(f, Some(&key));
                }
                match nav {
                    Nav::Left => f.focus = f.focus.saturating_sub(1),
                    Nav::Stay => {}
                    _ => {
                        // Enter on the last field submits
                        if f.focus + 1 == n && k.code == KeyCode::Enter {
                            self.submit_form(f, false);
                            return f.awaiting.is_some() || f.busy || f.message.as_ref().is_some_and(|m| m.1) || f.kind == FormKind::Login;
                        }
                        f.focus += 1;
                    }
                }
            }
            EdOut::Cancel => {
                if f.kind == FormKind::Login {
                    return true;
                }
                return false;
            }
            EdOut::Ignored => match k.code {
                KeyCode::Up => f.focus = f.focus.saturating_sub(1),
                KeyCode::Down => f.focus = (f.focus + 1).min(n + 1),
                KeyCode::PageDown => f.focus = n,
                _ => {}
            },
            EdOut::Consumed => {}
        }
        true
    }

    /// Validate and send. `force`: ^S also overrides conflicts in a record form.
    fn submit_form(&mut self, f: &mut Form, force: bool) {
        f.message = None;
        for fl in f.fields.iter_mut() {
            fl.error = None;
            fl.ed.list_open = false;
        }
        let missing: Vec<usize> = f
            .fields
            .iter()
            .enumerate()
            .filter(|(_, fl)| fl.required && matches!(fl.ed.value(), EdValue::Text(ref s) if s.trim().is_empty()) || fl.required && matches!(fl.ed.value(), EdValue::Value(Value::Null)))
            .map(|(i, _)| i)
            .collect();
        if let Some(&i) = missing.first() {
            for &j in &missing {
                f.fields[j].error = Some("required".into());
            }
            f.focus = i;
            f.message = Some(("Please fill in the required fields.".into(), true));
            return;
        }
        match f.kind {
            FormKind::Login => {
                let server = f.text("server");
                let url = match ServerUrl::parse(&server) {
                    Ok(u) => u,
                    Err(e) => {
                        f.message = Some((format!("Server: {e}"), true));
                        return;
                    }
                };
                self.url = url.clone();
                let user = f.text("username").trim().to_string();
                let pw = f.text("password");
                let remember = f.value("remember") == Value::Bool(true);
                f.busy = true;
                f.message = Some(("Logging in…".into(), false));
                let (tls, tx) = (self.tls.clone(), self.internal_tx.clone());
                tokio::spawn(async move {
                    let res = net::login(&url, &tls, &user, &pw).await.map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Internal::Login(res, remember));
                });
            }
            FormKind::SaveFilter => {
                let name = f.text("name").trim().to_string();
                self.save_filter(name);
            }
            FormKind::BoardScope => {
                self.views.board.program = f.value("program").as_i64();
                self.views.board.project = f.value("project").as_i64();
                self.views.board.current = None;
                self.views.board.scroll = 0;
            }
            FormKind::SetPassword(uid, need_old) => {
                let (new, again) = (f.text("new"), f.text("again"));
                if new != again {
                    if let Some(x) = f.field("again") {
                        x.error = Some("doesn't match".into());
                    }
                    f.message = Some(("The passwords don't match.".into(), true));
                    return;
                }
                if !(8..=256).contains(&new.chars().count()) {
                    f.message = Some(("Use 8–256 characters.".into(), true));
                    return;
                }
                let req = self.next_req_pub();
                self.pending.insert(req, crate::app::Pending { kind: PendKind::Password, cells: vec![], ops: vec![], label: "password".into(), select: None });
                let old = need_old.then(|| f.text("old"));
                if !self.send(ClientMsg::SetPassword { req, user_id: uid, password: new, old_password: old }) {
                    f.message = Some(("You're offline.".into(), true));
                    return;
                }
                f.awaiting = Some(req);
                f.busy = true;
            }
            FormKind::NewProject | FormKind::NewTask | FormKind::Create(_) => {
                let e = match f.kind {
                    FormKind::NewProject => Entity::Project,
                    FormKind::NewTask => Entity::Task,
                    FormKind::Create(e) => e,
                    _ => return,
                };
                if e == Entity::User && f.text("password") != f.text("again") {
                    f.message = Some(("The passwords don't match.".into(), true));
                    return;
                }
                let mut values = Map::new();
                let mut bad = None;
                for (i, fl) in f.fields.iter_mut().enumerate() {
                    if fl.key == "program" || fl.key == "again" {
                        continue;
                    }
                    let v = match fl.ed.value() {
                        EdValue::Value(v) => Ok(v),
                        EdValue::Text(s) => cg::parse_input(&self.r, e, &fl.key, 0, &s),
                    };
                    match v {
                        Ok(v) if v.is_null() || v == json!("") || v == json!([]) => {}
                        Ok(v) => {
                            values.insert(fl.key.clone(), v);
                        }
                        Err(m) => {
                            fl.error = Some(m);
                            bad.get_or_insert(i);
                        }
                    }
                }
                if let Some(i) = bad {
                    f.focus = i;
                    f.message = Some(("Please fix the highlighted fields.".into(), true));
                    return;
                }
                if e == Entity::TemplateTask {
                    if let Some(t) = f.extra {
                        values.insert("template_id".into(), json!(t));
                    }
                }
                let label = format!("new {}", e.label());
                let op = Op::Create { entity: e, reference: Some("new".into()), values };
                match self.mutate(vec![op], label, PendKind::Form, Some((e, "new".into()))) {
                    Some(req) => {
                        f.awaiting = Some(req);
                        f.busy = true;
                        f.message = Some(("Saving…".into(), false));
                    }
                    None => f.message = Some(("Not saved (offline or read-only).".into(), true)),
                }
            }
            FormKind::Record(e, id) => {
                let mut ops = vec![];
                let mut bad = None;
                for (i, fl) in f.fields.iter_mut().enumerate() {
                    let v = match fl.ed.value() {
                        EdValue::Value(v) => Ok(v),
                        EdValue::Text(s) => cg::parse_input(&self.r, e, &fl.key, id, &s),
                    };
                    match v {
                        Ok(v) => {
                            if !fields::values_equal(&v, &fl.base) || (force && fl.conflict.is_some()) {
                                let force_this = force && fl.conflict.is_some();
                                ops.push(Op::Set { entity: e, id, field: fl.key.clone(), base: fl.base.clone(), value: v, force: force_this });
                            }
                        }
                        Err(m) => {
                            fl.error = Some(m);
                            bad.get_or_insert(i);
                        }
                    }
                }
                if let Some(i) = bad {
                    f.focus = i;
                    f.message = Some(("Please fix the highlighted fields.".into(), true));
                    return;
                }
                if ops.is_empty() {
                    f.message = Some(("No changes.".into(), false));
                    f.busy = false;
                    f.awaiting = None;
                    // closes: caller sees no awaiting / no error
                    return;
                }
                let label = format!("{} ({} field{})", self.r.row_label(e, id), ops.len(), if ops.len() == 1 { "" } else { "s" });
                match self.mutate(ops, label, PendKind::Form, None) {
                    Some(req) => {
                        f.awaiting = Some(req);
                        f.busy = true;
                        f.message = Some(("Saving…".into(), false));
                    }
                    None => f.message = Some(("Not saved (offline or read-only).".into(), true)),
                }
            }
        }
    }

    pub fn next_req_pub(&mut self) -> u64 {
        self.req += 1;
        self.req
    }

    /// A result for a form submission.
    pub fn form_result(&mut self, req: u64, res: &ResultMsg) {
        let Some(pos) = self.dialogs.iter().position(|d| matches!(d, Dialog::Form(f) if f.awaiting == Some(req))) else {
            if !res.ok {
                let msg = res.error.as_ref().map_or("failed".into(), |e| e.message.clone());
                self.toast(format!("✗ {msg}"), Level::Err);
            } else {
                self.set_status("✓ Saved".into(), Level::Ok);
            }
            return;
        };
        let Dialog::Form(f) = &mut self.dialogs[pos] else { return };
        f.busy = false;
        f.awaiting = None;
        if !res.ok {
            let e = res.error.clone().unwrap_or_else(|| pt_core::protocol::ErrorBody::new("internal", "failed"));
            f.message = Some((e.message.clone(), true));
            // point at the failing field if we can tell
            let lower = e.message.to_lowercase();
            if let Some(i) = f.fields.iter().position(|fl| lower.contains(&fl.label.to_lowercase()) || lower.contains(&fl.key.replace('_', " "))) {
                f.fields[i].error = Some(e.message);
                f.focus = i;
            }
            return;
        }
        if !res.conflicts.is_empty() {
            let mut msgs = vec![];
            for c in &res.conflicts {
                if let Some(fl) = f.fields.iter_mut().find(|fl| fl.key == c.field) {
                    fl.conflict = Some((c.current.clone(), c.by));
                    let who = c.by.map(|b| self.r.username(Some(b)).to_string()).unwrap_or("someone".into());
                    msgs.push(format!("{} (changed by {who})", fl.label));
                }
            }
            f.message = Some((format!("⚠ Not saved: {}. Enter on a field = keep theirs, ^S = save mine.", msgs.join(", ")), true));
            // other fields were saved: refresh their bases
            if let FormKind::Record(e, id) = f.kind {
                for fl in f.fields.iter_mut().filter(|fl| fl.conflict.is_none()) {
                    fl.base = self.r.field_value(e, id, &fl.key);
                }
            }
            return;
        }
        let kind = f.kind;
        self.dialogs.remove(pos);
        match kind {
            FormKind::SetPassword(..) => self.set_status("✓ Password changed".into(), Level::Ok),
            FormKind::Record(e, id) => {
                let l = self.r.row_label(e, id);
                self.set_status(format!("✓ Saved {l}"), Level::Ok);
            }
            _ => self.set_status("✓ Created".into(), Level::Ok),
        }
    }

    fn confirm_key(&mut self, c: &mut Confirm, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => c.sel = 1 - c.sel,
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.run_confirm(c.action.clone());
                return false;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => return false,
            KeyCode::Enter => {
                if c.sel == 1 {
                    self.run_confirm(c.action.clone());
                }
                return false;
            }
            _ => {}
        }
        true
    }

    fn run_confirm(&mut self, a: ConfirmAction) {
        match a {
            ConfirmAction::Mutate(ops, label) => {
                self.mutate(ops, label, PendKind::Cells, None);
                for gs in [GridSel::Projects, GridSel::ProjectTasks, GridSel::Tasks, GridSel::Templates, GridSel::TemplateTasks, GridSel::Fields, GridSel::Programs] {
                    self.views.grid(gs).marks.clear();
                }
            }
        }
    }

    fn columns_key(&mut self, c: &mut Columns, k: KeyEvent) -> bool {
        let r = &self.r;
        let g = self.views.grid(c.grid);
        let n = g.cols.len();
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::F(4) => return false,
            KeyCode::Up if shift => {
                if c.sel > 0 {
                    g.cols.swap(c.sel, c.sel - 1);
                    c.sel -= 1;
                }
            }
            KeyCode::Down if shift => {
                if c.sel + 1 < n {
                    g.cols.swap(c.sel, c.sel + 1);
                    c.sel += 1;
                }
            }
            KeyCode::Char('[') => {
                if c.sel > 0 {
                    g.cols.swap(c.sel, c.sel - 1);
                    c.sel -= 1;
                }
            }
            KeyCode::Char(']') => {
                if c.sel + 1 < n {
                    g.cols.swap(c.sel, c.sel + 1);
                    c.sel += 1;
                }
            }
            KeyCode::Up => c.sel = c.sel.saturating_sub(1),
            KeyCode::Down => c.sel = (c.sel + 1).min(n.saturating_sub(1)),
            KeyCode::Char(' ') => {
                let visible = g.cols.iter().filter(|x| !x.hidden).count();
                if !(visible == 1 && !g.cols[c.sel].hidden) {
                    g.cols[c.sel].hidden = !g.cols[c.sel].hidden;
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => g.cols[c.sel].width = (g.cols[c.sel].width + 1).min(80),
            KeyCode::Char('-') => g.cols[c.sel].width = g.cols[c.sel].width.saturating_sub(1).max(3),
            KeyCode::Char('r') | KeyCode::Char('R') => g.reset_columns(r),
            _ => {}
        }
        g.touch();
        true
    }

    fn sort_key(&mut self, s: &mut Sort, k: KeyEvent) -> bool {
        let ncols = self.views.grid_ref(s.grid).cols.len();
        match k.code {
            KeyCode::Esc => return false,
            KeyCode::Enter => {
                let g = self.views.grid(s.grid);
                g.sort = s.keys.iter().map(|(i, d)| SortSpec { key: g.cols[*i].key.clone(), desc: *d }).collect();
                g.touch();
                return false;
            }
            KeyCode::Up => s.sel = s.sel.saturating_sub(1),
            KeyCode::Down => s.sel = (s.sel + 1).min(s.keys.len().min(2)),
            KeyCode::Left | KeyCode::Right => {
                let delta = if k.code == KeyCode::Right { 1 } else { ncols - 1 };
                if s.sel >= s.keys.len() {
                    s.keys.push((0, false));
                    s.sel = s.keys.len() - 1;
                }
                s.keys[s.sel].0 = (s.keys[s.sel].0 + delta) % ncols;
            }
            KeyCode::Char(' ') => {
                if let Some(x) = s.keys.get_mut(s.sel) {
                    x.1 = !x.1;
                }
            }
            KeyCode::Delete | KeyCode::Backspace if s.sel < s.keys.len() => {
                s.keys.remove(s.sel);
            }
            _ => {}
        }
        true
    }

    fn palette_key(&mut self, p: &mut Palette, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc => return false,
            KeyCode::Up => p.sel = p.sel.saturating_sub(1),
            KeyCode::Down => p.sel = (p.sel + 1).min(p.filtered.len().saturating_sub(1)),
            KeyCode::PageUp => p.sel = p.sel.saturating_sub(10),
            KeyCode::PageDown => p.sel = (p.sel + 10).min(p.filtered.len().saturating_sub(1)),
            KeyCode::Enter => {
                if let Some((i, _)) = p.filtered.get(p.sel) {
                    let act = p.items[*i].2.clone();
                    self.run_palette(act);
                }
                return false;
            }
            _ => {
                if p.input.handle(&k) {
                    refilter_palette(p);
                }
            }
        }
        true
    }

    fn run_palette(&mut self, a: PalAct) {
        match a {
            PalAct::Action(a) => self.action(a),
            PalAct::Project(id) => {
                if !matches!(self.view, views::PROJECTS | views::TIMELINE) {
                    self.view = views::PROJECTS;
                }
                self.reveal(Entity::Project, id);
            }
            PalAct::Task(id) => {
                self.view = views::TASKS;
                self.reveal(Entity::Task, id);
            }
            PalAct::Theme(i) => {
                self.theme_idx = i;
                self.t = Theme::new(i);
            }
            PalAct::Filter(i) => self.apply_filter(i),
        }
    }

    fn context_key(&mut self, m: &mut ContextMenu, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc => return false,
            KeyCode::Up => m.sel = (m.sel + m.items.len() - 1) % m.items.len(),
            KeyCode::Down => m.sel = (m.sel + 1) % m.items.len(),
            KeyCode::Enter => {
                let a = m.items[m.sel].2.clone();
                self.run_context(a);
                return false;
            }
            _ => {}
        }
        true
    }

    fn run_context(&mut self, a: CtxAct) {
        match a {
            CtxAct::Action(Action::EditRecord) if self.view == views::ACTIVITY => {
                self.on_event(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
            }
            CtxAct::Action(a) => self.action(a),
            CtxAct::FilterBy(exclude) => self.filter_by_cell(exclude),
            CtxAct::Sort(desc) => {
                if let Some(gs) = self.views.active_grid(self.view) {
                    let g = self.views.grid(gs);
                    if let Some(key) = g.current_col().map(|c| c.key.clone()) {
                        g.sort = vec![SortSpec { key, desc }];
                        g.touch();
                    }
                }
            }
            CtxAct::HideColumn => {
                if let Some(gs) = self.views.active_grid(self.view) {
                    let g = self.views.grid(gs);
                    let vis = g.visible();
                    if vis.len() > 1 {
                        if let Some(&i) = vis.get(g.ccol) {
                            g.cols[i].hidden = true;
                            g.touch();
                        }
                    }
                }
            }
        }
    }

    /// Context menu: add `field:value` (or `-field:value`) for the current cell to the filter.
    fn filter_by_cell(&mut self, exclude: bool) {
        let Some(gs) = self.views.active_grid(self.view) else { return };
        let g = self.views.grid_ref(gs);
        let (Some(id), Some(col)) = (g.current_id(), g.current_col().cloned()) else { return };
        let r = &self.r;
        let term = match (g.kind, col.key.as_str()) {
            (crate::grid::GridKind::Activity, "user") => r.activity.get(&id).map(|a| format!("user:{}", r.username(a.user_id))),
            (crate::grid::GridKind::Activity, "action") => r.activity.get(&id).map(|a| format!("action:{}", a.action.as_str())),
            (_, "status" | "priority" | "health") => {
                let e = g.entity().unwrap_or(Entity::Task);
                r.field_value(e, id, &col.key).as_str().map(|v| format!("{}:{v}", col.key))
            }
            (_, "owner_id" | "assignee_id") => {
                let e = g.entity().unwrap_or(Entity::Task);
                let u = r.field_value(e, id, &col.key).as_i64();
                Some(match u {
                    Some(u) => format!("@{}", r.username(Some(u))),
                    None => format!("{}:none", if col.key == "owner_id" { "owner" } else { "assignee" }),
                })
            }
            (_, "program_id") => r.projects.get(&id).and_then(|p| p.program_id).and_then(|p| r.programs.get(&p)).map(|p| format!("program:{}", p.prefix.to_lowercase())),
            (_, "project") => r.tasks.get(&id).map(|t| format!("project:{}", r.project_code(t.project_id).to_lowercase())),
            (_, "phase") => r.tasks.get(&id).map(|t| format!("phase:\"{}\"", t.phase)),
            (_, "tags") => {
                let e = g.entity().unwrap_or(Entity::Task);
                r.field_value(e, id, "tags").as_array().and_then(|a| a.first().and_then(|t| t.as_str()).map(|t| format!("#{t}")))
            }
            (_, "due_date" | "start_date") => {
                let e = g.entity().unwrap_or(Entity::Task);
                let f = if col.key == "due_date" { "due" } else { "start" };
                Some(match r.field_value(e, id, &col.key).as_str() {
                    Some(d) => format!("{f}:{d}"),
                    None => format!("{f}:none"),
                })
            }
            _ => {
                let text = cg::cell(r, &col, id).text;
                (!text.is_empty()).then(|| format!("\"{text}\""))
            }
        };
        if let Some(t) = term {
            let qb = &mut self.queries[self.view];
            let mut q = qb.input.text();
            if !q.is_empty() && !q.ends_with(' ') {
                q.push(' ');
            }
            if exclude {
                q.push('-');
            }
            q.push_str(&t);
            qb.input.set(&q);
        }
    }

    // ---- mouse ------------------------------------------------------------------------------

    pub fn dialog_mouse(&mut self, m: MouseEvent) {
        let Some(mut d) = self.dialogs.pop() else { return };
        let base_len = self.dialogs.len();
        let (x, y) = (m.column, m.row);
        let mut keep = true;
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        let wheel = match m.kind {
            MouseEventKind::ScrollDown => 1,
            MouseEventKind::ScrollUp => -1,
            _ => 0,
        };
        match &mut d {
            Dialog::Form(f) => {
                if click {
                    if let Some(fl) = f.fields.get_mut(f.focus) {
                        if let Some(out) = fl.ed.click(x, y) {
                            let key = fl.key.clone();
                            if let EdOut::Commit(_) = out {
                                fl.ed.list_open = false;
                            }
                            self.cascade(f, Some(&key));
                            self.restore_dialog(d, base_len);
                            return;
                        }
                    }
                    if let Some(i) = f.fields.iter().position(|fl| rect_contains(fl.rect, x, y)) {
                        if f.focus == i && f.fields[i].ed.kind.is_list() {
                            f.fields[i].ed.list_open = !f.fields[i].ed.list_open;
                        } else if f.fields[i].ed.kind == EdKind::Bool && f.focus == i {
                            f.fields[i].ed.flag = !f.fields[i].ed.flag;
                        }
                        for (j, fl) in f.fields.iter_mut().enumerate() {
                            if j != i {
                                fl.ed.list_open = false;
                            }
                        }
                        f.focus = i;
                    } else if rect_contains(f.buttons[0], x, y) {
                        f.focus = f.n();
                        self.submit_form(f, false);
                        keep = f.awaiting.is_some() || f.busy || f.message.as_ref().is_some_and(|m| m.1) || f.kind == FormKind::Login;
                    } else if rect_contains(f.buttons[1], x, y) {
                        keep = f.kind == FormKind::Login;
                    }
                }
                if wheel != 0 {
                    if let Some(fl) = f.fields.get_mut(f.focus) {
                        fl.ed.scroll_list(wheel > 0);
                    }
                }
            }
            Dialog::Confirm(c) => {
                if click {
                    if rect_contains(c.buttons[1], x, y) {
                        self.run_confirm(c.action.clone());
                        keep = false;
                    } else if rect_contains(c.buttons[0], x, y) {
                        keep = false;
                    }
                }
            }
            Dialog::Palette(p) => {
                if click && rect_contains(p.rows, x, y) {
                    let row = p.scroll + (y - p.rows.y) as usize;
                    if let Some((i, _)) = p.filtered.get(row) {
                        let act = p.items[*i].2.clone();
                        self.run_palette(act);
                        keep = false;
                    }
                } else if wheel != 0 {
                    p.sel = (p.sel as i64 + wheel * 3).clamp(0, p.filtered.len().saturating_sub(1) as i64) as usize;
                }
            }
            Dialog::Context(c) => {
                if rect_contains(c.rect, x, y) && y > c.rect.y && y < c.rect.bottom() - 1 {
                    let row = (y - c.rect.y - 1) as usize;
                    if row < c.items.len() {
                        c.sel = row;
                        if click {
                            let a = c.items[row].2.clone();
                            self.run_context(a);
                            keep = false;
                        }
                    }
                } else if click || matches!(m.kind, MouseEventKind::Down(_)) {
                    keep = false;
                }
            }
            Dialog::Columns(c) => {
                if click && rect_contains(c.rows, x, y) {
                    let i = (y - c.rows.y) as usize;
                    let g = self.views.grid(c.grid);
                    if i < g.cols.len() {
                        if c.sel == i {
                            g.cols[i].hidden = !g.cols[i].hidden;
                            g.touch();
                        }
                        c.sel = i;
                    }
                }
            }
            Dialog::Help(h) => {
                if wheel != 0 {
                    h.scroll = (h.scroll as i64 + wheel * 3).max(0) as usize;
                }
            }
            Dialog::Message(_) => {
                if click {
                    keep = false;
                }
            }
            Dialog::Sort(_) => {}
        }
        if keep {
            self.restore_dialog(d, base_len);
        }
    }
}

fn refilter_palette(p: &mut Palette) {
    let q = p.input.text();
    if q.trim().is_empty() {
        // without a query: commands and views only (not the 1,000 tasks)
        p.filtered = p.items.iter().enumerate().filter(|(_, it)| !matches!(it.2, PalAct::Project(_) | PalAct::Task(_))).map(|(i, _)| (i, vec![])).collect();
    } else {
        p.filtered = fuzzy::rank(&q, p.items.iter().map(|i| i.0.as_str())).into_iter().take(200).map(|(i, m)| (i, m.indices)).collect();
    }
    p.sel = 0;
    p.scroll = 0;
}

fn help_key(h: &mut Help, k: KeyEvent) -> bool {
    match k.code {
        KeyCode::Esc | KeyCode::F(1) => return false,
        KeyCode::Up => h.scroll = h.scroll.saturating_sub(1),
        KeyCode::Down => h.scroll += 1,
        KeyCode::PageUp => h.scroll = h.scroll.saturating_sub(15),
        KeyCode::PageDown => h.scroll += 15,
        KeyCode::Home => h.scroll = 0,
        _ => {
            if h.input.handle(&k) {
                h.scroll = 0;
            }
        }
    }
    true
}

/// Help text: intro, every binding by context, editors, the query language.
fn help_lines() -> Vec<(String, bool)> {
    let mut v: Vec<(String, bool)> = vec![];
    let head = |v: &mut Vec<(String, bool)>, s: &str| {
        v.push((String::new(), false));
        v.push((s.to_string(), true));
    };
    v.push(("pt keeps ~50 projects and their tasks in one shared database. Everyone sees changes live.".into(), false));
    v.push(("Edit cells directly like a spreadsheet: move with the arrows, type to replace, Enter to open an editor.".into(), false));
    v.push(("The bottom line always shows the keys that work right now. The mouse works everywhere too.".into(), false));
    for (title, ctx) in [("Everywhere", Ctx::Global), ("Grids (Projects, Tasks, Activity, Templates, Fields, Users)", Ctx::Grid), ("Board", Ctx::Board), ("Timeline", Ctx::Timeline), ("Dashboard", Ctx::Dashboard)] {
        head(&mut v, title);
        for b in keymap::BINDINGS.iter().filter(|b| b.ctx == ctx) {
            let keys: Vec<String> = b.keys.iter().map(keymap::key_label).collect();
            v.push((format!("  {:<18} {}", keys.join(" / "), b.action.label()), false));
        }
    }
    head(&mut v, "Grid navigation");
    for (k, d) in [
        ("↑ ↓ ← →", "Move between cells"),
        ("Tab / ⇧Tab", "Next / previous cell"),
        ("Home / End", "First / last column"),
        ("^Home / ^End", "First / last row"),
        ("PgUp / PgDn", "Page"),
        ("Enter / F2 / dbl-click", "Edit the cell"),
        ("type", "Start editing with that character"),
        ("Del", "Clear the cell"),
        ("Space", "Mark / unmark the row"),
        ("⇧↑ / ⇧↓", "Extend the marked rows"),
        ("^C / ^V", "Copy / paste (TSV, many cells at once)"),
        ("← → on a group row", "Collapse / expand; + / - all"),
        ("Esc", "Dismiss a conflict marker / clear marks"),
        ("Mouse", "Click header = sort (⇧ adds), drag border = resize, right-click = menu"),
    ] {
        v.push((format!("  {k:<22} {d}"), false));
    }
    head(&mut v, "Editors");
    for (name, kind) in [("Text", EdKind::Text), ("Date", EdKind::Date), ("Lists", EdKind::Combo), ("Tags & dependencies", EdKind::Chips { free: true }), ("Numbers", EdKind::Number), ("Notes", EdKind::Multiline)] {
        let hs: Vec<String> = keymap::editor_hints(kind).into_iter().map(|(k, d)| format!("{k} {d}")).collect();
        v.push((format!("  {name:<20} {}", hs.join(" · ")), false));
    }
    head(&mut v, "Filter bar (^F)");
    for l in [
        "  word \"two words\"     free text in code, name, title, phase, tags, notes",
        "  status:active,on_hold  field:value (comma = or); -term negates",
        "  @me @bob owner:none    people; #tag for tags",
        "  due<+7d start>=2026-11-01 est>8 pct<50   comparisons; dates: today tom fri +3d eow eom",
        "  is:overdue is:blocked is:mine is:open    has:notes has:deps has:due",
        "  Tab completes field names and values; Enter returns to the grid.",
    ] {
        v.push((l.to_string(), false));
    }
    head(&mut v, "Working together");
    for l in [
        "  Coloured initials in the left gutter show where others are; their cell is underlined.",
        "  If someone changed a value you were editing, the cell turns red (⚠): Enter edits again from",
        "  their value, ^S in the editor saves yours anyway, Esc dismisses. ^Z undoes your own last change.",
    ] {
        v.push((l.to_string(), false));
    }
    v
}

// ---- rendering --------------------------------------------------------------------------

pub fn render(d: &mut Dialog, buf: &mut Buffer, screen: Rect, app: &App) {
    let t = &app.t;
    match d {
        Dialog::Form(f) => render_form(f, buf, screen, app),
        Dialog::Confirm(c) => {
            let w = (width(&c.text) as u16 + 8).clamp(40, screen.width.saturating_sub(4));
            let r = centered(screen, w, 7);
            let inner = modal(buf, r, "Confirm", t);
            put(buf, inner.x + 2, inner.y + 1, &c.text, t.panel().add_modifier(Modifier::BOLD), inner.width as usize - 4);
            let by = inner.y + 3;
            let cancel = Rect::new(inner.x + inner.width / 2 - 14, by, 12, 1);
            let ok = Rect::new(inner.x + inner.width / 2 + 2, by, 12, 1);
            button(buf, cancel, "Cancel (N)", c.sel == 0, t);
            button(buf, ok, "OK (Y)", c.sel == 1, t);
            c.buttons = [cancel, ok];
        }
        Dialog::Columns(c) => {
            let g = app.views.grid_ref(c.grid);
            let h = (g.cols.len() as u16 + 4).min(screen.height.saturating_sub(2));
            let r = centered(screen, 46, h);
            let inner = modal(buf, r, "Columns", t);
            c.rows = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
            let vis = c.rows.height as usize;
            let top = c.sel.saturating_sub(vis.saturating_sub(1));
            for (i, col) in g.cols.iter().enumerate().skip(top).take(vis) {
                let y = inner.y + (i - top) as u16;
                let st = if i == c.sel { Style::new().bg(t.cur_bg).fg(t.fg) } else { t.panel() };
                fill(buf, Rect::new(inner.x, y, inner.width, 1), st);
                let check = if col.hidden { "☐" } else { "☑" };
                put(buf, inner.x + 1, y, &format!("{check} {:<24} {:>3} ch", col.title, col.width), st, inner.width as usize - 2);
            }
            put(buf, inner.x + 1, inner.bottom() - 1, "Space show · [ ] move · +/- width · R reset", t.muted().bg(t.panel), inner.width as usize - 2);
        }
        Dialog::Sort(s) => {
            let g = app.views.grid_ref(s.grid);
            let r = centered(screen, 50, 9);
            let inner = modal(buf, r, "Sort", t);
            for i in 0..3usize {
                let y = inner.y + 1 + i as u16;
                let st = if i == s.sel { Style::new().bg(t.cur_bg).fg(t.fg) } else { t.panel() };
                fill(buf, Rect::new(inner.x + 1, y, inner.width - 2, 1), st);
                let label = ["1st", "2nd", "3rd"][i];
                let text = match s.keys.get(i) {
                    Some((c, desc)) => format!("{label}:  ◂ {:<20} ▸  {}", g.cols[*c].title, if *desc { "▼ descending" } else { "▲ ascending" }),
                    None => format!("{label}:  (none — ←/→ to pick)"),
                };
                put(buf, inner.x + 2, y, &text, st, inner.width as usize - 4);
            }
            put(buf, inner.x + 2, inner.bottom() - 1, "Enter apply · Esc cancel", t.muted().bg(t.panel), 40);
        }
        Dialog::Palette(p) => {
            let w = (screen.width * 3 / 5).clamp(50, 100).min(screen.width);
            let h = (screen.height * 3 / 5).clamp(10, 30).min(screen.height);
            let r = Rect::new(screen.x + (screen.width - w) / 2, screen.y + 2, w, h);
            let inner = modal(buf, r, "Commands — type to search projects, tasks, actions", t);
            put(buf, inner.x + 1, inner.y, "›", Style::new().fg(t.accent).bg(t.panel), 1);
            p.input.render(buf, Rect::new(inner.x + 3, inner.y, inner.width - 4, 1), Style::new().fg(t.fg).bg(t.sel_bg), true);
            p.rows = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(2));
            let vis = p.rows.height as usize;
            if p.sel < p.scroll {
                p.scroll = p.sel;
            } else if p.sel >= p.scroll + vis {
                p.scroll = p.sel + 1 - vis;
            }
            for (row, (i, hits)) in p.filtered.iter().enumerate().skip(p.scroll).take(vis) {
                let y = p.rows.y + (row - p.scroll) as u16;
                let (label, key, _) = &p.items[*i];
                let bg = if row == p.sel { t.cur_bg } else { t.panel };
                fill(buf, Rect::new(inner.x, y, inner.width, 1), Style::new().bg(bg));
                let kw = width(key).min(24);
                put_highlighted(buf, inner.x + 2, y, label, hits, Style::new().bg(bg).fg(t.fg), Style::new().fg(t.accent).add_modifier(Modifier::BOLD), (inner.width as usize).saturating_sub(kw + 5));
                put(buf, inner.right().saturating_sub(kw as u16 + 1), y, key, Style::new().bg(bg).fg(t.muted), kw);
            }
            if p.filtered.is_empty() {
                put(buf, inner.x + 2, p.rows.y, "No matches", t.muted().bg(t.panel), 20);
            }
        }
        Dialog::Help(h) => {
            let r = Rect::new(screen.x + 2, screen.y + 1, screen.width.saturating_sub(4), screen.height.saturating_sub(2));
            let inner = modal(buf, r, "Help — pt keyboard guide", t);
            put(buf, inner.x + 1, inner.y, "Search:", Style::new().fg(t.muted).bg(t.panel), 8);
            h.input.render(buf, Rect::new(inner.x + 9, inner.y, inner.width.saturating_sub(10).min(40), 1), Style::new().fg(t.fg).bg(t.sel_bg), true);
            let q = h.input.text().to_lowercase();
            let lines: Vec<&(String, bool)> = if q.is_empty() {
                h.lines.iter().collect()
            } else {
                // matching lines, each section header kept only if something under it matches
                let mut out: Vec<&(String, bool)> = vec![];
                let mut header: Option<&(String, bool)> = None;
                for l in &h.lines {
                    if l.1 {
                        header = Some(l);
                    } else if l.0.to_lowercase().contains(&q) {
                        if let Some(hd) = header.take() {
                            out.push(hd);
                        }
                        out.push(l);
                    }
                }
                out
            };
            let body = inner.height.saturating_sub(2) as usize;
            h.scroll = h.scroll.min(lines.len().saturating_sub(body));
            for (i, (l, head)) in lines.iter().skip(h.scroll).take(body).enumerate() {
                let st = if *head { Style::new().fg(t.accent).bg(t.panel).add_modifier(Modifier::BOLD) } else { t.panel() };
                put(buf, inner.x + 1, inner.y + 2 + i as u16, l, st, inner.width as usize - 2);
            }
        }
        Dialog::Context(c) => {
            let w = c.items.iter().map(|(l, k, _)| width(l) + width(k) + 6).max().unwrap_or(20).max(24) as u16;
            let h = c.items.len() as u16 + 2;
            let x = c.x.min(screen.right().saturating_sub(w + 2));
            let y = if c.y + h > screen.bottom() { c.y.saturating_sub(h) } else { c.y };
            let r = Rect::new(x, y, w, h);
            shadow(buf, r);
            let inner = frame(buf, r, "", t, true);
            c.rect = r;
            for (i, (label, key, _)) in c.items.iter().enumerate() {
                let yy = inner.y + i as u16;
                let bg = if i == c.sel { t.cur_bg } else { t.panel };
                fill(buf, Rect::new(inner.x, yy, inner.width, 1), Style::new().bg(bg));
                put(buf, inner.x + 1, yy, label, Style::new().bg(bg).fg(t.fg), inner.width as usize - 2);
                put(buf, inner.right().saturating_sub(width(key) as u16 + 1), yy, key, Style::new().bg(bg).fg(t.muted), 12);
            }
        }
        Dialog::Message(m) => {
            let w = m.lines.iter().map(|l| width(l)).max().unwrap_or(20).max(30) as u16 + 6;
            let r = centered(screen, w, m.lines.len() as u16 + 4);
            let inner = modal(buf, r, &m.title, t);
            for (i, l) in m.lines.iter().enumerate() {
                put(buf, inner.x + 2, inner.y + 1 + i as u16, l, t.panel(), inner.width as usize - 3);
            }
        }
    }
}

fn button(buf: &mut Buffer, r: Rect, label: &str, focused: bool, t: &Theme) {
    let st = if focused { Style::new().bg(t.accent).fg(t.bg).add_modifier(Modifier::BOLD) } else { Style::new().bg(t.header_bg).fg(t.fg) };
    fill(buf, r, st);
    put(buf, r.x, r.y, &crate::widgets::draw::center(&format!("[ {label} ]"), r.width as usize), st, r.width as usize);
}

fn render_form(f: &mut Form, buf: &mut Buffer, screen: Rect, app: &App) {
    let t = &app.t;
    let label_w = 16u16;
    let w = (screen.width.saturating_sub(4)).clamp(40, 84);
    let mut rows_needed: u16 = f.fields.iter().map(|fl| fl.height + u16::from(fl.error.is_some() || fl.conflict.is_some() || !fl.note.is_empty())).sum();
    rows_needed += 4 + f.footer.len() as u16 + u16::from(f.message.is_some());
    let h = (rows_needed + 2).min(screen.height.saturating_sub(1));
    let r = centered(screen, w, h);
    let inner = modal(buf, r, &f.title, t);
    f.box_rect = r;
    let y = inner.y + 1;
    let bottom = inner.bottom().saturating_sub(2 + f.footer.len().min(6) as u16 + u16::from(f.message.is_some()));
    // scroll so the focused field is visible
    let mut ys = vec![];
    let mut acc = 0u16;
    for fl in &f.fields {
        ys.push(acc);
        acc += fl.height + u16::from(fl.error.is_some() || fl.conflict.is_some() || !fl.note.is_empty());
    }
    let avail = bottom.saturating_sub(y);
    if let Some(&fy) = ys.get(f.focus) {
        let fh = f.fields[f.focus].height + 1;
        if fy < f.scroll as u16 {
            f.scroll = fy as usize;
        } else if fy + fh > f.scroll as u16 + avail {
            f.scroll = (fy + fh).saturating_sub(avail) as usize;
        }
    }
    let skip = f.scroll as u16;
    let mut popup: Option<(usize, Rect)> = None;
    for (i, fl) in f.fields.iter_mut().enumerate() {
        fl.rect = Rect::default();
        let top = ys[i];
        if top < skip {
            continue;
        }
        let fy = y + (top - skip);
        if fy + fl.height > bottom {
            break;
        }
        let focused = i == f.focus;
        let lst = if focused { Style::new().fg(t.accent).bg(t.panel).add_modifier(Modifier::BOLD) } else { t.panel() };
        let label = format!("{}{}", fl.label, if fl.required { " *" } else { "" });
        put(buf, inner.x + 1, fy, &pad(&label, label_w as usize, false), lst, label_w as usize);
        let vr = Rect::new(inner.x + 2 + label_w, fy, inner.width.saturating_sub(label_w + 4), fl.height);
        fl.rect = vr;
        if focused {
            fl.ed.render_inline(buf, vr, t, true);
            popup = Some((i, vr));
        } else {
            fill(buf, vr, Style::new().bg(t.header_bg));
            if fl.ed.kind == EdKind::Multiline {
                for (j, line) in fl.ed.display().lines().take(fl.height as usize).enumerate() {
                    put(buf, vr.x, vr.y + j as u16, line, Style::new().bg(t.header_bg).fg(t.fg), vr.width as usize);
                }
            } else if fl.ed.kind.is_list() && fl.ed.kind != EdKind::Suggest || fl.ed.kind == EdKind::Bool {
                fl.ed.render_inline(buf, vr, t, false);
                crate::widgets::draw::restyle(buf, vr, Style::new().bg(t.header_bg));
            } else {
                put(buf, vr.x, vr.y, &fl.ed.display(), Style::new().bg(t.header_bg).fg(t.fg), vr.width as usize);
            }
        }
        let ny = fy + fl.height;
        let note_x = vr.x;
        if let Some((theirs, by)) = &fl.conflict {
            let who = by.map(|b| app.r.username(Some(b)).to_string()).unwrap_or("someone".into());
            let ent = match f.kind {
                FormKind::Record(e, _) => e,
                _ => Entity::Task,
            };
            let v = cg::display_value(&app.r, ent, &fl.key, theirs);
            put(buf, note_x, ny, &format!("⚠ {who} set this to “{v}” — Enter keeps theirs, ^S saves yours"), Style::new().fg(t.err).bg(t.panel), vr.width as usize);
        } else if let Some(e) = &fl.error {
            put(buf, note_x, ny, &format!("✗ {e}"), Style::new().fg(t.err).bg(t.panel), vr.width as usize);
        } else if !fl.note.is_empty() {
            put(buf, note_x, ny, &fl.note, Style::new().fg(t.muted).bg(t.panel).add_modifier(Modifier::ITALIC), vr.width as usize);
        }
    }
    // message, footer, buttons
    let mut fy = bottom;
    if let Some((m, err)) = &f.message {
        put(buf, inner.x + 1, fy, m, Style::new().fg(if *err { t.err } else { t.info }).bg(t.panel).add_modifier(Modifier::BOLD), inner.width as usize - 2);
        fy += 1;
    }
    for line in f.footer.iter().take(6) {
        put(buf, inner.x + 1, fy, line, Style::new().fg(t.muted).bg(t.panel), inner.width as usize - 2);
        fy += 1;
    }
    let by = inner.bottom() - 1;
    let n = f.fields.len();
    let save_label = match f.kind {
        FormKind::Login => "Log in",
        FormKind::Record(..) | FormKind::SetPassword(..) | FormKind::SaveFilter => "Save ^S",
        FormKind::BoardScope => "Apply ^S",
        _ => "Create ^S",
    };
    let save = Rect::new(inner.right().saturating_sub(30), by, 14, 1);
    let cancel = Rect::new(inner.right().saturating_sub(15), by, 13, 1);
    button(buf, save, if f.busy { "…" } else { save_label }, f.focus == n, t);
    if f.kind != FormKind::Login {
        button(buf, cancel, "Cancel", f.focus == n + 1, t);
    } else {
        put(buf, cancel.x, by, "^Q quits", t.muted().bg(t.panel), 13);
    }
    f.buttons = [save, cancel];
    if let Some((i, vr)) = popup {
        let fl = &mut f.fields[i];
        fl.ed.render_popup(buf, vr, screen, t);
    }
}
