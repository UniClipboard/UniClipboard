package main

import (
	"context"
	"encoding/json"
	"net/http"
	"os"

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
		"get_content_unlocked": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			var status struct {
				Unlocked bool `json:"unlocked"`
			}
			if err := h.client.Get(ctx, "/content-lock", &status); err != nil {
				return nil, internalError(err)
			}
			return status.Unlocked, nil
		},
		"get_profile_recovery": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			var out json.RawMessage
			if err := h.client.Get(ctx, "/encryption/recovery", &out); err != nil {
				return nil, internalError(err)
			}
			return out, nil
		},
		"unlock_content": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var request struct {
				Passphrase string `json:"passphrase"`
			}
			if err := args.decode("request", &request); err != nil {
				return nil, err
			}
			err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/content-lock/unlock", JSON: map[string]string{"passphrase": request.Passphrase}}, nil)
			if err != nil {
				// Only the stable code crosses this boundary; server text may contain private data.
				return nil, codeError{Code: contentUnlockCode(daemonclient.ErrorCode(err))}
			}
			h.emit(contentLockChangedEvent, nil)
			return nil, nil
		},
		"unlock_content_from_keyring": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
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

var contentUnlockCodes = map[string]bool{
	"WRONG_PASSPHRASE": true, "CORRUPTED_KEY_MATERIAL": true, "SETUP_NOT_COMPLETED": true,
	"SPACE_NOT_INITIALIZED": true, "PROFILE_RECOVERY_REQUIRED": true, "PROFILE_RECOVERY_PARTIAL": true,
	"PROFILE_RECOVERY_UNSUPPORTED": true, "PROFILE_RECOVERY_PERSISTENCE_FAILED": true,
}

func contentUnlockCode(code string) string {
	if contentUnlockCodes[code] {
		return code
	}
	return "INTERNAL"
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
