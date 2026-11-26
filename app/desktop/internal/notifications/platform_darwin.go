//go:build darwin

package notifications

import (
	"context"

	"github.com/uniclipboard/unic/internal/pairnotice"
)

// PermissionStatus passively queries the OS notification authorization status
// without prompting the user. Returns one of granted, denied, not_determined,
// unsupported, or unknown.
func PermissionStatus(ctx context.Context) string {
	return pairnotice.PermissionStatus(ctx)
}

// OpenSystemSettings opens macOS System Settings to the Notifications pane.
func OpenSystemSettings(ctx context.Context) error {
	return pairnotice.OpenSettings(ctx)
}
