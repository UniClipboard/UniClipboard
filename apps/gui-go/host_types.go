package main

import "encoding/json"

// Data transfer objects of the host commands and events. Wails generates the TypeScript types
// (apps/gui-go/frontend/bindings/.../models.ts) from these declarations, so each exported field, JSON tag and
// named string type here is part of the frontend contract.
//
// Wails turns a named string type with constants into a TypeScript `enum` whose members carry the constant names,
// so the constant names below are what the frontend refers to.

// ---- connection and identity

// DaemonConnection is where the frontend reaches the daemon.
type DaemonConnection struct {
	BaseURL string `json:"baseUrl"`
	WSURL   string `json:"wsUrl"`
}

// DaemonSession is a short-lived daemon session credential for the webview.
type DaemonSession struct {
	SessionToken  string `json:"sessionToken"`
	ExpiresInSecs int64  `json:"expiresInSecs"`
	RefreshAtSecs int64  `json:"refreshAtSecs"`
}

// DaemonBootstrapFailureKind classifies why the host could not attach to a daemon.
type DaemonBootstrapFailureKind string

const (
	// BootstrapFailureVersionTooOld means a newer daemon than this GUI is running: the fix is to update the app.
	BootstrapFailureVersionTooOld DaemonBootstrapFailureKind = "versionTooOld"
	// BootstrapFailureUnavailable is any other terminal bootstrap failure.
	BootstrapFailureUnavailable DaemonBootstrapFailureKind = "unavailable"
)

// DaemonBootstrapFailure is the machine-readable reason the daemon bootstrap failed.
type DaemonBootstrapFailure struct {
	Kind            DaemonBootstrapFailureKind `json:"kind"`
	Detail          string                     `json:"detail"`
	ObservedVersion *string                    `json:"observedVersion"`
	ExpectedVersion *string                    `json:"expectedVersion"`
}

// DeviceMeta is the host's device and application metadata, used to complete the frontend's Sentry scope.
type DeviceMeta struct {
	DeviceID        string `json:"deviceId"`
	DeviceRole      string `json:"deviceRole"`
	Platform        string `json:"platform"`
	AppVersion      string `json:"appVersion"`
	AppChannel      string `json:"appChannel"`
	RuntimeProfile  string `json:"runtimeProfile"`
	DevelopmentMode bool   `json:"developmentMode"`
}

// ---- appearance

// DesktopTheme is a desktop-provided colour theme.
type DesktopTheme struct {
	Dark      bool              `json:"dark"`
	Variables map[string]string `json:"variables"`
}

// DesktopThemeSnapshot is the desktop theme state; only Linux hosts with an Omarchy theme source report it available.
type DesktopThemeSnapshot struct {
	Revision           int           `json:"revision"`
	FollowOmarchyTheme bool          `json:"followOmarchyTheme"`
	OmarchyAvailable   bool          `json:"omarchyAvailable"`
	Theme              *DesktopTheme `json:"theme"`
	WindowCornerRadius *float64      `json:"windowCornerRadius"`
}

// EffectsMode is the user's visual effects preference.
type EffectsMode string

const (
	EffectsModeAuto    EffectsMode = "auto"
	EffectsModeEffects EffectsMode = "effects"
	EffectsModeSmooth  EffectsMode = "smooth"
)

// AutoResult is what the automatic mode resolved to.
type AutoResult string

const (
	AutoResultEffects AutoResult = "effects"
	AutoResultSmooth  AutoResult = "smooth"
)

// SystemMotion is the operating system's reduce-motion preference as the page reports it.
type SystemMotion string

const (
	SystemMotionReduce  SystemMotion = "reduce"
	SystemMotionAllow   SystemMotion = "allow"
	SystemMotionUnknown SystemMotion = "unknown"
)

// EffectsReason says why the effective mode is what it is.
type EffectsReason string

const (
	EffectsReasonManual          EffectsReason = "manual"
	EffectsReasonSystem          EffectsReason = "system"
	EffectsReasonPlatformDefault EffectsReason = "platform_default"
	EffectsReasonHardware        EffectsReason = "hardware"
	EffectsReasonUnknown         EffectsReason = "unknown"
	EffectsReasonRuntime         EffectsReason = "runtime"
)

// EffectsPersistence says whether the preference survives a restart.
type EffectsPersistence string

const (
	EffectsPersistenceSaved       EffectsPersistence = "saved"
	EffectsPersistenceSessionOnly EffectsPersistence = "session_only"
)

// EffectsSnapshot is the visual effects state of the running session.
type EffectsSnapshot struct {
	SessionID      string             `json:"sessionId"`
	Revision       int                `json:"revision"`
	Mode           EffectsMode        `json:"mode"`
	AutoForSession AutoResult         `json:"autoForSession"`
	NextAuto       *AutoResult        `json:"nextAuto"`
	SystemMotion   SystemMotion       `json:"systemMotion"`
	ReduceMotion   bool               `json:"reduceMotion"`
	LowEffects     bool               `json:"lowEffects"`
	Reason         EffectsReason      `json:"reason"`
	Persistence    EffectsPersistence `json:"persistence"`
}

// EffectsSample is one frame-timing sample reported by the page.
type EffectsSample struct {
	SessionID  string   `json:"sessionId"`
	SampleID   int      `json:"sampleId"`
	Revision   int      `json:"revision"`
	Frames     int      `json:"frames"`
	DurationMs *float64 `json:"durationMs"`
	LongFrames int      `json:"longFrames"`
	LongestMs  *float64 `json:"longestMs"`
}

// SamplePermit allows the page to measure one sample.
type SamplePermit struct {
	SampleID  int    `json:"sampleId"`
	Revision  int    `json:"revision"`
	SessionID string `json:"sessionId"`
}

// ---- files and configuration packages

// ExportConfigResult is the outcome of a configuration package export.
type ExportConfigResult struct {
	// Path is the absolute path the bundle was written to.
	Path string `json:"path"`
}

// ConfigImportPreview is the descriptive, non-secret metadata of a configuration bundle.
type ConfigImportPreview struct {
	AppVersion        string `json:"appVersion"`
	SourceMode        string `json:"sourceMode"`
	CreatedAtUnixMs   int64  `json:"createdAtUnixMs"`
	ProfileID         string `json:"profileId"`
	DeviceFingerprint string `json:"deviceFingerprint"`
}

// ImportConfigStageResult is the outcome of staging a configuration bundle for the next start.
type ImportConfigStageResult struct {
	// StagedOK is true on success: the bundle was validated and staged.
	StagedOK bool `json:"stagedOk"`
	// UnlockRequiredAfterApply is true when applying the staged migration requires the passphrase again.
	UnlockRequiredAfterApply bool `json:"unlockRequiredAfterApply"`
}

// ---- quick panel and shortcuts

// QuickPanelPosition is where the quick panel appears.
type QuickPanelPosition string

const (
	QuickPanelPositionCenter       QuickPanelPosition = "center"
	QuickPanelPositionFollowCursor QuickPanelPosition = "follow_cursor"
)

// QuickPanelDoubleTapModifier is the modifier whose double tap opens the quick panel.
type QuickPanelDoubleTapModifier string

const (
	DoubleTapModifierDisabled QuickPanelDoubleTapModifier = "disabled"
	DoubleTapModifierAlt      QuickPanelDoubleTapModifier = "alt"
	DoubleTapModifierControl  QuickPanelDoubleTapModifier = "control"
	DoubleTapModifierMeta     QuickPanelDoubleTapModifier = "meta"
)

// ModifierDoubleTapAvailability says whether the modifier double tap trigger can work in this session.
type ModifierDoubleTapAvailability string

const (
	DoubleTapSupported                     ModifierDoubleTapAvailability = "supported"
	DoubleTapAccessibilityPermissionNeeded ModifierDoubleTapAvailability = "accessibility_permission_required"
	DoubleTapUnsupportedDisplaySession     ModifierDoubleTapAvailability = "unsupported_display_session"
)

// QuickPanelExpandSide is the side the inline preview opens toward.
type QuickPanelExpandSide string

const (
	ExpandSideRight QuickPanelExpandSide = "right"
	ExpandSideLeft  QuickPanelExpandSide = "left"
)

// FilePathInputRequest holds file paths to type into the application that was focused before the panel opened.
type FilePathInputRequest struct {
	FilePaths []string `json:"filePaths"`
}

// UpdateKeyboardShortcutsResult is the merged shortcut table after an update. A value is one accelerator string or a
// list of alternatives; Wails cannot express that union, so the value is opaque JSON here and the frontend narrows
// it with the daemon's `ShortcutKeyDto`.
type UpdateKeyboardShortcutsResult struct {
	KeyboardShortcuts map[string]json.RawMessage `json:"keyboardShortcuts"`
}

// ---- updater

// UpdateMetadata describes an available update.
type UpdateMetadata struct {
	Version        string  `json:"version"`
	CurrentVersion string  `json:"currentVersion"`
	Body           *string `json:"body"`
	Date           *string `json:"date"`
}

// DownloadPhase is the coarse state of the pending update.
type DownloadPhase string

const (
	DownloadPhaseIdle        DownloadPhase = "idle"
	DownloadPhaseAvailable   DownloadPhase = "available"
	DownloadPhaseDownloading DownloadPhase = "downloading"
	DownloadPhaseReady       DownloadPhase = "ready"
)

// DownloadProgressSnapshot is the queryable download state, so a window that mounts mid-download can catch up.
type DownloadProgressSnapshot struct {
	Phase          DownloadPhase `json:"phase"`
	Downloaded     int64         `json:"downloaded"`
	Total          *int64        `json:"total"`
	Version        *string       `json:"version"`
	CurrentVersion string        `json:"currentVersion"`
	Body           *string       `json:"body"`
	Date           *string       `json:"date"`
}

// DownloadEventKind tells the stage of an update download.
type DownloadEventKind string

const (
	DownloadEventStarted  DownloadEventKind = "Started"
	DownloadEventProgress DownloadEventKind = "Progress"
	DownloadEventFinished DownloadEventKind = "Finished"
	DownloadEventFailed   DownloadEventKind = "Failed"
)

// DownloadEvent is one step of an update download or install. Wails has no tagged unions, so the payload of each
// kind is a flat optional field: Started carries ContentLength (absent when the size is unknown), Progress carries
// ChunkLength, Failed carries Error, Finished carries none.
type DownloadEvent struct {
	Event DownloadEventKind  `json:"event"`
	Data  *DownloadEventData `json:"data,omitempty"`
}

// DownloadEventData holds the fields of a DownloadEvent; which one is set depends on the kind.
type DownloadEventData struct {
	ContentLength *int64  `json:"contentLength,omitempty"`
	ChunkLength   *int64  `json:"chunkLength,omitempty"`
	Error         *string `json:"error,omitempty"`
}

// InstallKind is how the running copy was installed.
type InstallKind string

const (
	InstallKindMacOS           InstallKind = "macos"
	InstallKindWindows         InstallKind = "windows"
	InstallKindWindowsPortable InstallKind = "windowsportable"
	InstallKindAppImage        InstallKind = "appimage"
	InstallKindDeb             InstallKind = "deb"
	InstallKindRPM             InstallKind = "rpm"
	InstallKindUnknown         InstallKind = "unknown"
)

// ---- notifications (adapter commands)

// NotificationPermission is the notification permission state the adapter reports.
type NotificationPermission string

const (
	NotificationGranted NotificationPermission = "granted"
	NotificationDenied  NotificationPermission = "denied"
)

// HostNotification is a notification to show; a stable ID replaces an earlier notification of the same kind.
type HostNotification struct {
	ID    *int   `json:"id"`
	Title string `json:"title"`
	Body  string `json:"body"`
}

// NotificationAction is delivered when the user clicks a notification, in the shape the plugin's handler receives.
type NotificationAction struct {
	ID *int `json:"id,omitempty"`
}
