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

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

const (
	updateAvailableEvent = "update-available"
	updateProgressEvent  = "update-download-progress"
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

// stringError is a command error serialised as a plain string, the wire shape
// of the `Result<_, String>` update commands.
type stringError string

func (e stringError) Error() string { return string(e) }

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

func (u *updater) metadata() map[string]any {
	if u.release == nil {
		return nil
	}
	return map[string]any{"version": u.release.Version, "currentVersion": u.release.CurrentVersion, "body": u.release.Body, "date": u.release.Date}
}

// checkForUpdate runs one lookup and broadcasts the outcome to every window.
func (h *HostService) checkForUpdate(ctx context.Context, channel *string) (any, error) {
	u := &h.updates
	client, err := h.updateClient()
	if err != nil {
		return nil, stringError(err.Error())
	}
	u.mu.Lock()
	if u.phase == phaseDownloading {
		u.mu.Unlock()
		return nil, stringError("updater: download in progress, cannot re-check")
	}
	u.mu.Unlock()

	rel, err := client.Check(ctx, h.resolveChannel(ctx, channel))
	if err != nil {
		return nil, stringError(err.Error())
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
	meta := u.metadata()
	if rel == nil {
		meta = nil
	}
	u.mu.Unlock()
	h.emit(updateAvailableEvent, meta)
	return meta, nil
}

func (h *HostService) downloadUpdate(ctx context.Context) error {
	u := &h.updates
	client, err := h.updateClient()
	if err != nil {
		return stringError(err.Error())
	}
	u.mu.Lock()
	switch u.phase {
	case phaseAvailable:
	case phaseReady:
		u.mu.Unlock()
		return nil
	case phaseDownloading:
		u.mu.Unlock()
		return stringError("updater: a download is already running")
	default:
		u.mu.Unlock()
		return stringError("updater: no pending update")
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
			var content any
			if total >= 0 {
				content = total
			}
			h.emit(updateProgressEvent, map[string]any{"event": "Started", "data": map[string]any{"contentLength": content}})
		}
		h.emit(updateProgressEvent, map[string]any{"event": "Progress", "data": map[string]any{"chunkLength": chunk}})
	})
	u.mu.Lock()
	defer u.mu.Unlock()
	u.cancel = nil
	if err != nil {
		u.phase, u.downloaded, u.total = phaseAvailable, 0, nil
		h.emit(updateProgressEvent, map[string]any{"event": "Failed", "data": map[string]any{"error": err.Error()}})
		return stringError(err.Error())
	}
	u.phase, u.bytes = phaseReady, data
	h.emit(updateProgressEvent, map[string]any{"event": "Finished"})
	return nil
}

func (h *HostService) cancelDownload() {
	u := &h.updates
	u.mu.Lock()
	defer u.mu.Unlock()
	if u.phase == phaseDownloading && u.cancel != nil {
		u.cancel()
	}
}

func (h *HostService) downloadProgress() map[string]any {
	u := &h.updates
	u.mu.Lock()
	defer u.mu.Unlock()
	snap := map[string]any{"phase": string(u.phase), "downloaded": u.downloaded, "total": u.total,
		"version": nil, "currentVersion": buildinfo.PackageVersion, "body": nil, "date": nil}
	if u.phase == "" {
		snap["phase"] = string(phaseIdle)
	}
	if u.release != nil {
		snap["version"], snap["currentVersion"], snap["body"], snap["date"] = u.release.Version, u.release.CurrentVersion, u.release.Body, u.release.Date
	}
	if u.phase == phaseReady {
		n := int64(len(u.bytes))
		snap["downloaded"], snap["total"] = n, &n
	}
	return snap
}

// installUpdate stops the daemon, swaps the app bundle in place and starts the
// new copy. The daemon is stopped first so the new app replaces it with its own
// bundled version.
func (h *HostService) installUpdate(ctx context.Context, send func(any)) error {
	u := &h.updates
	u.mu.Lock()
	if u.phase != phaseReady && u.phase != phaseAvailable {
		phase := u.phase
		u.mu.Unlock()
		if phase == phaseDownloading {
			return stringError("updater: download in progress; wait or cancel first")
		}
		return stringError("updater: no pending update")
	}
	u.mu.Unlock()
	if err := h.downloadUpdate(ctx); err != nil {
		send(map[string]any{"event": "Failed", "data": map[string]any{"error": err.Error()}})
		return err
	}
	u.mu.Lock()
	data := u.bytes
	u.mu.Unlock()
	send(map[string]any{"event": "Started", "data": map[string]any{"contentLength": len(data)}})
	send(map[string]any{"event": "Progress", "data": map[string]any{"chunkLength": len(data)}})

	exe, err := os.Executable()
	if err == nil {
		var bundle string
		if bundle, err = update.BundleOf(exe); err == nil {
			stopDaemon()
			err = update.Install(data, bundle)
		}
	}
	if err != nil {
		send(map[string]any{"event": "Failed", "data": map[string]any{"error": err.Error()}})
		return stringError(err.Error())
	}
	send(map[string]any{"event": "Finished"})
	if err := h.restartGUI(); err != nil {
		return stringError(err.Error())
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
		return stringError(err.Error())
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
		return stringError(err.Error())
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return stringError(err.Error())
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, raw, 0o600); err != nil {
		return stringError(err.Error())
	}
	return os.Rename(tmp, path)
}

func (h *HostService) setAutoDownload(ctx context.Context, enabled bool) error {
	patch := map[string]any{"general": map[string]any{"autoDownloadUpdate": enabled}}
	if err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, nil); err != nil {
		return stringError("failed to save settings: " + err.Error())
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
		return false, stringError("failed to load settings: " + err.Error())
	}
	return settings.General.AutoDownloadUpdate, nil
}

// checkUpdateFromTray is the tray's manual check: open the updater window when a
// release exists, otherwise tell the user they are current.
func (h *HostService) checkUpdateFromTray() {
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	meta, err := h.checkForUpdate(ctx, nil)
	switch {
	case err != nil:
		h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error()).Show()
	case meta != nil:
		h.openUpdater(false)
	default:
		h.app.Dialog.Info().SetTitle("UniClipboard").SetMessage(fmt.Sprintf("UniClipboard %s is up to date.", buildinfo.PackageVersion)).Show()
	}
}

func init() {
	register(map[string]commandFunc{
		"check_for_update": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var channel *string
			if err := args.decode("channel", &channel); err != nil {
				return nil, err
			}
			return h.checkForUpdate(ctx, channel)
		},
		"download_update": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			return nil, h.downloadUpdate(ctx)
		},
		"cancel_download": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			h.cancelDownload()
			return nil, nil
		},
		"get_download_progress": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			return h.downloadProgress(), nil
		},
		"install_update": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			send, err := args.channel("onEvent")
			if err != nil {
				return nil, err
			}
			return nil, h.installUpdate(ctx, func(message any) { send(h, message) })
		},
		"skip_version": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var version string
			if err := args.decode("version", &version); err != nil {
				return nil, err
			}
			return nil, h.skipVersion(ctx, version)
		},
		"get_auto_download_update": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			return h.autoDownload(ctx)
		},
		"set_auto_download_update": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var enabled bool
			if err := args.decode("enabled", &enabled); err != nil {
				return nil, err
			}
			return nil, h.setAutoDownload(ctx, enabled)
		},
	})
}
