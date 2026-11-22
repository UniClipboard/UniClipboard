package main

import (
	"errors"
	"os"
	"path/filepath"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
)

// Bind only the launcher-selected profile directory. The view supplies neither
// a path nor a URL, and the native host never executes a shell command here.
func bindStorageActions(screen *desktopui.Desktop, dir string) {
	screen.SetStorageDirectoryOpener(func() error {
		if !filepath.IsAbs(dir) {
			return errors.New("data directory is unavailable")
		}
		info, err := os.Stat(dir)
		if err != nil || !info.IsDir() {
			return errors.New("data directory is unavailable")
		}
		return mygo.Shell.OpenPath(dir)
	})
}
