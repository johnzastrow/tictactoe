package core

// Calendar dates without the time package's zones: days since 1970-01-01
// plus the natural-language parser from protocol §10. "Today" is always
// passed in, so everything here is pure and deterministic.

import (
	"fmt"
	"strconv"
	"strings"
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

// FromYMD uses Howard Hinnant's days_from_civil; ok is false for invalid dates.
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

func ParseISO(s string) (Date, bool) {
	if len(s) != 10 || s[4] != '-' || s[7] != '-' {
		return 0, false
	}
	y, e1 := strconv.Atoi(s[0:4])
	m, e2 := strconv.Atoi(s[5:7])
	d, e3 := strconv.Atoi(s[8:10])
	if e1 != nil || e2 != nil || e3 != nil || s[0] == '+' || s[5] == '+' || s[8] == '+' || s[0] == '-' {
		return 0, false
	}
	return FromYMD(y, m, d)
}

func (d Date) String() string {
	y, m, dd := d.YMD()
	return fmt.Sprintf("%04d-%02d-%02d", y, m, dd)
}

func (d Date) ISO() string { return d.String() }

// Long: "Fri 23 Oct 2026".
func (d Date) Long() string {
	y, m, dd := d.YMD()
	return fmt.Sprintf("%s %d %s %d", WeekdayNames[d.Weekday()], dd, MonthNames[m-1], y)
}

// Short: "23 Oct" (or "23 Oct 25" when not in thisYear).
func (d Date) Short(thisYear int) string {
	y, m, dd := d.YMD()
	if y == thisYear {
		return fmt.Sprintf("%2d %s", dd, MonthNames[m-1])
	}
	return fmt.Sprintf("%2d %s %02d", dd, MonthNames[m-1], floorMod(y, 100))
}

// CivilFromUnix: UTC (date, hour, minute, second).
func CivilFromUnix(secs int64) (Date, int, int, int) {
	days := secs / 86400
	rem := secs % 86400
	if rem < 0 {
		rem += 86400
		days--
	}
	return Date(days), int(rem / 3600), int(rem % 3600 / 60), int(rem % 60)
}

// RFC3339 formats a Unix time as UTC.
func RFC3339(secs int64) string {
	d, h, m, s := CivilFromUnix(secs)
	return fmt.Sprintf("%sT%02d:%02d:%02dZ", d, h, m, s)
}

// ParseRFC3339 reads YYYY-MM-DDTHH:MM:SSZ back to Unix seconds.
func ParseRFC3339(s string) (int64, bool) {
	if len(s) < 20 {
		return 0, false
	}
	d, ok := ParseISO(s[0:10])
	if !ok {
		return 0, false
	}
	h, e1 := strconv.Atoi(s[11:13])
	m, e2 := strconv.Atoi(s[14:16])
	sec, e3 := strconv.Atoi(s[17:19])
	if e1 != nil || e2 != nil || e3 != nil {
		return 0, false
	}
	return int64(d)*86400 + int64(h)*3600 + int64(m)*60 + int64(sec), true
}

// RelativeTime: "3m ago", "2h ago", "5d ago", or the date.
func RelativeTime(ts string, nowSecs int64) string {
	t, ok := ParseRFC3339(ts)
	if !ok {
		return ts
	}
	d := max(nowSecs-t, 0)
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
	return ts[0:10]
}

func monthIndex(word string) (int, bool) {
	if len(word) < 3 {
		return 0, false
	}
	for i, m := range monthsShort {
		if strings.HasPrefix(word, m) && strings.HasPrefix(monthsFull[i], word) {
			return i + 1, true
		}
	}
	return 0, false
}

func weekdayIndex(word string) (int, bool) {
	if len(word) < 3 {
		return 0, false
	}
	for i, full := range weekdaysFull {
		if strings.HasPrefix(full, word) {
			return i, true
		}
	}
	return 0, false
}

func parseYear(s string) (int, bool) {
	y, err := strconv.Atoi(s)
	if err != nil || strings.HasPrefix(s, "+") || strings.HasPrefix(s, "-") {
		return 0, false
	}
	switch len(s) {
	case 2:
		return 2000 + y, true
	case 4:
		return y, true
	}
	return 0, false
}

func atoiPlain(s string) (int, bool) {
	if s == "" || strings.HasPrefix(s, "+") || strings.HasPrefix(s, "-") {
		return 0, false
	}
	n, err := strconv.Atoi(s)
	return n, err == nil
}

// ParseNatural parses a natural-language date (protocol §10). It returns
// (nil, nil) for empty input.
func ParseNatural(input string, today Date) (*Date, error) {
	s := strings.ToLower(strings.TrimSpace(input))
	if s == "" {
		return nil, nil
	}
	errf := func() (*Date, error) {
		return nil, fmt.Errorf("can't read '%s' as a date", strings.TrimSpace(input))
	}
	ret := func(d Date) (*Date, error) { return &d, nil }
	switch s {
	case "today", "tod", "now":
		return ret(today)
	case "tomorrow", "tom", "tmr":
		return ret(today.AddDays(1))
	case "yesterday", "yest":
		return ret(today.AddDays(-1))
	case "eow":
		return ret(today.AddDays(floorMod(4-today.Weekday(), 7)))
	case "eom":
		return ret(today.EndOfMonth())
	case "eoy":
		if d, ok := FromYMD(today.Year(), 12, 31); ok {
			return ret(d)
		}
		return ret(today)
	}
	if d, ok := ParseISO(s); ok {
		return ret(d)
	}
	// Relative offsets: +3d -2w 1m +1y
	{
		sign, rest := 1, s
		switch s[0] {
		case '+':
			rest = s[1:]
		case '-':
			sign, rest = -1, s[1:]
		}
		if len(rest) >= 2 {
			num, unit := rest[:len(rest)-1], rest[len(rest)-1:]
			if n, err := strconv.Atoi(num); err == nil {
				n *= sign
				switch unit {
				case "d":
					return ret(today.AddDays(n))
				case "w":
					return ret(today.AddDays(n * 7))
				case "m":
					return ret(today.AddMonths(n))
				case "y":
					return ret(today.AddMonths(n * 12))
				}
			}
		}
	}
	if wd, ok := weekdayIndex(s); ok {
		delta := floorMod(wd-today.Weekday(), 7)
		if delta == 0 {
			delta = 7
		}
		return ret(today.AddDays(delta))
	}
	// US M/D or M/D/YYYY
	if strings.Contains(s, "/") {
		parts := strings.Split(s, "/")
		var y int
		yok := false
		switch len(parts) {
		case 2:
			y, yok = today.Year(), true
		case 3:
			y, yok = parseYear(parts[2])
		}
		m, mok := atoiPlain(parts[0])
		d, dok := 0, false
		if len(parts) > 1 {
			d, dok = atoiPlain(parts[1])
		}
		if yok && mok && dok {
			if r, ok := FromYMD(y, m, d); ok {
				return ret(r)
			}
		}
		return errf()
	}
	// "nov 3", "3 nov", "nov 3 2026", "3 nov 2026"
	words := strings.FieldsFunc(s, func(r rune) bool { return r == ' ' || r == '\t' || r == '\n' || r == ',' })
	if len(words) == 2 || len(words) == 3 {
		mw, dw := words[1], words[0]
		if _, ok := monthIndex(words[0]); ok {
			mw, dw = words[0], words[1]
		}
		m, mok := monthIndex(mw)
		d, dok := atoiPlain(strings.TrimRight(dw, "stndrh"))
		if mok && dok {
			y := today.Year()
			if len(words) == 3 {
				var ok bool
				if y, ok = parseYear(words[2]); !ok {
					return errf()
				}
			}
			if r, ok := FromYMD(y, m, d); ok {
				return ret(r)
			}
			return errf()
		}
	}
	return errf()
}
