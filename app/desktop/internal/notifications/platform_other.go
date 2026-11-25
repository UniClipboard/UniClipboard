//go:build !darwin && !windows

package notifications

import (
	"context"
	"errors"

	"github.com/egoist/mygo"
)

// PermissionStatus reports whether desktop notifications are available on this host.
func PermissionStatus(context.Context) string {
	if mygo.NotificationsSupported() {
		return "supported"
	}
	return "unsupported"
}

// OpenSystemSettings is unsupported on generic Unix desktops.
func OpenSystemSettings(context.Context) error {
	return errors.ErrUnsupported
}
