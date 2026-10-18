package tui

// Saved filters (ui-spec §5) live in the user config dir:
// <client.ConfigDir()>/tui-go.json, mode 0600.

import (
	"encoding/json"
	"os"
	"path/filepath"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/client"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type savedFilter struct {
	Name  string          `json:"name"`
	View  int             `json:"view"`
	Query string          `json:"query"`
	Sort  []core.SortSpec `json:"sort,omitempty"`
	Group string          `json:"group,omitempty"`
}

type tuiConfig struct {
	Filters []savedFilter `json:"filters"`
}

// configPath can be overridden in tests.
var configPath = func() (string, error) {
	dir, err := client.ConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "tui-go.json"), nil
}

func loadSaved() []savedFilter {
	p, err := configPath()
	if err != nil {
		return nil
	}
	b, err := os.ReadFile(p)
	if err != nil {
		return nil
	}
	var c tuiConfig
	if json.Unmarshal(b, &c) != nil {
		return nil
	}
	for i := range c.Filters {
		c.Filters[i].View = max(0, min(len(viewNames)-1, c.Filters[i].View))
	}
	return c.Filters
}

func storeSaved(f []savedFilter) error {
	p, err := configPath()
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(p), 0o700); err != nil {
		return err
	}
	b, _ := json.MarshalIndent(tuiConfig{Filters: f}, "", "  ")
	tmp := p + ".tmp"
	if err := os.WriteFile(tmp, b, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, p)
}
