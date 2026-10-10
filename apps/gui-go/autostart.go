package main

import (
	"bytes"
	"context"
	"encoding/xml"
	"errors"
	"io/fs"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// productName names the login item of the primary (profile-less) app. It is injected at build time from the Tauri configuration so both
// shells register the same item; the fallback only serves `go run` and tests.
var productName = "UniClipboard"

// autostartLaunchArg tags launches started by the login item. Nothing branches on it (the Tauri shell kept it
// for diagnostics too); it is dropped on the SMAppService path, where macOS launches the bundle without arguments.
const autostartLaunchArg = "--autostart"

// errProfileLoginItem refuses OS login-item changes from a named profile instance running inside an app bundle.
var errProfileLoginItem = errors.New("a named profile instance must not change the login item of its app bundle")

// osAutostart is the part of the Wails `app.Autostart` manager this adapter uses.
type osAutostart interface {
	EnableWithOptions(application.AutostartOptions) error
	Disable() error
	Status() (application.AutostartStatus, error)
}

// loginItemPolicy is what Wails does not decide for us: which login item this instance may touch.
//
// Pinned beta.28 behavior (pkg/application/autostart_darwin*.go): on the SMAppService path (bundled, macOS 13+)
// `Identifier` and `Arguments` are ignored and the item belongs to the whole bundle; on the LaunchAgent path
// `Status` and `Disable` find the plist by executable path, not by label. `Identifier` therefore only names the
// LaunchAgent of an unbundled binary and cannot isolate a profile; the guard below does.
type loginItemPolicy struct {
	ProductName string
	Profile     string
	Executable  string
	Home        string
	// AllowProfileBundle lets a named profile act on its bundle's login item. Only the isolated test build sets it.
	AllowProfileBundle bool
}

// name is the login item's identifier, and the name of its LaunchAgent plist on the LaunchAgent path.
func (p loginItemPolicy) name() string {
	if p.Profile != "" {
		return p.ProductName + "-" + p.Profile
	}
	return p.ProductName
}

// runningFromAppBundle reports whether exe lives at <name>.app/Contents/MacOS/<binary>.
func runningFromAppBundle(exe string) bool {
	macOS := filepath.Dir(exe)
	contents := filepath.Dir(macOS)
	return filepath.Base(macOS) == "MacOS" && filepath.Base(contents) == "Contents" && strings.HasSuffix(filepath.Dir(contents), ".app")
}

// apply makes the OS login item follow `enabled`. At startup (`reconcile`) a registration that already exists is
// left alone: re-enabling on every launch would re-bootstrap a LaunchAgent and spawn another instance.
func (p loginItemPolicy) apply(login osAutostart, enabled, reconcile bool) error {
	if p.Profile != "" && runningFromAppBundle(p.Executable) && !p.AllowProfileBundle {
		return errProfileLoginItem
	}
	if err := p.sweepLegacy(); err != nil {
		return err
	}
	if !enabled {
		// Not gated on IsEnabled: it reports a registration the user turned off in System Settings as absent.
		return login.Disable()
	}
	if reconcile {
		if status, err := login.Status(); err == nil && status.Enabled {
			return nil
		}
	}
	return login.EnableWithOptions(application.AutostartOptions{Identifier: p.name(), Arguments: []string{autostartLaunchArg}})
}

// sweepLegacy removes a macOS LaunchAgent with this login item's name that points at another executable: the
// Tauri shell wrote `<product name>.plist` for its own binary, which Wails never sees (it matches the running
// executable), so it would launch the app a second time next to the Wails registration. Only this instance's own
// name is considered, so a named profile never reads the primary entry. The job is not booted out: it may be the
// process that is running this code.
func (p loginItemPolicy) sweepLegacy() error {
	switch runtime.GOOS {
	case "windows":
		return p.sweepLegacyRunValue()
	case "linux":
		return p.sweepLegacyDesktopEntry()
	}
	if runtime.GOOS != "darwin" || p.Home == "" {
		return nil
	}
	path := filepath.Join(p.Home, "Library", "LaunchAgents", p.name()+".plist")
	data, err := os.ReadFile(path)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	var escaped strings.Builder
	_ = xml.EscapeText(&escaped, []byte(p.Executable))
	if bytes.Contains(data, []byte(p.Executable)) || bytes.Contains(data, []byte(escaped.String())) {
		return nil
	}
	if err := os.Remove(path); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	log.Printf("removed the legacy login item %s (pointed at another executable)", path)
	return nil
}

func currentLoginItemPolicy() (loginItemPolicy, error) {
	exe, err := os.Executable()
	if err != nil {
		return loginItemPolicy{}, err
	}
	if resolved, err := filepath.EvalSymlinks(exe); err == nil {
		exe = resolved
	}
	home, _ := os.UserHomeDir()
	profile, _ := apppaths.Profile()
	return loginItemPolicy{ProductName: productName, Profile: profile, Executable: exe, Home: home, AllowProfileBundle: profileBundleLoginItemAllowed()}, nil
}

// autoStartStore is the stored preference (the daemon's settings), the source of truth.
type autoStartStore interface {
	get() (bool, error)
	set(enabled bool) error
}

type daemonAutoStartStore struct {
	ctx    context.Context
	client *daemonclient.Client
}

func (s daemonAutoStartStore) get() (bool, error) {
	var settings struct {
		General struct {
			AutoStart bool `json:"autoStart"`
		} `json:"general"`
	}
	err := s.client.Get(s.ctx, "/settings", &settings)
	return settings.General.AutoStart, err
}

func (s daemonAutoStartStore) set(enabled bool) error {
	patch := map[string]any{"general": map[string]any{"autoStart": enabled}}
	return s.client.Enveloped(s.ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, nil)
}

// applyAutoStart persists the preference first, then applies the OS registration; if that fails the preference is
// rolled back so it never claims a state the OS did not reach.
func applyAutoStart(store autoStartStore, policy loginItemPolicy, login osAutostart, enabled bool) error {
	previous, err := store.get()
	if err != nil {
		return hostapi.Internal(err)
	}
	if err := store.set(enabled); err != nil {
		return hostapi.Internal(err)
	}
	if err := policy.apply(login, enabled, false); err != nil {
		if rollback := store.set(previous); rollback != nil {
			log.Printf("failed to roll back autoStart after the OS registration failed: %v", rollback)
		}
		return hostapi.New(hostapi.CodeInternalError, "Failed to apply OS autostart: "+err.Error())
	}
	return nil
}

func (h *HostService) autoStartSetting(ctx context.Context) (bool, error) {
	return daemonAutoStartStore{ctx: ctx, client: h.daemon()}.get()
}

func (h *HostService) updateAutoStart(ctx context.Context, enabled bool) error {
	policy, err := currentLoginItemPolicy()
	if err != nil {
		return hostapi.Internal(err)
	}
	return applyAutoStart(daemonAutoStartStore{ctx: ctx, client: h.daemon()}, policy, h.loginItem(), enabled)
}

// reconcileAutoStart makes the OS registration follow the stored preference at startup. When enabled it
// registers only if nothing is registered (a stale entry is invisible to Wails and is replaced or swept). An
// unreadable setting leaves the OS untouched: its default is `false`, and acting on it would remove a login item
// the user had enabled.
func (h *HostService) reconcileAutoStart() {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	enabled, err := h.autoStartSetting(ctx)
	if err != nil {
		log.Printf("skipping OS autostart reconcile: settings failed to load: %v", err)
		return
	}
	policy, err := currentLoginItemPolicy()
	if err == nil {
		err = policy.apply(h.loginItem(), enabled, true)
	}
	if errors.Is(err, errProfileLoginItem) {
		log.Printf("skipping OS autostart reconcile: %v", err)
	} else if err != nil {
		log.Printf("failed to reconcile OS autostart on startup: %v", err)
	}
}

// UpdateAutostart registers or removes the login item and stores the preference. The preference is saved first and
// put back if the operating system refuses the registration.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) UpdateAutostart(ctx context.Context, enabled bool) error {
	ctx, cancel := commandContext(ctx, "update_autostart")
	defer cancel()
	return h.updateAutoStart(ctx, enabled)
}

// firstCommandToken is the executable of a Run value: the first token of the command line, honouring surrounding
// double quotes (paths with spaces), like Wails' own matching in autostart_windows.go.
func firstCommandToken(command string) string {
	command = strings.TrimSpace(command)
	if strings.HasPrefix(command, `"`) {
		if end := strings.IndexByte(command[1:], '"'); end >= 0 {
			return command[1 : 1+end]
		}
		return command[1:]
	}
	if i := strings.IndexAny(command, " \t"); i >= 0 {
		return command[:i]
	}
	return command
}
