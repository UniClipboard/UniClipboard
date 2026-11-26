//go:build windows

package notifications

import (
	"context"
	"os/exec"

	"github.com/egoist/mygo"
)

// PermissionStatus reports whether native notifications are available on Windows.
func PermissionStatus(context.Context) string {
	if mygo.NotificationsSupported() {
		return "supported"
	}
	return "unsupported"
}

// OpenSystemSettings opens Windows Settings to Notifications.
func OpenSystemSettings(ctx context.Context) error {
	return exec.CommandContext(ctx, "rundll32", "url.dll,FileProtocolHandler", "ms-settings:notifications").Run()
}
