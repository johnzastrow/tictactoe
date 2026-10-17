// Package tui is the pt terminal client (Bubble Tea v2): login, a live
// replica of the server's data, and the views from docs/pt/ui-spec.md.
package tui

import (
	"fmt"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/client"
)

type Options struct {
	Server string
	User   string
	CACert string
	Theme  string
	// ForceColors: "", "256" or "truecolor" (overrides detection).
	ForceColors string
}

// Run starts the TUI. A cached session token for the server is reused; a
// login screen appears when there is none or it has expired.
func Run(opts Options) error {
	cl, err := client.New(opts.Server, opts.CACert)
	if err != nil {
		return err
	}
	cl.SetToken(client.LoadToken(opts.Server))
	a := newApp(opts, cl)
	defer a.cancel()
	p := tea.NewProgram(Model{a})
	if _, err := p.Run(); err != nil {
		return fmt.Errorf("tui: %w", err)
	}
	return nil
}

// ForgetSession deletes the cached session token for a server.
func ForgetSession(server string) error { return client.ForgetToken(server) }
