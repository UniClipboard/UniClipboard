package v1migration

import (
	"encoding/hex"
	"os"
	"path/filepath"
	"runtime"

	"github.com/zeebo/blake3"
)

func generationToken(generation []byte) string {
	b := append([]byte("uniclipboard/profile-upgrade-generation-path/v1\x00"), generation...)
	sum := blake3.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func DefaultSource() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", ErrFormat
	}
	var base string
	switch runtime.GOOS {
	case "darwin":
		base = filepath.Join(home, "Library", "Application Support")
	case "windows":
		base = os.Getenv("LOCALAPPDATA")
	default:
		base = os.Getenv("XDG_DATA_HOME")
		if base == "" {
			base = filepath.Join(home, ".local", "share")
		}
	}
	if base == "" {
		return "", ErrFormat
	}
	return filepath.Join(base, "app.uniclipboard.desktop"), nil
}
