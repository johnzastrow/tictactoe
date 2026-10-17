// Package auth: Argon2id password hashes (PHC strings) and session tokens.
//
// Hashes must verify across the Go and Rust servers, so parameters and
// encoding are fixed by protocol §1: m=19456 KiB, t=2, p=1, 16-byte salt,
// 32-byte key, standard base64 without padding.
package auth

import (
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"sync"

	"golang.org/x/crypto/argon2"
)

const (
	SessionTTLSecs = 14 * 24 * 3600
	memKiB         = 19456
	iterations     = 2
	parallelism    = 1
	keyLen         = 32
	saltLen        = 16
)

var b64 = base64.RawStdEncoding

// HashPassword returns a PHC string.
func HashPassword(password string) (string, error) {
	salt := make([]byte, saltLen)
	if _, err := rand.Read(salt); err != nil {
		return "", err
	}
	key := argon2.IDKey([]byte(password), salt, iterations, memKiB, parallelism, keyLen)
	return fmt.Sprintf("$argon2id$v=19$m=%d,t=%d,p=%d$%s$%s", memKiB, iterations, parallelism, b64.EncodeToString(salt), b64.EncodeToString(key)), nil
}

func parsePHC(phc string) (m uint32, t uint32, p uint8, salt, key []byte, err error) {
	parts := strings.Split(phc, "$")
	// "", "argon2id", "v=19", "m=..,t=..,p=..", salt, hash
	if len(parts) != 6 || parts[1] != "argon2id" || parts[2] != "v=19" {
		return 0, 0, 0, nil, nil, errors.New("unsupported hash")
	}
	var pp uint32
	if _, err = fmt.Sscanf(parts[3], "m=%d,t=%d,p=%d", &m, &t, &pp); err != nil {
		return
	}
	if m == 0 || t == 0 || pp == 0 || pp > 255 || m > 1<<22 || t > 64 {
		return 0, 0, 0, nil, nil, errors.New("bad parameters")
	}
	p = uint8(pp)
	if salt, err = b64.DecodeString(parts[4]); err != nil {
		return
	}
	if key, err = b64.DecodeString(parts[5]); err != nil {
		return
	}
	if len(key) == 0 || len(key) > 1024 {
		err = errors.New("bad key")
	}
	return
}

// VerifyPassword compares in constant time; any parse error is a mismatch.
func VerifyPassword(password, phc string) bool {
	m, t, p, salt, key, err := parsePHC(phc)
	if err != nil {
		return false
	}
	got := argon2.IDKey([]byte(password), salt, t, m, p, uint32(len(key)))
	return subtle.ConstantTimeCompare(got, key) == 1
}

var (
	dummyOnce sync.Once
	dummy     string
)

// DummyHash: verify against this when the user doesn't exist, so unknown
// users cost the same time as wrong passwords.
func DummyHash() string {
	dummyOnce.Do(func() { dummy, _ = HashPassword("not-a-real-password-just-timing") })
	return dummy
}

// NewToken: 32 random bytes, base64url without padding (43 chars).
func NewToken() string {
	b := make([]byte, 32)
	if _, err := rand.Read(b); err != nil {
		panic("crypto/rand failed: " + err.Error())
	}
	return base64.RawURLEncoding.EncodeToString(b)
}

func TokenHash(token string) string {
	d := sha256.Sum256([]byte(token))
	return hex.EncodeToString(d[:])
}

// RandomPassword: human-typeable (no ambiguous characters), uniform.
func RandomPassword(n int) string {
	const alpha = "abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789"
	out := make([]byte, 0, n)
	b := make([]byte, 1)
	for len(out) < n {
		if _, err := rand.Read(b); err != nil {
			panic("crypto/rand failed: " + err.Error())
		}
		// rejection sampling keeps the distribution uniform
		if int(b[0]) < 256-256%len(alpha) {
			out = append(out, alpha[int(b[0])%len(alpha)])
		}
	}
	return string(out)
}
