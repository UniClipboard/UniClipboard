//go:build !linux

package main

import "os/exec"

// startHostHelper starts a host program. Only the Linux AppImage changes the environment of its helpers (host_helper_env_linux.go).
func startHostHelper(name string, args ...string) error {
	cmd := exec.Command(name, args...)
	if err := cmd.Start(); err != nil {
		return err
	}
	go func() { _ = cmd.Wait() }()
	return nil
}
