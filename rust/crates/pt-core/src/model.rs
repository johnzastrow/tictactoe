//! Row types exactly as they travel on the wire (protocol §3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Declares a string enum with serde names, a label and the full list.
macro_rules! str_enum {
    ($name:ident { $($variant:ident = $s:literal / $label:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $s)] $variant),+
        }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub const NAMES: &'static [&'static str] = &[$($s),+];
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $s),+ }
            }
            pub fn label(self) -> &'static str {
                match self { $($name::$variant => $label),+ }
            }
            pub fn parse(s: &str) -> Option<$name> {
                let t = s.trim();
                $name::ALL.iter().copied().find(|v| {
                    v.as_str().eq_ignore_ascii_case(t) || v.label().eq_ignore_ascii_case(t)
                })
            }
            pub fn index(self) -> usize {
                self as usize
            }
        }
    };
}

str_enum!(Role {
    Admin = "admin" / "Admin",
    Member = "member" / "Member",
    Viewer = "viewer" / "Viewer",
});

str_enum!(ProjectStatus {
    Planning = "planning" / "Planning",
    Active = "active" / "Active",
    OnHold = "on_hold" / "On hold",
    Done = "done" / "Done",
    Cancelled = "cancelled" / "Cancelled",
});

str_enum!(TaskStatus {
    Todo = "todo" / "To do",
    InProgress = "in_progress" / "In progress",
    Blocked = "blocked" / "Blocked",
    Review = "review" / "Review",
    Done = "done" / "Done",
});

str_enum!(Priority {
    Low = "low" / "Low",
    Medium = "medium" / "Medium",
    High = "high" / "High",
    Critical = "critical" / "Critical",
});

str_enum!(Health {
    Green = "green" / "Green",
    Amber = "amber" / "Amber",
    Red = "red" / "Red",
});

str_enum!(CfKind {
    Text = "text" / "Text",
    Number = "number" / "Number",
    Date = "date" / "Date",
    Enum = "enum" / "Enum",
});

str_enum!(CfEntity {
    Project = "project" / "Project",
    Task = "task" / "Task",
});

str_enum!(Action {
    Set = "set" / "Set",
    Create = "create" / "Create",
    Delete = "delete" / "Delete",
});

str_enum!(ActState {
    Done = "done" / "Done",
    Undone = "undone" / "Undone",
    Dead = "dead" / "Final",
});

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub active: bool,
}

impl User {
    pub fn initials(&self) -> String {
        let mut it = self.display_name.split_whitespace();
        let a = it.next().and_then(|w| w.chars().next());
        let b = it.last().and_then(|w| w.chars().next());
        match (a, b) {
            (Some(a), Some(b)) => format!("{}{}", a, b).to_uppercase(),
            _ => self.username.chars().take(2).collect::<String>().to_uppercase(),
        }
    }
    pub fn can_edit(&self) -> bool {
        self.role != Role::Viewer
    }
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub id: i64,
    pub name: String,
    pub client: String,
    pub prefix: String,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TemplateTask {
    pub id: i64,
    pub template_id: i64,
    pub seq: i64,
    pub title: String,
    pub phase: String,
    pub est_hours: Option<f64>,
    pub offset_days: i64,
    pub duration_days: i64,
}

/// Custom field values keyed by field id as a string (protocol §3).
pub type CfValues = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub program_id: Option<i64>,
    pub template_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub status: ProjectStatus,
    pub priority: Priority,
    pub health: Health,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub budget_hours: Option<f64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub cf: CfValues,
    pub created_at: String,
    pub updated_at: String,
    pub updated_by: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    pub seq: i64,
    pub title: String,
    pub phase: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub assignee_id: Option<i64>,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub est_hours: Option<f64>,
    pub actual_hours: f64,
    #[serde(default)]
    pub deps: Vec<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub cf: CfValues,
    pub created_at: String,
    pub updated_at: String,
    pub updated_by: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomField {
    pub id: i64,
    pub entity: CfEntity,
    pub name: String,
    pub kind: CfKind,
    #[serde(default)]
    pub options: Vec<String>,
    pub seq: i64,
}

impl CustomField {
    /// Name as used in the query language: lowercase, spaces → `_`.
    pub fn query_name(&self) -> String {
        self.name
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .collect()
    }
    pub fn key(&self) -> String {
        format!("cf.{}", self.id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub id: i64,
    pub ts: String,
    pub user_id: Option<i64>,
    pub group: i64,
    pub action: Action,
    pub entity: String,
    pub entity_id: i64,
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub old: Value,
    #[serde(default)]
    pub new: Value,
    #[serde(default)]
    pub label: String,
    pub state: ActState,
}

/// The entity names used on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Entity {
    User,
    Program,
    Template,
    TemplateTask,
    Project,
    Task,
    CustomField,
}

impl Entity {
    pub const ALL: [Entity; 7] = [
        Entity::User,
        Entity::Program,
        Entity::Template,
        Entity::TemplateTask,
        Entity::Project,
        Entity::Task,
        Entity::CustomField,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Entity::User => "user",
            Entity::Program => "program",
            Entity::Template => "template",
            Entity::TemplateTask => "template_task",
            Entity::Project => "project",
            Entity::Task => "task",
            Entity::CustomField => "custom_field",
        }
    }
    /// Key in snapshot / changes objects.
    pub fn plural(self) -> &'static str {
        match self {
            Entity::User => "users",
            Entity::Program => "programs",
            Entity::Template => "templates",
            Entity::TemplateTask => "template_tasks",
            Entity::Project => "projects",
            Entity::Task => "tasks",
            Entity::CustomField => "custom_fields",
        }
    }
    pub fn table(self) -> &'static str {
        self.plural()
    }
    pub fn parse(s: &str) -> Option<Entity> {
        Entity::ALL.iter().copied().find(|e| e.as_str() == s)
    }
    pub fn label(self) -> &'static str {
        match self {
            Entity::User => "user",
            Entity::Program => "program",
            Entity::Template => "template",
            Entity::TemplateTask => "template task",
            Entity::Project => "project",
            Entity::Task => "task",
            Entity::CustomField => "custom field",
        }
    }
}

impl Serialize for Entity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Entity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Entity::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown entity {s}")))
    }
}

/// Everything a snapshot carries. Also the shape of `changes.upserts`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tables {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<User>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<Program>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<Template>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub template_tasks: Vec<TemplateTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<Project>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<Task>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_fields: Vec<CustomField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity: Vec<Activity>,
}

impl Tables {
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
            && self.programs.is_empty()
            && self.templates.is_empty()
            && self.template_tasks.is_empty()
            && self.projects.is_empty()
            && self.tasks.is_empty()
            && self.custom_fields.is_empty()
            && self.activity.is_empty()
    }
}

/// `changes.deletes`: ids per entity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Deletes {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub template_tasks: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_fields: Vec<i64>,
}

impl Deletes {
    pub fn get_mut(&mut self, e: Entity) -> &mut Vec<i64> {
        match e {
            Entity::User => &mut self.users,
            Entity::Program => &mut self.programs,
            Entity::Template => &mut self.templates,
            Entity::TemplateTask => &mut self.template_tasks,
            Entity::Project => &mut self.projects,
            Entity::Task => &mut self.tasks,
            Entity::CustomField => &mut self.custom_fields,
        }
    }
    pub fn get(&self, e: Entity) -> &[i64] {
        match e {
            Entity::User => &self.users,
            Entity::Program => &self.programs,
            Entity::Template => &self.templates,
            Entity::TemplateTask => &self.template_tasks,
            Entity::Project => &self.projects,
            Entity::Task => &self.tasks,
            Entity::CustomField => &self.custom_fields,
        }
    }
    pub fn is_empty(&self) -> bool {
        Entity::ALL.iter().all(|e| self.get(*e).is_empty())
    }
}
