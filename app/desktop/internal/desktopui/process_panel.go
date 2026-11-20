package desktopui

import (
	"context"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/quickpanel"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// PanelClipboard delegates application to the single adapter owner.
type PanelClipboard interface {
	Apply(context.Context, *api.Client, string, bool) (string, error)
	Matches(string) bool
}

// ProcessPanel belongs exclusively to the quick-panel process. Its persistent
// native platform never registers shortcuts and never acquires an adapter.
type ProcessPanel struct {
	ctx       context.Context
	client    *api.Client
	clipboard PanelClipboard
	native    *capturedPlatform
	root      func(string)
	state     *quickPanelState
	window    *mygo.Window
}
type capturedPlatform struct {
	quickpanel.Platform
	target       quickpanel.Target
	captureError error
}

func (p *capturedPlatform) Capture() (quickpanel.Target, error) { return p.target, p.captureError }
func NewProcessPanel(ctx context.Context, client *api.Client, clipboard PanelClipboard, root func(string)) *ProcessPanel {
	return &ProcessPanel{ctx: ctx, client: client, clipboard: clipboard, native: &capturedPlatform{Platform: quickpanel.NewNative()}, root: root}
}
func (p *ProcessPanel) Visible() bool {
	return p.state != nil && p.state.visible && p.window != nil && p.window.IsVisible()
}
func (p *ProcessPanel) Open(target quickpanel.Target, captureError string) {
	if p.Visible() {
		p.Hide()
		return
	}
	p.native.target = target
	p.native.captureError = nil
	if captureError != "" {
		p.native.captureError = quickpanel.FailureFromCode(captureError)
	}
	if p.state == nil {
		q := newLiveQuick(p.ctx, p.client, "system", p.root)
		p.state = q
		show := func(visible bool) {
			q.visible = visible
			q.blurDue = time.Time{}
			if visible && p.window != nil {
				p.window.Show()
				p.window.Focus()
			}
			q.update()
		}
		q.live.system = quickpanel.New(quickpanel.Options{Context: p.ctx, Client: p.client, Clipboard: p.clipboard, Platform: p.native, Dispatch: mygo.RunOnMain, Visible: show, Changed: func() { q.message = q.live.system.Message; q.update() }})
		q.live.system.Open()
		p.window = openQuickWindow(q, func() { q.live.system.Dismiss(false); q.live.d.live.cancel(); p.state = nil; p.window = nil })
		// A windowless panel process must explicitly display its first window.
		// Do not depend on a later native event turn to settle its initial show.
		p.window.Show()
		p.window.Focus()
		p.window.Invalidate()
	} else {
		q := p.state
		q.live.system.Open()
		q.visible = true
		q.menu = ""
		q.blurDue = time.Time{}
		q.applySearch()
		p.window.Show()
		p.window.Focus()
		p.window.Invalidate()
	}
}
func (p *ProcessPanel) Hide() {
	if p.state != nil {
		p.state.live.system.Dismiss(true)
	}
}
func (p *ProcessPanel) Close() {
	if p.state != nil {
		p.state.live.system.Dismiss(false)
		p.state.live.d.live.cancel()
	}
	if p.window != nil {
		p.window.Destroy()
		p.window = nil
	}
	if closer, ok := p.native.Platform.(interface{ Close() }); ok {
		closer.Close()
	}
}

// SetExternalShortcut exposes the tray's registration receipt without owning a
// second global registration in the GUI process.
func (s *Desktop) SetExternalShortcut(status func() (string, string), retry func()) {
	s.d.quickShortcutStatus = status
	s.d.retryQuickShortcut = retry
}

// OpenFromPanel preserves the panel's main-window and settings destinations.
func (s *Desktop) OpenFromPanel(action string) {
	switch action {
	case "settings":
		s.d.page = "settings"
		s.d.settings.Value = "quickPanel"
	case "main":
		s.d.page = "history"
	}
}
