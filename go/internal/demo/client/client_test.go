package client

import "testing"

func TestNewValidatesURL(t *testing.T) {
	if _, err := New("ftp://x", ""); err == nil {
		t.Fatal("scheme")
	}
	if _, err := New("http://", ""); err == nil {
		t.Fatal("host")
	}
	c, err := New("https://h:1/", "")
	if err != nil {
		t.Fatal(err)
	}
	if got := c.WSURL("tui-go", "Ada L"); got != "wss://h:1/ws?client=tui-go&name=Ada+L" {
		t.Fatal(got)
	}
	c, _ = New(" http://127.0.0.1:8790 ", "")
	if got := c.WSURL("conform", "bob&x=1"); got != "ws://127.0.0.1:8790/ws?client=conform&name=bob%26x%3D1" {
		t.Fatal(got)
	}
	if _, err := New("https://h:1", "/nonexistent/ca.pem"); err == nil {
		t.Fatal("missing CA file")
	}
}
