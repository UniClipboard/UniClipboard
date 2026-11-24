package main

import (
	"context"
	"errors"
	"runtime"

	"github.com/egoist/mygo/plugins/permissionflow"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
)

// Query only this executable's permission receipt. Every installed Desktop
// role uses the same executable/bundle, but the native paste owner still checks
// permission immediately before using it. This is not a paste-capability probe.
func bindDesktopShortcutPermissions(screen *desktopui.Desktop) {
	read := func(ctx context.Context) (string, error) {
		if err := ctx.Err(); err != nil {
			return "unknown", err
		}
		if runtime.GOOS != "darwin" {
			return "unsupported", nil
		}
		status, err := permissionflow.Check(permissionflow.Accessibility)
		if errors.Is(err, errors.ErrUnsupported) {
			return "unsupported", nil
		}
		if err != nil {
			return "unknown", errors.New("cannot read accessibility permission")
		}
		switch status {
		case permissionflow.Granted:
			return "granted", nil
		case permissionflow.Denied, permissionflow.NotDetermined:
			return "denied", nil
		default:
			return "unknown", nil
		}
	}
	var open func(context.Context) error
	if runtime.GOOS == "darwin" {
		open = func(ctx context.Context) error {
			if err := ctx.Err(); err != nil {
				return err
			}
			// This action opens guidance only. Never request or imply a grant.
			return permissionflow.OpenSettings(permissionflow.Accessibility)
		}
	}
	screen.SetShortcutPermissionActions(read, open)
}
