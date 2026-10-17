package tui

// Themes (ui-spec §8): palettes of named roles. Every colour is RGB; the
// renderer maps them to xterm-256 when the terminal lacks truecolor.

import (
	"os"
	"strings"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type Theme struct {
	Name                                  string
	BG, Panel, Panel2, Zebra, FG, Muted   Color
	Accent, Accent2, Border, SelBG, CurBG Color
	CellBG, CellFG, OK, Warn, Err, Info   Color
	Shadow                                Color
	Users                                 [8]Color
}

var themes = []Theme{
	{
		Name: "Midnight",
		BG:   hex("#10131a"), Panel: hex("#161b24"), Panel2: hex("#1d2430"), Zebra: hex("#131821"),
		FG: hex("#dfe5ee"), Muted: hex("#6f7a8c"), Accent: hex("#5aa9ff"), Accent2: hex("#c792ea"),
		Border: hex("#334055"), SelBG: hex("#23324a"), CurBG: hex("#1f2a3a"), CellBG: hex("#3d6fb6"),
		CellFG: hex("#ffffff"), OK: hex("#5fd38d"), Warn: hex("#f2c14e"), Err: hex("#ff6b6b"),
		Info: hex("#4fc1e9"), Shadow: hex("#07090d"),
		Users: [8]Color{hex("#ff8a65"), hex("#4dd0e1"), hex("#aed581"), hex("#ba68c8"), hex("#ffd54f"), hex("#f06292"), hex("#7986cb"), hex("#4db6ac")},
	},
	{
		Name: "Daylight",
		BG:   hex("#f7f8fb"), Panel: hex("#ffffff"), Panel2: hex("#e9edf4"), Zebra: hex("#f3f5f9"),
		FG: hex("#1d2433"), Muted: hex("#7b8496"), Accent: hex("#1f6fd1"), Accent2: hex("#8a3fd1"),
		Border: hex("#c5cdda"), SelBG: hex("#dce8fb"), CurBG: hex("#eaf1fc"), CellBG: hex("#1f6fd1"),
		CellFG: hex("#ffffff"), OK: hex("#1f9d55"), Warn: hex("#c27c0e"), Err: hex("#d63c3c"),
		Info: hex("#0b84a5"), Shadow: hex("#c9ced8"),
		Users: [8]Color{hex("#d9480f"), hex("#0c8599"), hex("#5c940d"), hex("#9c36b5"), hex("#e67700"), hex("#c2255c"), hex("#3b5bdb"), hex("#087f5b")},
	},
	{
		Name: "Solarized Dark",
		BG:   hex("#002b36"), Panel: hex("#04313c"), Panel2: hex("#073642"), Zebra: hex("#022f3a"),
		FG: hex("#eee8d5"), Muted: hex("#839496"), Accent: hex("#268bd2"), Accent2: hex("#6c71c4"),
		Border: hex("#2d5560"), SelBG: hex("#0d4a59"), CurBG: hex("#08404d"), CellBG: hex("#268bd2"),
		CellFG: hex("#fdf6e3"), OK: hex("#859900"), Warn: hex("#b58900"), Err: hex("#dc322f"),
		Info: hex("#2aa198"), Shadow: hex("#00141a"),
		Users: [8]Color{hex("#cb4b16"), hex("#2aa198"), hex("#859900"), hex("#d33682"), hex("#b58900"), hex("#6c71c4"), hex("#268bd2"), hex("#93a1a1")},
	},
	{
		Name: "Gruvbox",
		BG:   hex("#1d2021"), Panel: hex("#282828"), Panel2: hex("#32302f"), Zebra: hex("#242424"),
		FG: hex("#ebdbb2"), Muted: hex("#928374"), Accent: hex("#83a598"), Accent2: hex("#d3869b"),
		Border: hex("#504945"), SelBG: hex("#3c3836"), CurBG: hex("#32302f"), CellBG: hex("#458588"),
		CellFG: hex("#fbf1c7"), OK: hex("#b8bb26"), Warn: hex("#fabd2f"), Err: hex("#fb4934"),
		Info: hex("#8ec07c"), Shadow: hex("#0f0f0f"),
		Users: [8]Color{hex("#fe8019"), hex("#8ec07c"), hex("#b8bb26"), hex("#d3869b"), hex("#fabd2f"), hex("#fb4934"), hex("#83a598"), hex("#bdae93")},
	},
}

func themeIndex(name string) int {
	for i, t := range themes {
		if strings.EqualFold(strings.ReplaceAll(t.Name, " ", ""), strings.ReplaceAll(name, " ", "")) {
			return i
		}
	}
	return 0
}

// detectTruecolor follows ui-spec §8: COLORTERM, or terminals known to
// support 24-bit colour (Windows Terminal sets WT_SESSION).
func detectTruecolor() bool {
	ct := strings.ToLower(os.Getenv("COLORTERM"))
	if ct == "truecolor" || ct == "24bit" {
		return true
	}
	if os.Getenv("WT_SESSION") != "" {
		return true
	}
	switch os.Getenv("TERM_PROGRAM") {
	case "ghostty", "iTerm.app", "WezTerm", "vscode":
		return true
	}
	return strings.Contains(os.Getenv("TERM"), "ghostty") || strings.Contains(os.Getenv("TERM"), "kitty")
}

// ---- semantic styles ----

func (t *Theme) base() Sty          { return Sty{FG: t.FG, BG: t.BG} }
func (t *Theme) panel() Sty         { return Sty{FG: t.FG, BG: t.Panel} }
func (t *Theme) muted(bg Color) Sty { return Sty{FG: t.Muted, BG: bg} }

func (t *Theme) userColor(id int64) Color {
	if id <= 0 {
		return t.Muted
	}
	return t.Users[int(id-1)%len(t.Users)]
}

func (t *Theme) projectStatus(s core.ProjectStatus) Color {
	switch s {
	case core.ProjectPlanning:
		return t.Info
	case core.ProjectActive:
		return t.OK
	case core.ProjectOnHold:
		return t.Warn
	}
	return t.Muted
}

func (t *Theme) taskStatus(s core.TaskStatus) Color {
	switch s {
	case core.TaskInProgress:
		return t.Info
	case core.TaskBlocked:
		return t.Err
	case core.TaskReview:
		return t.Accent2
	case core.TaskDone:
		return t.OK
	}
	return t.Muted
}

func (t *Theme) priority(p core.Priority) Color {
	switch p {
	case core.PriorityLow:
		return t.Muted
	case core.PriorityHigh:
		return t.Warn
	case core.PriorityCritical:
		return t.Err
	}
	return t.FG
}

func (t *Theme) health(h core.Health) Color {
	switch h {
	case core.HealthAmber:
		return t.Warn
	case core.HealthRed:
		return t.Err
	}
	return t.OK
}

// cellStyle maps a core style hint to a style plus an optional glyph prefix.
func (t *Theme) cellStyle(st core.Style, base Sty) (Sty, string) {
	s := base
	switch st.Kind {
	case core.StMuted:
		s.FG = t.Muted
	case core.StStrong:
		s.A |= Bold
	case core.StProjectStatus:
		s.FG = t.projectStatus(core.ProjectStatus(st.Value))
		if st.Value == string(core.ProjectCancelled) {
			s.A |= Strike
		}
		return s, "● "
	case core.StTaskStatus:
		s.FG = t.taskStatus(core.TaskStatus(st.Value))
		return s, taskGlyph(core.TaskStatus(st.Value))
	case core.StPriority:
		s.FG = t.priority(core.Priority(st.Value))
		if st.Value == string(core.PriorityCritical) {
			s.A |= Bold
			return s, "◆ "
		}
		return s, ""
	case core.StHealth:
		s.FG = t.health(core.Health(st.Value))
		return s, "● "
	case core.StRole:
		switch core.Role(st.Value) {
		case core.RoleAdmin:
			s.FG = t.Accent2
		case core.RoleViewer:
			s.FG = t.Muted
		}
	case core.StOverdue, core.StBad:
		s.FG = t.Err
	case core.StDueSoon:
		s.FG = t.Warn
	case core.StGood:
		s.FG = t.OK
	case core.StTags:
		s.FG = t.Accent2
	case core.StColor:
		if c := hex(st.Value); c.Set {
			s.FG = c
		}
	case core.StUser:
		s.FG = t.userColor(st.UserID)
	case core.StStruck:
		s.FG = t.Muted
		s.A |= Strike
	}
	return s, ""
}

func taskGlyph(s core.TaskStatus) string {
	switch s {
	case core.TaskInProgress:
		return "◐ "
	case core.TaskBlocked:
		return "⊘ "
	case core.TaskReview:
		return "◑ "
	case core.TaskDone:
		return "✓ "
	}
	return "○ "
}
