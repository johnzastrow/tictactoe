package client

import (
	"os"
	"path/filepath"
	"testing"
)

func TestSessionCache(t *testing.T) {
	p := filepath.Join(t.TempDir(), "pt", "sessions.json")
	sessionsFile = func() (string, error) { return p, nil }
	if LoadToken("http://a:1") != "" {
		t.Fatal("empty")
	}
	if err := SaveToken("http://a:1/", "tok"); err != nil {
		t.Fatal(err)
	}
	if LoadToken("http://a:1") != "tok" {
		t.Fatal("normalised key")
	}
	if fi, _ := os.Stat(p); fi.Mode().Perm() != 0o600 {
		t.Fatalf("mode %v", fi.Mode())
	}
	_ = ForgetToken("http://a:1")
	if LoadToken("http://a:1") != "" {
		t.Fatal("forget")
	}
}

func TestNewValidatesURL(t *testing.T) {
	if _, err := New("ftp://x", ""); err == nil {
		t.Fatal("scheme")
	}
	c, err := New("https://h:1/", "")
	if err != nil || c.wsURL("tui-go") != "wss://h:1/ws?client=tui-go" {
		t.Fatal(c.wsURL("tui-go"))
	}
}
