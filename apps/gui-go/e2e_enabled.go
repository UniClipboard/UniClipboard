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

type EvidenceService struct {
	host *HostService
	mu   sync.Mutex
}
type Evidence struct {
	Window  string `json:"window"`
	HTTP    bool   `json:"http"`
	WS      bool   `json:"ws"`
	Session bool   `json:"session"`
	Origin  string `json:"origin"`
	PID     uint32 `json:"pid"`
	Refresh bool   `json:"refresh"`
}

func (s *EvidenceService) Record(e Evidence) error {
	if e.Window != "main" && e.Window != "secondary" {
		return fmt.Errorf("invalid evidence window")
	}
	if !e.HTTP || !e.WS || !e.Session || !e.Refresh {
		return fmt.Errorf("incomplete native evidence")
	}
	actual, err := s.host.Connection()
	if err != nil {
		return err
	}
	if actual.PID != e.PID {
		return fmt.Errorf("evidence PID mismatch")
	}
	if _, ok := s.host.app.Window.GetByName(e.Window); !ok {
		return fmt.Errorf("evidence window absent")
	}
	if e.Window == "secondary" {
		go func() { time.Sleep(25 * time.Second); s.host.app.Quit() }()
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	data, err := json.Marshal(e)
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
func e2eServices(h *HostService) []application.Service {
	return []application.Service{application.NewService(&EvidenceService{host: h})}
}
