package desktopui

import (
	"maps"
	"slices"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui/preview"
)

// NewPreview renders caller-supplied data without starting a service or loading fixtures.
func NewPreview(data preview.Desktop) *Desktop {
	d := &viewState{
		page: data.Page, theme: data.Theme, layout: data.Layout, selected: data.Selected,
		nav: desktopkit.NavState{Value: "all"}, settings: desktopkit.NavState{Value: "general"},
		deviceList: desktopkit.SourceState{Value: "local"}, gridSize: .4,
		deviceName: data.DeviceName, options: maps.Clone(data.Options), choices: maps.Clone(data.Choices),
		inviteCode: data.InviteCode, inviteMode: data.InviteMode,
		reference: data.Reference, previewImage: data.Image,
		quick: newPreviewQuick(data.Quick),
	}
	if len(data.Devices) > 1 {
		for _, v := range data.Devices {
			if v.ID != "local" {
				d.deviceList.Value = v.ID
				break
			}
		}
	}
	for _, e := range data.Entries {
		d.entries = append(d.entries, entry{id: e.ID, title: e.Title, text: e.Text, source: e.Source, date: e.Date, size: e.Size, sourceAppName: e.SourceAppName, sourceAppID: e.SourceAppID, sourceAppIcon: e.SourceAppIcon, kind: e.Kind, tags: slices.Clone(e.Tags), pinned: e.Pinned})
	}
	for _, ch := range data.SyncChannels {
		d.syncChannels = append(d.syncChannels, syncChannel{
			id: ch.ID, implementationID: ch.ImplementationID, transportKind: ch.TransportKind,
			state: ch.State, pairing: ch.Pairing, externalChannels: ch.ExternalChannels,
			restart: ch.Restart, policy: ch.Policy,
		})
	}
	for _, ch := range d.syncChannels {
		if ch.pairing && ch.state == "running" {
			d.pairingChannelID = ch.id
			break
		}
	}
	for _, v := range data.Devices {
		item := device{
			id: v.ID, name: v.Name, userName: v.UserName, reportedName: v.ReportedName,
			deviceID: v.DeviceID, icon: v.Icon, online: v.Online, lastSeen: v.LastSeen,
			fingerprint: v.Fingerprint,
		}
		for _, b := range v.Bindings {
			item.bindings = append(item.bindings, deviceBinding{
				id: b.ID, instanceID: b.InstanceID, implementationID: b.ImplementationID,
				state: b.State, syncEnabled: b.SyncEnabled, connection: b.Connection,
				lastActive: b.LastActive, lastError: b.LastError,
			})
		}
		d.devices = append(d.devices, item)
	}
	for _, ch := range data.Channels {
		d.channels = append(d.channels, externalChannel{
			id: ch.ID, name: ch.Name, backendID: ch.BackendID, enabled: ch.Enabled,
			autoApply: ch.AutoApply, remoteRevocation: ch.RemoteRevocation,
			lastSeen: ch.LastSeen, lastError: ch.LastError,
		})
	}
	for _, v := range data.Pending {
		pd := pendingDevice{id: v.ID, name: v.Name, via: v.Via, byCode: v.ByCode, fingerprint: v.Fingerprint, icon: v.Icon}
		for _, pp := range v.Paths {
			pd.paths = append(pd.paths, pendingPath{id: pp.ID, instanceID: pp.InstanceID, state: pp.State})
		}
		d.pending = append(d.pending, pd)
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
