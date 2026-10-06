//go:build e2e

package main

import (
	"path/filepath"

	"context"
	"encoding/json"
	"fmt"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"os"
	"strconv"
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
		return s.write(Step{Window: quickPanelWindowName, Step: "native-quick-panel-visible", OK: s.waitVisible(quickPanelWindowName, true), Detail: map[string]bool{"ready": h.panel.toggle.isReady()}})
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
	case "update-verify":
		// Check and download against the configured feed, then report which key was trusted and what happened.
		// A feed signed by another key must fail the download; an unconfigured key must refuse to check at all.
		ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
		defer cancel()
		detail := map[string]any{"bakedKeyConfigured": updaterPublicKey != ""}
		if pub, err := update.ParsePublicKey(updaterPublicKey); err == nil {
			detail["bakedKeyID"] = fmt.Sprintf("%016X", pub.ID())
		}
		meta, err := h.checkForUpdate(ctx, nil)
		detail["meta"] = meta
		if err != nil {
			detail["checkError"] = err.Error()
		} else if meta != nil {
			if err = h.downloadUpdate(ctx); err != nil {
				detail["downloadError"] = err.Error()
			} else {
				detail["downloaded"] = true
			}
		}
		return s.write(Step{Window: "update", Step: "update-verify", OK: true, Detail: detail})
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
			if w, found := h.app.Window.GetByName(updaterWindowName); found {
				detail["exists"], detail["visible"], detail["minimised"] = true, w.IsVisible(), w.IsMinimised()
				ok = w.IsVisible()
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

// ServiceStartup lets a launch report its own state without any WebView driver: with
// UC_GUI_GO_E2E_OBSERVE=<seconds> the service writes a `startup-state` step after that delay and then quits,
// which is how the startup-mode scenarios observe a hidden window.
func (s *EvidenceService) ServiceStartup(context.Context, application.ServiceOptions) error {
	if path := os.Getenv("UC_GUI_GO_E2E_CONTROL_FILE"); path != "" {
		go s.watchControlFile(path)
	}
	seconds, err := strconv.Atoi(os.Getenv("UC_GUI_GO_E2E_OBSERVE"))
	if err != nil || seconds <= 0 {
		return nil
	}
	go func() {
		time.Sleep(time.Duration(seconds) * time.Second)
		h := s.host
		w, ok := h.app.Window.GetByName("main")
		stored, _ := h.loadStartupSettings()
		detail := map[string]any{"mainExists": ok, "mainVisible": ok && w.IsVisible(), "pid": os.Getpid(), "storedStartup": stored}
		_ = s.write(Step{Window: "main", Step: "startup-state", OK: true, Detail: detail})
		h.quit(os.Getenv("UC_GUI_GO_EXIT_MODE") != "full")
	}()
	return nil
}

// Secret hands the driver the throwaway passphrase of the e2e profile, which the orchestrator chose.
func (s *EvidenceService) Secret() string { return os.Getenv("UC_GUI_GO_E2E_SECRET") }

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

// profileBundleLoginItemAllowed lets the isolated test bundle (a distinct bundle id, run with a throwaway
// HOME) exercise the bundle login item path with a named profile.
func profileBundleLoginItemAllowed() bool {
	return os.Getenv("UC_GUI_GO_ISOLATED") == "1" && os.Getenv("UC_GUI_GO_E2E_DENY_PROFILE_BUNDLE") != "1"
}

// notifierServices: the notification service refuses to start without a bundle identifier. The launch-at-login
// test runs the bare binary (the LaunchAgent strategy only exists for an unbundled executable) and opts out.
func notifierServices(h *HostService) []application.Service {
	if os.Getenv("UC_GUI_GO_E2E_UNBUNDLED") == "1" {
		return nil
	}
	return []application.Service{application.NewService(h.notifier)}
}

// evidenceWriter is shared by the hooks that run outside the Wails service lifecycle.
var evidenceWriter = &EvidenceService{}

// e2eLaunch records that this process passed the single-instance lock and is the first instance: a second
// instance exits inside application.New and never reaches it.
func e2eLaunch(h *HostService) {
	startTimerProbe()
	_ = evidenceWriter.write(Step{Window: "app", Step: "launch", OK: true, Detail: map[string]any{
		"pid": os.Getpid(), "ppid": os.Getppid(), "uniqueID": h.singleInstanceID, "args": os.Args[1:], "bundleID": bundleID}})
}

// e2eSecondInstance records the activation the first instance received and what it did with it.
func e2eSecondInstance(h *HostService, data application.SecondInstanceData, action secondLaunchAction) {
	_, mainExists := h.app.Window.GetByName("main")
	_ = evidenceWriter.write(Step{Window: "app", Step: "second-instance", OK: true, Detail: map[string]any{
		"pid": os.Getpid(), "args": data.Args, "action": string(action), "mainExists": mainExists}})
}

// e2eBootstrapped records the end of the daemon bootstrap and whether it replayed a held second launch.
func e2eBootstrapped(h *HostService, replayed bool) {
	_, mainExists := h.app.Window.GetByName("main")
	_ = evidenceWriter.write(Step{Window: "app", Step: "bootstrapped", OK: true, Detail: map[string]any{
		"pid": os.Getpid(), "replayedHeldShow": replayed, "mainExists": mainExists}})
}
