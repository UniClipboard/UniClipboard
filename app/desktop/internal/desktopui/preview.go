package desktopui

import (
	"maps"
	"slices"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui/preview"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

// NewPreview renders caller-supplied data without starting a service or loading fixtures.
func NewPreview(data preview.Desktop) *Desktop {
	d := &viewState{
		page: data.Page, theme: data.Theme, layout: data.Layout, selected: data.Selected,
		nav: uikit.NavState{Value: "all"}, settings: uikit.NavState{Value: "general"},
		deviceList: uikit.SourceState{Value: "local"}, gridSize: .4,
		deviceName: data.DeviceName, options: maps.Clone(data.Options), choices: maps.Clone(data.Choices),
		inviteCode: data.InviteCode, inviteMode: data.InviteMode,
		reference: data.Reference, previewImage: data.Image,
		quick: newPreviewQuick(data.Quick),
	}
	if d.page == "devices" {
		d.page, d.devicesOpen = "history", true
	}
	for _, e := range data.Entries {
		d.entries = append(d.entries, entry{id: e.ID, title: e.Title, text: e.Text, source: e.Source, date: e.Date, size: e.Size, kind: e.Kind, tags: slices.Clone(e.Tags), pinned: e.Pinned})
	}
	for _, v := range data.Devices {
		item := device{id: v.ID, name: v.Name, platform: v.Platform, icon: v.Icon, online: v.Online, latency: v.Latency, link: v.Link, lastSeen: v.LastSeen, fingerprint: v.Fingerprint, direction: v.Direction, text: v.Text, image: v.Image, file: v.File}
		for _, p := range v.Paths {
			item.paths = append(item.paths, devicePath{name: p.Name, state: p.State, lastSync: p.LastSync, failed: p.Failed})
		}
		d.devices = append(d.devices, item)
	}
	for _, v := range data.Pending {
		d.pending = append(d.pending, pendingDevice{id: v.ID, name: v.Name, platform: v.Platform, via: v.Via, fingerprint: v.Fingerprint, icon: v.Icon})
	}
	for _, v := range data.Candidates {
		d.candidates = append(d.candidates, deviceCandidate{id: v.ID, name: v.Name, introducer: v.Introducer, backendID: v.BackendID, icon: v.Icon})
	}
	for _, tag := range data.Tags {
		d.tags = append(d.tags, historyTag{id: tag.ID, name: tag.Name, color: tag.Color, sidebar: tag.Sidebar})
	}
	d.tags = append(d.tags, builtinTags()...)
	d.tagManager.sort = "mostUsed"
	d.list.Select(d.selected)
	return &Desktop{d: d}
}

// SetQuickPanelOpener lets the preview launcher own its independent window.
func (s *Desktop) SetQuickPanelOpener(open func(theme string)) {
	s.d.openQuickPanel = func() { open(s.d.theme) }
}

// QuickPreview renders supplied quick-panel data without a production controller.
type QuickPreview struct {
	p      *quickPanelState
	window *mygo.Window
}

func NewQuickPreview(data preview.Quick) *QuickPreview {
	return &QuickPreview{p: newPreviewQuick(data)}
}
func newPreviewQuick(data preview.Quick) *quickPanelState {
	p := &quickPanelState{theme: data.Theme, mode: data.Mode, visible: true, hovered: -1, keyboard: true, previewImage: data.Image}
	for _, e := range data.Entries {
		p.entries = append(p.entries, qpEntry{id: e.ID, title: e.Title, text: e.Text, kind: e.Kind, source: e.Source, age: e.Age, tags: slices.Clone(e.Tags), favorite: e.Favorite, lost: e.Lost})
	}
	p.applySearch()
	return p
}
func (p *QuickPreview) Open(closed func()) *mygo.Window {
	p.window = openQuickWindow(p.p, closed)
	return p.window
}
func (p *QuickPreview) Show() {
	p.p.visible, p.p.expanded, p.p.menu = true, false, ""
	p.p.applySearch()
	p.window.Show()
	p.window.Invalidate()
}
