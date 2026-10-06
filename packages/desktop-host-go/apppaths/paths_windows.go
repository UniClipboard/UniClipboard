package apppaths

import (
	"path/filepath"

	"golang.org/x/sys/windows"
)

// localAppData resolves FOLDERID_LocalAppData like the Rust `dirs` crate.
func localAppData() (string, bool) {
	path, err := windows.KnownFolderPath(windows.FOLDERID_LocalAppData, 0)
	return path, err == nil && path != ""
}

func dataLocalDir() (string, bool) { return localAppData() }

func cacheDir() (string, bool) { return localAppData() }

func platformLogDir(appDirName string) (string, bool) {
	base, ok := localAppData()
	if !ok {
		return "", false
	}
	return filepath.Join(base, appDirName, "logs"), true
}
