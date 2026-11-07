//go:build !windows

package engine

import (
	"os"

	"golang.org/x/sys/unix"
)

// dirLock is an exclusive advisory lock on a file in the data directory. The
// kernel drops it if the process dies, so a crash never leaves a stale lock.
type dirLock struct{ f *os.File }

// lockDir returns errLocked if another instance holds the lock.
func lockDir(path string) (*dirLock, error) {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, err
	}
	if err := unix.Flock(int(f.Fd()), unix.LOCK_EX|unix.LOCK_NB); err != nil {
		f.Close()
		if err == unix.EWOULDBLOCK {
			return nil, errLocked
		}
		return nil, err
	}
	return &dirLock{f}, nil
}

func (l *dirLock) release() {
	_ = unix.Flock(int(l.f.Fd()), unix.LOCK_UN)
	_ = l.f.Close()
}
