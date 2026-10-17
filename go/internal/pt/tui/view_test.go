package tui

import (
	"fmt"
	"strings"
	"testing"
)

func TestEveryViewRenders(t *testing.T) {
	for _, size := range [][2]int{{160, 45}, {80, 24}, {120, 32}} {
		a := testApp(t, size[0], size[1], "alice")
		for v := range viewNames {
			a.setView(v)
			a.afterUpdate()
			s := a.screen()
			dump(t, a, fmt.Sprintf("view%d_%dx%d", v, size[0], size[1]))
			lines := strings.Split(s, "\n")
			if len(lines) != size[1] {
				t.Fatalf("view %d: %d lines, want %d", v, len(lines), size[1])
			}
			if !strings.Contains(lines[0], "File") || !strings.Contains(lines[1], viewNames[v]) {
				t.Errorf("view %d %v: missing menu/tabs:\n%s", v, size, s)
			}
		}
	}
}
