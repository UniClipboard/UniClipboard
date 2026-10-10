package main

import (
	"context"
	"encoding/json"
	"net/http"
	"os"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// Daemon connection, identity and content lock commands.

// GetDaemonConnectionInfo tells the page where the daemon listens. It is nil until the daemon bootstrap finished.
//
//uc:errors none
//uc:os all=real
func (h *HostService) GetDaemonConnectionInfo() *DaemonConnection {
	if h.client == nil {
		return nil
	}
	return &DaemonConnection{BaseURL: h.client.BaseURL, WSURL: h.client.WSURL}
}

// GetDaemonSession exchanges a short-lived daemon session for the page. It is nil until the daemon bootstrap
// finished.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) GetDaemonSession(ctx context.Context) (*DaemonSession, error) {
	if h.client == nil {
		return nil, nil
	}
	ctx, cancel := commandContext(ctx, "get_daemon_session")
	defer cancel()
	session, err := h.client.ExchangeSession(ctx, "gui")
	if err != nil {
		return nil, hostapi.Internal(err)
	}
	return &DaemonSession{SessionToken: session.SessionToken, ExpiresInSecs: session.ExpiresInSecs, RefreshAtSecs: session.RefreshAtSecs}, nil
}

// GetDaemonBootstrapFailure reports why the daemon bootstrap failed. It is always nil in this host: a bootstrap
// failure ends the process with a native dialog (see HostService.fatal), so no page is left to ask.
//
//uc:errors none
//uc:os all=noop
func (h *HostService) GetDaemonBootstrapFailure() *DaemonBootstrapFailure {
	return nil
}

// GetDaemonStartupStatus returns the daemon's startup progress (`GET /startup`) as the daemon sent it: nil while
// the daemon is not reachable yet. The payload is owned by the daemon contract (crates/uc-daemon-contract startup.rs),
// which the OpenAPI document does not export, so the host passes it through untouched and the page declares its shape.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) GetDaemonStartupStatus(ctx context.Context) (json.RawMessage, error) {
	ctx, cancel := commandContext(ctx, "get_daemon_startup_status")
	defer cancel()
	raw, err := h.client.StartupStatus(ctx)
	if err != nil {
		return nil, hostapi.Internal(err)
	}
	return raw, nil
}

// GetDeviceID returns this device's peer id.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) GetDeviceID(ctx context.Context) (string, error) {
	ctx, cancel := commandContext(ctx, "get_device_id")
	defer cancel()
	return h.deviceID(ctx)
}

// GetDeviceMeta returns the host's device and application metadata for the page's Sentry scope.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) GetDeviceMeta(ctx context.Context) (DeviceMeta, error) {
	ctx, cancel := commandContext(ctx, "get_device_meta")
	defer cancel()
	id, err := h.deviceID(ctx)
	if err != nil {
		return DeviceMeta{}, err
	}
	return DeviceMeta{
		DeviceID:        id,
		DeviceRole:      "gui-host",
		Platform:        hostPlatform(),
		AppVersion:      buildinfo.PackageVersion,
		AppChannel:      "dev",
		RuntimeProfile:  os.Getenv("UC_PROFILE"),
		DevelopmentMode: os.Getenv("UNICLIPBOARD_ENV") == "development",
	}, nil
}

func (h *HostService) deviceID(ctx context.Context) (string, error) {
	var me struct {
		PeerID string `json:"peerId"`
	}
	if err := h.client.Get(ctx, "/device/me", &me); err != nil {
		return "", hostapi.Internal(err)
	}
	return me.PeerID, nil
}

// GetContentUnlocked reports whether the daemon lets this GUI show content right now.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) GetContentUnlocked(ctx context.Context) (bool, error) {
	ctx, cancel := commandContext(ctx, "get_content_unlocked")
	defer cancel()
	var status struct {
		Unlocked bool `json:"unlocked"`
	}
	if err := h.client.Get(ctx, "/content-lock", &status); err != nil {
		return false, hostapi.Internal(err)
	}
	return status.Unlocked, nil
}

// GetProfileRecovery returns the daemon's profile recovery state (`GET /encryption/recovery`) untouched. Its shape
// is the daemon's `ProfileRecoveryResponse`, which the page takes from the generated OpenAPI client.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) GetProfileRecovery(ctx context.Context) (json.RawMessage, error) {
	ctx, cancel := commandContext(ctx, "get_profile_recovery")
	defer cancel()
	var out json.RawMessage
	if err := h.client.Get(ctx, "/encryption/recovery", &out); err != nil {
		return nil, hostapi.Internal(err)
	}
	return out, nil
}

// ContentUnlockRequest is the passphrase submitted to unlock content.
type ContentUnlockRequest struct {
	Passphrase string `json:"passphrase"`
}

// UnlockContent unlocks content with the user's passphrase. It rejects with a hostapi.UnlockError: only the stable
// code crosses the bridge, because the daemon's text may contain private data.
//
//uc:errors unlock
//uc:os all=real
func (h *HostService) UnlockContent(ctx context.Context, request ContentUnlockRequest) error {
	ctx, cancel := commandContext(ctx, "unlock_content")
	defer cancel()
	err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/content-lock/unlock", JSON: map[string]string{"passphrase": request.Passphrase}}, nil)
	if err != nil {
		return hostapi.UnlockError{Code: hostapi.UnlockFromDaemon(daemonclient.ErrorCode(err))}
	}
	h.emit(contentLockChangedEvent, nil)
	return nil
}

// UnlockContentFromKeyring unlocks content with the key kept in the system keychain. It resolves false when the
// user refused the keychain prompt or the keychain had no usable key: the page then shows the passphrase form.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) UnlockContentFromKeyring(ctx context.Context) (bool, error) {
	if keyringUnlockDenied() {
		return false, nil
	}
	ctx, cancel := commandContext(ctx, "unlock_content_from_keyring")
	defer cancel()
	var status struct {
		Unlocked bool `json:"unlocked"`
	}
	if err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/content-lock/unlock-keyring"}, &status); err != nil {
		return false, hostapi.Internal(err)
	}
	if status.Unlocked {
		h.emit(contentLockChangedEvent, nil)
	}
	return status.Unlocked, nil
}

// ShowContentUnlock brings the main window forward so the user can unlock content.
//
//uc:errors none
//uc:os all=real
func (h *HostService) ShowContentUnlock() {
	h.showMainWindow()
}
