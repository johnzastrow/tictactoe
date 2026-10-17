// Package tui is the pt terminal client (placeholder until the Go TUI milestone).
package tui

import (
	"errors"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/client"
)

type Options struct {
	Server string
	User   string
	CACert string
	Theme  string
}

func Run(opts Options) error { return errors.New("the TUI is not built yet") }

// ForgetSession deletes the cached session token for a server.
func ForgetSession(server string) error { return client.ForgetToken(server) }
