package main

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
)

// Run the shipped helper directly, without shell interpolation or exposing
// the passphrase in arguments. The helper owns locking and import recovery.
func runMigration(args []string) error {
	executable, err := os.Executable()
	if err != nil {
		return errors.New("cannot locate migration helper")
	}
	name := "unic-migrate"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	helper := filepath.Join(filepath.Dir(executable), name)
	if st, err := os.Stat(helper); err != nil || !st.Mode().IsRegular() {
		return errors.New("unic-migrate must be installed beside unic")
	}
	cmd := exec.Command(helper, args...)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
	if err := cmd.Run(); err != nil {
		return errors.New("migration did not complete; see its report and rerun to resume")
	}
	return nil
}
