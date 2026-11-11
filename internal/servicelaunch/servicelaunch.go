// Package servicelaunch holds process primitives shared by every launcher of unicd.
// It contains no launch policy: the desktop and the CLI decide when to start or stop.
package servicelaunch

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/rogpeppe/go-internal/lockedfile"
)

// Sibling resolves unicd from the installation of the running executable.
// It never searches PATH, so a tested pair of programs stays together.
func Sibling() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", err
	}
	executable, err = filepath.EvalSymlinks(executable)
	if err != nil {
		return "", err
	}
	name := "unicd"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	return filepath.Join(filepath.Dir(executable), name), nil
}

// CheckExecutable verifies that path is a launchable regular file.
func CheckExecutable(path string) error {
	info, err := os.Stat(path)
	if err != nil {
		return fmt.Errorf("locate unicd beside this program: %w", err)
	}
	if info.IsDir() || (runtime.GOOS != "windows" && info.Mode()&0111 == 0) {
		return errors.New("unicd is not an executable file")
	}
	return nil
}

// Lock serializes service creation and retirement for one state directory.
// Hold it only for a transition, never for a whole command.
func Lock(dir string) (unlock func(), err error) {
	return lockedfile.MutexAt(filepath.Join(dir, "service-transition.lock")).Lock()
}

// Wait polls check every 50 milliseconds until it succeeds, fails or ctx ends.
func Wait(ctx context.Context, check func() (bool, error)) error {
	ticker := time.NewTicker(50 * time.Millisecond)
	defer ticker.Stop()
	for {
		if done, err := check(); done || err != nil {
			return err
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-ticker.C:
		}
	}
}
