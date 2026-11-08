package v1migration

import (
	"strings"
	"testing"

	"github.com/zeebo/blake3"
)

func TestLegacyContentHash(t *testing.T) {
	sum := blake3.Sum256(nil)
	value := "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
	for _, encoded := range []string{"blake3v1:" + value, "blake3v1:" + strings.ToUpper(value)} {
		if !matchesContentHash(encoded, sum[:]) {
			t.Fatal("valid legacy digest rejected")
		}
	}
	for _, encoded := range []string{value, "blake3:" + value, "blake3v1:" + value[:62], "blake3v1:" + strings.Repeat("0", 64)} {
		if matchesContentHash(encoded, sum[:]) {
			t.Fatal("invalid legacy digest accepted")
		}
	}
}
