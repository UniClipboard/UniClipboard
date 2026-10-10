package main

import "github.com/wailsapp/wails/v3/pkg/application"

// Every event the host sends to, or accepts from, the shared frontend. Each is registered below with its payload
// type, so `wails3 generate bindings` writes the typed listener table (Events.CustomEvents) and Wails refuses an
// emit whose payload does not match ("data types are matched exactly": pass the declared type, not a map).
//
// Window events (`common:WindowDidResize`) belong to the framework and are not listed here. The frontend listens
// through the adapter in frontend/src/host/event.ts; `scripts/check-host-events.mjs` compares the names it
// listens to and emits with this table.
const (
	contentLockChangedEvent   = "content-lock-changed"            // host -> page, no payload: the content lock state changed
	visualEffectsChangedEvent = "visual-effects://changed"        // host -> page, EffectsSnapshot
	notificationActionEvent   = "notification://action"           // host -> page, NotificationAction: a notification was clicked
	updateAvailableEvent      = "update-available"                // host -> page, *UpdateMetadata (nil = up to date)
	updateProgressEvent       = "update-download-progress"        // host -> page, DownloadEvent: a background download
	updateInstallEvent        = "update-install-progress"         // host -> page, DownloadEvent: the install_update call
	settingsSyncChangedEvent  = "settings://sync-changed"         // host -> page, no payload: the tray flipped the sync switch
	uiNavigateEvent           = "ui://navigate"                   // host -> page, string route
	quickPanelPrepareShow     = "quick-panel://prepare-show"      // host -> page, no payload: clear state before showing
	appShuttingDownEvent      = "app://shutting-down"             // host -> page, no payload: close the daemon WebSocket before the daemon stops
	daemonConnectionChanged   = "app://daemon-connection-changed" // host -> page, no payload: the daemon was replaced, reconnect
	settingsChangedEvent      = "settings://changed"              // page -> host, SettingsChanged: settings were saved
	devicesChangedEvent       = "devices://sync-changed"          // both ways, string device id: a device's sync switch changed
)

// SettingsChanged is what the page sends after saving settings; the tray reads the sync switch out of the JSON.
type SettingsChanged struct {
	SettingJSON string `json:"settingJson"`
	Timestamp   int64  `json:"timestamp"`
}

func init() {
	application.RegisterEvent[application.Void](contentLockChangedEvent)
	application.RegisterEvent[EffectsSnapshot](visualEffectsChangedEvent)
	application.RegisterEvent[NotificationAction](notificationActionEvent)
	application.RegisterEvent[*UpdateMetadata](updateAvailableEvent)
	application.RegisterEvent[DownloadEvent](updateProgressEvent)
	application.RegisterEvent[DownloadEvent](updateInstallEvent)
	application.RegisterEvent[application.Void](settingsSyncChangedEvent)
	application.RegisterEvent[string](uiNavigateEvent)
	application.RegisterEvent[application.Void](quickPanelPrepareShow)
	application.RegisterEvent[application.Void](appShuttingDownEvent)
	application.RegisterEvent[application.Void](daemonConnectionChanged)
	application.RegisterEvent[SettingsChanged](settingsChangedEvent)
	application.RegisterEvent[string](devicesChangedEvent)
}
