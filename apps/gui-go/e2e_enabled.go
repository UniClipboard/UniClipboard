//go:build e2e

package main

import (
	"path/filepath"

	"context"
	"encoding/json"
	"fmt"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"os"
	"sync"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// EvidenceService is the e2e-only control plane. It exists solely in builds
// tagged `e2e`; normal builds have neither this service nor its bindings.
type EvidenceService struct {
	host *HostService
	mu   sync.Mutex
}

// Step is one assertion reported by the in-WebView driver or by this service.
type Step struct {
	Window string `json:"window"`
	Step   string `json:"step"`
	OK     bool   `json:"ok"`
	Detail any    `json:"detail,omitempty"`
	At     int64  `json:"at"`
}

func (s *EvidenceService) write(step Step) error {
	step.At = time.Now().UnixMilli()
	s.mu.Lock()
	defer s.mu.Unlock()
	data, err := json.Marshal(step)
	if err != nil {
		return err
	}
	f, err := os.OpenFile(os.Getenv("UC_GUI_GO_EVIDENCE"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer f.Close()
	if _, err = f.Write(append(data, '\n')); err != nil {
		return err
	}
	return f.Sync()
}

// Record stores a driver step after cross-checking the daemon PID it observed.
func (s *EvidenceService) Record(step Step) error { return s.write(step) }

// Control performs native window actions the DOM cannot, and records the
// resulting native window state as its own evidence step.
func (s *EvidenceService) Control(action string) error {
	if handled, err := s.controlQuickPanel(action); handled {
		return err
	}
	h := s.host
	main, ok := h.app.Window.GetByName("main")
	switch action {
	case "close-main":
		if !ok {
			return fmt.Errorf("main window absent")
		}
		main.Close()
		time.Sleep(500 * time.Millisecond)
		return s.write(Step{Window: "main", Step: "native-main-closed", OK: !main.IsVisible(), Detail: map[string]bool{"visible": main.IsVisible()}})
	case "reopen-main":
		if !ok {
			return fmt.Errorf("main window absent")
		}
		main.Show()
		main.Focus()
		time.Sleep(500 * time.Millisecond)
		return s.write(Step{Window: "main", Step: "native-main-reopened", OK: main.IsVisible(), Detail: map[string]bool{"visible": main.IsVisible()}})
	case "open-updater":
		h.openUpdater(true)
		return s.write(Step{Window: updaterWindowName, Step: "native-updater-opened", OK: s.waitVisible(updaterWindowName, true)})
	case "close-updater":
		w, ok := h.app.Window.GetByName(updaterWindowName)
		if !ok {
			return fmt.Errorf("updater window absent")
		}
		w.Close()
		time.Sleep(500 * time.Millisecond)
		_, still := h.app.Window.GetByName(updaterWindowName)
		return s.write(Step{Window: updaterWindowName, Step: "native-updater-closed", OK: !still})
	case "show-quick-panel":
		h.showQuickPanel()
		return s.write(Step{Window: quickPanelWindowName, Step: "native-quick-panel-visible", OK: s.waitVisible(quickPanelWindowName, true), Detail: map[string]bool{"ready": h.panel.ready.Load()}})
	case "dismiss-quick-panel":
		h.dismissQuickPanel()
		time.Sleep(300 * time.Millisecond)
		w, ok := h.app.Window.GetByName(quickPanelWindowName)
		return s.write(Step{Window: quickPanelWindowName, Step: "native-quick-panel-dismissed", OK: ok && !w.IsVisible()})
	case "tray-check":
		// The tray menu handlers call the same functions; the DOM cannot click a native menu.
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()
		before, err := h.readSyncEnabled(ctx)
		if err != nil {
			return s.write(Step{Window: "tray", Step: "tray-sync-toggle", OK: false, Detail: err.Error()})
		}
		beforeLabel := h.tray.syncLabel()
		err = h.toggleSync()
		after, readErr := h.readSyncEnabled(ctx)
		afterLabel := h.tray.syncLabel()
		ok := err == nil && readErr == nil && after == !before && beforeLabel != afterLabel
		if err == nil {
			err = h.toggleSync() // restore the original setting
		}
		h.tray.setLanguage("zh-CN")
		zh := h.tray.syncLabel()
		h.tray.setLanguage("en")
		return s.write(Step{Window: "tray", Step: "tray-sync-toggle", OK: ok && err == nil, Detail: map[string]any{
			"before": before, "after": after, "labelBefore": beforeLabel, "labelAfter": afterLabel, "zhLabel": zh, "iconCreated": h.tray.tray != nil}})
	case "update-check":
		// Same sequence as the tray's manual check: look up the release, then show the updater window.
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		meta, err := h.checkForUpdate(ctx, nil)
		detail := map[string]any{"meta": meta}
		if err != nil {
			detail["error"] = err.Error()
		}
		if meta != nil {
			h.openUpdater(false)
		}
		return s.write(Step{Window: "update", Step: "update-check", OK: err == nil && meta != nil, Detail: detail})
	case "update-state":
		// Reports whether this process runs from a bundle that already carries the update marker.
		exe, _ := os.Executable()
		bundle, _ := update.BundleOf(exe)
		_, markerErr := os.Stat(filepath.Join(bundle, "Contents", "Resources", "update-marker.txt"))
		return s.write(Step{Window: "update", Step: "update-state", OK: true, Detail: map[string]any{"installed": markerErr == nil, "pid": os.Getpid(), "bundle": bundle}})
	case "scheduler-wait-updater":
		// Nothing asked for a check: the background scheduler alone must surface the update.
		detail := map[string]any{}
		ok := false
		for deadline := time.Now().Add(60 * time.Second); time.Now().Before(deadline) && !ok; time.Sleep(200 * time.Millisecond) {
			if w, found := h.app.Window.GetByName(updaterWindowName); found && w.IsVisible() {
				ok = true
			}
		}
		return s.write(Step{Window: updaterWindowName, Step: "scheduler-updater-opened", OK: ok, Detail: detail})
	case "scheduler-quiet":
		// Several scheduler iterations must pass without re-prompting a version already announced.
		timing := schedulerTimingOverride(defaultSchedulerTiming)
		time.Sleep(4 * timing.success)
		_, reopened := h.app.Window.GetByName(updaterWindowName)
		return s.write(Step{Window: updaterWindowName, Step: "scheduler-no-reprompt", OK: !reopened})
	case "exit":
		// Lightweight exit leaves the daemon for the next launch; a full quit stops it.
		keep := os.Getenv("UC_GUI_GO_EXIT_MODE") != "full"
		go func() { time.Sleep(300 * time.Millisecond); h.quit(keep) }()
		return nil
	}
	return fmt.Errorf("unknown control action %q", action)
}

func e2eServices(h *HostService) []application.Service {
	return []application.Service{application.NewService(&EvidenceService{host: h})}
}

// Phase tells the in-WebView driver which scenario this launch runs.
func (s *EvidenceService) Phase() string { return os.Getenv("UC_GUI_GO_E2E_PHASE") }

// Installed reports whether the running bundle carries the update marker.
func (s *EvidenceService) Installed() bool {
	exe, _ := os.Executable()
	bundle, err := update.BundleOf(exe)
	if err != nil {
		return false
	}
	_, err = os.Stat(filepath.Join(bundle, "Contents", "Resources", "update-marker.txt"))
	return err == nil
}

// waitVisible polls a window's visibility; window show/hide completes asynchronously.
func (s *EvidenceService) waitVisible(name string, want bool) bool {
	for deadline := time.Now().Add(8 * time.Second); time.Now().Before(deadline); time.Sleep(100 * time.Millisecond) {
		if w, ok := s.host.app.Window.GetByName(name); ok && w.IsVisible() == want {
			return true
		}
	}
	return false
}
