package webapp

// Key specs such as "C-k", "S-F6", "A-1", "Enter" matched against a
// keyboard event's fields. Pure Go (no syscall/js) so it is unit-tested on
// the host. "C-" means Ctrl, or Cmd on macOS.

import "strings"

// Key is the part of a KeyboardEvent the app cares about.
type Key struct {
	Name    string // event.key
	Code    string // event.code
	Ctrl    bool   // ctrlKey || metaKey
	Alt     bool
	Shift   bool
	InInput bool // the event target is a text field
}

type keySpec struct {
	ctrl, alt, shift bool
	name             string
}

func parseSpec(spec string) keySpec {
	var k keySpec
	for {
		switch {
		case strings.HasPrefix(spec, "C-") && len(spec) > 2:
			k.ctrl, spec = true, spec[2:]
			continue
		case strings.HasPrefix(spec, "A-") && len(spec) > 2:
			k.alt, spec = true, spec[2:]
			continue
		case strings.HasPrefix(spec, "S-") && len(spec) > 2:
			k.shift, spec = true, spec[2:]
			continue
		}
		break
	}
	k.name = spec
	return k
}

// Is reports whether the key matches any of the specs.
func (k Key) Is(specs ...string) bool {
	for _, s := range specs {
		if k.matches(parseSpec(s)) {
			return true
		}
	}
	return false
}

func (k Key) matches(s keySpec) bool {
	if s.ctrl != k.Ctrl || s.alt != k.Alt {
		return false
	}
	name := s.name
	single := len([]rune(name)) == 1
	// Shift is implied by the character for printable keys ("?" or "<");
	// for letters and named keys it must match exactly.
	if !single || isLetter(name) {
		if s.shift != k.Shift {
			return false
		}
	} else if s.shift && !k.Shift {
		return false
	}
	switch {
	case isLetter(name):
		// With Alt (macOS Option) or Ctrl the key may be a symbol: use the code.
		if k.Code == "Key"+strings.ToUpper(name) {
			return true
		}
		return strings.EqualFold(k.Name, name)
	case len(name) == 1 && name[0] >= '0' && name[0] <= '9' && (s.alt || s.ctrl):
		return k.Code == "Digit"+name || k.Name == name
	}
	return k.Name == name
}

func isLetter(s string) bool {
	return len(s) == 1 && (s[0] >= 'a' && s[0] <= 'z' || s[0] >= 'A' && s[0] <= 'Z')
}

// Printable: a key that types a character (starts editing in the grid).
func (k Key) Printable() bool {
	return !k.Ctrl && !k.Alt && len([]rune(k.Name)) == 1
}

// Label renders a spec for hints and help: "C-k" → "^K", "S-F6" → "⇧F6".
func Label(spec string) string {
	s := parseSpec(spec)
	var b strings.Builder
	if s.ctrl {
		b.WriteString("^")
	}
	if s.alt {
		b.WriteString("Alt+")
	}
	if s.shift {
		b.WriteString("⇧")
	}
	switch s.name {
	case "ArrowUp":
		b.WriteString("↑")
	case "ArrowDown":
		b.WriteString("↓")
	case "ArrowLeft":
		b.WriteString("←")
	case "ArrowRight":
		b.WriteString("→")
	case "Insert":
		b.WriteString("Ins")
	case "Delete":
		b.WriteString("Del")
	case "Escape":
		b.WriteString("Esc")
	case " ":
		b.WriteString("Space")
	case "PageUp":
		b.WriteString("PgUp")
	case "PageDown":
		b.WriteString("PgDn")
	default:
		if isLetter(s.name) {
			b.WriteString(strings.ToUpper(s.name))
		} else {
			b.WriteString(s.name)
		}
	}
	return b.String()
}
