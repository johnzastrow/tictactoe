//! The control keys (protocol §5): catalogue, defaults, validation and
//! normalisation. The server runs [`validate`] + [`check_cross`] on every
//! `set`; clients run the same functions before sending, so a value the UI
//! accepts is a value the server accepts, normalised identically.
//!
//! Values travel as `serde_json::Value`. The typed structs below
//! ([`GridRow`], [`KanbanCard`], …) are for clients that want to read or
//! build list/object values; [`crate::Values::get`] deserialises into them.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::dataset::{self, CHANNELS, KANBAN_COLUMNS, PRIORITIES, SIZES, STATUSES};
use crate::date::Date;

/// One control key: its wire name, owning tab id, a short UI label, and its
/// default as JSON text (parsed by [`default_value`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySpec {
    pub key: &'static str,
    pub tab: &'static str,
    pub label: &'static str,
    pub default_json: &'static str,
}

const fn k(key: &'static str, tab: &'static str, label: &'static str, default_json: &'static str) -> KeySpec {
    KeySpec { key, tab, label, default_json }
}

/// Every key, in protocol §5 order (which is also tab order).
pub const KEYS: [KeySpec; 26] = [
    k("text.name", "text", "Name", r#""Ada Lovelace""#),
    k("text.email", "text", "Email", r#""ada@example.com""#),
    k("text.code", "text", "Code", r#""ENG-042""#),
    k("number.hours", "number", "Hours", "12.5"),
    k("number.qty", "number", "Quantity", "3"),
    k("number.percent", "number", "Percent", "40"),
    k("date.start", "date", "Start", r#""2026-11-02""#),
    k("date.due", "date", "Due", r#""2026-11-20""#),
    k("select.status", "select", "Status", r#""in_progress""#),
    k("select.priority", "select", "Priority", r#""high""#),
    k("combo.owner", "combo", "Owner", r#""carol""#),
    k("combo.reviewer", "combo", "Reviewer", "null"),
    k("tags.tags", "tags", "Tags", r#"["frontend","q4"]"#),
    k(
        "textarea.notes",
        "textarea",
        "Notes",
        r#""Welcome to demo.\nEach tab shows one control on its own.\nChanges are shared live with everyone connected.""#,
    ),
    k("choice.notify", "choice", "Notify me", "true"),
    k("choice.channels", "choice", "Channels", r#"["email","push"]"#),
    k("choice.size", "choice", "Size", r#""m""#),
    k("choice.dark", "choice", "Dark mode", "false"),
    k("cascade.pick", "cascade", "Program ▸ Project ▸ Phase", r#"{"program":"Northwind Retail","project":"POS refresh","phase":"Build"}"#),
    k(
        "grid.rows",
        "grid",
        "Rows",
        r#"[{"id":1,"item":"Laptop stand","qty":4,"price":39.5,"status":"done","due":"2026-10-30"},
 {"id":2,"item":"USB-C dock","qty":2,"price":129,"status":"review","due":"2026-11-04"},
 {"id":3,"item":"Monitor 27in","qty":3,"price":289.99,"status":"in_progress","due":"2026-11-12"},
 {"id":4,"item":"Desk lamp","qty":6,"price":24.75,"status":"todo","due":null},
 {"id":5,"item":"Keyboard","qty":5,"price":89,"status":"blocked","due":"2026-11-20"},
 {"id":6,"item":"Headset","qty":2,"price":149,"status":"todo","due":"2026-12-01"}]"#,
    ),
    k(
        "menus.files",
        "menus",
        "Files",
        r#"[{"name":"Budget 2027.xlsx","starred":false},{"name":"Roadmap.pdf","starred":true},
 {"name":"Team photo.png","starred":false},{"name":"Meeting notes.md","starred":true},
 {"name":"Invoice 1042.pdf","starred":false},{"name":"Logo.svg","starred":false},
 {"name":"Contracts.zip","starred":false},{"name":"Ideas.txt","starred":false}]"#,
    ),
    k("palette.last", "palette", "Last command", r#""""#),
    k("dialog.profile", "dialogs", "Profile", r#"{"name":"Grace Hopper","email":"grace@example.com"}"#),
    k("charts.series", "charts", "Series", "[35,48,52,61,58,70,76,72,64,55,43,38]"),
    k(
        "kanban.cards",
        "kanban",
        "Cards",
        r#"[{"id":1,"title":"Write brief","col":"done"},{"id":2,"title":"Sketch layouts","col":"review"},
 {"id":3,"title":"Pick colours","col":"doing"},{"id":4,"title":"Build prototype","col":"doing"},
 {"id":5,"title":"User test","col":"todo"},{"id":6,"title":"Fix feedback","col":"todo"},
 {"id":7,"title":"Write docs","col":"todo"},{"id":8,"title":"Launch","col":"todo"}]"#,
    ),
    k(
        "gantt.bars",
        "gantt",
        "Bars",
        r#"[{"id":1,"label":"Discovery","start":"2026-10-19","days":10},{"id":2,"label":"Design","start":"2026-10-26","days":14},
 {"id":3,"label":"Build","start":"2026-11-09","days":21},{"id":4,"label":"Test","start":"2026-11-23","days":12},
 {"id":5,"label":"Pilot","start":"2026-12-07","days":7},{"id":6,"label":"Launch","start":"2026-12-14","days":1}]"#,
    ),
];

// ---- limits (protocol §5) ----

pub const MAX_NAME: usize = 80;
pub const MAX_EMAIL: usize = 120;
pub const MAX_HOURS: f64 = 1000.0;
pub const MAX_TAGS: usize = 12;
pub const MAX_NOTES: usize = 5000;
pub const MAX_GRID_ROWS: usize = 50;
pub const MAX_ITEM: usize = 40;
pub const MAX_QTY: i64 = 9999;
pub const MAX_PRICE: f64 = 1_000_000.0;
pub const MAX_FILES: usize = 30;
pub const MAX_FILE_NAME: usize = 40;
pub const MAX_PALETTE: usize = 80;
pub const SERIES_LEN: usize = 12;
pub const MAX_CARDS: usize = 40;
pub const MAX_CARD_TITLE: usize = 60;
pub const MAX_BARS: usize = 20;
pub const MAX_BAR_LABEL: usize = 40;
pub const MAX_BAR_DAYS: i64 = 365;

/// The spec of a key, or `None` if unknown.
pub fn spec(key: &str) -> Option<&'static KeySpec> {
    KEYS.iter().find(|k| k.key == key)
}

pub fn is_key(key: &str) -> bool {
    spec(key).is_some()
}

/// A key's default value, or `None` if unknown.
pub fn default_value(key: &str) -> Option<Value> {
    // The defaults are constants checked by the tests; parsing cannot fail.
    spec(key).and_then(|s| serde_json::from_str(s.default_json).ok())
}

/// Every key with its default, in catalogue order.
pub fn defaults() -> Vec<(&'static str, Value)> {
    KEYS.iter().filter_map(|s| default_value(s.key).map(|v| (s.key, v))).collect()
}

// ---- typed views of the structured values ----

/// `cascade.pick`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CascadePick {
    pub program: Option<String>,
    pub project: Option<String>,
    pub phase: Option<String>,
}

/// `dialog.profile`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub email: String,
}

/// One row of `grid.rows`. `due` is `YYYY-MM-DD` or `None`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridRow {
    pub id: u64,
    pub item: String,
    pub qty: u64,
    pub price: f64,
    pub status: String,
    pub due: Option<String>,
}

impl GridRow {
    /// qty × price, rounded to cents.
    pub fn total(&self) -> f64 {
        round2(self.qty as f64 * self.price)
    }
}

/// One entry of `menus.files`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub starred: bool,
}

/// One card of `kanban.cards`; `col` is a [`KANBAN_COLUMNS`] id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KanbanCard {
    pub id: u64,
    pub title: String,
    pub col: String,
}

/// One bar of `gantt.bars`; `start` is `YYYY-MM-DD`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GanttBar {
    pub id: u64,
    pub label: String,
    pub start: String,
    pub days: u32,
}

impl GanttBar {
    pub fn start_date(&self) -> Option<Date> {
        Date::parse_iso(&self.start)
    }
    /// Last day covered (start + days − 1).
    pub fn end_date(&self) -> Option<Date> {
        self.start_date().map(|d| d.add_days(self.days.max(1) as i32 - 1))
    }
}

/// Next id for a new list item: `max(id) + 1` (1 for an empty list).
pub fn next_id(ids: impl IntoIterator<Item = u64>) -> u64 {
    ids.into_iter().max().unwrap_or(0) + 1
}

// ---- errors ----

/// Why a `set` was refused before touching the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetError {
    UnknownKey,
    Invalid(String),
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetError::UnknownKey => f.write_str("unknown key"),
            SetError::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for SetError {}

type R<T> = Result<T, String>;

// ---- small validators, public for live field validation in the UIs ----

/// Round to 2 decimal places (half away from zero).
pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// A JSON number, written as an integer when it has no fraction
/// (`129` rather than `129.0`, as Go and JavaScript print it).
pub fn num_value(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9.0e15 {
        json!(x as i64)
    } else {
        json!(x)
    }
}

/// `\s` as RE2/Go define it.
fn is_re_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// `^[^@\s]+@[^@\s]+\.[^@\s]+$` (no length check).
pub fn valid_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else { return false };
    let ok = |p: &str| !p.is_empty() && !p.chars().any(|c| c == '@' || is_re_space(c));
    // a dot with at least one char on each side somewhere in the domain
    ok(local) && ok(domain) && domain.char_indices().any(|(i, c)| c == '.' && i > 0 && i + 1 < domain.len())
}

/// `^[A-Z]{3}-[0-9]{3}$`
pub fn valid_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b[..3].iter().all(u8::is_ascii_uppercase) && b[3] == b'-' && b[4..].iter().all(u8::is_ascii_digit)
}

/// `^[a-z0-9][a-z0-9_-]{0,23}$`
pub fn valid_tag(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=24).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b[1..].iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

/// True if `s` holds a control character (Unicode Cc: C0, DEL, C1), the
/// bytes a peer could use to inject terminal escapes into another's TUI.
/// `allow_layout` permits `\n` and `\t` (multi-line text).
pub fn has_control(s: &str, allow_layout: bool) -> bool {
    s.chars().any(|c| c.is_control() && !(allow_layout && (c == '\n' || c == '\t')))
}

/// Text from a peer made safe to draw: every control character becomes
/// U+FFFD. Use it for any single-line value, name or toast before display.
pub fn safe_text(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '\u{fffd}' } else { c }).collect()
}

/// Like [`safe_text`] but keeps `\n` and `\t` (for `textarea.notes`).
pub fn safe_multiline(s: &str) -> String {
    s.chars().map(|c| if c.is_control() && c != '\n' && c != '\t' { '\u{fffd}' } else { c }).collect()
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// A single-line string: no control characters.
fn as_str<'a>(v: &'a Value, what: &str) -> R<&'a str> {
    let s = v.as_str().ok_or_else(|| format!("{what} must be a string"))?;
    if has_control(s, false) {
        return Err(format!("{what} must not contain control characters"));
    }
    Ok(s)
}

fn as_bool(v: &Value, what: &str) -> R<bool> {
    v.as_bool().ok_or_else(|| format!("{what} must be true or false"))
}

fn as_num(v: &Value, what: &str) -> R<f64> {
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{what} must be a number"))
}

/// An integer in `min..=max`. Integral floats (`3.0`) are accepted.
fn as_int(v: &Value, what: &str, min: i64, max: i64) -> R<i64> {
    let n = match (v.as_i64(), v.as_f64()) {
        (Some(n), _) => n,
        (None, Some(f)) if f.fract() == 0.0 && f.abs() < 9.0e15 => f as i64,
        (None, Some(_)) => return Err(format!("{what} must be a whole number")),
        _ => return Err(format!("{what} must be a number")),
    };
    if !(min..=max).contains(&n) {
        return Err(format!("{what} must be {min}–{max}"));
    }
    Ok(n)
}

fn as_list<'a>(v: &'a Value, what: &str, max: usize) -> R<&'a Vec<Value>> {
    let list = v.as_array().ok_or_else(|| format!("{what} must be a list"))?;
    if list.len() > max {
        return Err(format!("{what}: at most {max} entries"));
    }
    Ok(list)
}

fn as_obj<'a>(v: &'a Value, what: &str) -> R<&'a Map<String, Value>> {
    v.as_object().ok_or_else(|| format!("{what} must be an object"))
}

fn field<'a>(o: &'a Map<String, Value>, name: &str) -> &'a Value {
    o.get(name).unwrap_or(&Value::Null)
}

/// Trimmed string with 1..=max chars (min 0 when `allow_empty`).
fn text(v: &Value, what: &str, max: usize, allow_empty: bool) -> R<String> {
    let s = as_str(v, what)?.trim();
    let n = chars(s);
    if n == 0 && !allow_empty {
        return Err(format!("{what} is required"));
    }
    if n > max {
        return Err(format!("{what} must be at most {max} characters"));
    }
    Ok(s.to_string())
}

/// Trimmed, lowercased email; `""` only when `!required`.
fn email(v: &Value, what: &str, required: bool) -> R<String> {
    let s = as_str(v, what)?.trim().to_lowercase();
    if s.is_empty() {
        return if required { Err(format!("{what} is required")) } else { Ok(s) };
    }
    if chars(&s) > MAX_EMAIL {
        return Err(format!("{what} must be at most {MAX_EMAIL} characters"));
    }
    if !valid_email(&s) {
        return Err(format!("{what} must look like name@example.com"));
    }
    Ok(s)
}

fn date(v: &Value, what: &str) -> R<String> {
    let s = as_str(v, what)?;
    Date::parse_iso(s).map(|d| d.iso()).ok_or_else(|| format!("{what} must be a date (YYYY-MM-DD)"))
}

fn opt_date(v: &Value, what: &str) -> R<Value> {
    if v.is_null() { Ok(Value::Null) } else { date(v, what).map(Value::String) }
}

fn one_of(v: &Value, what: &str, list: &[dataset::Choice]) -> R<String> {
    let s = as_str(v, what)?;
    match list.iter().find(|c| c.id == s) {
        Some(c) => Ok(c.id.to_string()),
        None => Err(format!("{what} must be one of {}", list.iter().map(|c| c.id).collect::<Vec<_>>().join(", "))),
    }
}

/// Unique positive integer id.
fn row_id(v: &Value, what: &str, seen: &mut HashSet<i64>) -> R<i64> {
    let id = as_int(v, &format!("{what} id"), 1, i64::MAX)?;
    if !seen.insert(id) {
        return Err(format!("{what}: duplicate id {id}"));
    }
    Ok(id)
}

// ---- per-key normalisation ----

/// Validate and normalise a value for `key` (protocol §5), without the
/// cross-key rule. Returns the normalised value to store/send.
pub fn validate(key: &str, v: &Value) -> Result<Value, SetError> {
    if !is_key(key) {
        return Err(SetError::UnknownKey);
    }
    normalise(key, v).map_err(SetError::Invalid)
}

/// Validate, normalise, then apply the cross-key rule against the other
/// keys' current values (`get` returns a key's current value).
pub fn validate_with(key: &str, v: &Value, get: impl Fn(&str) -> Option<Value>) -> Result<Value, SetError> {
    let norm = validate(key, v)?;
    check_cross(key, &norm, get).map_err(SetError::Invalid)?;
    Ok(norm)
}

/// The cross-key rule (protocol §3.5): `date.due` ≥ `date.start` when both
/// are set, checked against the *other* key's current value.
pub fn check_cross(key: &str, value: &Value, get: impl Fn(&str) -> Option<Value>) -> Result<(), String> {
    let parse = |v: Option<&Value>| v.and_then(Value::as_str).and_then(Date::parse_iso);
    let (start, due) = match key {
        "date.start" => (parse(Some(value)), parse(get("date.due").as_ref())),
        "date.due" => (parse(get("date.start").as_ref()), parse(Some(value))),
        _ => return Ok(()),
    };
    match (start, due) {
        (Some(s), Some(d)) if d < s => Err("due must be on or after start".into()),
        _ => Ok(()),
    }
}

fn normalise(key: &str, v: &Value) -> R<Value> {
    Ok(match key {
        "text.name" => Value::String(text(v, "name", MAX_NAME, false)?),
        "text.email" => Value::String(email(v, "email", false)?),
        "text.code" => {
            let s = as_str(v, "code")?.trim().to_uppercase();
            if !valid_code(&s) {
                return Err("code must look like ABC-123".into());
            }
            Value::String(s)
        }
        "number.hours" => {
            let n = round2(as_num(v, "hours")?);
            if !(0.0..=MAX_HOURS).contains(&n) {
                return Err("hours must be 0–1000".into());
            }
            num_value(n)
        }
        "number.qty" => json!(as_int(v, "quantity", 0, 99)?),
        "number.percent" => json!(as_int(v, "percent", 0, 100)?),
        "date.start" => opt_date(v, "start")?,
        "date.due" => opt_date(v, "due")?,
        "select.status" => Value::String(one_of(v, "status", &STATUSES)?),
        "select.priority" => Value::String(one_of(v, "priority", &PRIORITIES)?),
        "combo.owner" | "combo.reviewer" => {
            if v.is_null() {
                Value::Null
            } else {
                let s = as_str(v, "person")?;
                match dataset::person(s) {
                    Some(p) => Value::String(p.username.to_string()),
                    None => return Err(format!("unknown person '{s}'")),
                }
            }
        }
        "tags.tags" => {
            let mut tags = Vec::new();
            for t in as_list(v, "tags", usize::MAX)? {
                let t = as_str(t, "tag")?.trim().to_lowercase();
                if !valid_tag(&t) {
                    return Err(format!("bad tag '{t}': use a–z, 0–9, _ or -, at most 24"));
                }
                tags.push(t);
            }
            tags.sort();
            tags.dedup();
            if tags.len() > MAX_TAGS {
                return Err(format!("at most {MAX_TAGS} tags"));
            }
            json!(tags)
        }
        "textarea.notes" => {
            let s = v.as_str().ok_or("notes must be a string")?.replace("\r\n", "\n");
            if has_control(&s, true) {
                return Err("notes must not contain control characters (other than newline and tab)".into());
            }
            if chars(&s) > MAX_NOTES {
                return Err(format!("notes must be at most {MAX_NOTES} characters"));
            }
            Value::String(s)
        }
        "choice.notify" => Value::Bool(as_bool(v, "notify")?),
        "choice.dark" => Value::Bool(as_bool(v, "dark mode")?),
        "choice.channels" => {
            let mut picked = [false; CHANNELS.len()];
            for c in as_list(v, "channels", usize::MAX)? {
                let s = as_str(c, "channel")?;
                match CHANNELS.iter().position(|ch| ch.id == s) {
                    Some(i) => picked[i] = true,
                    None => return Err(format!("unknown channel '{s}'")),
                }
            }
            json!(CHANNELS.iter().zip(picked).filter(|(_, p)| *p).map(|(c, _)| c.id).collect::<Vec<_>>())
        }
        "choice.size" => Value::String(one_of(v, "size", &SIZES)?),
        "cascade.pick" => cascade(v)?,
        "grid.rows" => grid(v)?,
        "menus.files" => {
            let mut out = Vec::new();
            let mut names = HashSet::new();
            for (i, f) in as_list(v, "files", MAX_FILES)?.iter().enumerate() {
                let what = format!("file {}", i + 1);
                let o = as_obj(f, &what)?;
                let name = text(field(o, "name"), &format!("{what} name"), MAX_FILE_NAME, false)?;
                if !names.insert(name.clone()) {
                    return Err(format!("duplicate file name '{name}'"));
                }
                let starred = as_bool(field(o, "starred"), &format!("{what} starred"))?;
                out.push(json!({"name": name, "starred": starred}));
            }
            Value::Array(out)
        }
        "palette.last" => Value::String(text(v, "last command", MAX_PALETTE, true)?),
        "dialog.profile" => {
            let o = as_obj(v, "profile")?;
            let name = text(field(o, "name"), "name", MAX_NAME, false)?;
            let email = email(field(o, "email"), "email", true)?;
            json!({"name": name, "email": email})
        }
        "charts.series" => {
            let list = as_list(v, "series", usize::MAX)?;
            if list.len() != SERIES_LEN {
                return Err(format!("series must have exactly {SERIES_LEN} values"));
            }
            let mut out = Vec::with_capacity(SERIES_LEN);
            for (i, n) in list.iter().enumerate() {
                out.push(as_int(n, &format!("month {}", i + 1), 0, 100)?);
            }
            json!(out)
        }
        "kanban.cards" => {
            let mut out = Vec::new();
            let mut seen = HashSet::new();
            for (i, c) in as_list(v, "cards", MAX_CARDS)?.iter().enumerate() {
                let what = format!("card {}", i + 1);
                let o = as_obj(c, &what)?;
                let id = row_id(field(o, "id"), &what, &mut seen)?;
                let title = text(field(o, "title"), &format!("{what} title"), MAX_CARD_TITLE, false)?;
                let col = one_of(field(o, "col"), &format!("{what} column"), &KANBAN_COLUMNS)?;
                out.push(json!({"id": id, "title": title, "col": col}));
            }
            Value::Array(out)
        }
        "gantt.bars" => {
            let mut out = Vec::new();
            let mut seen = HashSet::new();
            for (i, b) in as_list(v, "bars", MAX_BARS)?.iter().enumerate() {
                let what = format!("bar {}", i + 1);
                let o = as_obj(b, &what)?;
                let id = row_id(field(o, "id"), &what, &mut seen)?;
                let label = text(field(o, "label"), &format!("{what} label"), MAX_BAR_LABEL, false)?;
                let start = date(field(o, "start"), &format!("{what} start"))?;
                let days = as_int(field(o, "days"), &format!("{what} days"), 1, MAX_BAR_DAYS)?;
                out.push(json!({"id": id, "label": label, "start": start, "days": days}));
            }
            Value::Array(out)
        }
        _ => return Err("unknown key".into()),
    })
}

fn cascade(v: &Value) -> R<Value> {
    let o = as_obj(v, "pick")?;
    let opt = |name: &str| -> R<Option<String>> {
        match field(o, name) {
            Value::Null => Ok(None),
            x => as_str(x, name).map(|s| Some(s.to_string())),
        }
    };
    let (program, project, phase) = (opt("program")?, opt("project")?, opt("phase")?);
    if let Some(p) = &program {
        if dataset::program(p).is_none() {
            return Err(format!("unknown program '{p}'"));
        }
    }
    if let Some(j) = &project {
        let Some(p) = &program else { return Err("pick a program before a project".into()) };
        if !dataset::projects_of(p).iter().any(|x| x.name == j) {
            return Err(format!("'{j}' is not a project of {p}"));
        }
    }
    if let Some(h) = &phase {
        let (Some(p), Some(j)) = (&program, &project) else { return Err("pick a project before a phase".into()) };
        if !dataset::phases_of(p, j).contains(&h.as_str()) {
            return Err(format!("'{h}' is not a phase of {j}"));
        }
    }
    Ok(json!({"program": program, "project": project, "phase": phase}))
}

fn grid(v: &Value) -> R<Value> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (i, r) in as_list(v, "rows", MAX_GRID_ROWS)?.iter().enumerate() {
        let what = format!("row {}", i + 1);
        let o = as_obj(r, &what)?;
        let id = row_id(field(o, "id"), &what, &mut seen)?;
        let item = text(field(o, "item"), &format!("{what} item"), MAX_ITEM, false)?;
        let qty = as_int(field(o, "qty"), &format!("{what} qty"), 0, MAX_QTY)?;
        let price = round2(as_num(field(o, "price"), &format!("{what} price"))?);
        if !(0.0..=MAX_PRICE).contains(&price) {
            return Err(format!("{what} price must be 0–1,000,000"));
        }
        let status = one_of(field(o, "status"), &format!("{what} status"), &STATUSES)?;
        let due = opt_date(field(o, "due"), &format!("{what} due"))?;
        out.push(json!({"id": id, "item": item, "qty": qty, "price": num_value(price), "status": status, "due": due}));
    }
    Ok(Value::Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(key: &str, v: Value) -> Value {
        validate(key, &v).unwrap_or_else(|e| panic!("{key} {v}: {e}"))
    }
    fn bad(key: &str, v: Value) -> String {
        match validate(key, &v) {
            Err(SetError::Invalid(m)) => m,
            other => panic!("{key} {v} should be invalid, got {other:?}"),
        }
    }

    #[test]
    fn defaults_are_valid_and_already_normal() {
        assert_eq!(defaults().len(), KEYS.len());
        for (key, v) in defaults() {
            assert_eq!(ok(key, v.clone()), v, "default of {key} must be its own normal form");
        }
        assert_eq!(default_value("combo.reviewer"), Some(Value::Null));
        assert_eq!(default_value("textarea.notes").unwrap().as_str().unwrap().lines().count(), 3);
        assert_eq!(default_value("grid.rows").unwrap().as_array().unwrap().len(), 6);
        assert_eq!(default_value("menus.files").unwrap()[3]["starred"], json!(true));
        assert!(default_value("nope").is_none());
        assert_eq!(validate("nope", &json!(1)), Err(SetError::UnknownKey));
        // defaults satisfy the cross-key rule
        let start = default_value("date.start");
        assert!(check_cross("date.due", &default_value("date.due").unwrap(), |_| start.clone()).is_ok());
    }

    #[test]
    fn text_keys() {
        assert_eq!(ok("text.name", json!("  Ada  ")), json!("Ada"));
        bad("text.name", json!("   "));
        bad("text.name", json!("x".repeat(81)));
        ok("text.name", json!("é".repeat(80))); // chars, not bytes
        bad("text.name", json!(5));
        assert_eq!(ok("text.email", json!(" Ada@Example.COM ")), json!("ada@example.com"));
        assert_eq!(ok("text.email", json!("")), json!(""));
        for e in ["ada", "ada@", "@x.y", "a@b", "a b@c.d", "a@b@c.d", "a@.b", "a@b."] {
            bad("text.email", json!(e));
        }
        bad("text.email", json!(format!("{}@example.com", "a".repeat(110))));
        assert_eq!(ok("text.code", json!(" eng-042 ")), json!("ENG-042"));
        for c in ["EN-042", "ENG042", "ENG-04", "ENG-0421", "ÉNG-042", ""] {
            bad("text.code", json!(c));
        }
    }

    #[test]
    fn control_characters() {
        bad("text.name", json!("Ada\u{1b}[31m"));
        bad("palette.last", json!("x\u{7}"));
        bad("text.name", json!("a\u{85}b")); // C1
        bad("menus.files", json!([{"name": "a\tb", "starred": false}]));
        bad("dialog.profile", json!({"name": "G\u{0}", "email": "g@x.org"}));
        assert_eq!(ok("textarea.notes", json!("a\tb\r\nc")), json!("a\tb\nc"));
        bad("textarea.notes", json!("a\u{1b}[2J"));
        bad("textarea.notes", json!("lone\rcr"));
        assert_eq!(safe_text("a\u{1b}[31mb\n"), "a\u{fffd}[31mb\u{fffd}");
        assert_eq!(safe_multiline("a\u{1b}\n\tb"), "a\u{fffd}\n\tb");
        assert!(!has_control("Café ✓", false));
    }

    #[test]
    fn numbers() {
        assert_eq!(ok("number.hours", json!(12.345)), json!(12.35));
        assert_eq!(ok("number.hours", json!(3.0)), json!(3));
        assert_eq!(ok("number.hours", json!(1000)), json!(1000));
        bad("number.hours", json!(-0.5));
        bad("number.hours", json!(1000.01));
        bad("number.hours", json!("12"));
        assert_eq!(ok("number.qty", json!(99)), json!(99));
        assert_eq!(ok("number.qty", json!(7.0)), json!(7));
        bad("number.qty", json!(100));
        bad("number.qty", json!(2.5));
        bad("number.qty", json!(-1));
        ok("number.percent", json!(0));
        bad("number.percent", json!(101));
        bad("number.percent", json!(null));
    }

    #[test]
    fn dates_and_cross_rule() {
        ok("date.start", json!(null));
        ok("date.due", json!("2028-02-29"));
        bad("date.due", json!("2026-02-29"));
        bad("date.due", json!("tomorrow"));
        bad("date.due", json!(""));
        let start = Some(json!("2026-11-10"));
        let r = validate_with("date.due", &json!("2026-11-09"), |k| if k == "date.start" { start.clone() } else { None });
        assert_eq!(r, Err(SetError::Invalid("due must be on or after start".into())));
        assert!(validate_with("date.due", &json!("2026-11-10"), |_| start.clone()).is_ok());
        assert!(validate_with("date.due", &json!(null), |_| start.clone()).is_ok());
        let due = Some(json!("2026-11-01"));
        assert!(validate_with("date.start", &json!("2026-11-02"), |k| if k == "date.due" { due.clone() } else { None }).is_err());
        assert!(validate_with("date.start", &json!("2026-11-02"), |_| Some(Value::Null)).is_ok());
    }

    #[test]
    fn selects_and_people() {
        ok("select.status", json!("blocked"));
        bad("select.status", json!("Blocked"));
        ok("select.priority", json!("critical"));
        bad("select.priority", json!("urgent"));
        ok("combo.owner", json!("ivan"));
        ok("combo.reviewer", json!(null));
        bad("combo.owner", json!("mallory"));
        bad("combo.owner", json!(""));
        ok("choice.size", json!("xl"));
        bad("choice.size", json!("xxl"));
    }

    #[test]
    fn tags() {
        assert_eq!(ok("tags.tags", json!(["Q4", "bug", "q4", " Docs "])), json!(["bug", "docs", "q4"]));
        assert_eq!(ok("tags.tags", json!([])), json!([]));
        bad("tags.tags", json!(["-x"]));
        bad("tags.tags", json!(["has space"]));
        bad("tags.tags", json!(["x".repeat(25)]));
        bad("tags.tags", json!("q4"));
        let thirteen: Vec<String> = (0..13).map(|i| format!("t{i}")).collect();
        bad("tags.tags", json!(thirteen));
        // duplicates collapse before the count
        let dup: Vec<&str> = std::iter::repeat_n("a", 20).collect();
        assert_eq!(ok("tags.tags", json!(dup)), json!(["a"]));
    }

    #[test]
    fn notes_and_choices() {
        assert_eq!(ok("textarea.notes", json!("  a\r\nb  ")), json!("  a\nb  "));
        ok("textarea.notes", json!("x".repeat(5000)));
        bad("textarea.notes", json!("x".repeat(5001)));
        ok("choice.notify", json!(false));
        bad("choice.dark", json!("true"));
        assert_eq!(ok("choice.channels", json!(["slack", "email", "slack"])), json!(["email", "slack"]));
        bad("choice.channels", json!(["fax"]));
    }

    #[test]
    fn cascade_rules() {
        ok("cascade.pick", json!({"program": null, "project": null, "phase": null}));
        assert_eq!(
            ok("cascade.pick", json!({"program": "Tailspin Air"})),
            json!({"program": "Tailspin Air", "project": null, "phase": null})
        );
        ok("cascade.pick", json!({"program": "Tailspin Air", "project": "Crew scheduling", "phase": "Go-live"}));
        bad("cascade.pick", json!({"program": "Nope"}));
        bad("cascade.pick", json!({"program": "Tailspin Air", "project": "POS refresh"}));
        bad("cascade.pick", json!({"project": "POS refresh"}));
        bad("cascade.pick", json!({"program": "Northwind Retail", "phase": "Build"}));
        bad("cascade.pick", json!({"program": "Northwind Retail", "project": "POS refresh", "phase": "Launch"}));
        bad("cascade.pick", json!("Northwind Retail"));
    }

    #[test]
    fn grid_rows() {
        let row = |id: i64| json!({"id": id, "item": " Pen ", "qty": 2, "price": 1.005, "status": "todo", "due": null});
        assert_eq!(
            ok("grid.rows", json!([row(1)])),
            json!([{"id": 1, "item": "Pen", "qty": 2, "price": 1, "status": "todo", "due": null}])
        );
        bad("grid.rows", json!([row(1), row(1)]));
        bad("grid.rows", json!([row(0)]));
        let rows: Vec<Value> = (1..=51).map(row).collect();
        bad("grid.rows", json!(rows));
        let mut r = row(1);
        r["qty"] = json!(10000);
        bad("grid.rows", json!([r]));
        let mut r = row(1);
        r["price"] = json!(1_000_000.01);
        bad("grid.rows", json!([r]));
        let mut r = row(1);
        r["status"] = json!("doing");
        bad("grid.rows", json!([r]));
        let mut r = row(1);
        r["due"] = json!("2026-13-01");
        bad("grid.rows", json!([r]));
        let mut r = row(1);
        r["item"] = json!("");
        assert!(bad("grid.rows", json!([r])).contains("row 1"));
        // typed view
        let rows: Vec<GridRow> = serde_json::from_value(default_value("grid.rows").unwrap()).unwrap();
        assert_eq!(rows[2].total(), 869.97);
        assert_eq!(next_id(rows.iter().map(|r| r.id)), 7);
    }

    #[test]
    fn files_profile_palette() {
        assert_eq!(ok("menus.files", json!([{"name": " a ", "starred": true}])), json!([{"name": "a", "starred": true}]));
        bad("menus.files", json!([{"name": "a", "starred": true}, {"name": "a ", "starred": false}]));
        bad("menus.files", json!([{"name": "a"}]));
        bad("menus.files", json!([{"name": "x".repeat(41), "starred": false}]));
        assert_eq!(ok("palette.last", json!("  Go to: Text ")), json!("Go to: Text"));
        ok("palette.last", json!(""));
        bad("palette.last", json!("x".repeat(81)));
        assert_eq!(
            ok("dialog.profile", json!({"name": " Grace ", "email": "G@X.ORG"})),
            json!({"name": "Grace", "email": "g@x.org"})
        );
        bad("dialog.profile", json!({"name": "Grace", "email": ""}));
        bad("dialog.profile", json!({"name": "", "email": "g@x.org"}));
    }

    #[test]
    fn series_kanban_gantt() {
        ok("charts.series", json!([0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        bad("charts.series", json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]));
        bad("charts.series", json!([101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(
            ok("kanban.cards", json!([{"id": 3, "title": " x ", "col": "doing"}])),
            json!([{"id": 3, "title": "x", "col": "doing"}])
        );
        bad("kanban.cards", json!([{"id": 1, "title": "x", "col": "in_progress"}]));
        bad("kanban.cards", json!([{"id": 1, "title": "x", "col": "todo"}, {"id": 1, "title": "y", "col": "todo"}]));
        bad("kanban.cards", json!([{"id": 1, "title": "x".repeat(61), "col": "todo"}]));
        ok("gantt.bars", json!([{"id": 1, "label": "a", "start": "2026-10-19", "days": 365}]));
        bad("gantt.bars", json!([{"id": 1, "label": "a", "start": "2026-10-19", "days": 0}]));
        bad("gantt.bars", json!([{"id": 1, "label": "a", "start": "19 oct", "days": 3}]));
        let bars: Vec<GanttBar> = serde_json::from_value(default_value("gantt.bars").unwrap()).unwrap();
        assert_eq!(bars[0].end_date().unwrap().iso(), "2026-10-28");
    }
}
