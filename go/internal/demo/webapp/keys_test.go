package webapp

import "testing"

func TestKeyMatching(t *testing.T) {
	cases := []struct {
		k    Key
		spec string
		want bool
	}{
		{Key{Name: "k", Code: "KeyK", Ctrl: true}, "C-k", true},
		{Key{Name: "K", Code: "KeyK", Ctrl: true, Shift: true}, "C-k", false},
		{Key{Name: "K", Code: "KeyK", Ctrl: true, Shift: true}, "C-S-k", true},
		{Key{Name: "¡", Code: "Digit1", Alt: true}, "A-1", true},
		{Key{Name: "˜", Code: "KeyN", Alt: true}, "A-n", true},
		{Key{Name: "F6", Code: "F6", Shift: true}, "S-F6", true},
		{Key{Name: "F6", Code: "F6"}, "S-F6", false},
		{Key{Name: "F6", Code: "F6"}, "F6", true},
		{Key{Name: "<", Code: "Comma", Shift: true}, "<", true},
		{Key{Name: "ArrowRight", Code: "ArrowRight", Alt: true}, "A-ArrowRight", true},
		{Key{Name: "Enter"}, "Enter", true},
		{Key{Name: "Enter", Ctrl: true}, "Enter", false},
		{Key{Name: "Delete", Ctrl: true}, "C-Delete", true},
	}
	for _, c := range cases {
		if got := c.k.Is(c.spec); got != c.want {
			t.Errorf("%+v Is(%q) = %v, want %v", c.k, c.spec, got, c.want)
		}
	}
}

func TestLabels(t *testing.T) {
	for spec, want := range map[string]string{
		"C-k": "^K", "S-F6": "⇧F6", "A-1": "Alt+1", "Insert": "Ins", "ArrowUp": "↑", " ": "Space", "C-Delete": "^Del",
	} {
		if got := Label(spec); got != want {
			t.Errorf("Label(%q) = %q, want %q", spec, got, want)
		}
	}
	if !(Key{Name: "a"}).Printable() || (Key{Name: "a", Ctrl: true}).Printable() || (Key{Name: "Enter"}).Printable() {
		t.Error("Printable")
	}
}

func TestKeyFromSpec(t *testing.T) {
	for _, spec := range []string{"C-s", "Enter", "S-ArrowRight", "A-1", "F8", "C-Delete", "<", "["} {
		if !KeyFromSpec(spec).Is(spec) {
			t.Errorf("KeyFromSpec(%q) does not match its own spec", spec)
		}
	}
}
