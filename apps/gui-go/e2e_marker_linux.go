//go:build e2e && linux

package main

import (
	"os"
	"path/filepath"
)

// updateMarker locates the file the update E2E puts into the replacement AppImage (usr/share/uniclipboard/update-marker.txt,
// written by package_linux.py --update-marker) and says whether it exists in the image this process runs from: $APPDIR is the
// mounted (or extracted) image that AppRun exported.
func updateMarker() (string, bool) {
	appDir := os.Getenv("APPDIR")
	if appDir == "" {
		return "", false
	}
	_, err := os.Stat(filepath.Join(appDir, "usr", "share", "uniclipboard", "update-marker.txt"))
	return appDir, err == nil
}
