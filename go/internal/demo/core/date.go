package core

// Calendar dates without the time package's zones: days since 1970-01-01
// plus the natural-date parser of protocol §6 (adapted from pt). "Today" is
// always passed in, so everything here is pure and deterministic.

import (
	"fmt"
	"strconv"
	"strings"
	"time"
)

// Date is days since 1970-01-01.
type Date int32

var (
	monthsShort  = []string{"jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"}
	monthsFull   = []string{"january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"}
	weekdaysFull = []string{"monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"}
	WeekdayNames = []string{"Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"}
	MonthNames   = []string{"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"}
)

func IsLeap(y int) bool { return (y%4 == 0 && y%100 != 0) || y%400 == 0 }

func DaysInMonth(y, m int) int {
	switch m {
	case 1, 3, 5, 7, 8, 10, 12:
		return 31
	case 4, 6, 9, 11:
		return 30
	}
	if IsLeap(y) {
		return 29
	}
	return 28
}

func floorDiv(a, b int) int {
	q := a / b
	if (a%b != 0) && ((a < 0) != (b < 0)) {
		q--
	}
	return q
}

func floorMod(a, b int) int { return a - floorDiv(a, b)*b }

// FromYMD uses Howard Hinnant's days_from_civil; ok is false for invalid
// dates (years 1–9999 only).
func FromYMD(y, m, d int) (Date, bool) {
	if m < 1 || m > 12 || d < 1 || d > DaysInMonth(y, m) || y < 1 || y > 9999 {
		return 0, false
	}
	if m <= 2 {
		y--
	}
	era := floorDiv(y, 400)
	yoe := y - era*400
	mp := (m + 9) % 12
	doy := (153*mp+2)/5 + d - 1
	doe := yoe*365 + yoe/4 - yoe/100 + doy
	return Date(era*146097 + doe - 719468), true
}

// YMD uses Howard Hinnant's civil_from_days.
func (d Date) YMD() (int, int, int) {
	z := int(d) + 719468
	era := floorDiv(z, 146097)
	doe := z - era*146097
	yoe := (doe - doe/1460 + doe/36524 - doe/146096) / 365
	y := yoe + era*400
	doy := doe - (365*yoe + yoe/4 - yoe/100)
	mp := (5*doy + 2) / 153
	dd := doy - (153*mp+2)/5 + 1
	m := mp + 3
	if mp >= 10 {
		m = mp - 9
	}
	if m <= 2 {
		y++
	}
	return y, m, dd
}

func (d Date) Year() int  { y, _, _ := d.YMD(); return y }
func (d Date) Month() int { _, m, _ := d.YMD(); return m }
func (d Date) Day() int   { _, _, dd := d.YMD(); return dd }

// Weekday: 0 = Monday … 6 = Sunday. 1970-01-01 was a Thursday.
func (d Date) Weekday() int { return floorMod(int(d)+3, 7) }

func (d Date) AddDays(n int) Date { return d + Date(n) }

// AddMonths clamps the day (Jan 31 + 1m = Feb 28/29).
func (d Date) AddMonths(n int) Date {
	y, m, dd := d.YMD()
	total := y*12 + (m - 1) + n
	ny := floorDiv(total, 12)
	nm := floorMod(total, 12) + 1
	nd := min(dd, DaysInMonth(ny, nm))
	if r, ok := FromYMD(ny, nm, nd); ok {
		return r
	}
	return d
}

func (d Date) FirstOfMonth() Date {
	y, m, _ := d.YMD()
	r, _ := FromYMD(y, m, 1)
	return r
}

func (d Date) EndOfMonth() Date {
	y, m, _ := d.YMD()
	r, _ := FromYMD(y, m, DaysInMonth(y, m))
	return r
}

// WeekStart is the Monday of this date's week.
func (d Date) WeekStart() Date { return d.AddDays(-d.Weekday()) }

// ParseISO reads YYYY-MM-DD (a real calendar date).
func ParseISO(s string) (Date, bool) {
	if len(s) != 10 || s[4] != '-' || s[7] != '-' {
		return 0, false
	}
	for i := 0; i < 10; i++ {
		if i != 4 && i != 7 && (s[i] < '0' || s[i] > '9') {
			return 0, false
		}
	}
	y, _ := strconv.Atoi(s[0:4])
	m, _ := strconv.Atoi(s[5:7])
	d, _ := strconv.Atoi(s[8:10])
	return FromYMD(y, m, d)
}

// String is YYYY-MM-DD.
func (d Date) String() string {
	y, m, dd := d.YMD()
	return fmt.Sprintf("%04d-%02d-%02d", y, m, dd)
}

func (d Date) ISO() string { return d.String() }

// Long: "Fri 23 Oct 2026" (the preview format of §6).
func (d Date) Long() string {
	y, m, dd := d.YMD()
	return fmt.Sprintf("%s %d %s %d", WeekdayNames[d.Weekday()], dd, MonthNames[m-1], y)
}

// Short: "23 Oct".
func (d Date) Short() string {
	_, m, dd := d.YMD()
	return fmt.Sprintf("%d %s", dd, MonthNames[m-1])
}

// DateOf is the calendar date of t in t's location (pass time.Now() for the
// client's local today).
func DateOf(t time.Time) Date {
	r, _ := FromYMD(t.Year(), int(t.Month()), t.Day())
	return r
}

// Today is the local calendar date.
func Today() Date { return DateOf(time.Now()) }

// NowTS is the current UTC time as YYYY-MM-DDTHH:MM:SSZ (the `at` format).
func NowTS() string { return FormatTS(time.Now()) }

// FormatTS formats t as UTC YYYY-MM-DDTHH:MM:SSZ.
func FormatTS(t time.Time) string { return t.UTC().Format("2006-01-02T15:04:05Z") }

// RelativeTime: "just now", "3m ago", "2h ago", "5d ago", or the date, for
// the Value panel ("v4 · by bob · 2m ago").
func RelativeTime(ts string, now time.Time) string {
	t, err := time.Parse(time.RFC3339, ts)
	if err != nil {
		return ts
	}
	d := max(int64(now.Sub(t).Seconds()), 0)
	switch {
	case d < 60:
		return "just now"
	case d < 3600:
		return fmt.Sprintf("%dm ago", d/60)
	case d < 86400:
		return fmt.Sprintf("%dh ago", d/3600)
	case d < 604800:
		return fmt.Sprintf("%dd ago", d/86400)
	}
	return ts[:10]
}

// monthIndex accepts a 3-letter abbreviation or a full month name.
func monthIndex(word string) (int, bool) {
	for i := range monthsShort {
		if word == monthsShort[i] || word == monthsFull[i] {
			return i + 1, true
		}
	}
	return 0, false
}

// weekdayIndex accepts a 3-letter abbreviation or a full weekday name.
func weekdayIndex(word string) (int, bool) {
	for i, full := range weekdaysFull {
		if word == full || word == full[:3] {
			return i, true
		}
	}
	return 0, false
}

// digits parses 1–4 ASCII digits.
func digits(s string) (int, bool) {
	if len(s) == 0 || len(s) > 4 {
		return 0, false
	}
	n := 0
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return 0, false
		}
		n = n*10 + int(s[i]-'0')
	}
	return n, true
}

// ParseNatural parses a natural date (protocol §6) relative to today. It
// returns (nil, nil) for empty input (= null) and an error for anything it
// can't read. Accepted: today/t, tomorrow/tom, yesterday, weekday names and
// 3-letter abbreviations (next occurrence, today excluded), ±N with unit
// d/w/m (bare ±N = days), eow, eom, YYYY-MM-DD, "D Mon" and "Mon D".
func ParseNatural(input string, today Date) (*Date, error) {
	s := strings.ToLower(strings.TrimSpace(input))
	if s == "" {
		return nil, nil
	}
	bad := func() (*Date, error) {
		return nil, fmt.Errorf("can't read %q as a date", truncate(strings.TrimSpace(input), 30))
	}
	ret := func(d Date) (*Date, error) {
		if _, ok := FromYMD(d.YMD()); !ok {
			return bad() // outside years 1–9999
		}
		return &d, nil
	}
	switch s {
	case "today", "t":
		return ret(today)
	case "tomorrow", "tom":
		return ret(today.AddDays(1))
	case "yesterday":
		return ret(today.AddDays(-1))
	case "eow":
		// Friday this week; on Saturday/Sunday, next week's.
		return ret(today.AddDays(floorMod(4-today.Weekday(), 7)))
	case "eom":
		return ret(today.EndOfMonth())
	}
	if d, ok := ParseISO(s); ok {
		return ret(d)
	}
	// +3d -2w +1m; bare +3 = days.
	if s[0] == '+' || s[0] == '-' {
		sign, rest := 1, s[1:]
		if s[0] == '-' {
			sign = -1
		}
		unit := byte('d')
		if n := len(rest); n > 0 && (rest[n-1] == 'd' || rest[n-1] == 'w' || rest[n-1] == 'm') {
			unit, rest = rest[n-1], rest[:n-1]
		}
		n, ok := digits(rest)
		if !ok {
			return bad()
		}
		n *= sign
		switch unit {
		case 'w':
			return ret(today.AddDays(n * 7))
		case 'm':
			return ret(today.AddMonths(n))
		}
		return ret(today.AddDays(n))
	}
	if wd, ok := weekdayIndex(s); ok {
		delta := floorMod(wd-today.Weekday(), 7)
		if delta == 0 {
			delta = 7
		}
		return ret(today.AddDays(delta))
	}
	// "23 oct" or "oct 23", this year.
	if words := strings.Fields(s); len(words) == 2 {
		mw, dw := words[1], words[0]
		if _, ok := monthIndex(words[0]); ok {
			mw, dw = words[0], words[1]
		}
		m, mok := monthIndex(mw)
		d, dok := digits(dw)
		if mok && dok && len(dw) <= 2 {
			if r, ok := FromYMD(today.Year(), m, d); ok {
				return ret(r)
			}
		}
	}
	return bad()
}

// DatePreview is the live preview a date field shows: "fri → Fri 23 Oct
// 2026", "" for empty input, or the error text. ok is false when invalid.
func DatePreview(input string, today Date) (preview string, ok bool) {
	d, err := ParseNatural(input, today)
	switch {
	case err != nil:
		return err.Error(), false
	case d == nil:
		return "", true
	}
	return strings.TrimSpace(input) + " → " + d.Long(), true
}
