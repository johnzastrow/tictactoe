package tui

import (
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/client"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/seed"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/store"
)

var (
	seedOnce   sync.Once
	seedTables core.Tables
	seedErr    error
)

// seeded returns the demo dataset (built once per test binary).
func seeded(t *testing.T) core.Tables {
	t.Helper()
	seedOnce.Do(func() {
		s, err := store.OpenMemory()
		if err != nil {
			seedErr = err
			return
		}
		defer s.Close()
		if _, err := seed.Seed(s, "test-pass-123"); err != nil {
			seedErr = err
			return
		}
		seedTables, seedErr = s.Snapshot()
	})
	if seedErr != nil {
		t.Fatal(seedErr)
	}
	return seedTables
}

// testApp is a logged-in app with the seed data, no network.
func testApp(t *testing.T, w, h int, username string) *App {
	t.Helper()
	// keep tests away from the real config dir
	dir := t.TempDir()
	configPath = func() (string, error) { return filepath.Join(dir, "tui-go.json"), nil }
	tables := seeded(t)
	cl, err := client.New("http://127.0.0.1:1", "")
	if err != nil {
		t.Fatal(err)
	}
	a := newApp(Options{Server: "http://127.0.0.1:1", ForceColors: "truecolor"}, cl)
	a.w, a.h = w, h
	var me core.User
	for _, u := range tables.Users {
		if u.Username == username {
			me = u
		}
	}
	a.rep.Apply(core.WelcomeMsg{Protocol: 1, Server: core.ServerInfo{Impl: "go", Version: "test"}, Me: me, Conn: 1})
	a.rep.Apply(core.SnapshotMsg{Data: tables})
	a.rep.Apply(core.PresenceMsg{Peers: []core.Peer{{Conn: 1, UserID: me.ID, Client: "tui-go"}}})
	a.ph = phMain
	a.conn = connOnline
	a.outbox = &[]any{}
	a.refreshColumns()
	a.rebuild()
	a.afterUpdate()
	return a
}

func (a *App) screen() string {
	b, _ := a.frame()
	return b.String()
}

func key(s string) tea.KeyPressMsg {
	switch s {
	case "enter":
		return tea.KeyPressMsg{Code: tea.KeyEnter}
	case "esc":
		return tea.KeyPressMsg{Code: tea.KeyEscape}
	case "tab":
		return tea.KeyPressMsg{Code: tea.KeyTab}
	case "shift+tab":
		return tea.KeyPressMsg{Code: tea.KeyTab, Mod: tea.ModShift}
	case "up":
		return tea.KeyPressMsg{Code: tea.KeyUp}
	case "down":
		return tea.KeyPressMsg{Code: tea.KeyDown}
	case "left":
		return tea.KeyPressMsg{Code: tea.KeyLeft}
	case "right":
		return tea.KeyPressMsg{Code: tea.KeyRight}
	case "shift+left":
		return tea.KeyPressMsg{Code: tea.KeyLeft, Mod: tea.ModShift}
	case "shift+right":
		return tea.KeyPressMsg{Code: tea.KeyRight, Mod: tea.ModShift}
	case "backspace":
		return tea.KeyPressMsg{Code: tea.KeyBackspace}
	case "delete":
		return tea.KeyPressMsg{Code: tea.KeyDelete}
	case "space":
		return tea.KeyPressMsg{Code: tea.KeySpace, Text: " "}
	case "insert":
		return tea.KeyPressMsg{Code: tea.KeyInsert}
	case "f1", "f2", "f3", "f4", "f6", "f8", "f10":
		n := int(s[1] - '0')
		if len(s) == 3 {
			n = 10
		}
		return tea.KeyPressMsg{Code: tea.KeyF1 + rune(n-1)}
	case "pgdown":
		return tea.KeyPressMsg{Code: tea.KeyPgDown}
	}
	if len(s) > 5 && s[:5] == "ctrl+" {
		return tea.KeyPressMsg{Code: rune(s[5]), Mod: tea.ModCtrl}
	}
	if len(s) > 4 && s[:4] == "alt+" {
		return tea.KeyPressMsg{Code: rune(s[4]), Mod: tea.ModAlt}
	}
	r := []rune(s)[0]
	return tea.KeyPressMsg{Code: r, Text: s}
}

// press sends keys through Update (typing text one rune at a time).
func (a *App) press(keys ...string) []tea.Cmd {
	var cmds []tea.Cmd
	m := Model{a}
	for _, k := range keys {
		_, cmd := m.Update(key(k))
		cmds = append(cmds, cmd)
	}
	return cmds
}

func (a *App) typeText(s string) {
	for _, r := range s {
		a.press(string(r))
	}
}

// dump writes a frame to $PT_TUI_DUMP/name.txt for eyeballing.
func dump(t *testing.T, a *App, name string) {
	dir := os.Getenv("PT_TUI_DUMP")
	if dir == "" {
		return
	}
	_ = os.WriteFile(filepath.Join(dir, name+".txt"), []byte(a.screen()), 0o644)
	b, _ := a.frame()
	_ = os.WriteFile(filepath.Join(dir, name+".ans"), []byte(b.Render(true)), 0o644)
}

// lastOps returns the ops of the last mutate sent.
func (a *App) lastOps(t *testing.T) []core.Op {
	t.Helper()
	for i := len(*a.outbox) - 1; i >= 0; i-- {
		if m, ok := (*a.outbox)[i].(core.MutateMsg); ok {
			return m.Ops
		}
	}
	t.Fatalf("no mutate sent; outbox: %#v", *a.outbox)
	return nil
}

// gotoCol puts the active grid's cursor on a column by key.
func (a *App) gotoCol(t *testing.T, key string) {
	t.Helper()
	g := a.activeGrid()
	i := g.ColIndex(key)
	if i < 0 {
		t.Fatalf("no column %s", key)
	}
	g.Col = i
	a.afterUpdate()
}

func (a *App) userID(name string) int64 {
	for _, u := range a.rep.Users {
		if u.Username == name {
			return u.ID
		}
	}
	return 0
}

func time1s() time.Duration { return time.Second }
