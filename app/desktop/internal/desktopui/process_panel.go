package desktopui

import (
	"context"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/quickpanel"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
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
	settings  func() config.Config
	root      func(string)
	state     *quickPanelState
	window    *mygo.Window
	resume    panelResume
}

type panelResume struct {
	query, kind          string
	pasteFailure         string
	favoriteOnly         bool
	chips                []qpFilter
	windowZoom, textZoom int
}
type capturedPlatform struct {
	quickpanel.Platform
	target       quickpanel.Target
	captureError error
}

func (p *capturedPlatform) Capture() (quickpanel.Target, error) { return p.target, p.captureError }
func (p *capturedPlatform) ActivateBeforeHide() bool {
	handoff, ok := p.Platform.(quickpanel.ForegroundHandoff)
	return ok && handoff.ActivateBeforeHide()
}
func (p *capturedPlatform) PreparePaste(ctx context.Context) error {
	if prep, ok := p.Platform.(quickpanel.PastePreparer); ok {
		return prep.PreparePaste(ctx)
	}
	return nil
}
func NewProcessPanel(ctx context.Context, client *api.Client, clipboard PanelClipboard, settings func() config.Config, root func(string)) *ProcessPanel {
	p := &ProcessPanel{ctx: ctx, client: client, clipboard: clipboard, native: &capturedPlatform{Platform: quickpanel.NewNative()}, settings: settings, root: root}
	if fc, ok := p.native.Platform.(interface{ SetFocusChecker(func() bool) }); ok {
		fc.SetFocusChecker(func() bool {
			return p.window != nil && p.window.IsVisible() && p.window.IsFocused()
		})
	}
	return p
}
func (p *ProcessPanel) SetProfileDir(dir string) {
	if cfg, ok := p.native.Platform.(interface{ SetProfileDir(string) }); ok {
		cfg.SetProfileDir(dir)
	}
}
func (p *ProcessPanel) Visible() bool {
	return p.state != nil && p.state.visible && p.window != nil && p.window.IsVisible()
}
func (p *ProcessPanel) Focused() bool {
	return p.Visible() && p.window.IsFocused()
}
func (p *ProcessPanel) Focus() {
	if p.Visible() && p.window != nil {
		p.window.Show()
		p.window.Focus()
		p.window.Invalidate()
	}
}
func (p *ProcessPanel) newOpenState(favorites bool) *quickPanelState {
	q := newLiveQuick(p.ctx, p.client, "system", p.root)
	q.live.d.live.config.Config = p.settings()
	q.live.d.language = q.live.d.live.config.Config.Desktop.Language
	q.live.d.devices = []device{{id: "local"}}
	q.query, q.kind, q.chips = p.resume.query, p.resume.kind, p.resume.chips
	q.favoriteOnly = p.resume.favoriteOnly
	q.windowZoom, q.textZoom = p.resume.windowZoom, p.resume.textZoom
	q.pasteFailure = p.resume.pasteFailure
	p.resume = panelResume{}
	if favorites {
		q.setFavoriteFilter(true)
	}
	q.normalizeFilters()
	return q
}
func (p *ProcessPanel) Open(target quickpanel.Target, captureError string, favorites bool) {
	if p.Visible() {
		if favorites {
			p.FilterFavorites()
		}
		p.Focus()
		return
	}
	if p.state != nil {
		p.release(p.state, true)
	}
	p.native.target = target
	p.native.captureError = nil
	if captureError != "" {
		p.native.captureError = quickpanel.FailureFromCode(captureError)
	}
	q := p.newOpenState(favorites)
	p.state = q
	show := func(visible bool) {
		if p.state != q {
			return
		}
		q.visible = visible
		q.blurDue = time.Time{}
		if visible && p.window != nil {
			p.window.Show()
			p.window.Focus()
		}
		q.update()
	}
	var releaseQueued bool
	changed := func() {
		if p.state != q {
			return
		}
		q.update()
		if q.visible || q.live.system.Busy || releaseQueued {
			return
		}
		// Dismissal can run inside a frame. Destroy the windows after this
		// UI callback and after any synchronous keep-open decision.
		releaseQueued = true
		go mygo.RunOnMain(func() {
			releaseQueued = false
			if p.state == q && !q.visible && !q.live.system.Busy {
				p.release(q, true)
			}
		})
	}
	q.live.system = quickpanel.New(quickpanel.Options{Context: q.live.d.live.ctx, Client: p.client, Clipboard: p.clipboard, Platform: p.native, Dispatch: mygo.RunOnMain, Visible: show, Changed: changed})
	q.live.system.Open()
	// The first-page read runs while the native window is being created.
	q.live.d.live.loadHistory(q.live.d, false)
	p.window = openQuickWindow(q, func() { p.release(q, false) })
	p.window.Show()
	p.window.Focus()
	p.window.Invalidate()
}
func (p *ProcessPanel) FilterFavorites() {
	if p.Visible() {
		p.state.setFavoriteFilter(true)
		p.state.applySearch()
		p.state.update()
	}
}
func (p *ProcessPanel) Hide() {
	if p.state != nil {
		p.state.live.system.Dismiss(true)
	}
}
func (p *ProcessPanel) Close() {
	if p.state != nil {
		p.release(p.state, true)
	}
	if closer, ok := p.clipboard.(interface{ Close() }); ok {
		closer.Close()
	}
	if closer, ok := p.native.Platform.(interface{ Close() }); ok {
		closer.Close()
	}
}

func (p *ProcessPanel) release(q *quickPanelState, destroy bool) {
	if p.state != q {
		return
	}
	p.resume = panelResume{query: q.query, kind: q.kind, favoriteOnly: q.favoriteOnly, chips: slices.Clone(q.chips), windowZoom: q.windowZoom, textZoom: q.textZoom}
	if !q.visible && q.live != nil && q.live.system != nil {
		p.resume.pasteFailure = q.live.system.FailureMessage()
	}
	w := p.window
	p.state, p.window = nil, nil
	if q.live != nil {
		if q.live.system != nil {
			q.live.system.Dismiss(false)
		}
		q.live.d.live.cancel()
	}
	p.native.target = quickpanel.Target{}
	p.native.captureError = nil
	if destroy && w != nil {
		w.Destroy()
	}
	// Closed window callbacks can outlive the native window. Drop the
	// cancelled binding and projected rows once the view is gone.
	q.live, q.entries, q.rows = nil, nil, nil
}

// SetExternalShortcut exposes the tray's registration receipt without owning a
// second global registration in the GUI process.
func (s *Desktop) SetExternalShortcut(status func() (string, string), retry func()) {
	s.d.quickShortcutStatus = status
	s.d.retryQuickShortcut = retry
}

// OpenFromPanel navigates the main window to a route requested by the quick
// panel, tray menu, or a clicked system notification.
func (s *Desktop) OpenFromPanel(action string) {
	switch {
	case action == "settings":
		s.d.page = "settings"
		s.d.settings.Value = "quickPanel"
	case action == "settings:notifications":
		s.d.page = "settings"
		s.d.settings.Value = "notifications"
	case action == "devices":
		s.d.openPendingDevice("")
	case strings.HasPrefix(action, "devices:"):
		s.d.openPendingDevice(strings.TrimPrefix(action, "devices:"))
	case action == "main":
		s.d.page = "history"
	}
}
