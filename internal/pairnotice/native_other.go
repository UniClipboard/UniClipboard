//go:build !darwin

package pairnotice

import (
	"context"
	"errors"
)

func Available() bool { return false }

func PermissionStatus(context.Context) string { return "unsupported" }

func OpenSettings(context.Context) error { return errors.ErrUnsupported }

func Show(context.Context, string, string) error {
	return errors.ErrUnsupported
}
