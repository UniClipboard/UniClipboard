package main

import (
	"context"
	"errors"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/notifications"
)

func bindDesktopNotifications(screen *desktopui.Desktop, dir string) {
	read := func(ctx context.Context) (string, error) {
		if err := ctx.Err(); err != nil {
			return "unknown", err
		}
		return notifications.PermissionStatus(ctx), nil
	}
	open := func(ctx context.Context) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		return notifications.OpenSystemSettings(ctx)
	}
	sendTest := func(ctx context.Context) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		err := desktopprocess.Call(ctx, dir, desktopprocess.Tray, "test-notification", nil, nil)
		if errors.Is(err, desktopprocess.ErrUnavailable) {
			return errors.ErrUnsupported
		}
		return err
	}
	screen.SetNotificationActions(read, open, sendTest)
}
