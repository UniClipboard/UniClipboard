package v1migration

import (
	"encoding/hex"

	paths "github.com/uniclipboard/unic/internal/localpaths"

	"github.com/zeebo/blake3"
)

func generationToken(generation []byte) string {
	b := append([]byte("uniclipboard/profile-upgrade-generation-path/v1\x00"), generation...)
	sum := blake3.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func DefaultSource() (string, error) {
	dir, err := paths.V1DataDir()
	if err != nil {
		return "", ErrFormat
	}
	return dir, nil
}
