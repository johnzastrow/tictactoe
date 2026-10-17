package auth

import (
	"strings"
	"testing"
)

// Produced by the Rust `pt user add` (argon2 crate) with password "rust-made-pass-1".
const rustHash = "$argon2id$v=19$m=19456,t=2,p=1$5fVLCxCw7Cipno9HNwhzQg$sPzAJ421xc435aVQwGuZ4sU2GKiTpcQLZqNN0pQ8lpU"

func TestRustHashVerifies(t *testing.T) {
	if !VerifyPassword("rust-made-pass-1", rustHash) {
		t.Fatal("Go must verify a Rust-made hash")
	}
	if VerifyPassword("wrong", rustHash) {
		t.Fatal("wrong password accepted")
	}
}

func TestRoundtrip(t *testing.T) {
	h, err := HashPassword("correct horse")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(h, "$argon2id$v=19$m=19456,t=2,p=1$") || len(strings.Split(h, "$")[4]) != 22 {
		t.Fatal(h)
	}
	if !VerifyPassword("correct horse", h) || VerifyPassword("x", h) || VerifyPassword("x", "garbage") {
		t.Fatal("verify")
	}
}

func TestTokens(t *testing.T) {
	a := NewToken()
	if len(a) != 43 || a == NewToken() || len(TokenHash("abc")) != 64 || len(RandomPassword(12)) != 12 {
		t.Fatal("tokens")
	}
}
