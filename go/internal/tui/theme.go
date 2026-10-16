package tui

import (
	"image/color"
	"os"
	"strings"

	"charm.land/lipgloss/v2"
)

// Theme mirrors the Rust version. Truecolor themes look best; "Classic" uses
// the 16 ANSI colours so it renders in terminals without 24-bit support
// (e.g. macOS Terminal.app, older Windows conhost).
type Theme struct {
	Name                             string
	BG, Panel, FG, Muted, Grid       color.Color
	X, O, Cursor, Win, WinFG, Accent color.Color
	Error                            color.Color
	truecolor                        bool
}

var c = lipgloss.Color

var themes = []Theme{
	{
		Name: "Midnight", truecolor: true,
		BG: c("#11141b"), Panel: c("#1a1f2a"), FG: c("#e7eaf0"), Muted: c("#6e7687"),
		Grid: c("#465064"), X: c("#ff8a4c"), O: c("#4dabf7"), Cursor: c("#2d3546"),
		Win: c("#ffd43b"), WinFG: c("#11141b"), Accent: c("#69db7c"), Error: c("#ff6b6b"),
	},
	{
		Name: "Paper", truecolor: true,
		BG: c("#f6f7fb"), Panel: c("#ffffff"), FG: c("#1d2330"), Muted: c("#788091"),
		Grid: c("#bec4d2"), X: c("#d9480f"), O: c("#1971c2"), Cursor: c("#e2e7f1"),
		Win: c("#fab005"), WinFG: c("#1d2330"), Accent: c("#2b8a3e"), Error: c("#c92a2a"),
	},
	{
		Name: "Classic",
		BG:   lipgloss.NoColor{}, Panel: lipgloss.NoColor{}, FG: lipgloss.NoColor{},
		Muted: c("8"), Grid: c("7"), X: c("9"), O: c("14"), Cursor: c("8"),
		Win: c("3"), WinFG: c("0"), Accent: c("10"), Error: c("1"),
	},
}

// defaultTheme starts on a truecolor theme only if the terminal advertises it.
func defaultTheme() int {
	ct := strings.ToLower(os.Getenv("COLORTERM"))
	if ct == "truecolor" || ct == "24bit" {
		return 0
	}
	return 2
}
