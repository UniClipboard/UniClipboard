package v1migration

import (
	"bytes"
	"encoding/hex"
	"strings"
)

// ContentHash::from_str in v1 accepts exactly blake3v1:<32 hex bytes>.
// Decode before comparing so uppercase hexadecimal follows Rust's semantics.
func matchesContentHash(encoded string, digest []byte) bool {
	algorithm, value, ok := strings.Cut(encoded, ":")
	if !ok || algorithm != "blake3v1" || len(value) != 64 || len(digest) != 32 {
		return false
	}
	var expected [32]byte
	if _, err := hex.Decode(expected[:], []byte(value)); err != nil {
		return false
	}
	return bytes.Equal(expected[:], digest)
}
