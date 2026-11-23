// Package appupdates connects the desktop installer to its update source.
package appupdates

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/desktopupdate"
)

// PublicKey is supplied by the production packager. Development builds disable updates.
var PublicKey string

const FeedBase = desktopupdate.FeedBase

type Source = desktopupdate.Source

// Owner identifies installations whose files must stay under their package manager.
func Owner(executable string) string {
	real, e := filepath.EvalSymlinks(executable)
	if e == nil {
		executable = real
	}
	dir := filepath.Dir(executable)
	if os.Getenv("SNAP") != "" {
		return "Snap"
	}
	if os.Getenv("FLATPAK_ID") != "" {
		return "Flatpak"
	}
	if os.Getenv("APPIMAGE") != "" {
		return "AppImage"
	}
	if b, e := os.ReadFile(filepath.Join(dir, "update-owner")); e == nil {
		v := strings.TrimSpace(string(b))
		if v != "" && v != "direct" {
			return v
		}
	}
	if _, e := os.Stat(filepath.Join(dir, "portable.dat")); e == nil {
		return "portable archive"
	}
	if _, e := os.Stat(filepath.Join(dir, "install.json")); e == nil {
		return "Scoop"
	}
	if runtime.GOOS == "linux" && (strings.HasPrefix(executable, "/usr/") || strings.HasPrefix(executable, "/nix/store/") || strings.HasPrefix(executable, "/snap/")) {
		return "system package manager"
	}
	if runtime.GOOS == "darwin" {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		bundle := filepath.Dir(filepath.Dir(dir))
		if output, e := exec.CommandContext(ctx, "brew", "list", "--cask", "uniclipboard").Output(); e == nil {
			for _, line := range strings.Split(string(output), "\n") {
				listed := strings.TrimSpace(strings.SplitN(line, " (", 2)[0])
				if resolved, err := filepath.EvalSymlinks(listed); err == nil {
					listed = resolved
				}
				relative, err := filepath.Rel(bundle, listed)
				if err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
					return "Homebrew"
				}
			}
		}
	}
	return "direct"
}
