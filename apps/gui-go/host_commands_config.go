package main

import (
	"context"
	"net/http"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

const (
	bundleFileName  = "uniclipboard-config.ucbundle"
	bundleFilterTag = "UniClipboard config bundle"
	bundlePattern   = "*.ucbundle"
)

// configError mirrors the Rust `ConfigCommandError` tagged union:
// `{kind:"cancelled"}`, `{kind:"daemon", status, code, message}` or `{kind:"internal", message}`.
type configError struct {
	Kind    string  `json:"kind"`
	Status  *int    `json:"status,omitempty"`
	Code    *string `json:"code"`
	Message string  `json:"message,omitempty"`
}

func (e configError) Error() string { return e.Kind + ": " + e.Message }

var errConfigCancelled = configError{Kind: "cancelled"}

// wrapConfigError maps a daemon failure to the config error union.
func wrapConfigError(err error) error {
	if err == nil {
		return nil
	}
	if re, ok := daemonclient.AsRequestError(err); ok && re.Kind == daemonclient.ErrStatus {
		status := re.Status
		out := configError{Kind: "daemon", Status: &status, Message: re.Message}
		if re.Code != "" {
			code := re.Code
			out.Code = &code
		}
		return out
	}
	return configError{Kind: "internal", Message: err.Error()}
}

// chooseBundleToOpen shows the native open dialog for a `.ucbundle`; ok=false means cancelled.
func (h *HostService) chooseBundleToOpen() (string, bool, error) {
	if path, ok := dialogOverride("open"); ok {
		return path, path != "", nil
	}
	path, err := h.app.Dialog.OpenFile().CanChooseFiles(true).CanChooseDirectories(false).AddFilter(bundleFilterTag, bundlePattern).PromptForSingleSelection()
	return path, path != "", err
}

func (h *HostService) configPost(ctx context.Context, route string, body, out any) error {
	return wrapConfigError(h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPost, Path: route, JSON: body}, out))
}

func init() {
	register(map[string]commandFunc{
		"export_config_package": func(ctx context.Context, h *HostService, _ commandArgs) (any, error) {
			target, ok, err := h.chooseSaveFile(bundleFileName, bundleFilterTag, bundlePattern)
			if err != nil {
				return nil, configError{Kind: "internal", Message: err.Error()}
			}
			if !ok {
				return nil, errConfigCancelled
			}
			var result struct {
				Path string `json:"path"`
			}
			if err := h.configPost(ctx, "/config/export", map[string]any{"targetPath": target}, &result); err != nil {
				return nil, err
			}
			return result, nil
		},
		"pick_config_bundle_path": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			path, ok, err := h.chooseBundleToOpen()
			if err != nil {
				return nil, configError{Kind: "internal", Message: err.Error()}
			}
			if !ok {
				return nil, nil
			}
			return path, nil
		},
		"preview_config_import": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var password, source string
			if err := args.decode("password", &password); err != nil {
				return nil, err
			}
			if err := args.decode("sourcePath", &source); err != nil {
				return nil, err
			}
			var preview map[string]any
			if err := h.configPost(ctx, "/config/import/preview", map[string]any{"password": password, "sourcePath": source}, &preview); err != nil {
				return nil, err
			}
			return preview, nil
		},
		"import_config_package": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var password, source string
			if err := args.decode("password", &password); err != nil {
				return nil, err
			}
			if err := args.decode("sourcePath", &source); err != nil {
				return nil, err
			}
			var staged map[string]any
			if err := h.configPost(ctx, "/config/import", map[string]any{"password": password, "sourcePath": source, "confirmed": true}, &staged); err != nil {
				return nil, err
			}
			return staged, nil
		},
	})
}
