package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// updaterPublicKey is the base64 minisign public key of the release signer. It
// is injected at build time from the Tauri updater configuration so both shells
// trust one key; an empty key disables updates instead of trusting anything.
var updaterPublicKey string

type updatePhase string

const (
	phaseIdle        updatePhase = "idle"
	phaseAvailable   updatePhase = "available"
	phaseDownloading updatePhase = "downloading"
	phaseReady       updatePhase = "ready"
)

// updater is the single owner of the pending-update state machine
// (idle → available → downloading → ready).
type updater struct {
	mu         sync.Mutex
	phase      updatePhase
	release    *update.Release
	bytes      []byte
	downloaded int64
	total      *int64
	cancel     context.CancelFunc
	skipMu     sync.Mutex
}

func (h *HostService) updateClient() (*update.Client, error) {
	endpoints, key := update.DefaultEndpoints, updaterPublicKey
	if dev, ok := devUpdateOverrides(); ok {
		endpoints, key = dev.endpoints, dev.publicKey
	}
	if key == "" {
		return nil, errors.New("updates are disabled: no updater public key in this build")
	}
	pub, err := update.ParsePublicKey(key)
	if err != nil {
		return nil, err
	}
	return &update.Client{
		HTTP: update.NewHTTPClient(), Endpoints: endpoints, PubKey: pub,
		Current: buildinfo.PackageVersion, Targets: update.DefaultTargets(installerKey()),
	}, nil
}

func installerKey() string {
	switch hostPlatform() {
	case "macos":
		return "app"
	case "windows":
		return "nsis"
	}
	return ""
}

// resolveChannel prefers an explicit channel, then the user's setting, then the running version.
func (h *HostService) resolveChannel(ctx context.Context, explicit *string) update.Channel {
	if explicit != nil {
		return update.ParseChannel(*explicit)
	}
	var settings struct {
		General struct {
			UpdateChannel *string `json:"updateChannel"`
		} `json:"general"`
	}
	if err := h.client.Get(ctx, "/settings", &settings); err == nil && settings.General.UpdateChannel != nil {
		return update.ParseChannel(*settings.General.UpdateChannel)
	}
	return update.DetectChannel(buildinfo.PackageVersion)
}

// reset replaces the pending-update state; callers hold u.mu.
func (u *updater) reset(phase updatePhase, rel *update.Release) {
	u.phase, u.release, u.bytes, u.downloaded, u.total, u.cancel = phase, rel, nil, 0, nil, nil
}

func (u *updater) metadata() *UpdateMetadata {
	if u.release == nil {
		return nil
	}
	return &UpdateMetadata{Version: u.release.Version, CurrentVersion: u.release.CurrentVersion, Body: u.release.Body, Date: u.release.Date}
}

// lookupUpdate runs one lookup and broadcasts the outcome to every window. Every source (manual, tray,
// scheduled) goes through it, so any finished check, successful or not, refreshes lastCheck like the Tauri
// shell does. Callers report the analytics event themselves because the order against the notification and
// the auto-download differs per source.
func (h *HostService) lookupUpdate(ctx context.Context, channel *string) (*UpdateMetadata, error) {
	defer h.lastCheck.recordNow()
	u := &h.updates
	client, err := h.updateClient()
	if err != nil {
		return nil, err
	}
	u.mu.Lock()
	if u.phase == phaseDownloading {
		u.mu.Unlock()
		return nil, errors.New("updater: download in progress, cannot re-check")
	}
	u.mu.Unlock()

	rel, err := client.Check(ctx, h.resolveChannel(ctx, channel))
	if err != nil {
		return nil, err
	}
	u.mu.Lock()
	switch {
	case u.phase == phaseDownloading:
		// A download started meanwhile keeps its state.
	case rel == nil:
		u.reset(phaseIdle, nil)
	case u.phase == phaseReady && u.release != nil && u.release.Version == rel.Version:
		u.release = rel // same version: keep the verified bytes
	default:
		u.reset(phaseAvailable, rel)
	}
	// A nil pointer when there is no release: callers (and the tray, scheduler and analytics) test it for nil,
	// and the page receives null on `update-available`.
	var meta *UpdateMetadata
	if rel != nil {
		meta = u.metadata()
	}
	u.mu.Unlock()
	h.emit(updateAvailableEvent, meta)
	return meta, nil
}

// checkForUpdate is the `CheckForUpdate` command: a manual check, reported as such.
func (h *HostService) checkForUpdate(ctx context.Context, channel *string) (*UpdateMetadata, error) {
	meta, err := h.lookupUpdate(ctx, channel)
	h.reportCheck(checkSourceManual, meta != nil, err)
	if err != nil {
		return nil, hostapi.TextError(err.Error())
	}
	return meta, nil
}

// errAlreadyDownloaded is the Tauri shell's "already downloaded" precondition: the release is verified and
// waiting, nothing starts and nothing is reported.
var errAlreadyDownloaded = preconditionError{hostapi.TextError("updater: already downloaded")}

// downloadUpdate downloads the pending release. A refusal before anything started is a preconditionError and
// a cancel is a cancelledError (both plain-string on the wire), so callers can report the outcome.
func (h *HostService) downloadUpdate(ctx context.Context) error {
	u := &h.updates
	client, err := h.updateClient()
	if err != nil {
		return preconditionError{hostapi.TextError(err.Error())}
	}
	u.mu.Lock()
	switch u.phase {
	case phaseAvailable:
	case phaseReady:
		u.mu.Unlock()
		return errAlreadyDownloaded
	case phaseDownloading:
		u.mu.Unlock()
		return preconditionError{hostapi.TextError("updater: a download is already running")}
	default:
		u.mu.Unlock()
		return preconditionError{hostapi.TextError("updater: no pending update")}
	}
	rel := u.release
	dctx, cancel := context.WithCancel(context.Background())
	u.phase, u.cancel, u.downloaded, u.total = phaseDownloading, cancel, 0, nil
	u.mu.Unlock()
	defer cancel()

	started := false
	data, err := client.Download(dctx, rel, func(chunk int, total int64) {
		u.mu.Lock()
		if total >= 0 {
			u.total = &total
		}
		u.downloaded += int64(chunk)
		u.mu.Unlock()
		if !started {
			started = true
			started := DownloadEvent{Event: DownloadEventStarted, Data: &DownloadEventData{}}
			if total >= 0 {
				started.Data.ContentLength = &total
			}
			h.emit(updateProgressEvent, started)
		}
		chunkLength := int64(chunk)
		h.emit(updateProgressEvent, DownloadEvent{Event: DownloadEventProgress, Data: &DownloadEventData{ChunkLength: &chunkLength}})
	})
	u.mu.Lock()
	defer u.mu.Unlock()
	u.cancel = nil
	if err != nil {
		u.phase, u.downloaded, u.total = phaseAvailable, 0, nil
		h.emit(updateProgressEvent, failedEvent(err))
		if errors.Is(err, context.Canceled) {
			return cancelledError{hostapi.TextError(err.Error())}
		}
		return hostapi.TextError(err.Error())
	}
	u.phase, u.bytes = phaseReady, data
	h.emit(updateProgressEvent, DownloadEvent{Event: DownloadEventFinished})
	return nil
}

// downloadUpdateReported is a user- or scheduler-started background download: the same download plus the
// download_bg analytics pair. The install path downloads through downloadUpdate and reports nothing here.
func (h *HostService) downloadUpdateReported(ctx context.Context) error {
	err := h.downloadUpdate(ctx)
	h.reportDownload(err)
	return err
}

func (h *HostService) cancelDownload() {
	u := &h.updates
	u.mu.Lock()
	defer u.mu.Unlock()
	if u.phase == phaseDownloading && u.cancel != nil {
		u.cancel()
	}
}

func (h *HostService) downloadProgress() DownloadProgressSnapshot {
	u := &h.updates
	u.mu.Lock()
	defer u.mu.Unlock()
	snap := DownloadProgressSnapshot{Phase: DownloadPhase(u.phase), Downloaded: u.downloaded, Total: u.total, CurrentVersion: buildinfo.PackageVersion}
	if u.phase == "" {
		snap.Phase = DownloadPhaseIdle
	}
	if u.release != nil {
		snap.Version, snap.CurrentVersion, snap.Body, snap.Date = &u.release.Version, u.release.CurrentVersion, u.release.Body, u.release.Date
	}
	if u.phase == phaseReady {
		n := int64(len(u.bytes))
		snap.Downloaded, snap.Total = n, &n
	}
	return snap
}

func failedEvent(err error) DownloadEvent {
	message := err.Error()
	return DownloadEvent{Event: DownloadEventFailed, Data: &DownloadEventData{Error: &message}}
}

// installUpdate installs the downloaded release in place (per platform: swap the app bundle, or run the NSIS
// installer) and then relaunches or quits as that platform's installer contract requires. Progress goes to send.
func (h *HostService) installUpdate(ctx context.Context, send func(DownloadEvent)) error {
	u := &h.updates
	u.mu.Lock()
	if u.phase != phaseReady && u.phase != phaseAvailable {
		phase := u.phase
		u.mu.Unlock()
		if phase == phaseDownloading {
			return hostapi.TextError("updater: download in progress; wait or cancel first")
		}
		return hostapi.TextError("updater: no pending update")
	}
	u.mu.Unlock()
	if err := h.downloadUpdate(ctx); err != nil && err != errAlreadyDownloaded {
		send(failedEvent(err))
		return hostapi.TextError(err.Error())
	}
	u.mu.Lock()
	data := u.bytes
	u.mu.Unlock()
	size := int64(len(data))
	send(DownloadEvent{Event: DownloadEventStarted, Data: &DownloadEventData{ContentLength: &size}})
	send(DownloadEvent{Event: DownloadEventProgress, Data: &DownloadEventData{ChunkLength: &size}})

	u.mu.Lock()
	version := ""
	if u.release != nil {
		version = u.release.Version
	}
	u.mu.Unlock()
	if err := h.installPayload(data, version); err != nil {
		send(failedEvent(err))
		return hostapi.TextError(err.Error())
	}
	send(DownloadEvent{Event: DownloadEventFinished})
	if err := h.relaunchAfterInstall(); err != nil {
		return hostapi.TextError(err.Error())
	}
	return nil
}

// skippedVersions maps channel → version the user chose to skip, stored beside the
// other profile data in the same file the Tauri shell uses.
func skippedVersionPath() (string, error) {
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return "", errors.New("data root unavailable")
	}
	return filepath.Join(root, "skipped_version.json"), nil
}

func (h *HostService) skipVersion(ctx context.Context, version string) error {
	path, err := skippedVersionPath()
	if err != nil {
		return hostapi.TextError(err.Error())
	}
	u := &h.updates
	u.skipMu.Lock()
	defer u.skipMu.Unlock()
	entries := map[string]string{}
	if raw, err := os.ReadFile(path); err == nil {
		_ = json.Unmarshal(raw, &entries) // a corrupt file is treated as empty, like the Tauri shell
	}
	entries[string(h.resolveChannel(ctx, nil))] = version
	raw, err := json.Marshal(entries)
	if err != nil {
		return hostapi.TextError(err.Error())
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return hostapi.TextError(err.Error())
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, raw, 0o600); err != nil {
		return hostapi.TextError(err.Error())
	}
	return asTextError(os.Rename(tmp, path))
}

func (h *HostService) setAutoDownload(ctx context.Context, enabled bool) error {
	patch := map[string]any{"general": map[string]any{"autoDownloadUpdate": enabled}}
	if err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, nil); err != nil {
		return hostapi.TextError("failed to save settings: " + err.Error())
	}
	return nil
}

func (h *HostService) autoDownload(ctx context.Context) (bool, error) {
	var settings struct {
		General struct {
			AutoDownloadUpdate bool `json:"autoDownloadUpdate"`
		} `json:"general"`
	}
	if err := h.client.Get(ctx, "/settings", &settings); err != nil {
		return false, hostapi.TextError("failed to load settings: " + err.Error())
	}
	return settings.General.AutoDownloadUpdate, nil
}

// checkUpdateFromTray is the tray's manual check: open the updater window when a release exists, otherwise
// tell the user they are current. Like the Tauri shell the announcement goes through the per-version dedup
// (without the scheduler's cooldown) and is reported; because the user asked, a release that was already
// announced or skipped still opens the window, silently.
func (h *HostService) checkUpdateFromTray() {
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	channel := h.resolveChannel(ctx, nil)
	meta, err := h.lookupUpdate(ctx, nil)
	if meta != nil && meta.Version != "" && !h.notifyIfNew(channel, meta.Version, false) {
		h.openUpdater(false)
	}
	h.reportCheck(checkSourceManual, meta != nil, err)
	switch {
	case err != nil:
		h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error()).Show()
	case meta == nil:
		h.app.Dialog.Info().SetTitle("UniClipboard").SetMessage(fmt.Sprintf("UniClipboard %s is up to date.", buildinfo.PackageVersion)).Show()
	}
}

// CheckForUpdate looks for a newer release on the given channel (null = the user's channel) and tells every window
// through `update-available`. It resolves nil when the app is up to date.
//
//uc:errors text
//uc:os all=real
func (h *HostService) CheckForUpdate(ctx context.Context, channel *string) (*UpdateMetadata, error) {
	ctx, cancel := commandContext(ctx, "check_for_update")
	defer cancel()
	return h.checkForUpdate(ctx, channel)
}

// DownloadUpdate downloads the pending release in the background; progress is broadcast on
// `update-download-progress`. It resolves when the download finished and rejects on failure or cancellation.
//
//uc:errors text
//uc:os all=real
func (h *HostService) DownloadUpdate(ctx context.Context) error {
	ctx, cancel := commandContext(ctx, "download_update")
	defer cancel()
	if err := h.downloadUpdateReported(ctx); err != nil {
		return hostapi.TextError(err.Error())
	}
	return nil
}

// CancelDownload cancels a running download. It does nothing when none is active.
//
//uc:errors none
//uc:os all=real
func (h *HostService) CancelDownload() {
	h.cancelDownload()
}

// GetDownloadProgress returns the pending update state, so a window that mounts mid-download can catch up before it
// listens to the broadcast events.
//
//uc:errors none
//uc:os all=real
func (h *HostService) GetDownloadProgress() DownloadProgressSnapshot {
	return h.downloadProgress()
}

// InstallUpdate downloads (if needed) and installs the pending release, then relaunches or quits as the platform's
// installer requires. Its progress is sent on `update-install-progress`, which only this call produces.
//
//uc:errors text
//uc:os all=real
func (h *HostService) InstallUpdate(ctx context.Context) error {
	ctx, cancel := commandContext(ctx, "install_update")
	defer cancel()
	return h.installUpdate(ctx, func(event DownloadEvent) { h.emit(updateInstallEvent, event) })
}

// SkipVersion remembers that the user does not want the given version on the current channel.
//
//uc:errors text
//uc:os all=real
func (h *HostService) SkipVersion(ctx context.Context, version string) error {
	ctx, cancel := commandContext(ctx, "skip_version")
	defer cancel()
	return h.skipVersion(ctx, version)
}

// GetAutoDownloadUpdate reads the "download updates automatically" setting.
//
//uc:errors text
//uc:os all=real
func (h *HostService) GetAutoDownloadUpdate(ctx context.Context) (bool, error) {
	ctx, cancel := commandContext(ctx, "get_auto_download_update")
	defer cancel()
	return h.autoDownload(ctx)
}

// SetAutoDownloadUpdate saves the "download updates automatically" setting.
//
//uc:errors text
//uc:os all=real
func (h *HostService) SetAutoDownloadUpdate(ctx context.Context, enabled bool) error {
	ctx, cancel := commandContext(ctx, "set_auto_download_update")
	defer cancel()
	return h.setAutoDownload(ctx, enabled)
}
