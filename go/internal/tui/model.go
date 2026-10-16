// Package tui is the Bubble Tea front end. Like the web page, it is just a
// hub client: it renders StateView and sends intents.
package tui

import (
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/johnzastrow/tictactoe/go/internal/core"
	"git.wharf.fluidgrid.org/johnzastrow/tictactoe/go/internal/hub"
)

const (
	tickEvery = 100 * time.Millisecond
	toastFor  = 2500 * time.Millisecond
)

type Model struct {
	hub     *hub.Hub
	updates <-chan hub.StateView
	state   hub.StateView
	webURL  string

	cursor   int
	theme    int
	help     bool
	ticks    int
	toast    string
	toastErr bool
	toastAt  time.Time
	w, h     int
}

// Messages. Hub calls run inside tea.Cmds so Update itself never blocks.
type (
	stateMsg  hub.StateView
	closedMsg struct{}
	tickMsg   struct{}
	resultMsg struct {
		ok  string
		err error
	}
)

// New builds the model. webURL is shown in the header ("" = web disabled).
// The caller must call the returned cancel func after the program exits.
func New(h *hub.Hub, webURL string) (Model, func()) {
	updates, cancel := h.Subscribe(false)
	return Model{
		hub: h, updates: updates, state: h.Snapshot(), webURL: webURL,
		cursor: 4, theme: defaultTheme(),
	}, cancel
}

func (m Model) Init() tea.Cmd {
	return tea.Batch(waitForState(m.updates), tick())
}

func waitForState(ch <-chan hub.StateView) tea.Cmd {
	return func() tea.Msg {
		v, ok := <-ch
		if !ok {
			return closedMsg{}
		}
		return stateMsg(v)
	}
}

func tick() tea.Cmd {
	return tea.Tick(tickEvery, func(time.Time) tea.Msg { return tickMsg{} })
}

func (m Model) call(ok string, fn func() error) tea.Cmd {
	return func() tea.Msg { return resultMsg{ok: ok, err: fn()} }
}

func (m Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.w, m.h = msg.Width, msg.Height
	case stateMsg:
		m.state = hub.StateView(msg)
		return m, waitForState(m.updates)
	case closedMsg:
		return m, tea.Quit
	case tickMsg:
		m.ticks++
		return m, tick()
	case resultMsg:
		switch {
		case msg.err != nil:
			m.setToast(msg.err.Error(), true)
		case msg.ok != "":
			m.setToast(msg.ok, false)
		}
	case tea.KeyPressMsg:
		return m.onKey(msg)
	case tea.MouseMotionMsg:
		if cell, ok := layoutFor(m.w, m.h).cellAt(msg.X, msg.Y); ok {
			m.cursor = cell
		}
	case tea.MouseClickMsg:
		if cell, ok := layoutFor(m.w, m.h).cellAt(msg.X, msg.Y); ok && msg.Button == tea.MouseLeft {
			m.cursor = cell
			return m, m.play(cell)
		}
	}
	return m, nil
}

func (m Model) onKey(k tea.KeyPressMsg) (tea.Model, tea.Cmd) {
	key := k.String()
	if m.help {
		// Any key closes help (but still allow quitting).
		m.help = false
		if key != "q" && key != "ctrl+c" {
			return m, nil
		}
	}
	row, col := m.cursor/3, m.cursor%3
	switch key {
	case "q", "esc", "ctrl+c":
		return m, tea.Quit
	case "left", "h":
		m.cursor = row*3 + (col+2)%3
	case "right", "l":
		m.cursor = row*3 + (col+1)%3
	case "up", "k":
		m.cursor = ((row+2)%3)*3 + col
	case "down", "j":
		m.cursor = ((row+1)%3)*3 + col
	case "enter", "space":
		return m, m.play(m.cursor)
	case "1", "2", "3", "4", "5", "6", "7", "8", "9":
		m.cursor = int(key[0] - '1')
		return m, m.play(m.cursor)
	case "n":
		return m, m.call("New game", m.hub.NewGame)
	case "a":
		// Cycle: humans -> AI plays O -> AI plays X -> humans.
		next, label := core.O, "AI plays O"
		switch m.state.AI {
		case core.O:
			next, label = core.X, "AI plays X"
		case core.X:
			next, label = core.None, "Two humans"
		}
		return m, m.call(label, func() error { return m.hub.SetAI(next) })
	case "t":
		m.theme = (m.theme + 1) % len(themes)
		m.setToast("Theme: "+themes[m.theme].Name, false)
	case "?":
		m.help = true
	}
	return m, nil
}

func (m Model) play(cell int) tea.Cmd {
	return m.call("", func() error { return m.hub.Play(cell) })
}

func (m *Model) setToast(text string, isErr bool) {
	m.toast, m.toastErr, m.toastAt = text, isErr, time.Now()
}

func (m Model) activeToast() (string, bool, bool) {
	if m.toast == "" || time.Since(m.toastAt) > toastFor {
		return "", false, false
	}
	return m.toast, m.toastErr, true
}
