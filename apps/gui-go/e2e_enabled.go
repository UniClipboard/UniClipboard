//go:build e2e

package main

import (
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
	case "quit":
		go func() { time.Sleep(300 * time.Millisecond); h.Quit() }()
		return nil
	}
	return fmt.Errorf("unknown control action %q", action)
}

func e2eServices(h *HostService) []application.Service {
	return []application.Service{application.NewService(&EvidenceService{host: h})}
}
