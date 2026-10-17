//! Per-view state. Grid views hold `GridState`s; Board, Timeline and
//! Dashboard have their own models, renderers and input handlers.

pub mod board;
pub mod card;
pub mod dashboard;
pub mod timeline;

use pt_core::replica::Replica;

use crate::grid::{GridKind, GridState};

pub const PROJECTS: usize = 0;
pub const TASKS: usize = 1;
pub const BOARD: usize = 2;
pub const TIMELINE: usize = 3;
pub const DASHBOARD: usize = 4;
pub const ACTIVITY: usize = 5;
pub const TEMPLATES: usize = 6;
pub const FIELDS: usize = 7;
pub const USERS: usize = 8;

/// Which grid an operation targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridSel {
    Projects,
    ProjectTasks,
    Tasks,
    Activity,
    Templates,
    TemplateTasks,
    Fields,
    Users,
    Programs,
}

/// A view with a master grid and an optional second grid (F6 switches).
pub struct TwoGrid {
    pub main: GridState,
    pub detail: GridState,
    pub show_detail: bool,
    pub pane: usize,
}

pub struct Views {
    pub projects: TwoGrid,
    pub tasks: GridState,
    pub tasks_card: bool,
    pub board: board::BoardView,
    pub timeline: timeline::TimelineView,
    pub dash: dashboard::DashView,
    pub activity: GridState,
    pub templates: TwoGrid,
    pub fields: GridState,
    pub users: TwoGrid,
}

impl Views {
    pub fn new(r: &Replica) -> Views {
        Views {
            projects: TwoGrid {
                main: GridState::new(GridKind::Projects, r),
                detail: GridState::new(GridKind::ProjectTasks, r),
                show_detail: true,
                pane: 0,
            },
            tasks: GridState::new(GridKind::Tasks, r),
            tasks_card: true,
            board: board::BoardView::default(),
            timeline: timeline::TimelineView::new(r.today),
            dash: dashboard::DashView::default(),
            activity: GridState::new(GridKind::Activity, r),
            templates: TwoGrid {
                main: GridState::new(GridKind::Templates, r),
                detail: GridState::new(GridKind::TemplateTasks, r),
                show_detail: true,
                pane: 0,
            },
            fields: GridState::new(GridKind::Fields, r),
            users: TwoGrid {
                main: GridState::new(GridKind::Users, r),
                detail: GridState::new(GridKind::Programs, r),
                show_detail: true,
                pane: 0,
            },
        }
    }

    pub fn grid(&mut self, g: GridSel) -> &mut GridState {
        match g {
            GridSel::Projects => &mut self.projects.main,
            GridSel::ProjectTasks => &mut self.projects.detail,
            GridSel::Tasks => &mut self.tasks,
            GridSel::Activity => &mut self.activity,
            GridSel::Templates => &mut self.templates.main,
            GridSel::TemplateTasks => &mut self.templates.detail,
            GridSel::Fields => &mut self.fields,
            GridSel::Users => &mut self.users.main,
            GridSel::Programs => &mut self.users.detail,
        }
    }

    pub fn grid_ref(&self, g: GridSel) -> &GridState {
        match g {
            GridSel::Projects => &self.projects.main,
            GridSel::ProjectTasks => &self.projects.detail,
            GridSel::Tasks => &self.tasks,
            GridSel::Activity => &self.activity,
            GridSel::Templates => &self.templates.main,
            GridSel::TemplateTasks => &self.templates.detail,
            GridSel::Fields => &self.fields,
            GridSel::Users => &self.users.main,
            GridSel::Programs => &self.users.detail,
        }
    }

    /// The focused grid of a view, if it is a grid view.
    pub fn active_grid(&self, view: usize) -> Option<GridSel> {
        Some(match view {
            PROJECTS if self.projects.pane == 1 && self.projects.show_detail => GridSel::ProjectTasks,
            PROJECTS => GridSel::Projects,
            TASKS => GridSel::Tasks,
            ACTIVITY => GridSel::Activity,
            TEMPLATES if self.templates.pane == 1 => GridSel::TemplateTasks,
            TEMPLATES => GridSel::Templates,
            FIELDS => GridSel::Fields,
            USERS if self.users.pane == 1 => GridSel::Programs,
            USERS => GridSel::Users,
            _ => return None,
        })
    }
}
