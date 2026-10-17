//! Field metadata and value validation (protocol §4).
//!
//! The server uses this to validate every write; clients use the same table
//! to pick editors and to parse typed or pasted text. Field and table names
//! here are the *only* identifiers the server ever puts into SQL text.

use serde_json::{Number, Value};

use crate::date::Date;
use crate::model::{CfKind, Entity};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Text { min: usize, max: usize },
    Code,
    Username,
    Prefix,
    Color,
    Enum(&'static [&'static str]),
    Ref(Entity),
    Date,
    Hours,
    Int { min: i64, max: i64 },
    Tags,
    Deps,
    Options,
    Bool,
    Password,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perm {
    Member,
    Admin,
}

#[derive(Debug)]
pub struct FieldMeta {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub nullable: bool,
    pub perm: Perm,
    pub create_only: bool,
    pub required: bool,
}

const fn f(name: &'static str, label: &'static str, kind: Kind) -> FieldMeta {
    FieldMeta {
        name,
        label,
        kind,
        nullable: false,
        perm: Perm::Member,
        create_only: false,
        required: false,
    }
}

impl FieldMeta {
    const fn null(mut self) -> Self {
        self.nullable = true;
        self
    }
    const fn admin(mut self) -> Self {
        self.perm = Perm::Admin;
        self
    }
    const fn create(mut self) -> Self {
        self.create_only = true;
        self
    }
    const fn req(mut self) -> Self {
        self.required = true;
        self
    }
    /// Stored in a side table rather than a column of the entity's table.
    pub fn is_side_table(&self) -> bool {
        matches!(self.kind, Kind::Tags | Kind::Deps | Kind::Password)
    }
}

pub const PROJECT_STATUS: &[&str] = &["planning", "active", "on_hold", "done", "cancelled"];
pub const TASK_STATUS: &[&str] = &["todo", "in_progress", "blocked", "review", "done"];
pub const PRIORITY: &[&str] = &["low", "medium", "high", "critical"];
pub const HEALTH: &[&str] = &["green", "amber", "red"];
pub const ROLES: &[&str] = &["admin", "member", "viewer"];
pub const CF_ENTITIES: &[&str] = &["project", "task"];
pub const CF_KINDS: &[&str] = &["text", "number", "date", "enum"];

const NOTES: Kind = Kind::Text { min: 0, max: 20_000 };

static PROJECT: &[FieldMeta] = &[
    f("code", "Code", Kind::Code).req(),
    f("name", "Name", Kind::Text { min: 1, max: 200 }).req(),
    f("program_id", "Program", Kind::Ref(Entity::Program)).null(),
    f("template_id", "Template", Kind::Ref(Entity::Template)).null().create(),
    f("owner_id", "Owner", Kind::Ref(Entity::User)).null(),
    f("status", "Status", Kind::Enum(PROJECT_STATUS)),
    f("priority", "Priority", Kind::Enum(PRIORITY)),
    f("health", "Health", Kind::Enum(HEALTH)),
    f("start_date", "Start", Kind::Date).null(),
    f("due_date", "Due", Kind::Date).null(),
    f("budget_hours", "Budget h", Kind::Hours).null(),
    f("notes", "Notes", NOTES),
    f("tags", "Tags", Kind::Tags),
];

static TASK: &[FieldMeta] = &[
    f("project_id", "Project", Kind::Ref(Entity::Project)).create().req(),
    f("title", "Title", Kind::Text { min: 1, max: 200 }).req(),
    f("phase", "Phase", Kind::Text { min: 0, max: 60 }),
    f("status", "Status", Kind::Enum(TASK_STATUS)),
    f("priority", "Priority", Kind::Enum(PRIORITY)),
    f("assignee_id", "Assignee", Kind::Ref(Entity::User)).null(),
    f("start_date", "Start", Kind::Date).null(),
    f("due_date", "Due", Kind::Date).null(),
    f("est_hours", "Est h", Kind::Hours).null(),
    f("actual_hours", "Actual h", Kind::Hours),
    f("notes", "Notes", NOTES),
    f("tags", "Tags", Kind::Tags),
    f("deps", "Deps", Kind::Deps),
];

static PROGRAM: &[FieldMeta] = &[
    f("name", "Name", Kind::Text { min: 1, max: 100 }).req().admin(),
    f("client", "Client", Kind::Text { min: 0, max: 100 }).admin(),
    f("prefix", "Prefix", Kind::Prefix).admin(),
    f("color", "Colour", Kind::Color).admin(),
];

static TEMPLATE: &[FieldMeta] = &[
    f("name", "Name", Kind::Text { min: 1, max: 100 }).req().admin(),
    f("description", "Description", Kind::Text { min: 0, max: 2000 }).admin(),
];

static TEMPLATE_TASK: &[FieldMeta] = &[
    f("template_id", "Template", Kind::Ref(Entity::Template)).create().req().admin(),
    f("seq", "Seq", Kind::Int { min: 0, max: 100_000 }).admin(),
    f("title", "Title", Kind::Text { min: 1, max: 200 }).req().admin(),
    f("phase", "Phase", Kind::Text { min: 0, max: 60 }).admin(),
    f("est_hours", "Est h", Kind::Hours).null().admin(),
    f("offset_days", "Offset d", Kind::Int { min: 0, max: 3650 }).admin(),
    f("duration_days", "Duration d", Kind::Int { min: 1, max: 3650 }).admin(),
];

static CUSTOM_FIELD: &[FieldMeta] = &[
    f("entity", "Entity", Kind::Enum(CF_ENTITIES)).create().req().admin(),
    f("name", "Name", Kind::Text { min: 1, max: 40 }).req().admin(),
    f("kind", "Kind", Kind::Enum(CF_KINDS)).create().req().admin(),
    f("options", "Options", Kind::Options).admin(),
    f("seq", "Seq", Kind::Int { min: 0, max: 100_000 }).admin(),
];

static USER: &[FieldMeta] = &[
    f("username", "Username", Kind::Username).create().req().admin(),
    f("display_name", "Name", Kind::Text { min: 1, max: 80 }).req().admin(),
    f("role", "Role", Kind::Enum(ROLES)).admin(),
    f("active", "Active", Kind::Bool).admin(),
    f("password", "Password", Kind::Password).create().req().admin(),
];

pub fn fields(e: Entity) -> &'static [FieldMeta] {
    match e {
        Entity::Project => PROJECT,
        Entity::Task => TASK,
        Entity::Program => PROGRAM,
        Entity::Template => TEMPLATE,
        Entity::TemplateTask => TEMPLATE_TASK,
        Entity::CustomField => CUSTOM_FIELD,
        Entity::User => USER,
    }
}

pub fn field(e: Entity, name: &str) -> Option<&'static FieldMeta> {
    fields(e).iter().find(|m| m.name == name)
}

/// `cf.12` → 12.
pub fn cf_id(field: &str) -> Option<i64> {
    field.strip_prefix("cf.")?.parse().ok().filter(|&n: &i64| n > 0)
}

pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn num(x: f64) -> Value {
    Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| {
            let f = n.as_f64()?;
            (f.fract() == 0.0 && f.abs() < 9e15).then_some(f as i64)
        }),
        _ => None,
    }
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

pub fn valid_code(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=16).contains(&b.len())
        && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'-')
}

pub fn valid_username(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-'))
}

pub fn valid_tag(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=32).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
}

/// Validate and normalise a value for a built-in field.
/// Reference existence and cross-field rules are checked by the server.
pub fn normalize(meta: &FieldMeta, v: &Value) -> Result<Value, String> {
    let label = meta.label;
    if v.is_null() || v.as_str() == Some("") && meta.nullable {
        if meta.nullable {
            return Ok(Value::Null);
        }
        if let Kind::Text { min: 0, .. } = meta.kind {
            return Ok(Value::String(String::new()));
        }
        if let Kind::Tags | Kind::Deps | Kind::Options = meta.kind {
            return Ok(Value::Array(vec![]));
        }
        return Err(format!("{label} is required"));
    }
    let want_str = || v.as_str().ok_or_else(|| format!("{label} must be text"));
    match meta.kind {
        Kind::Text { min, max } => {
            let s = want_str()?;
            // notes keep their whitespace; one-line fields are trimmed
            let s = if max > 2000 { s.to_string() } else { s.trim().to_string() };
            let n = chars(&s);
            if n < min {
                return Err(format!("{label} is required"));
            }
            if n > max {
                return Err(format!("{label} is longer than {max} characters"));
            }
            Ok(Value::String(s))
        }
        Kind::Code => {
            let s = want_str()?.trim().to_uppercase();
            if !valid_code(&s) {
                return Err(format!("{label} must be 2–16 of A–Z, 0–9 and '-'"));
            }
            Ok(Value::String(s))
        }
        Kind::Username => {
            let s = want_str()?.trim().to_lowercase();
            if !valid_username(&s) {
                return Err("username must be 2–32 of a–z, 0–9, '_', '.', '-' starting with a letter".into());
            }
            Ok(Value::String(s))
        }
        Kind::Prefix => {
            let s = want_str()?.trim().to_uppercase();
            if s.len() > 5 || !s.bytes().all(|c| c.is_ascii_uppercase()) {
                return Err("prefix must be up to 5 letters".into());
            }
            Ok(Value::String(s))
        }
        Kind::Color => {
            let s = want_str()?.trim().to_lowercase();
            let b = s.as_bytes();
            if b.len() != 7 || b[0] != b'#' || !b[1..].iter().all(|c| c.is_ascii_hexdigit()) {
                return Err("colour must look like #3a7bd5".into());
            }
            Ok(Value::String(s))
        }
        Kind::Enum(names) => {
            let s = want_str()?;
            if names.contains(&s) {
                Ok(Value::String(s.to_string()))
            } else {
                Err(format!("{label} must be one of {}", names.join(", ")))
            }
        }
        Kind::Ref(_) => match as_i64(v) {
            Some(n) if n > 0 => Ok(Value::from(n)),
            _ => Err(format!("{label} must be an id")),
        },
        Kind::Date => {
            let s = want_str()?;
            Date::parse_iso(s)
                .map(|d| Value::String(d.iso()))
                .ok_or_else(|| format!("{label} must be a date (YYYY-MM-DD)"))
        }
        Kind::Hours => match as_f64(v) {
            Some(x) if x.is_finite() && (0.0..=1_000_000.0).contains(&x) => Ok(num(round2(x))),
            _ => Err(format!("{label} must be a number of hours between 0 and 1,000,000")),
        },
        Kind::Int { min, max } => match as_i64(v) {
            Some(n) if (min..=max).contains(&n) => Ok(Value::from(n)),
            _ => Err(format!("{label} must be a whole number from {min} to {max}")),
        },
        Kind::Tags => {
            let arr = v.as_array().ok_or_else(|| format!("{label} must be a list"))?;
            let mut out: Vec<String> = Vec::with_capacity(arr.len());
            for t in arr {
                let t = t.as_str().ok_or_else(|| format!("{label} must be text"))?.trim().to_lowercase();
                if !valid_tag(&t) {
                    return Err(format!("tag '{t}' must be 1–32 of a–z, 0–9, '_', '-'"));
                }
                out.push(t);
            }
            out.sort();
            out.dedup();
            if out.len() > 20 {
                return Err("at most 20 tags".into());
            }
            Ok(Value::from(out))
        }
        Kind::Deps => {
            let arr = v.as_array().ok_or_else(|| format!("{label} must be a list"))?;
            let mut out = Vec::with_capacity(arr.len());
            for t in arr {
                out.push(as_i64(t).filter(|n| *n > 0).ok_or_else(|| format!("{label} must be task ids"))?);
            }
            out.sort();
            out.dedup();
            if out.len() > 50 {
                return Err("at most 50 dependencies".into());
            }
            Ok(Value::from(out))
        }
        Kind::Options => {
            let arr = v.as_array().ok_or_else(|| format!("{label} must be a list"))?;
            let mut out: Vec<String> = Vec::with_capacity(arr.len());
            for t in arr {
                let t = t.as_str().ok_or_else(|| format!("{label} must be text"))?.trim().to_string();
                if t.is_empty() || chars(&t) > 40 {
                    return Err("each option must be 1–40 characters".into());
                }
                if !out.contains(&t) {
                    out.push(t);
                }
            }
            if out.len() > 50 {
                return Err("at most 50 options".into());
            }
            Ok(Value::from(out))
        }
        Kind::Bool => match v {
            Value::Bool(b) => Ok(Value::Bool(*b)),
            Value::Number(n) if n.as_i64() == Some(0) || n.as_i64() == Some(1) => {
                Ok(Value::Bool(n.as_i64() == Some(1)))
            }
            _ => Err(format!("{label} must be true or false")),
        },
        Kind::Password => {
            let s = want_str()?;
            let n = chars(s);
            if !(8..=256).contains(&n) {
                return Err("password must be 8–256 characters".into());
            }
            Ok(Value::String(s.to_string()))
        }
    }
}

/// Validate a custom field value. `Null` means "remove".
pub fn normalize_cf(kind: CfKind, options: &[String], name: &str, v: &Value) -> Result<Value, String> {
    if v.is_null() || v.as_str() == Some("") {
        return Ok(Value::Null);
    }
    match kind {
        CfKind::Text => {
            let s = v.as_str().ok_or_else(|| format!("{name} must be text"))?.trim();
            if chars(s) > 500 {
                return Err(format!("{name} is longer than 500 characters"));
            }
            Ok(if s.is_empty() { Value::Null } else { Value::String(s.into()) })
        }
        CfKind::Number => match as_f64(v) {
            Some(x) if x.is_finite() => Ok(num(x)),
            _ => Err(format!("{name} must be a number")),
        },
        CfKind::Date => v
            .as_str()
            .and_then(Date::parse_iso)
            .map(|d| Value::String(d.iso()))
            .ok_or_else(|| format!("{name} must be a date (YYYY-MM-DD)")),
        CfKind::Enum => {
            let s = v.as_str().ok_or_else(|| format!("{name} must be text"))?;
            if options.iter().any(|o| o == s) {
                Ok(Value::String(s.into()))
            } else {
                Err(format!("{name} must be one of {}", options.join(", ")))
            }
        }
    }
}

/// Equality used for optimistic checks (protocol §5).
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() < 1e-9,
            _ => false,
        },
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return false;
            }
            let key = |v: &Value| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let mut xs: Vec<String> = x.iter().map(key).collect();
            let mut ys: Vec<String> = y.iter().map(key).collect();
            xs.sort();
            ys.sort();
            xs == ys
        }
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        // A missing value and an empty string are the same thing for text.
        (Value::Null, Value::String(s)) | (Value::String(s), Value::Null) => s.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn n(e: Entity, f: &str, v: Value) -> Result<Value, String> {
        normalize(field(e, f).unwrap(), &v)
    }

    #[test]
    fn normalisation() {
        assert_eq!(n(Entity::Project, "code", json!(" nwr-004 ")), Ok(json!("NWR-004")));
        assert!(n(Entity::Project, "code", json!("X")).is_err());
        assert_eq!(n(Entity::Task, "tags", json!(["B", "a", "b"])), Ok(json!(["a", "b"])));
        assert!(n(Entity::Task, "tags", json!(["bad tag"])).is_err());
        assert_eq!(n(Entity::Task, "est_hours", json!(1.234)), Ok(json!(1.23)));
        assert!(n(Entity::Task, "est_hours", json!(-1)).is_err());
        assert_eq!(n(Entity::Task, "est_hours", json!(null)), Ok(json!(null)));
        assert!(n(Entity::Task, "actual_hours", json!(null)).is_err());
        assert_eq!(n(Entity::Task, "notes", json!(null)), Ok(json!("")));
        assert!(n(Entity::Task, "title", json!("  ")).is_err());
        assert!(n(Entity::Task, "status", json!("done")).is_ok());
        assert!(n(Entity::Task, "status", json!("Done")).is_err());
        assert_eq!(n(Entity::Task, "due_date", json!("")), Ok(json!(null)));
        assert!(n(Entity::Task, "due_date", json!("2026-02-30")).is_err());
        assert_eq!(n(Entity::Task, "deps", json!([3, 1, 3])), Ok(json!([1, 3])));
        assert_eq!(n(Entity::User, "active", json!(1)), Ok(json!(true)));
        assert_eq!(n(Entity::Program, "color", json!("#AABBCC")), Ok(json!("#aabbcc")));
    }

    #[test]
    fn custom() {
        let opts = vec!["North".to_string(), "South".to_string()];
        assert_eq!(normalize_cf(CfKind::Enum, &opts, "Region", &json!("North")), Ok(json!("North")));
        assert!(normalize_cf(CfKind::Enum, &opts, "Region", &json!("East")).is_err());
        assert_eq!(normalize_cf(CfKind::Number, &[], "Risk", &json!(4.5)), Ok(json!(4.5)));
        assert_eq!(normalize_cf(CfKind::Text, &[], "X", &json!("")), Ok(json!(null)));
    }

    #[test]
    fn equality() {
        assert!(values_equal(&json!(1), &json!(1.0)));
        assert!(values_equal(&json!(["b", "a"]), &json!(["a", "b"])));
        assert!(values_equal(&json!([2, 1]), &json!([1, 2])));
        assert!(!values_equal(&json!("a"), &json!(null)));
        assert!(values_equal(&json!(""), &json!(null)));
        assert!(!values_equal(&json!("todo"), &json!("done")));
    }
}
