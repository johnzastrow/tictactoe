//! The built-in dataset (protocol §4) and the fixed UI catalogue every
//! front end shares: tabs, choice lists with colours, peer colours.
//! Order is display order everywhere.

use crate::fuzzy::{self, Match};

// ---- people ------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Person {
    pub username: &'static str,
    pub name: &'static str,
    pub team: &'static str,
}

impl Person {
    /// The text the combobox fuzzy-matches against: `name username team`.
    pub fn search_text(&self) -> String {
        format!("{} {} {}", self.name, self.username, self.team)
    }
}

pub const PEOPLE: [Person; 9] = [
    Person { username: "alice", name: "Alice Nguyen", team: "Design" },
    Person { username: "bob", name: "Bob Okafor", team: "Engineering" },
    Person { username: "carol", name: "Carol Diaz", team: "Engineering" },
    Person { username: "dave", name: "Dave Kowalski", team: "Product" },
    Person { username: "erin", name: "Erin Walsh", team: "QA" },
    Person { username: "frank", name: "Frank Moreau", team: "Operations" },
    Person { username: "grace", name: "Grace Liu", team: "Engineering" },
    Person { username: "heidi", name: "Heidi Schmidt", team: "Design" },
    Person { username: "ivan", name: "Ivan Petrov", team: "Support" },
];

pub fn person(username: &str) -> Option<&'static Person> {
    PEOPLE.iter().find(|p| p.username == username)
}

/// People matching a combobox query, best first, with the match against
/// [`Person::search_text`] (whose first `name.chars().count()` positions are
/// the display name, for highlighting). Empty query: everyone, in order.
pub fn search_people(query: &str) -> Vec<(&'static Person, Match)> {
    let texts: Vec<String> = PEOPLE.iter().map(Person::search_text).collect();
    fuzzy::rank(query, texts.iter().map(String::as_str))
        .into_iter()
        .map(|(i, m)| (&PEOPLE[i], m))
        .collect()
}

// ---- programs ▸ projects ▸ phases ---------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Project {
    pub name: &'static str,
    pub phases: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program {
    pub name: &'static str,
    pub projects: &'static [Project],
}

pub const PROGRAMS: [Program; 4] = [
    Program {
        name: "Northwind Retail",
        projects: &[
            Project { name: "POS refresh", phases: &["Discover", "Build", "Pilot", "Rollout"] },
            Project { name: "Loyalty app", phases: &["Design", "Build", "Launch"] },
            Project { name: "Store Wi-Fi", phases: &["Survey", "Install", "Handover"] },
        ],
    },
    Program {
        name: "Contoso Health",
        projects: &[
            Project { name: "Patient portal", phases: &["Discover", "Design", "Build", "Test", "Launch"] },
            Project { name: "Records migration", phases: &["Plan", "Migrate", "Verify"] },
        ],
    },
    Program {
        name: "Fabrikam Energy",
        projects: &[
            Project { name: "Smart meters", phases: &["Pilot", "Deploy", "Monitor"] },
            Project { name: "Grid analytics", phases: &["Data", "Model", "Report"] },
            Project { name: "Field app", phases: &["Design", "Build", "Test"] },
        ],
    },
    Program {
        name: "Tailspin Air",
        projects: &[
            Project { name: "Crew scheduling", phases: &["Define", "Build", "Test", "Go-live"] },
            Project { name: "Baggage tracking", phases: &["Pilot", "Rollout"] },
        ],
    },
];

pub fn program(name: &str) -> Option<&'static Program> {
    PROGRAMS.iter().find(|p| p.name == name)
}

/// A program's projects (empty for an unknown program).
pub fn projects_of(program_name: &str) -> &'static [Project] {
    program(program_name).map_or(&[], |p| p.projects)
}

/// A project's phases within a program (empty if either is unknown).
pub fn phases_of(program_name: &str, project_name: &str) -> &'static [&'static str] {
    projects_of(program_name).iter().find(|p| p.name == project_name).map_or(&[], |p| p.phases)
}

// ---- tags ----------------------------------------------------------------------------

/// Tag suggestions (tag, uses), already ranked by uses desc then name.
pub const TAG_SUGGESTIONS: [(&str, u32); 10] = [
    ("urgent", 12),
    ("frontend", 9),
    ("backend", 8),
    ("q4", 7),
    ("design", 6),
    ("bug", 5),
    ("research", 4),
    ("docs", 3),
    ("security", 3),
    ("perf", 2),
];

/// Suggestions whose tag contains `query` (case-insensitive, trimmed),
/// excluding tags already present, in the §4 ranking.
pub fn suggest_tags(query: &str, present: &[String]) -> Vec<(&'static str, u32)> {
    let q = query.trim().to_lowercase();
    TAG_SUGGESTIONS
        .iter()
        .filter(|(t, _)| t.contains(&q) && !present.iter().any(|p| p == t))
        .copied()
        .collect()
}

// ---- choice lists --------------------------------------------------------------------

/// One entry of a fixed choice list: wire id, label, colour (`#rrggbb`, or
/// `""` where the list has no colours).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: &'static str,
    pub label: &'static str,
    pub color: &'static str,
}

const fn c(id: &'static str, label: &'static str, color: &'static str) -> Choice {
    Choice { id, label, color }
}

/// `select.status`, grid status column.
pub const STATUSES: [Choice; 5] = [
    c("todo", "To do", "#9aa5b1"),
    c("in_progress", "In progress", "#61afef"),
    c("blocked", "Blocked", "#e06c75"),
    c("review", "Review", "#c678dd"),
    c("done", "Done", "#98c379"),
];

/// `select.priority`.
pub const PRIORITIES: [Choice; 4] = [
    c("low", "Low", "#7f848e"),
    c("medium", "Medium", "#61afef"),
    c("high", "High", "#e5c07b"),
    c("critical", "Critical", "#e06c75"),
];

/// `choice.channels`, in canonical order.
pub const CHANNELS: [Choice; 4] = [c("email", "Email", ""), c("sms", "SMS", ""), c("push", "Push", ""), c("slack", "Slack", "")];

/// `choice.size`.
pub const SIZES: [Choice; 4] = [c("s", "S", ""), c("m", "M", ""), c("l", "L", ""), c("xl", "XL", "")];

/// `kanban.cards[].col`, left to right (colours reuse the status palette).
pub const KANBAN_COLUMNS: [Choice; 4] = [
    c("todo", "To do", "#9aa5b1"),
    c("doing", "Doing", "#61afef"),
    c("review", "Review", "#c678dd"),
    c("done", "Done", "#98c379"),
];

/// Find a choice by id in one of the lists above.
pub fn choice(list: &'static [Choice], id: &str) -> Option<&'static Choice> {
    list.iter().find(|c| c.id == id)
}

/// Month labels for `charts.series` (index 0 = Jan).
pub const CHART_MONTHS: [&str; 12] = crate::date::MONTH_NAMES;

// ---- peers ---------------------------------------------------------------------------

pub const PEER_COLORS: [&str; 8] = ["#e06c75", "#61afef", "#98c379", "#e5c07b", "#c678dd", "#56b6c2", "#d19a66", "#f472b6"];

/// A connection's colour: `PEER_COLORS[(conn - 1) % 8]` (conn starts at 1).
pub fn peer_color(conn: u64) -> &'static str {
    PEER_COLORS[(conn.saturating_sub(1) % 8) as usize]
}

/// Up to two initials for presence chips: "Alice Nguyen" → "AN", "bob" → "BO".
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let s: String = match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect(),
        [first, .., last] => first.chars().take(1).chain(last.chars().take(1)).collect(),
    };
    s.to_uppercase()
}

// ---- tabs ----------------------------------------------------------------------------

/// One gallery tab (ui-spec §4): `id` is what `focus` sends; `keys` are the
/// control keys it shows (protocol §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tab {
    pub number: u8,
    pub id: &'static str,
    pub title: &'static str,
    /// Heading shown at the top of the tab body.
    pub heading: &'static str,
    /// The one-sentence explanation under the heading.
    pub blurb: &'static str,
    pub keys: &'static [&'static str],
}

const fn tab(
    number: u8,
    id: &'static str,
    title: &'static str,
    heading: &'static str,
    blurb: &'static str,
    keys: &'static [&'static str],
) -> Tab {
    Tab { number, id, title, heading, blurb, keys }
}

pub const TABS: [Tab; 16] = [
    tab(1, "text", "Text", "Text input", "Single-line fields with live validation.", &["text.name", "text.email", "text.code"]),
    tab(2, "number", "Number", "Numbers", "Spinners for decimals and integers, and a slider.", &["number.hours", "number.qty", "number.percent"]),
    tab(3, "date", "Date", "Dates", "Natural-language date fields with a month calendar.", &["date.start", "date.due"]),
    tab(4, "select", "Dropdown", "Dropdowns", "Closed lists that open to pick one coloured value.", &["select.status", "select.priority"]),
    tab(5, "combo", "Combobox", "Combobox", "Type to fuzzy-search people, then pick one.", &["combo.owner", "combo.reviewer"]),
    tab(6, "tags", "Tags", "Tags", "Chips with suggestions; type to filter or create a tag.", &["tags.tags"]),
    tab(7, "textarea", "Text area", "Text area", "A multi-line editor with word wrap and a status footer.", &["textarea.notes"]),
    tab(8, "choice", "Choices", "Choices", "A checkbox, a checkbox group, radio buttons and a toggle switch.", &["choice.notify", "choice.channels", "choice.size", "choice.dark"]),
    tab(9, "cascade", "Cascade", "Cascading dropdowns", "Program, project and phase: each list depends on the one before.", &["cascade.pick"]),
    tab(10, "grid", "Grid", "Editable grid", "A spreadsheet-like table with typed cell editors and totals.", &["grid.rows"]),
    tab(11, "menus", "Menus", "Menus", "A menu bar with submenus, and a file list with context menus.", &["menus.files"]),
    tab(12, "palette", "Palette", "Command palette", "Fuzzy-search every command and run it.", &["palette.last"]),
    tab(13, "dialogs", "Dialogs", "Dialogs", "Confirm, form, message and toast popups.", &["dialog.profile"]),
    tab(14, "charts", "Charts", "Charts", "One series drawn as bars, a sparkline, a gauge and a heat strip.", &["charts.series"]),
    tab(15, "kanban", "Kanban", "Kanban board", "Cards in four columns; move them between columns.", &["kanban.cards"]),
    tab(16, "gantt", "Gantt", "Gantt chart", "Bars on a time axis; move, resize and zoom.", &["gantt.bars"]),
];

/// Tab by id (`"date"`), for `focus` and presence.
pub fn tab_by_id(id: &str) -> Option<&'static Tab> {
    TABS.iter().find(|t| t.id == id)
}

/// The tab that owns a control key (`"date.due"` → the Date tab).
pub fn tab_of_key(key: &str) -> Option<&'static Tab> {
    TABS.iter().find(|t| t.keys.contains(&key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_and_search() {
        assert_eq!(person("carol").unwrap().name, "Carol Diaz");
        assert!(person("mallory").is_none());
        let r = search_people("eng");
        // the three engineers match "Engineering" contiguously-ish; all in results
        assert!(r.iter().filter(|(p, _)| p.team == "Engineering").count() == 3);
        let r = search_people("");
        assert_eq!(r.len(), 9);
        assert_eq!(r[0].0.username, "alice");
        let r = search_people("gl");
        assert_eq!(r[0].0.username, "grace"); // Grace Liu: two word starts
    }

    #[test]
    fn cascade_lookup() {
        assert_eq!(projects_of("Contoso Health").len(), 2);
        assert_eq!(phases_of("Northwind Retail", "POS refresh"), &["Discover", "Build", "Pilot", "Rollout"]);
        assert!(phases_of("Northwind Retail", "Patient portal").is_empty());
        assert!(projects_of("Nope").is_empty());
    }

    #[test]
    fn tags_suggest() {
        let present = vec!["frontend".to_string()];
        let s = suggest_tags("", &present);
        assert_eq!(s.len(), 9);
        assert_eq!(s[0], ("urgent", 12));
        assert_eq!(suggest_tags("END", &[]), vec![("frontend", 9), ("backend", 8)]);
        // the constant is ranked: uses desc, then name
        for w in TAG_SUGGESTIONS.windows(2) {
            assert!(w[0].1 > w[1].1 || (w[0].1 == w[1].1 && w[0].0 < w[1].0));
        }
    }

    #[test]
    fn peers() {
        assert_eq!(peer_color(1), "#e06c75");
        assert_eq!(peer_color(8), "#f472b6");
        assert_eq!(peer_color(9), "#e06c75");
        assert_eq!(peer_color(0), "#e06c75");
        assert_eq!(initials("Alice Nguyen"), "AN");
        assert_eq!(initials("bob"), "BO");
        assert_eq!(initials("mary-jo van dyke"), "MD");
        assert_eq!(initials(""), "");
    }

    #[test]
    fn tabs_cover_every_key_once() {
        let mut keys: Vec<&str> = TABS.iter().flat_map(|t| t.keys.iter().copied()).collect();
        assert_eq!(keys, crate::keys::KEYS.iter().map(|k| k.key).collect::<Vec<_>>());
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), crate::keys::KEYS.len());
        for (i, t) in TABS.iter().enumerate() {
            assert_eq!(t.number as usize, i + 1);
        }
        assert_eq!(tab_of_key("date.due").unwrap().id, "date");
        assert_eq!(tab_by_id("dialogs").unwrap().number, 13);
    }
}
