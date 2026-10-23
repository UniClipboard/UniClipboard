package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/wailsapp/wails/v3/pkg/application"
)

const uiChildArg = "--ui-child"

var uiChild bool
var uiInput, uiOutput *os.File

// uiProcess owns the single UI child. The resident host never constructs a WebView.
type uiProcess struct {
	host    *HostService
	mu      sync.Mutex
	child   *exec.Cmd
	peer    *uiPeer
	exited  chan struct{}
	closing bool
}

func (u *uiProcess) startLocked() (*uiPeer, error) {
	if u.host.quitting.Load() {
		return nil, errors.New("resident host is shutting down")
	}
	if u.child != nil {
		if u.closing {
			select {
			case <-u.exited:
			case <-time.After(10 * time.Second):
				return nil, errors.New("previous UI process is still closing")
			}
		}
		select {
		case <-u.exited:
			u.child = nil
			u.peer = nil
		default:
			return u.peer, nil
		}
	}
	exe, err := os.Executable()
	if err != nil {
		return nil, err
	}
	input, send, err := os.Pipe()
	if err != nil {
		return nil, err
	}
	receive, output, err := os.Pipe()
	if err != nil {
		input.Close()
		send.Close()
		return nil, err
	}
	ready := make(chan struct{}, 1)
	helper := exe
	if runtime.GOOS == "darwin" {
		helper = filepath.Join(filepath.Dir(exe), "..", "Helpers", "UniClipboardUI.app", "Contents", "MacOS", "gui-go")
	}
	cmd := exec.Command(helper, uiChildArg)
	// The child has a UI service but must not consume the resident host's test control file.
	cmd.Env = os.Environ()
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := startUICommand(cmd, input, output); err != nil {
		input.Close()
		send.Close()
		receive.Close()
		output.Close()
		return nil, err
	}
	input.Close()
	output.Close()
	u.closing = false
	u.child = cmd
	u.exited = make(chan struct{})
	exited := u.exited
	peer := newUIPeer(receive, send, func(m uiPacket) (any, error) {
		if m.Operation == "ready" {
			select {
			case ready <- struct{}{}:
			default:
			}
			return nil, nil
		}
		return u.host.handleUIOwnerRequest(m)
	})
	u.peer = peer
	go func() {
		err := cmd.Wait()
		peer.close()
		close(exited)
		log.Printf("UI process exited pid=%d error=%v", cmd.Process.Pid, err)
	}()
	select {
	case <-ready:
		log.Printf("UI process ready pid=%d", cmd.Process.Pid)
		return peer, nil
	case <-exited:
		return nil, errors.New("UI process exited before readiness")
	case <-time.After(30 * time.Second):
		peer.close()
		return nil, errors.New("UI process readiness timed out")
	}
}
func (u *uiProcess) call(operation string, data any, out any) error {
	u.mu.Lock()
	p, err := u.startLocked()
	u.mu.Unlock()
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	return p.call(ctx, operation, data, out)
}
func (u *uiProcess) open(operation string, data any) {
	go func() {
		if err := u.call(operation, data, nil); err != nil {
			log.Printf("UI request %s failed: %v", operation, err)
			u.host.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error()).Show()
		}
	}()
}
func (u *uiProcess) stop() {
	u.mu.Lock()
	defer u.mu.Unlock()
	if u.child == nil {
		return
	}
	select {
	case <-u.exited:
		return
	default:
	}
	u.peer.close() // EOF requests child shutdown even when it has no window.
	select {
	case <-u.exited:
	case <-time.After(10 * time.Second):
		// This process handle belongs to the exact child we spawned, never a name/PID lookup.
		_ = u.child.Process.Kill()
		<-u.exited
	}
}
func (u *uiProcess) event(name string, payload any) {
	u.mu.Lock()
	p := u.peer
	u.mu.Unlock()
	if p != nil {
		data, _ := json.Marshal(struct {
			Name    string
			Payload any
		}{name, payload})
		_ = p.send(uiPacket{Operation: "event", Data: data})
	}
}
func (u *uiProcess) state() map[string]any {
	u.mu.Lock()
	defer u.mu.Unlock()
	result := map[string]any{"uiPid": 0, "mainExists": false, "mainVisible": false, "dockIconVisible": false}
	if u.child == nil {
		return result
	}
	select {
	case <-u.exited:
		return result
	default:
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := u.peer.call(ctx, "state", nil, &result); err != nil {
		result["stateError"] = err.Error()
	}
	return result
}
func (h *HostService) ownerCall(ctx context.Context, operation string, data any, out any) error {
	return h.owner.call(ctx, operation, data, out)
}
func (h *HostService) handleUIOwnerRequest(m uiPacket) (any, error) {
	ctx, cancel := context.WithTimeout(context.Background(), commandTimeout("install_update"))
	defer cancel()
	switch m.Operation {
	case "desktop-theme":
		return h.GetDesktopTheme(), nil
	case "desktop-theme-follow":
		var enabled bool
		if err := json.Unmarshal(m.Data, &enabled); err != nil {
			return nil, err
		}
		return h.SetFollowOmarchyTheme(enabled)
	case "ui-closing":
		h.ui.mu.Lock()
		h.ui.closing = true
		h.ui.mu.Unlock()
	case "quit-app":
		go h.quit(false)
	case "restart-app":
		go h.fullRestart()
	case "restart-gui":
		go func() {
			if err := h.restartGUI(); err != nil {
				log.Printf("GUI restart failed: %v", err)
			}
		}()
	case "restart-daemon":
		return nil, h.RestartDaemon(ctx)
	case "autostart":
		var enabled bool
		if err := json.Unmarshal(m.Data, &enabled); err != nil {
			return nil, err
		}
		return nil, h.UpdateAutostart(ctx, enabled)
	case "quick-enabled":
		var v bool
		if err := json.Unmarshal(m.Data, &v); err != nil {
			return nil, err
		}
		return nil, h.SetQuickPanelEnabled(ctx, v)
	case "quick-modifier":
		var v QuickPanelDoubleTapModifier
		if err := json.Unmarshal(m.Data, &v); err != nil {
			return nil, err
		}
		return nil, h.SetQuickPanelDoubleTapModifier(ctx, v)
	case "quick-shortcuts":
		var v map[string]json.RawMessage
		if err := json.Unmarshal(m.Data, &v); err != nil {
			return nil, err
		}
		return h.UpdateKeyboardShortcuts(ctx, v)
	case "quick-status":
		return h.GetQuickPanelShortcutStatus(), nil
	case "quick-availability":
		return h.GetQuickPanelDoubleTapAvailability(), nil
	case "notification-permission":
		var request bool
		if err := json.Unmarshal(m.Data, &request); err != nil {
			return nil, err
		}
		if request {
			return h.requestNotificationPermission(), nil
		}
		return h.notificationsGranted(), nil
	case "notification-send":
		var n struct{ ID, Title, Body string }
		if err := json.Unmarshal(m.Data, &n); err != nil {
			return nil, err
		}
		return nil, h.notify(n.ID, n.Title, n.Body)
	case "update-check":
		var channel *string
		if err := json.Unmarshal(m.Data, &channel); err != nil {
			return nil, err
		}
		return h.CheckForUpdate(ctx, channel)
	case "update-download":
		return nil, h.DownloadUpdate(ctx)
	case "update-cancel":
		h.CancelDownload()
	case "update-progress":
		return h.GetDownloadProgress(), nil
	case "update-install":
		return nil, h.InstallUpdate(ctx)
	case "update-skip":
		var version string
		if err := json.Unmarshal(m.Data, &version); err != nil {
			return nil, err
		}
		return nil, h.SkipVersion(ctx, version)
	case "e2e-control":
		var action string
		_ = json.Unmarshal(m.Data, &action)
		return nil, e2eUIControl(h, action, false)
	default:
		return nil, fmt.Errorf("unknown resident-host operation %q", m.Operation)
	}
	return nil, nil
}
func (h *HostService) handleUIRequest(m uiPacket) (any, error) {
	switch m.Operation {
	case "save-window-size":
		h.persistMainWindowSize()
	case "main":
		h.showMainWindow()
	case "settings":
		h.showSettings()
	case "updater":
		var dev bool
		_ = json.Unmarshal(m.Data, &dev)
		h.openUpdater(dev)
	case "panel":
		if _, ok := h.app.Window.GetByName(quickPanelWindowName); !ok {
			h.preCreateQuickPanel()
			h.panel.toggle.configure(true)
		}
		h.requestPanelToggle()
	case "dismiss-panel":
		h.dismissQuickPanel()
	case "event":
		var e struct {
			Name    string
			Payload json.RawMessage
		}
		if err := json.Unmarshal(m.Data, &e); err != nil {
			return nil, err
		}
		payload, err := decodeUIEventPayload(e.Name, e.Payload)
		if err != nil {
			return nil, fmt.Errorf("decode UI event %q: %w", e.Name, err)
		}
		if e.Name == daemonConnectionChanged {
			if client, err := daemonclient.FromEnv(); err == nil {
				h.client.Store(client)
			}
		}
		h.app.Event.Emit(e.Name, payload)
	case "state":
		w, ok := h.app.Window.GetByName("main")
		state := map[string]any{"uiPid": os.Getpid(), "mainExists": ok, "mainVisible": ok && w.IsVisible()}
		if ok {
			state["mainWindowId"] = w.ID()
		}
		e2eUIState(state)
		return state, nil
	case "e2e-control":
		var action string
		_ = json.Unmarshal(m.Data, &action)
		return nil, e2eUIControl(h, action, true)
	default:
		return nil, fmt.Errorf("unknown UI operation %q", m.Operation)
	}
	return nil, nil
}
func (h *HostService) bootstrapUI() {
	client, err := daemonclient.FromEnv()
	if err != nil {
		h.fatal(err)
	}
	h.client.Store(client)
	h.readyMu.Lock()
	h.ready = true
	h.readyMu.Unlock()
	go func() { <-h.owner.done; h.quit(true) }()
	if err := h.owner.send(uiPacket{Operation: "ready"}); err != nil {
		h.quit(true)
	}
}
func uiProcessEnabled() bool { return !uiChild }

// validateUIRole refuses direct UI launches without the inherited private pipes.
func validateUIRole() error {
	exe, _ := os.Executable()
	nested := filepath.Base(filepath.Dir(filepath.Dir(filepath.Dir(exe)))) == "UniClipboardUI.app"
	uiChild = hasArg(os.Args[1:], uiChildArg) || (runtime.GOOS == "darwin" && nested)
	if !uiChild {
		return nil
	}
	var err error
	uiInput, uiOutput, err = inheritedUIPipes()
	return err
}

func openUIPipes(inputFD, outputFD uintptr) (*os.File, *os.File, error) {
	files := make([]*os.File, 0, 2)
	for _, fd := range []uintptr{inputFD, outputFD} {
		if err := prepareUIPipe(fd); err != nil {
			for _, f := range files {
				f.Close()
			}
			return nil, nil, fmt.Errorf("prepare UI control pipe: %w", err)
		}
		f := os.NewFile(fd, "ui-control")
		if f == nil {
			for _, f := range files {
				f.Close()
			}
			return nil, nil, errors.New("invalid UI control pipe")
		}
		files = append(files, f)
		info, err := f.Stat()
		if err != nil || info.Mode()&os.ModeNamedPipe == 0 {
			for _, f := range files {
				f.Close()
			}
			return nil, nil, errors.New("UI child requires inherited control pipes")
		}
	}
	return files[0], files[1], nil
}

func initialActivationPolicy() application.ActivationPolicy {
	if uiProcessEnabled() {
		return application.ActivationPolicyAccessory
	}
	return activationPolicy()
}

// closeUI marks the child as closing before quitting, so a concurrent reopen waits
// for its exit instead of delivering an open to a process that is about to disappear.
func (h *HostService) closeUI() {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := h.ownerCall(ctx, "ui-closing", nil, nil); err != nil {
		log.Printf("UI close coordination failed: %v", err)
	}
	h.quit(true)
}

func (h *HostService) closeIdleUI() {
	if !uiChild {
		return
	}
	if _, ok := h.app.Window.GetByName("main"); ok {
		return
	}
	if _, ok := h.app.Window.GetByName(updaterWindowName); ok {
		return
	}
	go h.closeUI()
}

func (u *uiProcess) running() bool {
	u.mu.Lock()
	defer u.mu.Unlock()
	if u.child == nil || u.closing {
		return false
	}
	select {
	case <-u.exited:
		return false
	default:
		return true
	}
}
func (u *uiProcess) existing(operation string) {
	u.mu.Lock()
	p := u.peer
	u.mu.Unlock()
	if p != nil {
		if err := p.send(uiPacket{Operation: operation}); err != nil {
			log.Printf("UI %s request failed: %v", operation, err)
		}
	}
}

// saveWindowSize flushes an existing UI before a daemon restart; it never creates UI.
func (u *uiProcess) saveWindowSize() {
	u.mu.Lock()
	p := u.peer
	u.mu.Unlock()
	if p == nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := p.call(ctx, "save-window-size", nil, nil); err != nil {
		log.Printf("UI window preference flush failed: %v", err)
	}
}
