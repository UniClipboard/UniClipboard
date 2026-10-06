package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"log"
	"os"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// bundleID identifies the shipped application. It is injected at build time from the Tauri configuration
// (the E2E build gets a distinct `.e2e` suffix), so a test build can never share an instance with a real install.
var bundleID = "app.uniclipboard.desktop"

// Launch arguments a second GUI process can carry. `--quick-panel` is the Tauri contract (toggle the panel of the
// running GUI); `--autostart` tags a launch started by the login item.
const (
	quickPanelLaunchArg = "--quick-panel"
	// restartParentEnv names the process a restarted GUI replaces. The new process starts before the old one has
	// released its instance lock, so it waits for the old process to exit first.
	restartParentEnv  = "UC_GUI_RESTART_PARENT_PID"
	restartParentWait = 20 * time.Second
)

// secondLaunchAction is what the running GUI does when another launch reaches it.
type secondLaunchAction string

const (
	actionShowMainWindow  secondLaunchAction = "show-main-window"
	actionIgnoreAutostart secondLaunchAction = "ignore-autostart"
	actionIgnoreQuickPane secondLaunchAction = "ignore-quick-panel"
)

// singleInstanceID is the Wails SingleInstance.UniqueID: the scope of "one GUI". On macOS the lock file lives in
// the per-user temporary directory (not under HOME) and the activation message is a system-wide distributed
// notification named by this ID, so the ID alone separates applications, environments, profiles and data roots.
// Every component is a segment of a valid D-Bus name too (the Linux backend derives its bus name from it).
func singleInstanceID() (string, error) {
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return "", fmt.Errorf("data root unavailable; cannot scope the single instance")
	}
	env := strings.ToLower(os.Getenv("UNICLIPBOARD_ENV"))
	profile, _ := apppaths.Profile()
	sum := sha256.Sum256([]byte(bundleID + "\x00" + env + "\x00" + profile + "\x00" + root))
	return fmt.Sprintf("%s.%s.%s.h%s", bundleID, segment(env), segment(profile), hex.EncodeToString(sum[:6])), nil
}

func segment(s string) string {
	if s == "" {
		return "default"
	}
	return strings.Map(func(r rune) rune {
		if r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' {
			return r
		}
		return '_'
	}, s)
}

func (h *HostService) singleInstanceOptions(uniqueID string) *application.SingleInstanceOptions {
	return &application.SingleInstanceOptions{UniqueID: uniqueID, OnSecondInstanceLaunch: h.onSecondInstance}
}

func hasArg(args []string, want string) bool {
	for _, a := range args {
		if a == want {
			return true
		}
	}
	return false
}

// classifySecondLaunch applies the Tauri single-instance rule (a second launch surfaces the main window) except for
// the launches that must not: the login item's own launch (a login autostart never pops the window) and the quick
// panel request (the Go shell has no host-level panel toggle yet; it arrives with the global shortcut slice).
func classifySecondLaunch(args []string) secondLaunchAction {
	switch {
	case hasArg(args, quickPanelLaunchArg):
		return actionIgnoreQuickPane
	case hasArg(args, autostartLaunchArg):
		return actionIgnoreAutostart
	}
	return actionShowMainWindow
}

// onSecondInstance runs in the first instance when another process of the same scope was launched; that process
// exits by itself after delivering this message. Wails (beta.28) calls it from the one goroutine that drains a
// one-slot channel which the macOS main thread fills from inside the notification handler. Doing UI work here would
// block that goroutine on the main thread while a burst of launches blocks the main thread on the full channel, so
// the callback only hands the message off.
func (h *HostService) onSecondInstance(data application.SecondInstanceData) {
	go h.handleSecondLaunch(data)
}

func (h *HostService) handleSecondLaunch(data application.SecondInstanceData) {
	action := classifySecondLaunch(data.Args)
	log.Printf("second instance launch detected (args %q): %s", data.Args, action)
	if action == actionShowMainWindow && !h.deferShowUntilReady() {
		h.showMainWindow()
	}
	e2eSecondInstance(h, data, action)
}

// deferShowUntilReady holds a "show the main window" request that arrives while the daemon bootstrap is still
// running (the shell has no client, tray or startup settings yet); finishBootstrap carries it out. It reports
// whether the request was held.
func (h *HostService) deferShowUntilReady() bool {
	h.readyMu.Lock()
	defer h.readyMu.Unlock()
	if h.ready {
		return false
	}
	h.showWhenReady = true
	return true
}

// heldShow reports whether a launch during startup already asked for the main window.
func (h *HostService) heldShow() bool {
	h.readyMu.Lock()
	defer h.readyMu.Unlock()
	return h.showWhenReady
}

// finishBootstrap runs after the cold-launch sequence (the last startup task that touches the UI) and marks the shell ready and replays a held second-launch request: the user launched the app again
// while it was starting, so the main window is shown now (a Silent startup still gets its window this way).
func (h *HostService) finishBootstrap() {
	h.readyMu.Lock()
	h.ready = true
	show := h.showWhenReady
	h.readyMu.Unlock()
	if show {
		log.Printf("showing the main window requested by a launch during startup")
		h.showMainWindow()
	}
	e2eBootstrapped(h, show)
}

// restartedGUI is set when this process was started by a restart of another GUI process.
var restartedGUI bool

// waitForRestartParent holds a restarted GUI back until the process it replaces is gone: that process still owns
// the instance lock until it exits, and a new process that raced it would count as a second instance and leave.
func waitForRestartParent() {
	raw := os.Getenv(restartParentEnv)
	if raw == "" {
		return
	}
	restartedGUI = true
	_ = os.Unsetenv(restartParentEnv)
	pid, err := strconv.Atoi(raw)
	if err != nil || pid <= 0 || pid == os.Getpid() {
		return
	}
	for deadline := time.Now().Add(restartParentWait); time.Now().Before(deadline); time.Sleep(50 * time.Millisecond) {
		if syscall.Kill(pid, 0) == syscall.ESRCH {
			return
		}
	}
	log.Printf("the restarted process %d did not exit within %s; continuing", pid, restartParentWait)
}
