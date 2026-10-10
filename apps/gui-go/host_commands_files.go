package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"
	"unicode"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/diaglogs"
)

// imageHandoffDir holds the one image handed to an external viewer. It is wiped before each
// hand-off so at most one decrypted copy lingers, and is never anywhere the app indexes or syncs.
const imageHandoffDir = "uniclipboard-image-handoff"

func notFound(format string, a ...any) error {
	return hostapi.New(hostapi.CodeNotFound, fmt.Sprintf(format, a...))
}

// sanitizeImageFileName reduces a caller-supplied name to a safe basename: no directory parts, no
// control characters, no leading dots, never empty.
func sanitizeImageFileName(raw string) string {
	if i := strings.LastIndexAny(raw, `/\`); i >= 0 {
		raw = raw[i+1:]
	}
	name := strings.TrimLeft(strings.TrimSpace(strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return -1
		}
		return r
	}, raw)), ".")
	if name == "" {
		return "image"
	}
	return name
}

// openWithSystem opens a path with the default application, or reveals it in the file manager.
func openWithSystem(path string, reveal bool) error {
	if handled, err := openerOverride(path, reveal); handled {
		return err
	}
	switch runtime.GOOS {
	case "darwin":
		if reveal {
			return startHostHelper("open", "-R", path)
		}
		return startHostHelper("open", path)
	case "windows":
		if reveal {
			return startHostHelper("explorer", "/select,"+path)
		}
		return startHostHelper("cmd", "/c", "start", "", path)
	default:
		target := path
		if reveal {
			target = filepath.Dir(path)
		}
		return startHostHelper("xdg-open", target)
	}
}

// openURLExternally opens a link in the default browser. Wails' Browser.OpenURL is used everywhere except Linux: its xdg-open call has no hook for the
// environment (pinned beta.28, internal/browser), and from an AppImage that call inherits AppRun's library paths (17c10).
func (h *HostService) openURLExternally(url string) error {
	if runtime.GOOS == "linux" {
		return startHostHelper("xdg-open", url)
	}
	return h.app.Browser.OpenURL(url)
}

// chooseDirectory shows the native folder picker; ok=false means the user cancelled.
func (h *HostService) chooseDirectory() (string, bool, error) {
	if path, ok := dialogOverride("directory"); ok {
		return path, path != "", nil
	}
	path, err := h.app.Dialog.OpenFile().CanChooseDirectories(true).CanChooseFiles(false).CanCreateDirectories(true).PromptForSingleSelection()
	return path, path != "", err
}

// chooseSaveFile shows the native save dialog; ok=false means the user cancelled.
func (h *HostService) chooseSaveFile(name string, filterName, pattern string) (string, bool, error) {
	if path, ok := dialogOverride("save"); ok {
		return path, path != "", nil
	}
	dialog := h.app.Dialog.SaveFile().SetFilename(name).CanCreateDirectories(true)
	if pattern != "" {
		dialog.AddFilter(filterName, pattern)
	}
	path, err := dialog.PromptForSingleSelection()
	return path, path != "", err
}

func (h *HostService) startupStatusForDiagnostics() []byte {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	status, err := h.daemon().StartupStatus(ctx)
	if err != nil {
		return nil // the daemon may be offline or past startup; the logs are still worth exporting
	}
	return status
}

// PickDirectory shows the native folder picker. It resolves nil when the user cancels.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) PickDirectory() (*string, error) {
	path, ok, err := h.chooseDirectory()
	if err != nil {
		return nil, hostapi.Internal(err)
	}
	if !ok {
		return nil, nil
	}
	return &path, nil
}

// OpenURL opens a link in the default browser. It is the command behind the page's opener module
// (frontend/src/host/opener.ts).
//
//uc:errors command InternalError
//uc:os all=real
//uc:adapter @/host/opener
func (h *HostService) OpenURL(url string) error {
	return hostapi.Internal(h.openURLExternally(url))
}

// OpenDataDirectory reveals the application data folder in the file manager.
//
//uc:errors command InternalError NotFound
//uc:os all=real
func (h *HostService) OpenDataDirectory() error {
	dir, ok := apppaths.AppDataRoot()
	if !ok {
		return hostapi.Internal(errors.New("data root unavailable"))
	}
	if _, err := os.Stat(dir); err != nil {
		return notFound("Directory does not exist: %s", dir)
	}
	return hostapi.Internal(openWithSystem(dir, false))
}

// OpenLogsDirectory opens the log folder in the file manager, creating it first.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) OpenLogsDirectory() error {
	dir, ok := apppaths.AppLogDir()
	if !ok {
		return hostapi.Internal(errors.New("log directory unavailable"))
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return hostapi.Internal(err)
	}
	return hostapi.Internal(openWithSystem(dir, false))
}

// RevealPath shows an existing file or folder in the file manager.
//
//uc:errors command InternalError NotFound
//uc:os all=real
func (h *HostService) RevealPath(path string) error {
	if _, err := os.Stat(path); err != nil {
		return notFound("Path does not exist: %s", path)
	}
	return hostapi.Internal(openWithSystem(path, true))
}

// SaveImageAs writes image bytes to a place the user picks. It resolves nil when the user cancels. The bytes cross
// the bridge as a base64 string (Go []byte), not as a number array.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) SaveImageAs(fileName string, data []byte) (*string, error) {
	path, ok, err := h.chooseSaveFile(sanitizeImageFileName(fileName), "", "")
	if err != nil {
		return nil, hostapi.Internal(err)
	}
	if !ok {
		return nil, nil
	}
	if err := os.WriteFile(path, data, 0o644); err != nil {
		return nil, hostapi.Internal(err)
	}
	return &path, nil
}

// OpenImageExternally hands image bytes to the system viewer through a single scratch file that is replaced on
// every call. The bytes cross the bridge as a base64 string.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) OpenImageExternally(fileName string, data []byte) error {
	dir := filepath.Join(os.TempDir(), imageHandoffDir)
	_ = os.RemoveAll(dir) // drop the previous hand-off; a missing directory is fine
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return hostapi.Internal(err)
	}
	target := filepath.Join(dir, sanitizeImageFileName(fileName))
	if err := os.WriteFile(target, data, 0o600); err != nil {
		return hostapi.Internal(err)
	}
	return hostapi.Internal(openWithSystem(target, false))
}

// ExportStartupLogs zips the logs to a place the user picks. It resolves nil when the user cancels.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) ExportStartupLogs() (*string, error) {
	logs, ok := apppaths.AppLogDir()
	if !ok {
		return nil, hostapi.Internal(errors.New("log directory unavailable"))
	}
	name := "uniclipboard-logs-" + time.Now().UTC().Format("20060102-150405") + ".zip"
	path, picked, err := h.chooseSaveFile(name, "ZIP archive", "*.zip")
	if err != nil {
		return nil, hostapi.Internal(err)
	}
	if !picked {
		return nil, nil
	}
	if err := diaglogs.ExportStartup(logs, path, h.startupStatusForDiagnostics()); err != nil {
		return nil, hostapi.Internal(err)
	}
	return &path, nil
}
