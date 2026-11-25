package quickpanel

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

type preparerPlatform struct {
	mu         sync.Mutex
	prepareErr error
	prepares   int
	pastes     int
	focused    bool
}

func (p *preparerPlatform) Capture() (Target, error) {
	return Target{ID: 0x5741594c, Name: "原应用"}, nil
}
func (p *preparerPlatform) Check(t Target) error {
	if t.ID != 0x5741594c {
		return ErrTarget
	}
	return nil
}
func (p *preparerPlatform) OwnsForeground() bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.focused
}
func (p *preparerPlatform) Activate(Target) error {
	p.mu.Lock()
	p.focused = false
	p.mu.Unlock()
	return nil
}
func (p *preparerPlatform) Ready(Target) (bool, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	return !p.focused, nil
}
func (p *preparerPlatform) PreparePaste(context.Context) error {
	p.mu.Lock()
	p.prepares++
	err := p.prepareErr
	p.mu.Unlock()
	return err
}
func (p *preparerPlatform) Paste(Target) error {
	p.mu.Lock()
	p.pastes++
	p.mu.Unlock()
	return nil
}

type spyClipboard struct {
	mu      sync.Mutex
	applies int
	mark    string
}

func (s *spyClipboard) Apply(context.Context, *api.Client, string, bool) (string, error) {
	s.mu.Lock()
	s.applies++
	m := s.mark
	s.mu.Unlock()
	return m, nil
}
func (s *spyClipboard) Matches(mark string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.mark == mark
}

func TestControllerPastePreparerConsentAndAbort(t *testing.T) {
	var mu sync.Mutex
	dispatch := func(fn func()) {
		mu.Lock()
		defer mu.Unlock()
		fn()
	}

	// 1. Declined portal consent aborts BEFORE mutating clipboard or hiding panel.
	plat := &preparerPlatform{focused: true, prepareErr: ErrPermission}
	cb := &spyClipboard{mark: "m1"}
	visible := true
	ctrl := New(Options{
		Context:   context.Background(),
		Clipboard: cb,
		Platform:  plat,
		Dispatch:  dispatch,
		Visible:   func(v bool) { visible = v },
	})
	mu.Lock()
	ctrl.Open()
	if !ctrl.CanPaste {
		mu.Unlock()
		t.Fatal("expected CanPaste=true for Wayland-capable platform")
	}
	if err := ctrl.Start("item-1", false, true, false); err != nil {
		mu.Unlock()
		t.Fatalf("Start: %v", err)
	}
	mu.Unlock()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		mu.Lock()
		busy := ctrl.Busy
		mu.Unlock()
		if !busy {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	mu.Lock()
	if ctrl.Sent || !visible {
		mu.Unlock()
		t.Fatalf("declined consent must not hide panel or send paste: sent=%v visible=%v", ctrl.Sent, visible)
	}
	mu.Unlock()
	if cb.applies != 0 || plat.pastes != 0 || plat.prepares != 1 {
		t.Fatalf("expected 1 prepare, 0 clipboard applies, 0 pastes; got %d / %d / %d", plat.prepares, cb.applies, plat.pastes)
	}

	// 2. Approved portal consent applies clipboard, hides panel, waits for focus leave, and pastes once.
	plat.prepareErr = nil
	plat.focused = true
	mu.Lock()
	if err := ctrl.Start("item-1", false, true, false); err != nil {
		mu.Unlock()
		t.Fatalf("Start approved: %v", err)
	}
	mu.Unlock()

	deadline = time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		mu.Lock()
		busy, sent := ctrl.Busy, ctrl.Sent
		mu.Unlock()
		if !busy && sent {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	mu.Lock()
	sent := ctrl.Sent
	mu.Unlock()
	if !sent || visible {
		t.Fatalf("expected paste sent and panel hidden: sent=%v visible=%v", sent, visible)
	}
	if cb.applies != 1 || plat.pastes != 1 {
		t.Fatalf("expected 1 clipboard apply and 1 paste; got %d / %d", cb.applies, plat.pastes)
	}

	// 3. Copy-only (paste=false) skips PreparePaste completely.
	plat.prepareErr = errors.New("should not be called on copy")
	mu.Lock()
	if err := ctrl.Start("item-1", false, false, false); err != nil {
		mu.Unlock()
		t.Fatalf("Start copy: %v", err)
	}
	mu.Unlock()
	deadline = time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		mu.Lock()
		busy := ctrl.Busy
		mu.Unlock()
		if !busy {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if plat.prepares != 2 || cb.applies != 2 || plat.pastes != 1 {
		t.Fatalf("Copy unexpectedly called PreparePaste or Paste: prepares=%d applies=%d pastes=%d", plat.prepares, cb.applies, plat.pastes)
	}
}
