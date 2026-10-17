//! Calendar dates without a date library: days since 1970-01-01 plus the
//! natural-language parser from protocol §10. "Today" is always passed in, so
//! everything here is pure and deterministic.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(i32); // days since 1970-01-01

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
pub const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

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
    /// Howard Hinnant's days_from_civil.
    pub fn from_ymd(y: i32, m: u32, d: u32) -> Option<Date> {
        if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) || !(1..=9999).contains(&y)
        {
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

    /// Howard Hinnant's civil_from_days.
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

    pub fn add_days(self, n: i32) -> Date {
        Date(self.0 + n)
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

    pub fn parse_iso(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
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
        format!(
            "{} {} {} {}",
            WEEKDAY_NAMES[self.weekday() as usize],
            d,
            MONTH_NAMES[m as usize - 1],
            y
        )
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
    (
        Date(days as i32),
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// RFC 3339 UTC timestamp for a Unix time.
pub fn rfc3339(secs: i64) -> String {
    let (d, h, m, s) = civil_from_unix(secs);
    format!("{d}T{h:02}:{m:02}:{s:02}Z")
}

/// Parse `YYYY-MM-DDTHH:MM:SSZ` back to Unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    if s.len() < 20 {
        return None;
    }
    let d = Date::parse_iso(&s[0..10])?;
    let h: i64 = s[11..13].parse().ok()?;
    let m: i64 = s[14..16].parse().ok()?;
    let sec: i64 = s[17..19].parse().ok()?;
    Some(d.0 as i64 * 86400 + h * 3600 + m * 60 + sec)
}

/// "3m ago", "2h ago", "5d ago", or a date.
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

fn month_index(word: &str) -> Option<u32> {
    if word.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|m| word.starts_with(m) && full_month(m).starts_with(word))
        .map(|i| i as u32 + 1)
}

fn full_month(abbr: &str) -> &'static str {
    match abbr {
        "jan" => "january",
        "feb" => "february",
        "mar" => "march",
        "apr" => "april",
        "may" => "may",
        "jun" => "june",
        "jul" => "july",
        "aug" => "august",
        "sep" => "september",
        "oct" => "october",
        "nov" => "november",
        _ => "december",
    }
}

fn weekday_index(word: &str) -> Option<u32> {
    const FULL: [&str; 7] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    if word.len() < 3 {
        return None;
    }
    WEEKDAYS
        .iter()
        .zip(FULL)
        .position(|(_, full)| full.starts_with(word))
        .map(|i| i as u32)
}

fn parse_year(s: &str) -> Option<i32> {
    let y: i32 = s.parse().ok()?;
    match s.len() {
        2 => Some(2000 + y),
        4 => Some(y),
        _ => None,
    }
}

/// Natural-language date (protocol §10). `Ok(None)` for empty input.
pub fn parse_natural(input: &str, today: Date) -> Result<Option<Date>, String> {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Ok(None);
    }
    let err = || Err(format!("can't read '{}' as a date", input.trim()));
    match s.as_str() {
        "today" | "tod" | "now" => return Ok(Some(today)),
        "tomorrow" | "tom" | "tmr" => return Ok(Some(today.add_days(1))),
        "yesterday" | "yest" => return Ok(Some(today.add_days(-1))),
        "eow" => {
            let wd = today.weekday() as i32;
            let delta = (4 - wd).rem_euclid(7);
            return Ok(Some(today.add_days(delta)));
        }
        "eom" => return Ok(Some(today.end_of_month())),
        "eoy" => return Ok(Some(Date::from_ymd(today.year(), 12, 31).unwrap_or(today))),
        _ => {}
    }
    if let Some(d) = Date::parse_iso(&s) {
        return Ok(Some(d));
    }
    // Relative offsets: +3d -2w 1m +1y
    {
        let (sign, rest) = match s.as_bytes()[0] {
            b'+' => (1, &s[1..]),
            b'-' => (-1, &s[1..]),
            _ => (1, s.as_str()),
        };
        if rest.len() >= 2 {
            let (num, unit) = rest.split_at(rest.len() - 1);
            if let Ok(n) = num.parse::<i32>() {
                let n = n * sign;
                let d = match unit {
                    "d" => Some(today.add_days(n)),
                    "w" => Some(today.add_days(n * 7)),
                    "m" => Some(today.add_months(n)),
                    "y" => Some(today.add_months(n * 12)),
                    _ => None,
                };
                if let Some(d) = d {
                    return Ok(Some(d));
                }
            }
        }
    }
    // Weekday: next occurrence, today excluded.
    if let Some(wd) = weekday_index(&s) {
        let delta = (wd as i32 - today.weekday() as i32).rem_euclid(7);
        return Ok(Some(today.add_days(if delta == 0 { 7 } else { delta })));
    }
    // US M/D or M/D/YYYY
    if s.contains('/') {
        let parts: Vec<&str> = s.split('/').collect();
        let y = match parts.len() {
            2 => Some(today.year()),
            3 => parse_year(parts[2]),
            _ => None,
        };
        let m = parts[0].parse::<u32>().ok();
        let d = parts.get(1).and_then(|p| p.parse::<u32>().ok());
        return match (y, m, d) {
            (Some(y), Some(m), Some(d)) => Date::from_ymd(y, m, d).map(Some).map_or_else(err, Ok),
            _ => err(),
        };
    }
    // "nov 3", "3 nov", "nov 3 2026", "3 nov 2026"
    let words: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() == 2 || words.len() == 3 {
        let (mw, dw) = if month_index(words[0]).is_some() {
            (words[0], words[1])
        } else {
            (words[1], words[0])
        };
        if let (Some(m), Ok(d)) = (month_index(mw), dw.trim_end_matches(['s', 't', 'n', 'd', 'r', 'h']).parse::<u32>()) {
            let y = if words.len() == 3 {
                match parse_year(words[2]) {
                    Some(y) => y,
                    None => return err(),
                }
            } else {
                today.year()
            };
            return match Date::from_ymd(y, m, d) {
                Some(d) => Ok(Some(d)),
                None => err(),
            };
        }
    }
    err()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse_iso(s).unwrap()
    }

    #[test]
    fn civil_roundtrip() {
        for days in -700_000..2_900_000 {
            if days % 997 != 0 {
                continue;
            }
            let dt = Date(days);
            let (y, m, dd) = dt.ymd();
            assert_eq!(Date::from_ymd(y, m, dd), Some(dt));
        }
        assert_eq!(Date::from_ymd(1970, 1, 1), Some(Date(0)));
        assert_eq!(d("2026-10-17").weekday(), 5); // Saturday
    }

    #[test]
    fn month_math() {
        assert_eq!(d("2026-01-31").add_months(1), d("2026-02-28"));
        assert_eq!(d("2028-01-31").add_months(1), d("2028-02-29"));
        assert_eq!(d("2026-12-15").add_months(1), d("2027-01-15"));
        assert_eq!(d("2026-01-15").add_months(-1), d("2025-12-15"));
    }

    #[test]
    fn natural() {
        let today = d("2026-10-17"); // Saturday
        let p = |s: &str| parse_natural(s, today).unwrap().unwrap().iso();
        assert_eq!(p("today"), "2026-10-17");
        assert_eq!(p("tom"), "2026-10-18");
        assert_eq!(p("fri"), "2026-10-23");
        assert_eq!(p("sat"), "2026-10-24");
        assert_eq!(p("monday"), "2026-10-19");
        assert_eq!(p("+3d"), "2026-10-20");
        assert_eq!(p("-2w"), "2026-10-03");
        assert_eq!(p("1m"), "2026-11-17");
        assert_eq!(p("eow"), "2026-10-23");
        assert_eq!(p("eom"), "2026-10-31");
        assert_eq!(p("eoy"), "2026-12-31");
        assert_eq!(p("11/3"), "2026-11-03");
        assert_eq!(p("11/3/2027"), "2027-11-03");
        assert_eq!(p("nov 3"), "2026-11-03");
        assert_eq!(p("3 Nov"), "2026-11-03");
        assert_eq!(p("november 3rd 2027"), "2027-11-03");
        assert_eq!(p("2026-02-28"), "2026-02-28");
        assert_eq!(parse_natural("  ", today), Ok(None));
        assert!(parse_natural("2026-02-30", today).is_err());
        assert!(parse_natural("blah", today).is_err());
        assert!(parse_natural("13/1", today).is_err());
        // eow on a Friday is that Friday
        assert_eq!(parse_natural("eow", d("2026-10-23")).unwrap().unwrap().iso(), "2026-10-23");
    }

    #[test]
    fn timestamps() {
        let t = parse_rfc3339("2026-10-17T14:03:05Z").unwrap();
        assert_eq!(rfc3339(t), "2026-10-17T14:03:05Z");
        assert_eq!(relative_time("2026-10-17T14:03:05Z", t + 125), "2m ago");
    }
}
