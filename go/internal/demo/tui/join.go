package tui

// The join form (ui-spec §5): server URL and display name, shown when
// `demo tui` runs without --name or when the server refuses the name.

import (
	"context"
	"fmt"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

func contextWithCancel() (context.Context, context.CancelFunc) {
	return context.WithCancel(context.Background())
}

type joinForm struct {
	server, name lineInput
	focus        int // 0 server, 1 name, 2 Join
	err          string
	rect         Rect
}

func newJoinForm(server, name string) *joinForm {
	f := &joinForm{focus: 1}
	f.server.SetText(server)
	f.name.SetText(name)
	f.name.max = 24
	f.name.placeholder = "e.g. alice"
	return f
}

func (f *joinForm) submit(a *App) {
	name, ok := core.CleanName(f.name.Text())
	if !ok {
		f.err = "Name: start with a letter; letters, digits, space _ . - (up to 24)"
		f.focus = 1
		return
	}
	if err := a.connect(f.server.Text(), name); err != nil {
		f.err = err.Error()
		f.focus = 0
		return
	}
	f.err = ""
	a.queue(a.waitNet())
}

func (f *joinForm) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	switch k.Keystroke() {
	case "esc":
		return a.quit()
	case "ctrl+s":
		f.submit(a)
		return nil
	case "enter":
		if f.focus == 0 {
			f.focus = 1
			return nil
		}
		f.submit(a)
		return nil
	case "tab", "down":
		f.focus = (f.focus + 1) % 3
		return nil
	case "shift+tab", "up":
		f.focus = (f.focus + 2) % 3
		return nil
	case "space":
		if f.focus == 2 {
			f.submit(a)
			return nil
		}
	}
	if in := f.input(); in != nil {
		if _, changed := in.Key(k); changed {
			f.err = ""
		}
	}
	return nil
}

func (f *joinForm) input() *lineInput {
	switch f.focus {
	case 0:
		return &f.server
	case 1:
		return &f.name
	}
	return nil
}

func (f *joinForm) paste(s string) {
	if in := f.input(); in != nil {
		in.Insert(s)
	}
}

func (f *joinForm) mouse(a *App, m hitMsg) {
	if !isClick(m) || !m.ok {
		return
	}
	switch m.hit.Kind {
	case HitField:
		f.focus = m.hit.A
	case HitButton:
		f.submit(a)
	}
}

func (f *joinForm) draw(a *App, b *Buf) []Hit {
	t := a.th()
	r := centered(64, 10, b.W, b.H)
	r.Y = min(b.H-11, max(r.Y, b.H/2-1))
	f.rect = r
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, fill, "Join demo", Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	var hits []Hit
	for i, lbl := range []string{"Server URL", "Name"} {
		y := r.Y + 2 + i*2
		st := fill.Fg(choose(f.focus == i, t.Accent, t.Muted))
		b.Fit(r.X+2, y, lbl, st, 11, 0)
		fr := Rect{r.X + 14, y, r.W - 17, 1}
		in := choose(i == 0, &f.server, &f.name)
		in.Draw(b, fr, t, f.focus == i, fieldStyle(t, f.focus == i))
		hits = append(hits, Hit{R: Rect{r.X + 2, y, r.W - 4, 1}, Kind: HitField, A: i})
	}
	if f.err != "" {
		for i, l := range wrap("✗ "+f.err, r.W-4) {
			if i < 1 {
				b.Text(r.X+2, r.Y+6, l, fill.Fg(t.Err).With(Bold), r.W-4)
			}
		}
	}
	by := r.Bottom() - 2
	b.Text(r.X+2, by, "Enter / ^S join · Tab next · Esc quit", fill.Fg(t.Muted), r.W-14)
	br := button(b, t, r.Right()-11, by, "Join", f.focus == 2, t.CellBG)
	hits = append(hits, Hit{R: br, Kind: HitButton})
	return hits
}

// connectFlags joins straight away with --name (a bad URL or name is an
// error before the screen opens).
func (f *joinForm) connectFlags(a *App) error {
	name, ok := core.CleanName(a.opts.Name)
	if !ok {
		return fmt.Errorf("--name %q: start with a letter; letters, digits, space _ . - (up to 24)", a.opts.Name)
	}
	return a.connect(a.opts.Server, name)
}
