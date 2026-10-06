//go:build darwin

package update

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// BundleOf returns the .app bundle containing the running executable.
func BundleOf(executable string) (string, error) {
	path := executable
	for path != "/" && path != "." {
		if strings.HasSuffix(path, ".app") {
			return path, nil
		}
		path = filepath.Dir(path)
	}
	return "", fmt.Errorf("%s is not inside an app bundle", executable)
}

// Install replaces the app bundle at target with the one inside a verified
// `.app.tar.gz` archive. The new bundle is staged beside the target so the
// final swap is a rename on one volume; the old bundle is restored if the
// swap fails.
func Install(archive []byte, target string) error {
	parent := filepath.Dir(target)
	stage, err := os.MkdirTemp(parent, ".uc-update-")
	if err != nil {
		return fmt.Errorf("stage update: %w", err)
	}
	defer os.RemoveAll(stage)
	tarball := filepath.Join(stage, "update.tar.gz")
	if err := os.WriteFile(tarball, archive, 0o600); err != nil {
		return err
	}
	if out, err := exec.Command("/usr/bin/tar", "-xzf", tarball, "-C", stage).CombinedOutput(); err != nil {
		return fmt.Errorf("extract update: %v: %s", err, out)
	}
	apps, _ := filepath.Glob(filepath.Join(stage, "*.app"))
	if len(apps) != 1 {
		return fmt.Errorf("update archive must contain exactly one app bundle, found %d", len(apps))
	}
	backup := target + ".old"
	_ = os.RemoveAll(backup)
	if err := os.Rename(target, backup); err != nil {
		return fmt.Errorf("move current app aside: %w", err)
	}
	if err := os.Rename(apps[0], target); err != nil {
		_ = os.Rename(backup, target)
		return fmt.Errorf("move new app into place: %w", err)
	}
	_ = os.RemoveAll(backup)
	return nil
}
