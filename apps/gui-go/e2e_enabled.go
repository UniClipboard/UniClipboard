//go:build e2e

package main

import (
	"context"
	"encoding/json"
	"fmt"
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
		time.Sleep(time.Second)
		w, ok := h.app.Window.GetByName(updaterWindowName)
		return s.write(Step{Window: updaterWindowName, Step: "native-updater-opened", OK: ok && w.IsVisible()})
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
		time.Sleep(time.Second)
		w, ok := h.app.Window.GetByName(quickPanelWindowName)
		return s.write(Step{Window: quickPanelWindowName, Step: "native-quick-panel-visible", OK: ok && w.IsVisible(), Detail: map[string]bool{"ready": h.panel.ready.Load()}})
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
