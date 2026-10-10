package main

import (
	"context"
	"net/http"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

const (
	bundleFileName  = "uniclipboard-config.ucbundle"
	bundleFilterTag = "UniClipboard config bundle"
	bundlePattern   = "*.ucbundle"
)

// Configuration package commands. They reject with a hostapi.ConfigError:
// `{kind:"cancelled"}`, `{kind:"daemon", status, code, message}` or `{kind:"internal", message}`.

var errConfigCancelled = hostapi.ConfigError{Kind: hostapi.ConfigCancelled}

func configInternal(err error) error {
	return hostapi.ConfigError{Kind: hostapi.ConfigInternal, Message: err.Error()}
}

// wrapConfigError maps a daemon failure to the config error union.
func wrapConfigError(err error) error {
	if err == nil {
		return nil
	}
	if re, ok := daemonclient.AsRequestError(err); ok && re.Kind == daemonclient.ErrStatus {
		status := re.Status
		out := hostapi.ConfigError{Kind: hostapi.ConfigDaemon, Status: &status, Message: re.Message}
		if re.Code != "" {
			code := re.Code
			out.Code = &code
		}
		return out
	}
	return configInternal(err)
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

// ExportConfigPackage asks where to save, then has the daemon write the configuration bundle there.
//
//uc:errors config
//uc:os all=real
func (h *HostService) ExportConfigPackage(ctx context.Context) (ExportConfigResult, error) {
	ctx, cancel := commandContext(ctx, "export_config_package")
	defer cancel()
	target, ok, err := h.chooseSaveFile(bundleFileName, bundleFilterTag, bundlePattern)
	if err != nil {
		return ExportConfigResult{}, configInternal(err)
	}
	if !ok {
		return ExportConfigResult{}, errConfigCancelled
	}
	var result ExportConfigResult
	if err := h.configPost(ctx, "/config/export", map[string]any{"targetPath": target}, &result); err != nil {
		return ExportConfigResult{}, err
	}
	return result, nil
}

// PickConfigBundlePath shows the native open dialog for a configuration bundle. It resolves nil when cancelled.
//
//uc:errors config
//uc:os all=real
func (h *HostService) PickConfigBundlePath() (*string, error) {
	path, ok, err := h.chooseBundleToOpen()
	if err != nil {
		return nil, configInternal(err)
	}
	if !ok {
		return nil, nil
	}
	return &path, nil
}

// PreviewConfigImport reads the descriptive metadata of a bundle without importing it.
//
//uc:errors config
//uc:os all=real
func (h *HostService) PreviewConfigImport(ctx context.Context, password string, sourcePath string) (ConfigImportPreview, error) {
	ctx, cancel := commandContext(ctx, "preview_config_import")
	defer cancel()
	var preview ConfigImportPreview
	if err := h.configPost(ctx, "/config/import/preview", map[string]any{"password": password, "sourcePath": sourcePath}, &preview); err != nil {
		return ConfigImportPreview{}, err
	}
	return preview, nil
}

// ImportConfigPackage validates a bundle and stages it to be applied at the next start.
//
//uc:errors config
//uc:os all=real
func (h *HostService) ImportConfigPackage(ctx context.Context, password string, sourcePath string) (ImportConfigStageResult, error) {
	ctx, cancel := commandContext(ctx, "import_config_package")
	defer cancel()
	var staged ImportConfigStageResult
	if err := h.configPost(ctx, "/config/import", map[string]any{"password": password, "sourcePath": sourcePath, "confirmed": true}, &staged); err != nil {
		return ImportConfigStageResult{}, err
	}
	return staged, nil
}
