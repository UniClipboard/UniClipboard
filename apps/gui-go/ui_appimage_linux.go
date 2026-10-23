//go:build linux

package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// prepareUICommand exposes immutable AppImage helpers at WebKit's absolute paths.
// Only the UI child enters this namespace; the resident host owns daemon and update
// lifetimes outside it. The host's /run entries, IPC, network and control pipes stay
// available, and WebKit applies its own process sandbox inside this namespace.
func prepareUICommand(cmd *exec.Cmd) error {
	appdir := os.Getenv("APPDIR")
	if appdir == "" || os.Getenv("APPIMAGE") == "" {
		return nil
	}
	metadata, err := os.ReadFile(filepath.Join(appdir, "usr/share/uniclipboard/webkit-runtime-root"))
	if err != nil {
		return fmt.Errorf("read AppImage WebKit runtime layout: %w", err)
	}
	root := strings.TrimSpace(string(metadata))
	if filepath.Dir(root) != "/run" || filepath.Clean(root) != root {
		return fmt.Errorf("invalid AppImage WebKit runtime root %q", root)
	}
	entries, err := os.ReadDir("/run")
	if err != nil {
		return fmt.Errorf("read runtime mount entries: %w", err)
	}
	args := []string{"--unshare-user", "--die-with-parent", "--bind", "/", "/", "--dev-bind", "/dev", "/dev", "--tmpfs", "/run"}
	for _, entry := range entries {
		path := filepath.Join("/run", entry.Name())
		if path == root {
			continue
		}
		if entry.Type()&os.ModeSymlink != 0 {
			target, err := os.Readlink(path)
			if err != nil {
				return fmt.Errorf("read runtime symlink: %w", err)
			}
			args = append(args, "--symlink", target, path)
		} else {
			args = append(args, "--bind-try", path, path)
		}
	}
	args = append(args, "--ro-bind", filepath.Join(appdir, "usr"), root, "--")
	args = append(args, cmd.Args...)
	cmd.Path = "/usr/bin/bwrap"
	cmd.Args = append([]string{cmd.Path}, args...)
	return nil
}
