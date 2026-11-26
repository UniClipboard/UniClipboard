//go:build linux

package quickpanel

import (
	"context"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/waylandwire"
)

// waylandCompositorTargetID is an opaque sentinel representing Wayland
// compositor-managed focus return to the previous window (PID remains 0).
const waylandCompositorTargetID uintptr = 0x5741594c

const defaultWaylandFocusSettle = 120 * time.Millisecond

type waylandInputBackend interface {
	Prepare(context.Context) error
	PasteCtrlV() error
	Close()
}

type waylandPlatform struct {
	mu           sync.Mutex
	backend      waylandInputBackend
	focusChecker func() bool
	settleDelay  time.Duration
	activatedAt  time.Time
}

func newWaylandPlatform() Platform {
	if sock := waylandwire.SocketPath(); probeWaylandVirtualKeyboard(sock) {
		return newWaylandPlatformWithBackend(&waylandVirtualKeyboardBackend{socketPath: sock})
	}
	if probeWaylandRemoteDesktopPortal() {
		return newWaylandPlatformWithBackend(&waylandPortalBackend{})
	}
	return unsupportedPlatform{}
}

func newWaylandPlatformWithBackend(backend waylandInputBackend) *waylandPlatform {
	return &waylandPlatform{
		backend:     backend,
		settleDelay: defaultWaylandFocusSettle,
	}
}

func (p *waylandPlatform) SetFocusChecker(fn func() bool) {
	p.mu.Lock()
	p.focusChecker = fn
	p.mu.Unlock()
}

func (p *waylandPlatform) SetProfileDir(dir string) {
	if configurable, ok := p.backend.(interface{ SetProfileDir(string) }); ok {
		configurable.SetProfileDir(dir)
	}
}

func (p *waylandPlatform) Close() {
	if p.backend != nil {
		p.backend.Close()
	}
}

func (p *waylandPlatform) Capture() (Target, error) {
	if p.backend == nil {
		return Target{}, ErrUnsupported
	}
	return Target{ID: waylandCompositorTargetID, Name: "原应用"}, nil
}

func (p *waylandPlatform) Check(t Target) error {
	if p.backend == nil {
		return ErrUnsupported
	}
	if t.ID != waylandCompositorTargetID || t.PID != 0 {
		return ErrTarget
	}
	return nil
}

func (p *waylandPlatform) OwnsForeground() bool {
	p.mu.Lock()
	checker := p.focusChecker
	p.mu.Unlock()
	if checker != nil {
		return checker()
	}
	return true
}

func (p *waylandPlatform) Activate(t Target) error {
	if err := p.Check(t); err != nil {
		return err
	}
	p.mu.Lock()
	p.activatedAt = time.Now()
	p.mu.Unlock()
	return nil
}

func (p *waylandPlatform) Ready(t Target) (bool, error) {
	if err := p.Check(t); err != nil {
		return false, err
	}
	p.mu.Lock()
	checker := p.focusChecker
	activatedAt := p.activatedAt
	settle := p.settleDelay
	p.mu.Unlock()
	if checker != nil && checker() {
		return false, nil
	}
	if !activatedAt.IsZero() && time.Since(activatedAt) < settle {
		return false, nil
	}
	return true, nil
}

func (p *waylandPlatform) PreparePaste(ctx context.Context) error {
	if p.backend == nil {
		return ErrUnsupported
	}
	return p.backend.Prepare(ctx)
}

func (p *waylandPlatform) Paste(t Target) error {
	ready, err := p.Ready(t)
	if err != nil {
		return err
	}
	if !ready {
		return ErrFocus
	}
	return p.backend.PasteCtrlV()
}
