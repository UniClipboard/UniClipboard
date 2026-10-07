//go:build linux

package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"syscall"
	"time"

	"github.com/godbus/dbus/v5"
)

// pacRunnerName is the session-bus name glib-networking's GNOME proxy resolver calls to evaluate a proxy auto-configuration
// (PAC) script (proxy/gnome/gproxyresolvergnome.c). Without a service behind it the resolver logs "Proxy autoconfiguration will
// not work" and every lookup fails (the 17c12 stage-4 `pac-nohelper-*` runs).
const pacRunnerName = "org.gtk.GLib.PACRunner"

// pacRunnerProcess is the helper this process started, if any (nil: the host provides the service, or there is no session bus).
var pacRunnerProcess *os.Process

// init starts the AppImage's own glib-pacrunner when - and only when - nothing on the user's session bus can provide the
// service. It never replaces or races a host service: a bus that can activate the name is left alone. The helper owns the name
// with the default (queueing) flags, so a second instance simply waits behind the first one; Pdeathsig ends it with this
// process (init runs on the main thread, which lives as long as the process), so nothing outlives the GUI. Runs before the
// WebView exists, because the resolver contacts the helper the first time a PAC configuration is read.
func init() {
	appDir := os.Getenv("APPDIR")
	if appDir == "" {
		return
	}
	helper := filepath.Join(appDir, "usr", "libexec", "glib-pacrunner")
	if _, err := os.Stat(helper); err != nil {
		return
	}
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		fmt.Fprintln(os.Stderr, "pacrunner: no session bus, PAC configurations cannot be evaluated:", err)
		return
	}
	defer conn.Close()
	var activatable []string
	if err := conn.BusObject().Call("org.freedesktop.DBus.ListActivatableNames", 0).Store(&activatable); err == nil && slices.Contains(activatable, pacRunnerName) {
		return // the host ships the service: the bus starts it when the resolver needs it
	}
	cmd := exec.Command(helper)
	cmd.SysProcAttr = &syscall.SysProcAttr{Pdeathsig: syscall.SIGTERM, Setpgid: true}
	if err := cmd.Start(); err != nil {
		fmt.Fprintln(os.Stderr, "pacrunner: cannot start the bundled helper:", err)
		return
	}
	pacRunnerProcess = cmd.Process
	go func() { _ = cmd.Wait() }()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		var owned bool
		if err := conn.BusObject().Call("org.freedesktop.DBus.NameHasOwner", 0, pacRunnerName).Store(&owned); err == nil && owned {
			return
		}
		time.Sleep(50 * time.Millisecond)
	}
	fmt.Fprintln(os.Stderr, "pacrunner: the bundled helper did not own", pacRunnerName, "within 2s")
}
