package main

import (
	"context"
	"net/http"
	"slices"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// trayMenu owns the system tray icon and its localized menu. It mirrors the
// Tauri tray: sync toggle, open, settings, restart, lightweight mode and quit.
type trayMenu struct {
	// languageMu serializes whole setLanguage calls so the root labels and the device submenu always end in the same language.
	// Order: languageMu, then deviceMenu.mu, then mu.
	languageMu  sync.Mutex
	mu          sync.Mutex
	language    string
	syncEnabled bool
	syncBusy    bool
	published   []menuEntry // what the platform menu last showed, so an unchanged menu is not republished

	tray                                                    *application.SystemTray
	menu                                                    *application.Menu
	devices                                                 *deviceMenu
	icon                                                    *trayIcon
	sync, open, settings, checkUpdate, restart, lightweight *application.MenuItem
	quit                                                    *application.MenuItem
}

func (h *HostService) initTray() {
	t := &trayMenu{language: "en"}
	h.tray = t
	menu := h.app.NewMenu()
	labels := trayLabelTable[t.language]
	t.sync = menu.Add(labels.syncOn).OnClick(func(*application.Context) { go h.toggleSyncFromTray() })
	t.devices = newDeviceMenu(h, menu, t.language)
	menu.AddSeparator()
	t.open = menu.Add(labels.open).OnClick(func(*application.Context) { h.showMainWindow() })
	t.settings = menu.Add(labels.settings).OnClick(func(*application.Context) { h.showSettings() })
	t.checkUpdate = menu.Add(labels.checkUpdate).OnClick(func(*application.Context) { go h.checkUpdateFromTray() })
	menu.AddSeparator()
	t.restart = menu.Add(labels.restart).OnClick(func(*application.Context) { go h.fullRestart() })
	t.lightweight = menu.Add(labels.lightweight).OnClick(func(*application.Context) { go h.enterLightweightMode() })
	t.quit = menu.Add(labels.quit).OnClick(func(*application.Context) { h.quit(false) })
	t.menu = menu

	t.tray = h.app.SystemTray.New()
	t.icon = newTrayIcon(t.tray)
	t.icon.show()
	watchSystemTheme(h.app, func() { go t.icon.show() }) // an event handler may run on the main thread, which the icon call waits for
	t.tray.SetTooltip("UniClipboard")
	t.tray.SetMenu(menu)
	// Set before the refresh goroutine and the event handlers below exist, so every render sees it. t.mu keeps the
	// root items (labels, sync state) still while the platform reads the menu.
	t.devices.mu.Lock() // publishMenu reads the field under this lock
	t.devices.publish = func() {
		t.mu.Lock()
		defer t.mu.Unlock()
		// Menu.Update and SystemTray.SetMenu rebuild the whole native menu, which closes an expanded submenu, so a refresh that
		// changed nothing must not publish. The comparison is against what the last publish showed, read from the items now.
		view := t.view()
		if slices.Equal(view, t.published) {
			e2eTrayPublishSkipped()
			return
		}
		defer e2eTrayPublish()() // e2e builds record when each publish started and ended; a no-op otherwise
		republishTrayMenu(t.tray, menu)
		t.published = view
	}
	t.devices.mu.Unlock()
	t.tray.OnClick(h.showMainWindow)

	// Keep the toggle label in step with settings changed from any window.
	h.app.Event.On(settingsChangedEvent, func(e *application.CustomEvent) { h.refreshTraySync(e.Data) })
	h.app.Event.On(devicesChangedEvent, func(*application.CustomEvent) { t.devices.requestRefresh() })
	go h.syncTrayFromDaemon()
	trayCtx, stopTray := context.WithCancel(context.Background())
	h.stopTray = stopTray
	go t.devices.run(trayCtx)
}

// menuEntry is the part of a menu item the user can see.
type menuEntry struct {
	id, label        string
	checked, enabled bool
}

func entryOf(id string, item *application.MenuItem) menuEntry {
	return menuEntry{id: id, label: item.Label(), checked: item.Checked(), enabled: item.Enabled()}
}

// view lists every visible item of the root and device menus; it runs with t.mu and the device menu's mu held.
func (t *trayMenu) view() []menuEntry {
	view := make([]menuEntry, 0, 16)
	for _, item := range []*application.MenuItem{t.sync, t.devices.subItem, t.open, t.settings, t.checkUpdate, t.restart, t.lightweight, t.quit} {
		if item != nil {
			view = append(view, entryOf("", item))
		}
	}
	return append(view, t.devices.entries()...)
}

func (h *HostService) showMainWindow() {
	h.mainMu.Lock()
	defer h.mainMu.Unlock() // two callers must not both create the window
	w, ok := h.app.Window.GetByName("main")
	if !ok {
		h.openMainWindow()
		return
	}
	w.UnMinimise()
	w.Show()
	focusWindow(w)
}

func (h *HostService) showSettings() {
	h.navMu.Lock()
	h.pendingNavigation = "/settings"
	h.navMu.Unlock()
	h.showMainWindow()
	h.emit(uiNavigateEvent, "/settings")
}

func (t *trayMenu) applyLabels() {
	labels := trayLabelTable[t.language]
	t.open.SetLabel(labels.open)
	t.settings.SetLabel(labels.settings)
	t.checkUpdate.SetLabel(labels.checkUpdate)
	t.restart.SetLabel(labels.restart)
	t.lightweight.SetLabel(labels.lightweight)
	t.quit.SetLabel(labels.quit)
	t.sync.SetLabel(t.syncLabel())
}

func (t *trayMenu) syncLabel() string {
	labels := trayLabelTable[t.language]
	if t.syncEnabled {
		return labels.syncOn
	}
	return labels.syncOff
}

func (t *trayMenu) setLanguage(tag string) {
	e2eTrayLanguage(tag)
	t.languageMu.Lock()
	defer t.languageMu.Unlock()
	t.mu.Lock()
	t.language = normalizeTrayLanguage(tag)
	t.applyLabels()
	language := t.language
	t.mu.Unlock() // the device menu takes its own lock and then t.mu again to publish
	e2eTrayLanguageGap()
	t.devices.setLanguage(language)
}

func (t *trayMenu) setSyncEnabled(enabled bool) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.syncEnabled = enabled
	t.sync.SetLabel(t.syncLabel())
}

// reserveSync serializes tray sync toggles; it reports false when one is running.
func (t *trayMenu) reserveSync(busy bool) bool {
	t.mu.Lock()
	defer t.mu.Unlock()
	if busy && t.syncBusy {
		return false
	}
	t.syncBusy = busy
	t.sync.SetEnabled(!busy)
	return true
}

func (h *HostService) refreshTraySync(data any) {
	payload, ok := data.(map[string]any)
	if !ok {
		return
	}
	raw, ok := payload["settingJson"].(string)
	if !ok {
		return
	}
	if enabled, ok := parseSyncEnabled([]byte(raw)); ok {
		h.tray.setSyncEnabled(enabled)
	}
}

func (h *HostService) syncTrayFromDaemon() {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if enabled, err := h.readSyncEnabled(ctx); err == nil {
		h.tray.setSyncEnabled(enabled)
	}
}

func (h *HostService) toggleSyncFromTray() {
	if !h.tray.reserveSync(true) {
		return
	}
	defer h.tray.reserveSync(false)
	if err := h.toggleSync(); err != nil {
		h.showSyncError()
	}
}

// showSyncError tells the user a tray sync change did not go through.
func (h *HostService) showSyncError() {
	h.tray.mu.Lock()
	message := trayLabelTable[h.tray.language].syncError
	h.tray.mu.Unlock()
	h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(message).Show()
}

// toggleSync flips the persisted sync switch through the daemon and notifies windows.
func (h *HostService) toggleSync() error {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	current, err := h.readSyncEnabled(ctx)
	if err != nil {
		return err
	}
	var result struct {
		Success bool `json:"success"`
	}
	patch := map[string]any{"sync": map[string]any{"syncEnabled": !current}}
	if err := h.daemon().Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, &result); err != nil {
		return err
	}
	if !result.Success {
		return errSyncRejected
	}
	h.tray.setSyncEnabled(!current)
	h.emit(settingsSyncChangedEvent, nil)
	return nil
}
