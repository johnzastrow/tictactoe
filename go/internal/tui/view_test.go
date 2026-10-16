package tui

import (
	"strings"
	"testing"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/x/ansi"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/hub"
)

func screen(t *testing.T, m Model, w, h int) []string {
	t.Helper()
	next, _ := m.Update(tea.WindowSizeMsg{Width: w, Height: h})
	m = next.(Model)
	lines := strings.Split(ansi.Strip(m.View().Content), "\n")
	t.Log("\n" + strings.Join(lines, "\n"))
	return lines
}

func TestFullLayoutAndMouseGeometry(t *testing.T) {
	h := hub.Start(t.Context(), core.None, core.Hard, 0)
	for _, c := range []int{4, 0, 8} {
		h.Play(c)
	}
	m, cancel := New(h, "http://127.0.0.1:8787")
	defer cancel()
	m.theme = 0
	const w, hgt = 100, 32
	lines := screen(t, m, w, hgt)

	if len(lines) != hgt {
		t.Fatalf("got %d lines, want %d", len(lines), hgt)
	}
	for i, l := range lines {
		if n := ansi.StringWidth(l); n != w {
			t.Fatalf("line %d is %d wide, want %d: %q", i, n, w, l)
		}
	}
	all := strings.Join(lines, "\n")
	for _, want := range []string{"TIC-TAC-TOE", "O to move", "bottom-right", "game #1", "1 browser"} {
		if want == "1 browser" {
			want = "0 browsers" // TUI subscriber is not counted as a browser
		}
		if !strings.Contains(all, want) {
			t.Errorf("screen lacks %q", want)
		}
	}

	// Mouse geometry must match what was drawn: occupied cells show glyph
	// ink where layoutFor says they are, and a click there maps back.
	g := layoutFor(w, hgt)
	for _, cell := range []int{0, 4, 8} {
		cx := g.gridX + 1 + (cell%3)*(g.cw+1) + g.cw/2
		cy := g.gridY + 1 + (cell/3)*(g.ch+1) + g.ch/2
		// Check the cell's whole middle row: an O's exact centre is hollow.
		row := []rune(lines[cy])
		if strings.TrimSpace(string(row[cx-g.cw/2:cx+g.cw/2+1])) == "" {
			t.Errorf("cell %d middle row at y=%d is blank: %q", cell, cy, lines[cy])
		}
		if got, ok := g.cellAt(cx, cy); !ok || got != cell {
			t.Errorf("cellAt(%d,%d) = %d,%v want %d", cx, cy, got, ok, cell)
		}
	}
	if _, ok := g.cellAt(g.gridX, g.gridY); ok {
		t.Error("grid corner should not hit a cell")
	}
}

func TestKeysAndClickPlay(t *testing.T) {
	h := hub.Start(t.Context(), core.None, core.Hard, 0)
	m, cancel := New(h, "")
	defer cancel()
	next, _ := m.Update(tea.WindowSizeMsg{Width: 100, Height: 32})
	m = next.(Model)

	next, _ = m.Update(tea.KeyPressMsg{Code: tea.KeyRight})
	if m = next.(Model); m.cursor != 5 {
		t.Fatalf("cursor = %d", m.cursor)
	}
	// Click the centre of cell 0; the returned Cmd performs the hub call.
	g := layoutFor(100, 32)
	_, cmd := m.Update(tea.MouseClickMsg{X: g.gridX + 2, Y: g.gridY + 2, Button: tea.MouseLeft})
	if cmd == nil {
		t.Fatal("click produced no command")
	}
	if res := cmd().(resultMsg); res.err != nil {
		t.Fatal(res.err)
	}
	deadline := time.Now().Add(time.Second)
	for h.Snapshot().Board[0] != core.X {
		if time.Now().After(deadline) {
			t.Fatal("click did not play cell 0")
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func TestNarrowAndTinyTerminals(t *testing.T) {
	h := hub.Start(t.Context(), core.None, core.Hard, 0)
	m, cancel := New(h, "")
	defer cancel()
	if all := strings.Join(screen(t, m, 50, 22), "\n"); !strings.Contains(all, "to move") {
		t.Error("narrow layout lost the board")
	}
	if all := strings.Join(screen(t, m, 20, 10), "\n"); !strings.Contains(all, "too small") {
		t.Error("tiny terminal should say so")
	}
	m.help = true
	if all := strings.Join(screen(t, m, 100, 32), "\n"); !strings.Contains(all, "press any key to close") {
		t.Error("help overlay missing")
	}
}
