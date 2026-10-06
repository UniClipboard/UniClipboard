//go:build e2e && darwin

package main

import (
	"os"
	"path/filepath"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

// updateMarker locates the file the update E2E puts into the replacement bundle and says whether it exists in the
// bundle this process runs from.
func updateMarker() (string, bool) {
	exe, _ := os.Executable()
	bundle, err := update.BundleOf(exe)
	if err != nil {
		return "", false
	}
	_, err = os.Stat(filepath.Join(bundle, "Contents", "Resources", "update-marker.txt"))
	return bundle, err == nil
}
