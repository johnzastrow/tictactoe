//! Application state (one `App`, provided as context), the action catalogue
//! shared by keys, menus, the palette and the hint bar, and the glue between
//! the WebSocket and the replica.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;

use leptos::prelude::*;
use leptos::task::spawn_local;
use pt_core::Replica;
use pt_core::grid::{self, Column, GridRow, SortSpec};
use pt_core::model::*;
use pt_core::protocol::{ClientMsg, Op, Peer, ResultMsg, ServerMsg};
use pt_core::query::{Query, Target};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{net, util};

// ---- identifiers -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ViewId {
    Projects,
    Tasks,
    Board,
    Timeline,
    Dashboard,
    Activity,
    Templates,
    Fields,
    Users,
}

impl ViewId {
    pub const ALL: [ViewId; 9] = [
        ViewId::Projects,
        ViewId::Tasks,
        ViewId::Board,
        ViewId::Timeline,
        ViewId::Dashboard,
        ViewId::Activity,
        ViewId::Templates,
        ViewId::Fields,
        ViewId::Users,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ViewId::Projects => "Projects",
            ViewId::Tasks => "Tasks",
            ViewId::Board => "Board",
            ViewId::Timeline => "Timeline",
            ViewId::Dashboard => "Dashboard",
            ViewId::Activity => "Activity",
            ViewId::Templates => "Templates",
            ViewId::Fields => "Fields",
            ViewId::Users => "Users",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            ViewId::Projects => "projects",
            ViewId::Tasks => "tasks",
            ViewId::Board => "board",
            ViewId::Timeline => "timeline",
            ViewId::Dashboard => "dashboard",
            ViewId::Activity => "activity",
            ViewId::Templates => "templates",
            ViewId::Fields => "fields",
            ViewId::Users => "users",
        }
    }
    pub fn index(self) -> usize {
        ViewId::ALL.iter().position(|v| *v == self).unwrap_or(0)
    }
    pub fn has_query(self) -> bool {
        matches!(
            self,
            ViewId::Projects | ViewId::Tasks | ViewId::Board | ViewId::Timeline | ViewId::Activity
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum GridKey {
    Projects,
    ProjTasks,
    Tasks,
    Activity,
    Templates,
    TemplateTasks,
    Fields,
    Users,
    Programs,
}

impl GridKey {
    pub fn dom_id(self) -> &'static str {
        match self {
            GridKey::Projects => "g-projects",
            GridKey::ProjTasks => "g-projtasks",
            GridKey::Tasks => "g-tasks",
            GridKey::Activity => "g-activity",
            GridKey::Templates => "g-templates",
            GridKey::TemplateTasks => "g-templatetasks",
            GridKey::Fields => "g-fields",
            GridKey::Users => "g-users",
            GridKey::Programs => "g-programs",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            GridKey::Projects => "Projects",
            GridKey::ProjTasks => "Project tasks",
            GridKey::Tasks => "Tasks",
            GridKey::Activity => "Activity",
            GridKey::Templates => "Templates",
            GridKey::TemplateTasks => "Template tasks",
            GridKey::Fields => "Custom fields",
            GridKey::Users => "Users",
            GridKey::Programs => "Programs",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnState {
    Connecting,
    Online,
    Offline,
}

/// What has keyboard focus; drives the hint bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ctx {
    Grid,
    Edit,
    Query,
    Board,
    Timeline,
    Dashboard,
    Dialog,
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

#[derive(Clone, PartialEq, Debug)]
pub enum ConfirmAct {
    Delete { entity: Entity, ids: Vec<i64> },
}

#[derive(Clone, PartialEq, Debug)]
pub enum Modal {
    Help,
    Palette,
    About,
    NewProject {
        program: Option<i64>,
    },
    NewTask {
        project: Option<i64>,
        status: Option<TaskStatus>,
    },
    Record {
        entity: Entity,
        id: i64,
    },
    Create {
        entity: Entity,
        preset: Vec<(String, Value)>,
    },
    Confirm {
        title: String,
        body: String,
        act: ConfirmAct,
    },
    Columns(GridKey),
    Sort(GridKey),
    SetPassword {
        user: i64,
    },
    SaveFilter,
    Context {
        x: f64,
        y: f64,
        grid: GridKey,
    },
}

pub type CellKey = (Entity, i64, String);

#[derive(Clone, PartialEq, Debug)]
pub enum Mark {
    Pending,
    Conflict(String),
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(default)]
    pub hidden: BTreeSet<String>,
    #[serde(default)]
    pub shown: BTreeSet<String>,
    #[serde(default)]
    pub widths: BTreeMap<String, u16>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SavedFilter {
    pub name: String,
    pub view: String,
    pub query: String,
    #[serde(default)]
    pub sort: Vec<(String, bool)>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct EditState {
    pub id: i64,
    pub col: String,
    pub field: String,
    pub editor: grid::Editor,
    /// text to seed a text editor with (first typed character, or current value)
    pub seed: Option<String>,
    pub base: Value,
    /// cell rectangle in viewport coordinates, for popups
    pub rect: (f64, f64, f64, f64),
}

// ---- grids --------------------------------------------------------------------------

/// Everything one grid instance needs; all signals, so it is `Copy`.
#[derive(Clone, Copy)]
pub struct Grid {
    pub key: GridKey,
    pub entity: Entity,
    pub layout: RwSignal<Layout>,
    pub all_cols: Memo<Vec<Column>>,
    pub cols: Memo<Vec<Column>>,
    pub ids: Memo<Vec<i64>>,
    pub rows: Memo<Vec<GridRow>>,
    pub sort: RwSignal<Vec<SortSpec>>,
    pub group: RwSignal<Option<String>>,
    pub collapsed: RwSignal<HashSet<String>>,
    pub cur_row: RwSignal<usize>,
    pub cur_col: RwSignal<usize>,
    pub cur_id: RwSignal<Option<i64>>,
    pub marks: RwSignal<BTreeSet<i64>>,
    pub anchor: RwSignal<Option<usize>>,
    pub edit: RwSignal<Option<EditState>>,
    pub frozen: usize,
    pub readonly: bool,
    pub admin_only: bool,
}

impl Grid {
    pub fn cur_column(&self) -> Option<Column> {
        self.cols
            .with(|c| c.get(self.cur_col.get_untracked()).cloned())
    }
    pub fn editable(&self, r: &Replica) -> bool {
        !self.readonly && r.can_edit() && (!self.admin_only || r.is_admin())
    }
    pub fn data_index_of(&self, id: i64) -> Option<usize> {
        self.rows
            .with_untracked(|rows| rows.iter().position(|r| r.id() == Some(id)))
    }
}

fn apply_layout(base: Vec<Column>, l: &Layout) -> Vec<Column> {
    let mut cols: Vec<Column> = base
        .into_iter()
        .map(|mut c| {
            if l.hidden.contains(&c.key) {
                c.hidden = true;
            }
            if l.shown.contains(&c.key) {
                c.hidden = false;
            }
            if let Some(w) = l.widths.get(&c.key) {
                c.width = *w;
            }
            c
        })
        .collect();
    if !l.order.is_empty() {
        let pos = |k: &str| l.order.iter().position(|o| o == k).unwrap_or(usize::MAX);
        // stable: unknown (new) columns keep their relative place at the end
        cols.sort_by_key(|c| pos(&c.key));
    }
    cols
}

#[allow(clippy::too_many_arguments)]
fn make_grid(
    key: GridKey,
    entity: Entity,
    rep: RwSignal<Replica>,
    cols_fn: fn(&Replica) -> Vec<Column>,
    ids: Memo<Vec<i64>>,
    sort: Vec<SortSpec>,
    group: Option<&str>,
    frozen: usize,
    readonly: bool,
    admin_only: bool,
) -> Grid {
    let layout =
        RwSignal::new(util::load_json::<Layout>(&format!("pt.layout.{key:?}")).unwrap_or_default());
    let all_cols = Memo::new(move |_| {
        let base = rep.with(cols_fn);
        layout.with(|l| apply_layout(base, l))
    });
    let cols = Memo::new(move |_| {
        all_cols
            .get()
            .into_iter()
            .filter(|c| !c.hidden)
            .collect::<Vec<_>>()
    });
    let sort = RwSignal::new(sort);
    let group = RwSignal::new(group.map(String::from));
    let collapsed = RwSignal::new(HashSet::new());
    let rows = Memo::new(move |_| {
        let ids = ids.get();
        let sort = sort.get();
        let group = group.get();
        let cols = all_cols.get();
        collapsed.with(|coll| {
            rep.with(|r| grid::build_rows(r, &cols, ids, &sort, group.as_deref(), coll))
        })
    });
    let g = Grid {
        key,
        entity,
        layout,
        all_cols,
        cols,
        ids,
        rows,
        sort,
        group,
        collapsed,
        cur_row: RwSignal::new(0),
        cur_col: RwSignal::new(frozen.saturating_sub(1)),
        cur_id: RwSignal::new(None),
        marks: RwSignal::new(BTreeSet::new()),
        anchor: RwSignal::new(None),
        edit: RwSignal::new(None),
        frozen,
        readonly,
        admin_only,
    };
    // Keep the cursor on the same row id when rows change underneath it.
    Effect::new(move |_| {
        let rows = g.rows.get();
        let id = g.cur_id.get_untracked();
        if let Some(i) = id.and_then(|id| rows.iter().position(|r| r.id() == Some(id))) {
            if g.cur_row.get_untracked() != i {
                g.cur_row.set(i);
            }
            return;
        }
        let mut i = g.cur_row.get_untracked().min(rows.len().saturating_sub(1));
        // prefer a data row over a group header
        if rows.get(i).is_some_and(|r| r.id().is_none()) {
            if let Some(j) = rows.iter().skip(i).position(|r| r.id().is_some()) {
                i += j;
            }
        }
        if g.cur_row.get_untracked() != i {
            g.cur_row.set(i);
        }
        let nid = rows.get(i).and_then(|r| r.id());
        if nid != id {
            g.cur_id.set(nid);
        }
    });
    Effect::new(move |_| {
        let n = g.cols.with(|c| c.len());
        if g.cur_col.get_untracked() >= n && n > 0 {
            g.cur_col.set(n - 1);
        }
    });
    // Persist layout changes.
    Effect::new(move |_| {
        let l = layout.get();
        util::save_json(&format!("pt.layout.{key:?}"), &l);
    });
    g
}

#[derive(Clone, Copy)]
pub struct Grids {
    pub projects: Grid,
    pub proj_tasks: Grid,
    pub tasks: Grid,
    pub activity: Grid,
    pub templates: Grid,
    pub template_tasks: Grid,
    pub fields: Grid,
    pub users: Grid,
    pub programs: Grid,
}

impl Grids {
    pub fn get(&self, k: GridKey) -> Grid {
        match k {
            GridKey::Projects => self.projects,
            GridKey::ProjTasks => self.proj_tasks,
            GridKey::Tasks => self.tasks,
            GridKey::Activity => self.activity,
            GridKey::Templates => self.templates,
            GridKey::TemplateTasks => self.template_tasks,
            GridKey::Fields => self.fields,
            GridKey::Users => self.users,
            GridKey::Programs => self.programs,
        }
    }
    pub fn of_view(&self, v: ViewId) -> Vec<Grid> {
        match v {
            ViewId::Projects => vec![self.projects, self.proj_tasks],
            ViewId::Tasks => vec![self.tasks],
            ViewId::Activity => vec![self.activity],
            ViewId::Templates => vec![self.templates, self.template_tasks],
            ViewId::Fields => vec![self.fields],
            ViewId::Users => vec![self.users, self.programs],
            _ => vec![],
        }
    }
}

// ---- board / timeline state ----------------------------------------------------

#[derive(Clone, Copy)]
pub struct BoardState {
    pub program: RwSignal<Option<i64>>,
    pub project: RwSignal<Option<i64>>,
    pub lanes: RwSignal<bool>,
    pub col: RwSignal<usize>,
    pub sel: RwSignal<Option<i64>>,
}

#[derive(Clone, Copy)]
pub struct TimelineState {
    pub zoom: RwSignal<usize>, // 0 day, 1 week, 2 month
    pub expanded: RwSignal<HashSet<i64>>,
    pub cur: RwSignal<usize>,
    pub sel: RwSignal<Option<(Entity, i64)>>,
    pub scroll_to_today: RwSignal<u64>,
}

#[derive(Clone, Copy)]
pub struct Queries {
    pub projects: RwSignal<String>,
    pub tasks: RwSignal<String>,
    pub board: RwSignal<String>,
    pub timeline: RwSignal<String>,
    pub activity: RwSignal<String>,
}

impl Queries {
    pub fn of(&self, v: ViewId) -> Option<(RwSignal<String>, Target)> {
        match v {
            ViewId::Projects => Some((self.projects, Target::Projects)),
            ViewId::Tasks => Some((self.tasks, Target::Tasks)),
            ViewId::Board => Some((self.board, Target::Tasks)),
            ViewId::Timeline => Some((self.timeline, Target::Projects)),
            ViewId::Activity => Some((self.activity, Target::Activity)),
            _ => None,
        }
    }
}

pub const THEMES: [(&str, &str); 4] = [
    ("midnight", "Midnight"),
    ("daylight", "Daylight"),
    ("solarized", "Solarized Dark"),
    ("gruvbox", "Gruvbox"),
];

// ---- the app ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct App {
    pub rep: RwSignal<Replica>,
    pub peers: RwSignal<Vec<Peer>>,
    pub user: RwSignal<Option<User>>,
    pub booting: RwSignal<bool>,
    pub conn: RwSignal<ConnState>,
    pub view: RwSignal<ViewId>,
    pub theme: RwSignal<usize>,
    pub compact: RwSignal<bool>,
    pub modal: RwSignal<Option<Modal>>,
    pub menu: RwSignal<Option<usize>>,
    pub menu_sel: RwSignal<usize>,
    pub toasts: RwSignal<Vec<Toast>>,
    pub status: RwSignal<(Level, String)>,
    pub pending: RwSignal<usize>,
    pub marks: RwSignal<HashMap<CellKey, Mark>>,
    pub flash: RwSignal<HashSet<(Entity, i64)>>,
    pub ctx: RwSignal<Ctx>,
    pub detail: RwSignal<bool>,
    pub q: Queries,
    pub grids: Grids,
    pub last_grid: RwSignal<GridKey>,
    pub board: BoardState,
    pub tl: TimelineState,
    pub saved: RwSignal<Vec<SavedFilter>>,
    pub clip: StoredValue<String>,
    backoff: StoredValue<u32>,
    toast_id: StoredValue<u64>,
    focus_gen: StoredValue<u64>,
}

pub fn use_app() -> App {
    expect_context::<App>()
}

fn task_ids(rep: RwSignal<Replica>, q: RwSignal<String>) -> Memo<Vec<i64>> {
    Memo::new(move |_| {
        let q = q.get();
        rep.with(|r| {
            let q = Query::compile(&q, Target::Tasks, r);
            r.tasks
                .values()
                .filter(|t| q.matches_task(t, r))
                .map(|t| t.id)
                .collect()
        })
    })
}

impl App {
    pub fn new() -> App {
        let rep = RwSignal::new(Replica::new(util::today()));
        let q = Queries {
            projects: RwSignal::new(String::new()),
            tasks: RwSignal::new(String::new()),
            board: RwSignal::new(String::new()),
            timeline: RwSignal::new(String::new()),
            activity: RwSignal::new(String::new()),
        };
        let asc = |k: &str| SortSpec {
            key: k.into(),
            desc: false,
        };

        let project_ids = Memo::new(move |_| {
            let q = q.projects.get();
            rep.with(|r| {
                let q = Query::compile(&q, Target::Projects, r);
                r.projects
                    .values()
                    .filter(|p| q.matches_project(p, r))
                    .map(|p| p.id)
                    .collect()
            })
        });
        let projects = make_grid(
            GridKey::Projects,
            Entity::Project,
            rep,
            grid::project_columns,
            project_ids,
            vec![asc("code")],
            None,
            2,
            false,
            false,
        );
        let cur_project = projects.cur_id;
        let pt_ids = Memo::new(move |_| {
            let pid = cur_project.get();
            rep.with(|r| pid.map(|p| r.project_tasks(p).to_vec()).unwrap_or_default())
        });
        let proj_tasks = make_grid(
            GridKey::ProjTasks,
            Entity::Task,
            rep,
            |r| grid::task_columns(r, false),
            pt_ids,
            vec![asc("seq")],
            None,
            2,
            false,
            false,
        );
        let tasks = make_grid(
            GridKey::Tasks,
            Entity::Task,
            rep,
            |r| grid::task_columns(r, true),
            task_ids(rep, q.tasks),
            vec![asc("project"), asc("seq")],
            Some("project"),
            3,
            false,
            false,
        );
        let act_ids = Memo::new(move |_| {
            let q = q.activity.get();
            rep.with(|r| {
                let q = Query::compile(&q, Target::Activity, r);
                r.activity
                    .values()
                    .rev()
                    .filter(|a| q.matches_activity(a, r))
                    .map(|a| a.id)
                    .collect()
            })
        });
        let activity = make_grid(
            GridKey::Activity,
            Entity::User,
            rep,
            |_| grid::activity_columns(),
            act_ids,
            vec![],
            None,
            1,
            true,
            false,
        );
        let tpl_ids = Memo::new(move |_| rep.with(|r| r.templates.keys().copied().collect()));
        let templates = make_grid(
            GridKey::Templates,
            Entity::Template,
            rep,
            |_| grid::template_columns(),
            tpl_ids,
            vec![asc("name")],
            None,
            1,
            false,
            true,
        );
        let cur_tpl = templates.cur_id;
        let tt_ids = Memo::new(move |_| {
            let t = cur_tpl.get();
            rep.with(|r| {
                t.map(|t| r.template_tasks_of(t).iter().map(|x| x.id).collect())
                    .unwrap_or_default()
            })
        });
        let template_tasks = make_grid(
            GridKey::TemplateTasks,
            Entity::TemplateTask,
            rep,
            |_| grid::template_task_columns(),
            tt_ids,
            vec![asc("seq")],
            None,
            2,
            false,
            true,
        );
        let cf_ids = Memo::new(move |_| rep.with(|r| r.custom_fields.keys().copied().collect()));
        let fields = make_grid(
            GridKey::Fields,
            Entity::CustomField,
            rep,
            |_| grid::custom_field_columns(),
            cf_ids,
            vec![asc("entity"), asc("seq")],
            None,
            2,
            false,
            true,
        );
        let user_ids = Memo::new(move |_| rep.with(|r| r.users.keys().copied().collect()));
        let users = make_grid(
            GridKey::Users,
            Entity::User,
            rep,
            |_| grid::user_columns(),
            user_ids,
            vec![asc("username")],
            None,
            1,
            false,
            true,
        );
        let prog_ids = Memo::new(move |_| rep.with(|r| r.programs.keys().copied().collect()));
        let programs = make_grid(
            GridKey::Programs,
            Entity::Program,
            rep,
            |_| grid::program_columns(),
            prog_ids,
            vec![asc("name")],
            None,
            1,
            false,
            true,
        );
        let theme = util::load("pt.theme")
            .and_then(|t| t.parse().ok())
            .filter(|t: &usize| *t < THEMES.len());
        let theme = theme.unwrap_or_else(|| {
            let light = util::window()
                .match_media("(prefers-color-scheme: light)")
                .ok()
                .flatten()
                .is_some_and(|m| m.matches());
            if light { 1 } else { 0 }
        });
        let app = App {
            rep,
            peers: RwSignal::new(vec![]),
            user: RwSignal::new(None),
            booting: RwSignal::new(true),
            conn: RwSignal::new(ConnState::Connecting),
            view: RwSignal::new(ViewId::Projects),
            theme: RwSignal::new(theme),
            compact: RwSignal::new(util::load("pt.compact").as_deref() == Some("1")),
            modal: RwSignal::new(None),
            menu: RwSignal::new(None),
            menu_sel: RwSignal::new(0),
            toasts: RwSignal::new(vec![]),
            status: RwSignal::new((
                Level::Info,
                "Welcome to pt. Press F1 for help, ^K for commands.".into(),
            )),
            pending: RwSignal::new(0),
            marks: RwSignal::new(HashMap::new()),
            flash: RwSignal::new(HashSet::new()),
            ctx: RwSignal::new(Ctx::Grid),
            detail: RwSignal::new(true),
            q,
            grids: Grids {
                projects,
                proj_tasks,
                tasks,
                activity,
                templates,
                template_tasks,
                fields,
                users,
                programs,
            },
            last_grid: RwSignal::new(GridKey::Projects),
            board: BoardState {
                program: RwSignal::new(None),
                project: RwSignal::new(None),
                lanes: RwSignal::new(false),
                col: RwSignal::new(0),
                sel: RwSignal::new(None),
            },
            tl: TimelineState {
                zoom: RwSignal::new(1),
                expanded: RwSignal::new(HashSet::new()),
                cur: RwSignal::new(0),
                sel: RwSignal::new(None),
                scroll_to_today: RwSignal::new(0),
            },
            saved: RwSignal::new(util::load_json("pt.saved").unwrap_or_default()),
            clip: StoredValue::new(String::new()),
            backoff: StoredValue::new(500),
            toast_id: StoredValue::new(0),
            focus_gen: StoredValue::new(0),
        };
        Effect::new(move |_| util::save("pt.theme", &app.theme.get().to_string()));
        Effect::new(move |_| util::save("pt.compact", if app.compact.get() { "1" } else { "0" }));
        Effect::new(move |_| {
            let s = app.saved.get();
            util::save_json("pt.saved", &s);
        });
        app.presence_effect();
        app
    }

    pub fn can_edit(&self) -> bool {
        self.rep.with_untracked(|r| r.can_edit())
    }

    pub fn is_admin(&self) -> bool {
        self.rep.with_untracked(|r| r.is_admin())
    }

    pub fn me_id(&self) -> i64 {
        self.rep.with_untracked(|r| r.me_id())
    }

    pub fn grid(&self, k: GridKey) -> Grid {
        self.grids.get(k)
    }

    /// The grid the user last worked in, if it belongs to the current view.
    pub fn current_grid(&self) -> Option<Grid> {
        let v = self.view.get_untracked();
        let gs = self.grids.of_view(v);
        let last = self.last_grid.get_untracked();
        gs.iter()
            .find(|g| g.key == last)
            .copied()
            .or_else(|| gs.first().copied())
    }

    // ---- messages to the user ----

    pub fn toast(&self, level: Level, text: impl Into<String>) {
        let text = text.into();
        let id = self.toast_id.get_value() + 1;
        self.toast_id.set_value(id);
        self.status.set((level, text.clone()));
        self.toasts.update(|t| {
            t.push(Toast { id, level, text });
            while t.len() > 3 {
                t.remove(0);
            }
        });
        let toasts = self.toasts;
        set_timeout(
            move || toasts.update(|t| t.retain(|x| x.id != id)),
            Duration::from_millis(if level == Level::Err { 6000 } else { 4000 }),
        );
    }

    pub fn say(&self, level: Level, text: impl Into<String>) {
        self.status.set((level, text.into()));
    }

    // ---- session ----

    pub fn boot(self) {
        // Only probe the session if this browser had one (avoids a noisy 401).
        if util::load("pt.session").as_deref() != Some("1") {
            self.booting.set(false);
            return;
        }
        spawn_local(async move {
            match net::me().await {
                Ok(Some(u)) => self.start_session(u),
                Ok(None) => {
                    util::save("pt.session", "0");
                    self.booting.set(false);
                }
                Err(e) => {
                    self.booting.set(false);
                    self.say(Level::Err, format!("Server unreachable: {e}"));
                }
            }
        });
    }

    pub fn start_session(self, user: User) {
        util::save("pt.session", "1");
        self.booting.set(false);
        self.rep.update(|r| r.me = Some(user.clone()));
        self.user.set(Some(user));
        self.conn.set(ConnState::Connecting);
        self.open_socket();
    }

    fn open_socket(self) {
        net::connect(
            move || {},
            move |m| self.on_server(m),
            move || self.on_disconnect(),
        );
    }

    fn on_disconnect(self) {
        if self.user.get_untracked().is_none() {
            return;
        }
        self.conn.set(ConnState::Offline);
        self.pending.set(0);
        self.marks.update(|m| m.retain(|_, v| *v != Mark::Pending));
        let delay = self.backoff.get_value();
        self.backoff.set_value((delay * 2).min(5000));
        self.say(
            Level::Warn,
            "Connection lost. Reconnecting… (edits are disabled)",
        );
        set_timeout(
            move || {
                spawn_local(async move {
                    match net::me().await {
                        Ok(Some(_)) => self.open_socket(),
                        Ok(None) => {
                            self.end_session();
                            self.toast(Level::Warn, "Your session ended. Please log in again.");
                        }
                        Err(_) => self.on_disconnect(),
                    }
                });
            },
            Duration::from_millis(delay as u64),
        );
    }

    fn end_session(self) {
        util::save("pt.session", "0");
        net::close();
        self.user.set(None);
        self.modal.set(None);
        self.rep.set(Replica::new(util::today()));
        self.peers.set(vec![]);
    }

    pub fn logout(self) {
        spawn_local(async move {
            net::logout().await;
            self.end_session();
            self.say(Level::Info, "Logged out.");
        });
    }

    pub fn on_server(self, msg: ServerMsg) {
        match msg {
            ServerMsg::Presence { peers } => self.peers.set(peers),
            ServerMsg::Welcome { .. } => {
                self.rep.update(|r| {
                    r.apply(&msg);
                });
                self.conn.set(ConnState::Online);
                self.backoff.set_value(500);
            }
            ServerMsg::Snapshot { .. } => {
                self.rep.update(|r| {
                    r.set_today(util::today());
                    r.apply(&msg);
                });
                let n = self
                    .rep
                    .with_untracked(|r| (r.projects.len(), r.tasks.len()));
                self.say(
                    Level::Ok,
                    format!("Connected · {} projects · {} tasks", n.0, n.1),
                );
            }
            ServerMsg::Changes(ref c) => {
                let mut applied = Default::default();
                self.rep.update(|r| applied = r.apply(&msg));
                let applied: pt_core::replica::Applied = applied;
                if c.by.is_some() && c.by != Some(self.me_id()) {
                    let keys: Vec<(Entity, i64)> = applied.upserted.clone();
                    self.flash.update(|f| f.extend(keys.iter().copied()));
                    let flash = self.flash;
                    set_timeout(
                        move || {
                            flash.update(|f| {
                                keys.iter().for_each(|k| {
                                    f.remove(k);
                                })
                            })
                        },
                        Duration::from_millis(1200),
                    );
                    let who = self.rep.with_untracked(|r| r.user_name(c.by).to_string());
                    let fresh: Vec<&Activity> = c
                        .activity
                        .iter()
                        .filter(|a| a.state == ActState::Done && a.group == c.group.unwrap_or(-1))
                        .collect();
                    if let Some(a) = fresh.first() {
                        let more = if fresh.len() > 1 {
                            format!(" (+{} more)", fresh.len() - 1)
                        } else {
                            String::new()
                        };
                        self.toast(Level::Info, format!("{who}: {}{more}", a.label));
                    } else if !c.activity.is_empty() {
                        self.toast(Level::Info, format!("{who} undid or redid a change"));
                    }
                }
            }
            ServerMsg::Error { message, .. } => self.toast(Level::Err, message),
            ServerMsg::Result(_) | ServerMsg::Pong => {}
        }
    }

    /// Debounced `focus` messages for presence.
    fn presence_effect(self) {
        Effect::new(move |_| {
            let view = self.view.get();
            let focus = self.current_focus();
            let gen_ = self.focus_gen.get_value() + 1;
            self.focus_gen.set_value(gen_);
            let fg = self.focus_gen;
            set_timeout(
                move || {
                    if fg.get_value() != gen_ {
                        return;
                    }
                    let (entity, id, field) = match focus {
                        Some((e, i, f)) => (Some(e), Some(i), f),
                        None => (None, None, None),
                    };
                    net::send(&ClientMsg::Focus {
                        view: Some(view.key().into()),
                        entity,
                        id,
                        field,
                    });
                },
                Duration::from_millis(150),
            );
        });
    }

    /// (entity, id, field) under the cursor of the active grid (tracked).
    fn current_focus(&self) -> Option<(Entity, i64, Option<String>)> {
        let view = self.view.get();
        let last = self.last_grid.get();
        match view {
            ViewId::Board => self
                .board
                .sel
                .get()
                .map(|id| (Entity::Task, id, Some("status".into()))),
            ViewId::Timeline => self.tl.sel.get().map(|(e, id)| (e, id, None)),
            _ => {
                let gs = self.grids.of_view(view);
                let g = gs
                    .iter()
                    .find(|g| g.key == last)
                    .copied()
                    .or_else(|| gs.first().copied())?;
                if g.key == GridKey::Activity {
                    return None;
                }
                let id = g.cur_id.get()?;
                let field = g
                    .cols
                    .with(|c| c.get(g.cur_col.get()).and_then(|c| c.field.clone()));
                Some((g.entity, id, field))
            }
        }
    }

    // ---- writes ----

    /// Send a mutate; marks set cells pending, then conflicted or clean.
    pub fn mutate(self, ops: Vec<Op>, then: impl FnOnce(&ResultMsg) + 'static) {
        if ops.is_empty() {
            return;
        }
        if self.conn.get_untracked() != ConnState::Online {
            self.toast(Level::Err, "Not connected — the change was not saved.");
            return;
        }
        if !self.can_edit() {
            self.toast(Level::Warn, "You have read-only access.");
            return;
        }
        let keys: Vec<Option<CellKey>> = ops
            .iter()
            .map(|op| match op {
                Op::Set {
                    entity, id, field, ..
                } => Some((*entity, *id, field.clone())),
                _ => None,
            })
            .collect();
        self.marks.update(|m| {
            for k in keys.iter().flatten() {
                m.insert(k.clone(), Mark::Pending);
            }
        });
        self.pending.update(|n| *n += 1);
        let keys2 = keys.clone();
        let sent = net::request(
            move |req| ClientMsg::Mutate { req, ops },
            move |r| {
                self.pending.update(|n| *n = n.saturating_sub(1));
                self.marks.update(|m| {
                    for k in keys.iter().flatten() {
                        if m.get(k) == Some(&Mark::Pending) {
                            m.remove(k);
                        }
                    }
                });
                if !r.conflicts.is_empty() {
                    let msgs: Vec<(CellKey, String)> = self.rep.with_untracked(|rep| {
                        r.conflicts
                            .iter()
                            .map(|c| {
                                let who =
                                    c.by.map(|b| rep.user_name(Some(b)).to_string())
                                        .filter(|s| !s.is_empty())
                                        .unwrap_or_else(|| "Someone".into());
                                let label = field_label(rep, c.entity, &c.field);
                                let val = grid::display_value(rep, c.entity, &c.field, &c.current);
                                (
                                    (c.entity, c.id, c.field.clone()),
                                    format!(
                                        "{who} changed {label} to {val}; your edit wasn't applied"
                                    ),
                                )
                            })
                            .collect()
                    });
                    if let Some((_, m)) = msgs.first() {
                        self.toast(Level::Warn, m.clone());
                    }
                    self.marks.update(|m| {
                        for (k, msg) in msgs {
                            m.insert(k, Mark::Conflict(msg));
                        }
                    });
                } else if let Some(e) = &r.error {
                    self.toast(Level::Err, e.message.clone());
                } else if r.ok {
                    let n = keys.len();
                    self.say(
                        Level::Ok,
                        if n == 1 {
                            "Saved.".to_string()
                        } else {
                            format!("Saved {n} changes.")
                        },
                    );
                }
                then(&r);
            },
        );
        if !sent {
            self.pending.update(|n| *n = n.saturating_sub(1));
            self.marks.update(|m| {
                for k in keys2.iter().flatten() {
                    m.remove(k);
                }
            });
            self.toast(Level::Err, "Not connected — the change was not saved.");
        }
    }

    pub fn undo(self, redo: bool) {
        if self.conn.get_untracked() != ConnState::Online {
            self.toast(Level::Err, "Not connected.");
            return;
        }
        let sent = net::request(
            move |req| {
                if redo {
                    ClientMsg::Redo { req }
                } else {
                    ClientMsg::Undo { req }
                }
            },
            move |r| {
                if r.ok {
                    self.say(Level::Ok, if redo { "Redone." } else { "Undone." });
                } else if let Some(e) = &r.error {
                    let lvl = if e.code.starts_with("nothing") {
                        Level::Info
                    } else {
                        Level::Warn
                    };
                    self.toast(lvl, e.message.clone());
                }
            },
        );
        if !sent {
            self.toast(Level::Err, "Not connected.");
        }
    }

    /// Run `f` once the row is in the replica. The server replies to a
    /// mutate before it broadcasts the `changes` that carry the new rows.
    pub fn when_present(self, e: Entity, id: i64, f: impl FnOnce() + 'static) {
        fn attempt(app: App, e: Entity, id: i64, tries: u32, f: Box<dyn FnOnce()>) {
            if app.rep.with_untracked(|r| r.exists(e, id)) || tries == 0 {
                f();
            } else {
                set_timeout(
                    move || attempt(app, e, id, tries - 1, f),
                    Duration::from_millis(25),
                );
            }
        }
        attempt(self, e, id, 80, Box::new(f));
    }

    pub fn mark_of(&self, e: Entity, id: i64, field: &str) -> Option<Mark> {
        self.marks
            .with(|m| m.get(&(e, id, field.to_string())).cloned())
    }

    pub fn clear_conflict(&self, e: Entity, id: i64, field: &str) {
        self.marks.update(|m| {
            m.remove(&(e, id, field.to_string()));
        });
    }

    // ---- navigation ----

    pub fn set_view(self, v: ViewId) {
        self.menu.set(None);
        self.view.set(v);
        if let Some(g) = self.grids.of_view(v).first() {
            self.last_grid.set(g.key);
        }
        let focus_target = match v {
            ViewId::Board => "board",
            ViewId::Timeline => "timeline",
            ViewId::Dashboard => "dashboard",
            _ => self.grids.of_view(v).first().map_or("", |g| g.key.dom_id()),
        };
        if !focus_target.is_empty() {
            util::focus_later(focus_target);
        }
    }

    /// Jump to a record in its home view, clearing filters that hide it.
    pub fn goto(self, e: Entity, id: i64) {
        match e {
            Entity::Project => {
                let g = self.grids.projects;
                if !g.ids.get_untracked().contains(&id) {
                    self.q.projects.set(String::new());
                }
                g.cur_id.set(Some(id));
                self.set_view(ViewId::Projects);
                if let Some(i) = g.data_index_of(id) {
                    g.cur_row.set(i);
                }
            }
            Entity::Task => {
                let g = self.grids.tasks;
                if !g.ids.get_untracked().contains(&id) {
                    self.q.tasks.set(String::new());
                }
                let code = self.rep.with_untracked(|r| {
                    r.tasks
                        .get(&id)
                        .map(|t| r.project_code(t.project_id).to_lowercase())
                });
                if let Some(code) = code {
                    g.collapsed.update(|c| {
                        c.remove(&code);
                    });
                }
                g.cur_id.set(Some(id));
                self.set_view(ViewId::Tasks);
                if let Some(i) = g.data_index_of(id) {
                    g.cur_row.set(i);
                }
            }
            Entity::Template | Entity::TemplateTask => self.set_view(ViewId::Templates),
            Entity::CustomField => self.set_view(ViewId::Fields),
            Entity::User | Entity::Program => self.set_view(ViewId::Users),
        }
    }
}

pub fn field_label(r: &Replica, e: Entity, field: &str) -> String {
    if let Some(id) = pt_core::fields::cf_id(field) {
        return r
            .custom_fields
            .get(&id)
            .map_or(field.to_string(), |c| c.name.clone());
    }
    pt_core::fields::field(e, field).map_or(field.to_string(), |m| m.label.to_string())
}

// ---- actions ------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Help,
    Menu,
    Palette,
    Filter,
    ClearFilter,
    View(ViewId),
    NextView,
    PrevView,
    NextPane,
    Undo,
    Redo,
    New,
    NewProject,
    NewTask,
    EditRecord,
    Delete,
    ToggleDetail,
    Columns,
    SortCol,
    SortDialog,
    GroupCol,
    ClearGroup,
    Theme,
    ThemeSet(usize),
    Density,
    Copy,
    Paste,
    FillDown,
    ClearCell,
    Edit,
    MarkRow,
    MarkAll,
    SaveFilter,
    Logout,
    About,
    Close,
    MoveLeft,
    MoveRight,
    Lanes,
    ZoomIn,
    ZoomOut,
    Today,
    Shrink,
    Extend,
    SetPassword,
}

impl Act {
    pub fn label(self) -> String {
        match self {
            Act::Help => "Keyboard help".into(),
            Act::Menu => "Menu".into(),
            Act::Palette => "Command palette".into(),
            Act::Filter => "Filter…".into(),
            Act::ClearFilter => "Clear filter".into(),
            Act::View(v) => format!("Go to {}", v.label()),
            Act::NextView => "Next view".into(),
            Act::PrevView => "Previous view".into(),
            Act::NextPane => "Next pane".into(),
            Act::Undo => "Undo".into(),
            Act::Redo => "Redo".into(),
            Act::New => "New row".into(),
            Act::NewProject => "New project…".into(),
            Act::NewTask => "New task…".into(),
            Act::EditRecord => "Edit record…".into(),
            Act::Delete => "Delete row(s)".into(),
            Act::ToggleDetail => "Toggle detail pane".into(),
            Act::Columns => "Columns…".into(),
            Act::SortCol => "Sort by column".into(),
            Act::SortDialog => "Sort…".into(),
            Act::GroupCol => "Group by column".into(),
            Act::ClearGroup => "Clear grouping".into(),
            Act::Theme => "Next theme".into(),
            Act::ThemeSet(i) => format!("Theme: {}", THEMES[i].1),
            Act::Density => "Toggle compact density".into(),
            Act::Copy => "Copy".into(),
            Act::Paste => "Paste".into(),
            Act::FillDown => "Fill down".into(),
            Act::ClearCell => "Clear cell".into(),
            Act::Edit => "Edit cell".into(),
            Act::MarkRow => "Mark row".into(),
            Act::MarkAll => "Mark all rows".into(),
            Act::SaveFilter => "Save filter…".into(),
            Act::Logout => "Log out".into(),
            Act::About => "About pt".into(),
            Act::Close => "Close".into(),
            Act::MoveLeft => "Move left".into(),
            Act::MoveRight => "Move right".into(),
            Act::Lanes => "Toggle swimlanes".into(),
            Act::ZoomIn => "Zoom in".into(),
            Act::ZoomOut => "Zoom out".into(),
            Act::Today => "Jump to today".into(),
            Act::Shrink => "Shrink (due −1)".into(),
            Act::Extend => "Extend (due +1)".into(),
            Act::SetPassword => "Set password…".into(),
        }
    }

    /// Key shown in menus, the palette and the hint bar (web bindings).
    pub fn key(self) -> String {
        match self {
            Act::Help => "F1".into(),
            Act::Menu => "F10".into(),
            Act::Palette => "^K".into(),
            Act::Filter => "^F".into(),
            Act::ClearFilter => "".into(),
            Act::View(v) => format!("Alt+{}", v.index() + 1),
            Act::NextView => "Alt+→".into(),
            Act::PrevView => "Alt+←".into(),
            Act::NextPane => "F6".into(),
            Act::Undo => "^Z".into(),
            Act::Redo => "^Y".into(),
            Act::New => "Ins".into(),
            Act::NewProject => "".into(),
            Act::NewTask => "".into(),
            Act::EditRecord => "^E".into(),
            Act::Delete => "F8".into(),
            Act::ToggleDetail => "^B".into(),
            Act::Columns => "F4".into(),
            Act::SortCol => "^O".into(),
            Act::SortDialog => "".into(),
            Act::GroupCol => "^R".into(),
            Act::ClearGroup => "".into(),
            Act::Theme => "^T".into(),
            Act::ThemeSet(_) => "".into(),
            Act::Density => "".into(),
            Act::Copy => "^C".into(),
            Act::Paste => "^V".into(),
            Act::FillDown => "^D".into(),
            Act::ClearCell => "Del".into(),
            Act::Edit => "F2".into(),
            Act::MarkRow => "Space".into(),
            Act::MarkAll => "^A".into(),
            Act::SaveFilter => "".into(),
            Act::Logout => "".into(),
            Act::About => "".into(),
            Act::Close => "Esc".into(),
            Act::MoveLeft => "Shift+←".into(),
            Act::MoveRight => "Shift+→".into(),
            Act::Lanes => "^R".into(),
            Act::ZoomIn => "+".into(),
            Act::ZoomOut => "−".into(),
            Act::Today => "t".into(),
            Act::Shrink => "[".into(),
            Act::Extend => "]".into(),
            Act::SetPassword => "".into(),
        }
    }
}

/// Run an action from anywhere (keys, menus, palette, hint bar).
pub fn run(app: App, act: Act) {
    app.menu.set(None);
    let close_modal = !matches!(act, Act::Close);
    if close_modal && !matches!(act, Act::ThemeSet(_) | Act::Theme | Act::Density) {
        app.modal.set(None);
    }
    let v = app.view.get_untracked();
    match act {
        Act::Help => app.modal.set(Some(Modal::Help)),
        Act::About => app.modal.set(Some(Modal::About)),
        Act::Menu => {
            app.menu.set(Some(0));
            app.menu_sel.set(0);
        }
        Act::Palette => app.modal.set(Some(Modal::Palette)),
        Act::Filter => {
            if v.has_query() {
                util::focus_id("query");
            } else {
                app.say(Level::Info, "This view has no filter.");
            }
        }
        Act::ClearFilter => {
            if let Some((q, _)) = app.q.of(v) {
                q.set(String::new());
            }
        }
        Act::View(v) => app.set_view(v),
        Act::NextView | Act::PrevView => {
            let i = v.index() as i32 + if act == Act::NextView { 1 } else { -1 };
            app.set_view(ViewId::ALL[i.rem_euclid(9) as usize]);
        }
        Act::NextPane => next_pane(),
        Act::Undo => app.undo(false),
        Act::Redo => app.undo(true),
        Act::NewProject => {
            let program = app.grids.projects.cur_id.get_untracked().and_then(|id| {
                app.rep
                    .with_untracked(|r| r.projects.get(&id).and_then(|p| p.program_id))
            });
            app.modal.set(Some(Modal::NewProject { program }));
        }
        Act::NewTask => {
            let project = context_project(app);
            app.modal.set(Some(Modal::NewTask {
                project,
                status: None,
            }));
        }
        Act::ToggleDetail => app.detail.update(|d| *d = !*d),
        Act::Theme => app.theme.update(|t| *t = (*t + 1) % THEMES.len()),
        Act::ThemeSet(i) => app.theme.set(i),
        Act::Density => app.compact.update(|c| *c = !*c),
        Act::SaveFilter => app.modal.set(Some(Modal::SaveFilter)),
        Act::Logout => app.logout(),
        Act::Close => {
            if app.modal.get_untracked().is_some() {
                app.modal.set(None);
            }
        }
        Act::MoveLeft | Act::MoveRight | Act::Lanes if v == ViewId::Board => {
            crate::board::act(app, act)
        }
        Act::ZoomIn
        | Act::ZoomOut
        | Act::Today
        | Act::Shrink
        | Act::Extend
        | Act::MoveLeft
        | Act::MoveRight
            if v == ViewId::Timeline =>
        {
            crate::timeline::act(app, act)
        }
        Act::New if v == ViewId::Board => {
            let status = TaskStatus::ALL.get(app.board.col.get_untracked()).copied();
            let project = app.board.project.get_untracked();
            app.modal.set(Some(Modal::NewTask { project, status }));
        }
        Act::New if v == ViewId::Timeline || v == ViewId::Dashboard => run(app, Act::NewProject),
        Act::EditRecord if v == ViewId::Board => {
            if let Some(id) = app.board.sel.get_untracked() {
                app.modal.set(Some(Modal::Record {
                    entity: Entity::Task,
                    id,
                }));
            }
        }
        Act::EditRecord if v == ViewId::Timeline => {
            if let Some((e, id)) = app.tl.sel.get_untracked() {
                app.modal.set(Some(Modal::Record { entity: e, id }));
            }
        }
        Act::SetPassword => {
            let id = if v == ViewId::Users {
                app.grids.users.cur_id.get_untracked()
            } else {
                None
            };
            app.modal.set(Some(Modal::SetPassword {
                user: id.unwrap_or_else(|| app.me_id()),
            }));
        }
        _ => match app.current_grid() {
            Some(g) => crate::gridops::act(app, g, act),
            None => app.say(
                Level::Info,
                format!("{} isn't available here.", act.label()),
            ),
        },
    }
}

fn context_project(app: App) -> Option<i64> {
    match app.view.get_untracked() {
        ViewId::Projects => app.grids.projects.cur_id.get_untracked(),
        ViewId::Tasks => app.grids.tasks.cur_id.get_untracked().and_then(|t| {
            app.rep
                .with_untracked(|r| r.tasks.get(&t).map(|t| t.project_id))
        }),
        ViewId::Board => app.board.project.get_untracked(),
        _ => None,
    }
}

/// Cycle keyboard focus through the panes of the current view (F6).
pub fn next_pane() {
    use wasm_bindgen::JsCast;
    let doc = util::document();
    let Ok(list) = doc.query_selector_all("[data-pane]") else {
        return;
    };
    let panes: Vec<web_sys::HtmlElement> = (0..list.length())
        .filter_map(|i| list.item(i))
        .filter_map(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .filter(|e| e.offset_parent().is_some())
        .collect();
    if panes.is_empty() {
        return;
    }
    let active = util::active_element();
    let cur = panes
        .iter()
        .position(|p| active.as_ref().is_some_and(|a| p.contains(Some(a))));
    let next = cur.map_or(0, |i| (i + 1) % panes.len());
    let _ = panes[next].focus();
}
