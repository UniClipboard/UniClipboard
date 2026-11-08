package engine

import (
	"errors"
	"os"
	"path/filepath"
)

// openInstanceDir prepares DataDir/plugin-data/<instance> (0700) and takes its
// runtime.lock. Symlinks are refused at every level so a backend cannot be
// pointed outside the data directory.
func openInstanceDir(dataDir, instance string) (string, *dirLock, error) {
	dir := filepath.Join(dataDir, "plugin-data", instance)
	cur := dataDir
	for _, part := range []string{"plugin-data", instance} {
		cur = filepath.Join(cur, part)
		if err := os.MkdirAll(cur, 0o700); err != nil {
			return "", nil, err
		}
		fi, err := os.Lstat(cur)
		if err != nil {
			return "", nil, err
		}
		if !fi.IsDir() || fi.Mode()&os.ModeSymlink != 0 {
			return "", nil, errors.New("instance directory is not a plain directory")
		}
		if err := os.Chmod(cur, 0o700); err != nil {
			return "", nil, err
		}
	}
	lock := filepath.Join(dir, "runtime.lock")
	if fi, err := os.Lstat(lock); err == nil && !fi.Mode().IsRegular() {
		return "", nil, errors.New("runtime.lock is not a regular file")
	}
	l, err := lockDir(lock)
	if err != nil {
		return "", nil, err
	}
	return dir, l, nil
}
