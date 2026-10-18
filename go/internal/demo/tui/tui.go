// Package tui is the demo Bubble Tea v2 front end (docs/demo/ui-spec.md): a
// sidebar of 16 tabs, one control each, with pt's menu bar, hint bar,
// command palette and searchable help. Rendering follows pt's TUI: a cell
// buffer encoded to ANSI once per frame, mouse hits resolved against it.
package tui

import (
	"fmt"

	tea "charm.land/bubbletea/v2"
)

// Options are `demo tui`'s flags.
type Options struct {
	Server string // http(s)://host:port
	Name   string // display name; "" shows the join form
	CACert string // extra CA certificate (PEM) for https servers
	Theme  string // midnight, daylight, solarized-dark, gruvbox
	Colors string // "", "256" or "truecolor"
}

// Run starts the TUI and returns when the user quits.
func Run(opts Options) error {
	a := newApp(opts)
	defer a.cancel()
	if opts.Name != "" {
		if err := a.join.connectFlags(a); err != nil {
			return err
		}
	}
	p := tea.NewProgram(Model{a})
	if _, err := p.Run(); err != nil {
		return fmt.Errorf("tui: %w", err)
	}
	return nil
}
