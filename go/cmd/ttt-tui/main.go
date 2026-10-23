// Command ttt-tui: tic-tac-toe in the terminal only.
//
// Same game and keys as ttt, but the web adapter is not linked in, so the
// binary opens no network port and carries no embedded web assets.
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/hub"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/tui"
)

const aiDelay = 350 * time.Millisecond

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

func run() error {
	fs := flag.NewFlagSet("ttt-tui", flag.ExitOnError)
	aiFlag := fs.String("ai", "o", "who the AI plays at startup: off, x, o")
	level := fs.String("difficulty", "medium", "how hard the AI plays at startup: easy, medium, hard")
	fs.Parse(os.Args[1:])

	ai, err := parseAI(*aiFlag)
	if err != nil {
		return err
	}
	difficulty, err := core.ParseDifficulty(*level)
	if err != nil {
		return err
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	h := hub.Start(ctx, ai, difficulty, aiDelay)

	// Empty URL: the TUI shows no web address.
	model, unsubscribe := tui.New(h, "")
	defer unsubscribe()
	_, err = tea.NewProgram(model, tea.WithContext(ctx)).Run()
	if errors.Is(err, tea.ErrProgramKilled) && ctx.Err() != nil {
		err = nil // Ctrl-C via signal, not a failure
	}
	return err
}

func parseAI(s string) (core.Player, error) {
	switch s {
	case "off":
		return core.None, nil
	case "x", "X":
		return core.X, nil
	case "o", "O":
		return core.O, nil
	}
	return core.None, fmt.Errorf("invalid -ai %q (want off, x or o)", s)
}
