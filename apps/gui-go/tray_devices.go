package main

import (
	"context"
	"net/http"
	"sort"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/wailsapp/wails/v3/pkg/application"
)

const devicesChangedEvent = "devices://sync-changed"

// deviceSyncLabels is the submenu title, the empty placeholder and the unavailable placeholder.
var deviceSyncLabels = map[string][3]string{
	"zh-CN": {"设备同步", "暂无已配对设备", "暂时无法读取设备"},
	"zh-TW": {"裝置同步", "尚無已配對裝置", "暫時無法讀取裝置"},
	"ja-JP": {"デバイスの同期", "ペアリング済みデバイスなし", "デバイスを読み込めません"},
	"ru-RU": {"Синхронизация устройств", "Нет сопряжённых устройств", "Устройства недоступны"},
	"pt-BR": {"Sincronização de dispositivos", "Nenhum dispositivo pareado", "Dispositivos indisponíveis"},
	"en":    {"Device Sync", "No paired devices", "Devices unavailable"},
}

type deviceRow struct {
	ID, Name string
	Enabled  *bool // nil: the preference could not be read
}

// deviceMenu is the tray's per-device sync submenu: one check item per paired device, refreshed every
// 10 seconds and whenever any window reports a device sync change. It mirrors
// crates/uc-tauri/src/tray/device_sync.rs.
type deviceMenu struct {
	h       *HostService
	root    *application.Menu
	sub     *application.Menu
	subItem *application.MenuItem // the submenu's entry in the root menu, whose label is what the user reads

	mu          sync.Mutex
	language    string
	rows        []deviceRow
	items       map[string]*application.MenuItem
	placeholder *application.MenuItem
	unavailable bool
	pending     map[string]bool

	refresh chan struct{}

	// publish makes a structural change visible to the platform tray; it is set once the tray exists.
	publish func()
}

func newDeviceMenu(h *HostService, root *application.Menu, language string) *deviceMenu {
	labels := deviceSyncLabels[language]
	d := &deviceMenu{h: h, root: root, language: language, items: map[string]*application.MenuItem{},
		pending: map[string]bool{}, unavailable: true, refresh: make(chan struct{}, 1)}
	d.sub = root.AddSubmenu(labels[0])
	d.subItem = root.FindByLabel(labels[0])
	d.placeholder = d.sub.Add(labels[2]).SetEnabled(false)
	return d
}

func (d *deviceMenu) requestRefresh() {
	select {
	case d.refresh <- struct{}{}:
	default: // one is already queued
	}
}

func (d *deviceMenu) setLanguage(language string) {
	d.mu.Lock()
	defer d.mu.Unlock()
	d.language = language
	labels := deviceSyncLabels[language]
	d.sub.SetLabel(labels[0])
	if d.subItem != nil {
		d.subItem.SetLabel(labels[0])
	}
	if d.placeholder != nil {
		d.placeholder.SetLabel(d.placeholderText())
	}
	d.publishMenu()
}

// publishMenu runs with d.mu held, so no goroutine of this file edits the menu while the platform reads it (on
// Linux that read happens on the main thread). Nothing on the main thread takes d.mu: Wails runs menu callbacks
// on their own goroutines, so waiting for the main thread here cannot deadlock. The lock order is d.mu, then the
// tray's mu (inside publish); trayMenu.setLanguage releases its mu before it calls in here.
func (d *deviceMenu) publishMenu() {
	if d.publish != nil {
		d.publish()
	}
}

// entries lists the submenu's visible items in display order; the caller holds d.mu.
func (d *deviceMenu) entries() []menuEntry {
	var view []menuEntry
	if d.placeholder != nil {
		view = append(view, entryOf("", d.placeholder))
	}
	for _, row := range d.rows {
		if item := d.items[row.ID]; item != nil {
			view = append(view, entryOf(row.ID, item))
		}
	}
	return view
}

func (d *deviceMenu) placeholderText() string {
	labels := deviceSyncLabels[d.language]
	if d.unavailable {
		return labels[2]
	}
	return labels[1]
}

// run refreshes the menu until ctx ends.
func (d *deviceMenu) run(ctx context.Context) {
	ticker := time.NewTicker(10 * time.Second)
	defer ticker.Stop()
	e2eTrayRefresh("initial")
	d.render(d.loadRows(ctx))
	for {
		cause := "timer"
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		case <-d.refresh:
			cause = "event"
		}
		e2eTrayRefresh(cause)
		d.render(d.loadRows(ctx))
	}
}

func (d *deviceMenu) loadRows(ctx context.Context) []deviceRow {
	cctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	var devices []struct {
		PeerID     string `json:"peerId"`
		DeviceName string `json:"deviceName"`
	}
	if err := d.h.client.Get(cctx, "/paired-devices", &devices); err != nil {
		return nil
	}
	rows := make([]deviceRow, 0, len(devices))
	for _, device := range devices {
		row := deviceRow{ID: device.PeerID, Name: device.DeviceName}
		if prefs, err := d.h.memberSyncPreferences(cctx, device.PeerID); err == nil {
			enabled := prefs.SendEnabled || prefs.ReceiveEnabled
			row.Enabled = &enabled
		}
		rows = append(rows, row)
	}
	sort.Slice(rows, func(i, j int) bool {
		if rows[i].Name != rows[j].Name {
			return rows[i].Name < rows[j].Name
		}
		return rows[i].ID < rows[j].ID
	})
	return rows
}

type memberSyncPrefs struct {
	SendEnabled    bool `json:"sendEnabled"`
	ReceiveEnabled bool `json:"receiveEnabled"`
}

func memberSyncPath(id string) (string, error) {
	seg, err := daemonclient.PathSegment(id)
	if err != nil {
		return "", err
	}
	return "/member/" + seg + "/sync-preferences", nil
}

func (h *HostService) memberSyncPreferences(ctx context.Context, id string) (memberSyncPrefs, error) {
	var prefs memberSyncPrefs
	path, err := memberSyncPath(id)
	if err != nil {
		return prefs, err
	}
	return prefs, h.client.Get(ctx, path, &prefs)
}

// render shows rows (nil means the daemon could not be read). The submenu is rebuilt only when the set of
// devices changed; otherwise items are updated in place, and the platform menu is republished only if a visible
// item changed, so an open menu neither flickers nor loses an expanded submenu.
func (d *deviceMenu) render(rows []deviceRow, completed ...string) {
	d.mu.Lock()
	defer d.mu.Unlock()
	for _, id := range completed {
		delete(d.pending, id)
	}
	d.unavailable = rows == nil
	same := len(rows) == len(d.rows) && (len(rows) > 0 || d.placeholder != nil)
	for i := 0; same && i < len(rows); i++ {
		same = rows[i].ID == d.rows[i].ID
	}
	if !same {
		d.sub.Clear()
		d.items = map[string]*application.MenuItem{}
		d.placeholder = nil
		if len(rows) == 0 {
			d.placeholder = d.sub.Add(d.placeholderText()).SetEnabled(false)
		}
		for _, row := range rows {
			row := row
			item := d.sub.AddCheckbox(row.Name, row.Enabled != nil && *row.Enabled)
			item.SetEnabled(row.Enabled != nil && !d.pending[row.ID])
			item.OnClick(func(*application.Context) { d.click(row.ID) })
			d.items[row.ID] = item
		}
	} else {
		if len(rows) == 0 {
			d.placeholder.SetLabel(d.placeholderText())
		}
		for _, row := range rows {
			item := d.items[row.ID]
			item.SetLabel(row.Name)
			item.SetChecked(row.Enabled != nil && *row.Enabled)
			item.SetEnabled(row.Enabled != nil && !d.pending[row.ID])
		}
	}
	d.rows = rows
	d.publishMenu()
}

// click handles a device item. The platform flips the check mark itself, so it is put back to the stored
// state until the daemon confirms the change; the item stays disabled while the change is in flight.
func (d *deviceMenu) click(id string) {
	d.mu.Lock()
	var row *deviceRow
	for i := range d.rows {
		if d.rows[i].ID == id {
			row = &d.rows[i]
		}
	}
	item := d.items[id]
	if row == nil || item == nil || row.Enabled == nil {
		d.mu.Unlock()
		return
	}
	enabled := *row.Enabled
	item.SetChecked(enabled)
	if d.pending[id] {
		d.mu.Unlock()
		return
	}
	item.SetEnabled(false)
	d.pending[id] = true
	d.mu.Unlock()
	go d.save(id, !enabled)
}

func (d *deviceMenu) save(id string, enabled bool) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	err := d.saveDeviceSync(ctx, id, enabled)
	e2eTrayRefresh("save")
	if err == nil {
		d.h.emit(devicesChangedEvent, id)
	} else {
		d.h.showSyncError()
	}
	d.render(d.loadRows(ctx), id)
}

func (d *deviceMenu) saveDeviceSync(ctx context.Context, id string, enabled bool) error {
	path, err := memberSyncPath(id)
	if err != nil {
		return err
	}
	var result struct {
		Success bool `json:"success"`
	}
	patch := map[string]any{"sendEnabled": enabled, "receiveEnabled": enabled}
	if err := d.h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPatch, Path: path, JSON: patch}, &result); err != nil {
		return err
	}
	if !result.Success {
		return errSyncRejected
	}
	return nil
}

// idByName and itemState serve the e2e controls; they read the menu exactly as the user sees it.
func (d *deviceMenu) idByName(name string) (string, bool) {
	d.mu.Lock()
	defer d.mu.Unlock()
	for _, row := range d.rows {
		if row.Name == name {
			return row.ID, true
		}
	}
	return "", false
}

func (d *deviceMenu) itemState(name string) map[string]any {
	d.mu.Lock()
	defer d.mu.Unlock()
	for _, row := range d.rows {
		if row.Name == name {
			item := d.items[row.ID]
			return map[string]any{"label": item.Label(), "checked": item.Checked(), "enabled": item.Enabled()}
		}
	}
	return nil
}
