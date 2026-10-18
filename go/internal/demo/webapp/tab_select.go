//go:build js && wasm

package webapp

// Tab 4 — Dropdown (Status, Priority) and tab 9 — Cascade (Program ▸
// Project ▸ Phase), both built on the Dropdown widget.

import (
	"fmt"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// ---- tab 4 ------------------------------------------------------------------------------

type selectTab struct {
	tabBase
	drops []*Dropdown
	keys  []string
}

func newSelectTab(b tabBase) Tab {
	t := &selectTab{tabBase: b}
	t.build = t.mount
	return t
}

func (t *selectTab) mount(area js.Value) {
	form := E("div", "form2")
	for _, s := range []struct {
		key, label, name string
		opts             []core.Choice
	}{{"select.status", "Status", "status", core.Statuses}, {"select.priority", "Priority", "priority", core.Priorities}} {
		key := s.key
		row, c, _ := field(s.label)
		d := newDropdown(t.a, &t.scope, s.opts, func(id string) { _ = t.a.Commit(key, id) })
		t.addCtl(d.btn, s.name, key)
		Add(c, d.btn)
		Add(form, row)
		t.drops = append(t.drops, d)
		t.keys = append(t.keys, key)
	}
	Add(area, form)
}

func (t *selectTab) Render() {
	for i, d := range t.drops {
		d.value = t.a.v.String(t.keys[i])
		d.Render()
	}
}

func (t *selectTab) Key(k Key) bool {
	if t.cur < len(t.drops) {
		return t.drops[t.cur].Key(k)
	}
	return false
}

func (t *selectTab) Hints() []hint {
	if t.cur < len(t.drops) {
		return t.drops[t.cur].Hints()
	}
	return nil
}

// ---- tab 9 ------------------------------------------------------------------------------

type cascadeTab struct {
	tabBase
	drops  [3]*Dropdown
	crumb  js.Value
	counts js.Value
}

func newCascadeTab(b tabBase) Tab {
	t := &cascadeTab{tabBase: b}
	t.build = t.mount
	return t
}

func ptr(s string) *string { return &s }

func (t *cascadeTab) mount(area js.Value) {
	form := E("div", "form2")
	labels := []string{"Program", "Project", "Phase"}
	for i := range 3 {
		i := i
		row, c, _ := field(labels[i])
		d := newDropdown(t.a, &t.scope, nil, func(id string) { t.pick(i, id) })
		d.placeholder = "— choose —"
		t.addCtl(d.btn, strings.ToLower(labels[i]), "cascade.pick")
		Add(c, d.btn)
		Add(form, row)
		t.drops[i] = d
	}
	t.crumb = E("div", "crumb")
	t.counts = E("div", "muted counts")
	Add(area, form, t.crumb, t.counts)
}

// pick sets one level and clears the levels below it; the whole object commits.
func (t *cascadeTab) pick(level int, id string) {
	c := t.a.v.Cascade()
	var v *string
	if id != "" {
		v = ptr(id)
	}
	switch level {
	case 0:
		c = core.Cascade{Program: v}
	case 1:
		c.Project, c.Phase = v, nil
	case 2:
		c.Phase = v
	}
	_ = t.a.Commit("cascade.pick", c)
}

func names(list []string) []core.Choice {
	out := make([]core.Choice, len(list))
	for i, s := range list {
		out[i] = core.Choice{ID: s, Label: s}
	}
	return out
}

func deref(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

func (t *cascadeTab) Render() {
	c := t.a.v.Cascade()
	prog, proj, phase := deref(c.Program), deref(c.Project), deref(c.Phase)
	var progs []string
	for _, p := range core.Programs {
		progs = append(progs, p.Name)
	}
	var projs []string
	for _, j := range core.ProjectsOf(prog) {
		projs = append(projs, j.Name)
	}
	phases := core.PhasesOf(prog, proj)
	t.drops[0].opts, t.drops[0].value = names(progs), prog
	t.drops[1].opts, t.drops[1].value = names(projs), proj
	t.drops[2].opts, t.drops[2].value = names(phases), phase
	t.drops[1].disabled, t.drops[2].disabled = "", ""
	if prog == "" {
		t.drops[1].disabled = "pick a program first"
	}
	if proj == "" {
		t.drops[2].disabled = "pick a project first"
	}
	for _, d := range t.drops {
		d.Render()
	}
	parts := []string{}
	for _, s := range []string{prog, proj, phase} {
		if s != "" {
			parts = append(parts, s)
		}
	}
	crumb := strings.Join(parts, " ▸ ")
	if crumb == "" {
		crumb = "nothing picked"
	}
	t.crumb.Set("textContent", crumb)
	t.counts.Set("textContent", fmt.Sprintf("%s · %s · %s", plural(len(progs), "program"), plural(len(projs), "project"), plural(len(phases), "phase")))
}

func plural(n int, w string) string {
	if n == 1 {
		return "1 " + w
	}
	return fmt.Sprintf("%d %ss", n, w)
}

func (t *cascadeTab) Key(k Key) bool {
	if t.cur >= 3 {
		return false
	}
	d := t.drops[t.cur]
	if !d.open && k.Is("Delete") {
		if d.value != "" {
			t.pick(t.cur, "")
		}
		return true
	}
	return d.Key(k)
}

func (t *cascadeTab) Hints() []hint {
	if t.cur < 3 {
		h := t.drops[t.cur].Hints()
		if !t.drops[t.cur].open {
			h = append(h, hint{"Del", "Clear level", "Delete"})
		}
		return h
	}
	return nil
}
