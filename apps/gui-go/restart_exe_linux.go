//go:build linux

package main

import "os"

// restartExecutable is what a restart launches: the AppImage file when running from one (the executable path is
// inside the image's temporary mount, which disappears with the process), otherwise the running binary.
func restartExecutable() (string, error) {
	if appImage := os.Getenv("APPIMAGE"); appImage != "" {
		return appImage, nil
	}
	return os.Executable()
}
