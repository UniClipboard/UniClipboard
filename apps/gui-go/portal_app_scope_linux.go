//go:build linux

package main

import (
	"log"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/godbus/dbus/v5"
)

// The XDG GlobalShortcuts portal refuses a caller it cannot name ("An app id is required"). For a host (non-sandboxed)
// process it learns the app id from the caller's systemd scope, app[-<launcher>]-<ApplicationID>[-<random>].scope, which
// the desktop environment creates when it launches a desktop entry. An AppImage started from a terminal, a file manager
// or a service has no such scope, so the quick panel shortcut would never bind. Wails opens its own private session bus
// connection for the portal, so org.freedesktop.host.portal.Registry cannot be used from here; moving this process into an
// app scope names every connection it owns instead.

const (
	systemdService = "org.freedesktop.systemd1"
	systemdPath    = "/org/freedesktop/systemd1"
	systemdManager = "org.freedesktop.systemd1.Manager"
)

// inAppScope reports whether the cgroup file content places the process in a desktop-launched app scope or service.
func inAppScope(cgroup string) bool {
	for _, line := range strings.Split(cgroup, "\n") {
		path := line[strings.LastIndex(line, ":")+1:]
		for _, element := range strings.Split(path, "/") {
			if strings.HasPrefix(element, "app-") && (strings.HasSuffix(element, ".scope") || strings.HasSuffix(element, ".service")) {
				return true
			}
		}
	}
	return false
}

// desktopEntryID is the application id of the desktop entry the Linux packages ship (packaging/linux/uniclipboard.desktop,
// also inside the AppImage). The portal and GNOME resolve the scope's app id through that entry, so an installed entry
// with this name is what makes the shortcut bindable from a launch that did not come from the entry.
const desktopEntryID = "uniclipboard"

// appScopeName builds the unit name the portal parses back into desktopEntryID.
func appScopeName(pid int) string {
	return "app-uniclipboard-" + escapeUnitName(desktopEntryID) + "-" + strconv.Itoa(pid) + ".scope"
}

// escapeUnitName escapes the characters systemd reserves in a unit name; an application id only needs "-".
func escapeUnitName(s string) string { return strings.ReplaceAll(s, "-", `\x2d`) }

// ensureAppScope moves this process into a transient app scope when it was not launched into one. It only matters
// under Wayland, where the shortcut goes through the portal, and it never fails startup.
func ensureAppScope() {
	if os.Getenv("XDG_SESSION_TYPE") != "wayland" && os.Getenv("WAYLAND_DISPLAY") == "" {
		return
	}
	if raw, err := os.ReadFile("/proc/self/cgroup"); err != nil || inAppScope(string(raw)) {
		return
	}
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		log.Printf("app scope: session bus unavailable: %v", err)
		return
	}
	defer conn.Close()
	pid := os.Getpid()
	properties := []struct {
		Name  string
		Value dbus.Variant
	}{
		{"Description", dbus.MakeVariant("UniClipboard")},
		{"PIDs", dbus.MakeVariant([]uint32{uint32(pid)})},
	}
	var job dbus.ObjectPath
	call := conn.Object(systemdService, systemdPath).Call(systemdManager+".StartTransientUnit", 0,
		appScopeName(pid), "fail", properties, []struct {
			Name       string
			Properties []struct {
				Name  string
				Value dbus.Variant
			}
		}{})
	if err := call.Store(&job); err != nil {
		log.Printf("app scope: could not start the transient scope: %v", err)
		return
	}
	// The move completes with the job; wait so the shortcut backend and the daemon start inside the scope.
	for deadline := time.Now().Add(3 * time.Second); time.Now().Before(deadline); time.Sleep(25 * time.Millisecond) {
		if raw, err := os.ReadFile("/proc/self/cgroup"); err == nil && inAppScope(string(raw)) {
			return
		}
	}
	log.Printf("app scope: the process did not enter %s in time", appScopeName(pid))
}
