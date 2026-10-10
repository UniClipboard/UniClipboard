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

const contentLockChangedEvent = "content-lock-changed"

func init() {
	register(map[string]commandFunc{
		"get_daemon_connection_info": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			return map[string]string{"baseUrl": h.client.BaseURL, "wsUrl": h.client.WSURL}, nil
		},
		"get_daemon_session": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			session, err := h.client.ExchangeSession(ctx, "gui")
			if err != nil {
				return nil, internalError(err)
			}
			return session, nil
		},
		"get_daemon_bootstrap_failure": func(context.Context, *HostService, commandArgs) (any, error) { return nil, nil },
		"get_daemon_startup_status": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			raw, err := h.client.StartupStatus(ctx)
			if err != nil {
				return nil, internalError(err)
			}
			if raw == nil {
				return nil, nil
			}
			return raw, nil
		},
		"get_tauri_pid": func(context.Context, *HostService, commandArgs) (any, error) { return os.Getpid(), nil },
		"get_device_id": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			return h.deviceID(ctx)
		},
		"get_device_meta": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			id, err := h.deviceID(ctx)
			if err != nil {
				return nil, err
			}
			return map[string]any{
				"deviceId":        id,
				"deviceRole":      "gui-host",
				"platform":        hostPlatform(),
				"appVersion":      buildinfo.PackageVersion,
				"appChannel":      "dev",
				"runtimeProfile":  os.Getenv("UC_PROFILE"),
				"developmentMode": os.Getenv("UNICLIPBOARD_ENV") == "development",
			}, nil
		},
		"get_profile_recovery": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			var out json.RawMessage
			if err := h.client.Get(ctx, "/encryption/recovery", &out); err != nil {
				return nil, internalError(err)
			}
			return out, nil
		},
		"unlock_content_from_keyring": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			if keyringUnlockDenied() {
				return false, nil // the user refused the keychain prompt: the page falls back to the passphrase form
			}
			var status struct {
				Unlocked bool `json:"unlocked"`
			}
			if err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/content-lock/unlock-keyring"}, &status); err != nil {
				return nil, internalError(err)
			}
			if status.Unlocked {
				h.emit(contentLockChangedEvent, nil)
			}
			return status.Unlocked, nil
		},
	})
}

func (h *HostService) deviceID(ctx context.Context) (string, error) {
	var me struct {
		PeerID string `json:"peerId"`
	}
	if err := h.client.Get(ctx, "/device/me", &me); err != nil {
		return "", internalError(err)
	}
	return me.PeerID, nil
}

// GetContentUnlocked reports whether the daemon lets this GUI show content right now.
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

// ContentUnlockRequest is the passphrase submitted to unlock content.
type ContentUnlockRequest struct {
	Passphrase string `json:"passphrase"`
}

// UnlockContent unlocks content with the user's passphrase. It rejects with a hostapi.UnlockError: only the stable
// code crosses the bridge, because the daemon's text may contain private data.
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
