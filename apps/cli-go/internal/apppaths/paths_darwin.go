package apppaths

import "path/filepath"

func dataLocalDir() (string, bool) { return homeJoin("Library", "Application Support") }

func cacheDir() (string, bool) { return homeJoin("Library", "Caches") }

func platformLogDir(appDirName string) (string, bool) {
	return homeJoin("Library", "Logs", appDirName)
}

func homeJoin(parts ...string) (string, bool) {
	home, ok := homeDir()
	if !ok {
		return "", false
	}
	return filepath.Join(append([]string{home}, parts...)...), true
}
