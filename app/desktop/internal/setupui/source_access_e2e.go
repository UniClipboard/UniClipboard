//go:build desktop_e2e

package setupui

import (
	"os"
	"path/filepath"

	"github.com/uniclipboard/unic/internal/v1migration"
)

// The native acceptance build must never discover personal profiles or touch
// the system credential store. Only the canonical runner creates this marker.
func defaultSourceDirectory() (string, error) {
	profile := os.Getenv("UNIC_DESKTOP_E2E_PROFILE")
	canonical, err := filepath.EvalSymlinks(profile)
	if err != nil || !filepath.IsAbs(profile) || profile != canonical || os.Getenv("UNIC_HOME") != profile {
		return "", v1migration.ErrFormat
	}
	marker, err := os.ReadFile(filepath.Join(profile, "e2e-owner"))
	if err != nil || string(marker) != "desktop-e2e" {
		return "", v1migration.ErrFormat
	}
	return filepath.Join(filepath.Dir(profile), "v1-source"), nil
}
func readSourceSecret(string, string) ([]byte, error) { return nil, v1migration.ErrUnlock }
