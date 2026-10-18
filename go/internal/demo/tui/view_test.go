package tui

import (
	"fmt"
	"strings"
	"testing"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

func TestEveryTabRenders(t *testing.T) {
	for _, size := range [][2]int{{115, 33}, {80, 24}, {160, 45}} {
		a := testApp(t, size[0], size[1])
		for i, tb := range core.Tabs {
			a.setTab(i)
			a.enterBody(0)
			s := a.screen()
			dump(t, a, fmt.Sprintf("tab%02d_%s_%dx%d", i+1, tb.ID, size[0], size[1]))
			lines := strings.Split(s, "\n")
			if len(lines) != size[1] {
				t.Fatalf("%s: %d lines, want %d", tb.ID, len(lines), size[1])
			}
			if !strings.Contains(lines[0], "File") || !strings.Contains(lines[0], "demo · alice") {
				t.Errorf("%s %v: menu bar: %q", tb.ID, size, lines[0])
			}
			if !strings.Contains(s, tb.Title) || !strings.Contains(s, "Keys") || !strings.Contains(s, "Value (shared)") {
				t.Errorf("%s %v: missing title or panels:\n%s", tb.ID, size, s)
			}
			// the Value panel shows the first key with its description
			if !strings.Contains(s, tb.Keys[0]+" = ") {
				t.Errorf("%s %v: value panel lacks %s:\n%s", tb.ID, size, tb.Keys[0], s)
			}
		}
	}
}

func TestSmallWindowNotice(t *testing.T) {
	a := testApp(t, 70, 20)
	if s := a.screen(); !strings.Contains(s, "this window is 70×20") {
		t.Fatalf("no notice:\n%s", s)
	}
}
