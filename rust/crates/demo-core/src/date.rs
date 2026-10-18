//! Calendar dates without a date library: days since 1970-01-01, plus the
//! natural-date parser of protocol §6. "Today" is always passed in, so
//! everything here is pure and deterministic.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(i32); // days since 1970-01-01

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
const MONTHS_FULL: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december",
];
const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const WEEKDAYS_FULL: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
/// `Mon` … `Sun`, indexed by [`Date::weekday`].
pub const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/// `Jan` … `Dec`, indexed by month − 1.
pub const MONTH_NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        _ => 28,
    }
}

impl Date {
    /// A real calendar date in years 1–9999, else `None`. Howard Hinnant's days_from_civil.
    pub fn from_ymd(y: i32, m: u32, d: u32) -> Option<Date> {
        if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) || !(1..=9999).contains(&y) {
            return None;
        }
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m as i32 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d as i32 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Some(Date(era * 146097 + doe - 719468))
    }

    pub fn from_days(days: i32) -> Date {
        Date(days)
    }

    pub fn days(self) -> i32 {
        self.0
    }

    /// (year, month 1–12, day 1–31). Howard Hinnant's civil_from_days.
    pub fn ymd(self) -> (i32, u32, u32) {
        let z = self.0 + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        (if m <= 2 { y + 1 } else { y }, m, d)
    }

    pub fn year(self) -> i32 {
        self.ymd().0
    }
    pub fn month(self) -> u32 {
        self.ymd().1
    }
    pub fn day(self) -> u32 {
        self.ymd().2
    }

    /// 0 = Monday … 6 = Sunday. 1970-01-01 was a Thursday.
    pub fn weekday(self) -> u32 {
        (self.0 + 3).rem_euclid(7) as u32
    }

    pub fn is_weekend(self) -> bool {
        self.weekday() >= 5
    }

    pub fn add_days(self, n: i32) -> Date {
        Date(self.0 + n)
    }

    /// Whole days from `self` to `other` (negative if `other` is earlier).
    pub fn days_until(self, other: Date) -> i32 {
        other.0 - self.0
    }

    /// Calendar months; the day is clamped (Jan 31 + 1m = Feb 28/29).
    pub fn add_months(self, n: i32) -> Date {
        let (y, m, d) = self.ymd();
        let total = y * 12 + (m as i32 - 1) + n;
        let ny = total.div_euclid(12);
        let nm = (total.rem_euclid(12) + 1) as u32;
        let nd = d.min(days_in_month(ny, nm));
        Date::from_ymd(ny, nm, nd).unwrap_or(self)
    }

    pub fn first_of_month(self) -> Date {
        let (y, m, _) = self.ymd();
        Date::from_ymd(y, m, 1).unwrap_or(self)
    }

    pub fn end_of_month(self) -> Date {
        let (y, m, _) = self.ymd();
        Date::from_ymd(y, m, days_in_month(y, m)).unwrap_or(self)
    }

    /// Monday of this date's week.
    pub fn week_start(self) -> Date {
        self.add_days(-(self.weekday() as i32))
    }

    /// The six Monday-first weeks (42 days) a month calendar shows for this
    /// date's month: starts on the Monday on or before the 1st.
    pub fn calendar_grid(self) -> [Date; 42] {
        let start = self.first_of_month().week_start();
        std::array::from_fn(|i| start.add_days(i as i32))
    }

    /// Strict `YYYY-MM-DD` with a real calendar date.
    pub fn parse_iso(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        if !b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
            return None;
        }
        let y = s[0..4].parse().ok()?;
        let m = s[5..7].parse().ok()?;
        let d = s[8..10].parse().ok()?;
        Date::from_ymd(y, m, d)
    }

    pub fn iso(self) -> String {
        self.to_string()
    }

    /// `Fri 23 Oct 2026`
    pub fn long(self) -> String {
        let (y, m, d) = self.ymd();
        format!("{} {} {} {}", WEEKDAY_NAMES[self.weekday() as usize], d, MONTH_NAMES[m as usize - 1], y)
    }

    /// `23 Oct` (or `23 Oct 25` when not in `this_year`).
    pub fn short(self, this_year: i32) -> String {
        let (y, m, d) = self.ymd();
        if y == this_year {
            format!("{:>2} {}", d, MONTH_NAMES[m as usize - 1])
        } else {
            format!("{:>2} {} {:02}", d, MONTH_NAMES[m as usize - 1], y.rem_euclid(100))
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

/// UTC civil time from a Unix timestamp: (date, hour, minute, second).
pub fn civil_from_unix(secs: i64) -> (Date, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    (Date(days as i32), (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}

/// `YYYY-MM-DDTHH:MM:SSZ` (UTC, seconds) for a Unix time: the `at` format.
pub fn rfc3339(secs: i64) -> String {
    let (d, h, m, s) = civil_from_unix(secs);
    format!("{d}T{h:02}:{m:02}:{s:02}Z")
}

/// Parse `YYYY-MM-DDTHH:MM:SSZ` back to Unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    if s.len() < 20 || !s.is_char_boundary(10) || !s.is_char_boundary(19) {
        return None;
    }
    let d = Date::parse_iso(&s[0..10])?;
    let h: i64 = s.get(11..13)?.parse().ok()?;
    let m: i64 = s.get(14..16)?.parse().ok()?;
    let sec: i64 = s.get(17..19)?.parse().ok()?;
    Some(d.0 as i64 * 86400 + h * 3600 + m * 60 + sec)
}

/// "just now", "3m ago", "2h ago", "5d ago", or the date. For the Value panel.
pub fn relative_time(ts: &str, now_secs: i64) -> String {
    let Some(t) = parse_rfc3339(ts) else {
        return ts.to_string();
    };
    let d = (now_secs - t).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        86400..=604799 => format!("{}d ago", d / 86400),
        _ => ts[0..10].to_string(),
    }
}

/// 1–12 for a 3-letter or full month name (already lowercase).
fn month_index(word: &str) -> Option<u32> {
    MONTHS
        .iter()
        .zip(MONTHS_FULL)
        .position(|(abbr, full)| word == *abbr || word == full)
        .map(|i| i as u32 + 1)
}

/// 0 (Mon) – 6 (Sun) for a 3-letter or full weekday name (already lowercase).
fn weekday_index(word: &str) -> Option<u32> {
    WEEKDAYS
        .iter()
        .zip(WEEKDAYS_FULL)
        .position(|(abbr, full)| word == *abbr || word == full)
        .map(|i| i as u32)
}

/// `+3d` `-2w` `+1m` `+3` (bare = days). The sign is required.
fn parse_offset(s: &str, today: Date) -> Option<Date> {
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    let (num, unit) = match rest.as_bytes().last()? {
        b'd' | b'w' | b'm' => (&rest[..rest.len() - 1], &rest[rest.len() - 1..]),
        _ => (rest, "d"),
    };
    // at most 4 digits keeps every result inside years 1–9999 without overflow
    if num.is_empty() || num.len() > 4 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: i32 = num.parse().ok()?;
    let n = n * sign;
    let d = match unit {
        "w" => today.add_days(n * 7),
        "m" => today.add_months(n),
        _ => today.add_days(n),
    };
    (1..=9999).contains(&d.year()).then_some(d)
}

/// Natural date (protocol §6), case-insensitive, relative to the client's
/// local `today`. `Ok(None)` for empty input (= null); `Err` with a short
/// message for anything else that isn't a date.
pub fn parse_natural(input: &str, today: Date) -> Result<Option<Date>, String> {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Ok(None);
    }
    let err = || Err(format!("can't read '{}' as a date", input.trim()));
    match s.as_str() {
        "today" | "t" => return Ok(Some(today)),
        "tomorrow" | "tom" => return Ok(Some(today.add_days(1))),
        "yesterday" => return Ok(Some(today.add_days(-1))),
        // Friday this week; next week's Friday on Saturday/Sunday
        "eow" => return Ok(Some(today.add_days((4 - today.weekday() as i32).rem_euclid(7)))),
        "eom" => return Ok(Some(today.end_of_month())),
        _ => {}
    }
    if let Some(d) = Date::parse_iso(&s) {
        return Ok(Some(d));
    }
    if let Some(d) = parse_offset(&s, today) {
        return Ok(Some(d));
    }
    // weekday: next occurrence, today excluded
    if let Some(wd) = weekday_index(&s) {
        let delta = (wd as i32 - today.weekday() as i32).rem_euclid(7);
        return Ok(Some(today.add_days(if delta == 0 { 7 } else { delta })));
    }
    // "23 oct" / "oct 23", this year
    let words: Vec<&str> = s.split_whitespace().collect();
    if let [a, b] = words.as_slice() {
        let (mw, dw) = if month_index(a).is_some() { (*a, *b) } else { (*b, *a) };
        let day = if dw.len() <= 2 { dw.parse::<u32>().ok() } else { None };
        if let (Some(m), Some(day)) = (month_index(mw), day) {
            return Date::from_ymd(today.year(), m, day).map(Some).map_or_else(err, Ok);
        }
    }
    err()
}

/// The live preview under a natural-date field: `fri → Fri 23 Oct 2026`,
/// `→ none` for empty input, or the error message.
pub fn preview(input: &str, today: Date) -> Result<String, String> {
    match parse_natural(input, today)? {
        Some(d) => Ok(format!("{} → {}", input.trim(), d.long())),
        None => Ok("→ none".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse_iso(s).unwrap()
    }

    #[test]
    fn civil_roundtrip() {
        for days in (-700_000..2_900_000).step_by(997) {
            let dt = Date(days);
            let (y, m, dd) = dt.ymd();
            assert_eq!(Date::from_ymd(y, m, dd), Some(dt));
        }
        assert_eq!(Date::from_ymd(1970, 1, 1), Some(Date(0)));
        assert_eq!(d("2026-10-17").weekday(), 5); // Saturday
        assert!(d("2026-10-18").is_weekend());
    }

    #[test]
    fn iso_is_strict() {
        assert!(Date::parse_iso("2026-02-29").is_none());
        assert!(Date::parse_iso("2028-02-29").is_some());
        assert!(Date::parse_iso("2026-1-01").is_none());
        assert!(Date::parse_iso("+026-01-01").is_none());
        assert!(Date::parse_iso("2026-13-01").is_none());
        assert!(Date::parse_iso("2026-10-1x").is_none());
    }

    #[test]
    fn month_math() {
        assert_eq!(d("2026-01-31").add_months(1), d("2026-02-28"));
        assert_eq!(d("2028-01-31").add_months(1), d("2028-02-29"));
        assert_eq!(d("2026-12-15").add_months(1), d("2027-01-15"));
        assert_eq!(d("2026-01-15").add_months(-1), d("2025-12-15"));
        let g = d("2026-10-18").calendar_grid();
        assert_eq!(g[0], d("2026-09-28")); // Monday on or before Oct 1
        assert_eq!(g[41], d("2026-11-08"));
    }

    #[test]
    fn natural() {
        let today = d("2026-10-17"); // Saturday
        let p = |s: &str| parse_natural(s, today).unwrap().unwrap().iso();
        assert_eq!(p("today"), "2026-10-17");
        assert_eq!(p("T"), "2026-10-17");
        assert_eq!(p("tom"), "2026-10-18");
        assert_eq!(p("Tomorrow"), "2026-10-18");
        assert_eq!(p("yesterday"), "2026-10-16");
        assert_eq!(p("fri"), "2026-10-23");
        assert_eq!(p("sat"), "2026-10-24"); // today excluded
        assert_eq!(p("monday"), "2026-10-19");
        assert_eq!(p("+3d"), "2026-10-20");
        assert_eq!(p("+3"), "2026-10-20");
        assert_eq!(p("-2w"), "2026-10-03");
        assert_eq!(p("+1m"), "2026-11-17");
        assert_eq!(p("eow"), "2026-10-23"); // Saturday → next week's Friday
        assert_eq!(p("eom"), "2026-10-31");
        assert_eq!(p("oct 23"), "2026-10-23");
        assert_eq!(p("23 Oct"), "2026-10-23");
        assert_eq!(p("3 november"), "2026-11-03");
        assert_eq!(p("2026-02-28"), "2026-02-28");
        assert_eq!(parse_natural("  ", today), Ok(None));
        for bad in ["2026-02-30", "blah", "fr", "3d", "+", "+3x", "31 feb", "oct", "+99999d", "11/3", "eoy"] {
            assert!(parse_natural(bad, today).is_err(), "{bad} should be invalid");
        }
        // eow: Friday itself, a weekday, and Sunday
        assert_eq!(parse_natural("eow", d("2026-10-23")).unwrap().unwrap().iso(), "2026-10-23");
        assert_eq!(parse_natural("eow", d("2026-10-20")).unwrap().unwrap().iso(), "2026-10-23");
        assert_eq!(parse_natural("eow", d("2026-10-18")).unwrap().unwrap().iso(), "2026-10-23");
    }

    #[test]
    fn previews() {
        let today = d("2026-10-17");
        assert_eq!(preview("fri", today).unwrap(), "fri → Fri 23 Oct 2026");
        assert_eq!(preview("", today).unwrap(), "→ none");
        assert!(preview("nope", today).is_err());
    }

    #[test]
    fn timestamps() {
        let t = parse_rfc3339("2026-10-17T14:03:05Z").unwrap();
        assert_eq!(rfc3339(t), "2026-10-17T14:03:05Z");
        assert_eq!(relative_time("2026-10-17T14:03:05Z", t + 125), "2m ago");
        assert_eq!(relative_time("garbage", t), "garbage");
        assert_eq!(parse_rfc3339("2026-10-17T1é:03:05Z"), None);
    }
}
