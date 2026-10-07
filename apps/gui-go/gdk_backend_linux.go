//go:build linux

package main

import (
	"log"
	"os"
	"strings"
)

// Wails v3.0.0-beta.28 (pkg/application/application_linux_gtk3.go, init) exports GDK_BACKEND=x11 when GDK_BACKEND is unset and XDG_SESSION_TYPE is empty,
// "unspecified" or "x11", whatever the display environment says. A session that does not export XDG_SESSION_TYPE but does offer a live Wayland socket (a launcher or
// service manager without the variable) therefore ran through XWayland and could never use Layer Shell. This init runs after the Wails package initialiser (dependencies
// are initialised first) and withdraws only that guess, and only when all of these hold: the user did not set GDK_BACKEND (the process's initial environment, which
// os.Setenv does not change, is the witness), the session type is unknown rather than "x11", and WAYLAND_DISPLAY names a Unix socket that exists. An explicit
// GDK_BACKEND (including "x11"), an "x11" session type and a host without a Wayland socket keep exactly what they had; GTK then picks its own backend, Wayland first.
func init() {
	if os.Getenv("GDK_BACKEND") != "x11" {
		return
	}
	switch os.Getenv("XDG_SESSION_TYPE") {
	case "", "unspecified":
	default:
		return
	}
	if initialEnvironmentHas("GDK_BACKEND") || !waylandSocketExists() {
		return
	}
	if err := os.Unsetenv("GDK_BACKEND"); err == nil {
		log.Printf("session type is unknown but a Wayland socket exists: left the display backend to GTK instead of Wails' X11 default (set GDK_BACKEND=x11 to force XWayland)")
	}
}

// initialEnvironmentHas reports whether the process was started with the variable, from /proc/self/environ (the initial environment block). An unreadable file
// answers true: without a witness the user's choice cannot be ruled out, so nothing is changed.
func initialEnvironmentHas(name string) bool {
	data, err := os.ReadFile("/proc/self/environ")
	if err != nil {
		return true
	}
	for _, entry := range strings.Split(string(data), "\x00") {
		if strings.HasPrefix(entry, name+"=") {
			return true
		}
	}
	return false
}

func waylandSocketExists() bool {
	display := os.Getenv("WAYLAND_DISPLAY")
	if display == "" {
		return false
	}
	path := display
	if !strings.HasPrefix(display, "/") {
		runtime := os.Getenv("XDG_RUNTIME_DIR")
		if runtime == "" {
			return false
		}
		path = runtime + "/" + display
	}
	info, err := os.Stat(path)
	return err == nil && info.Mode()&os.ModeSocket != 0
}
