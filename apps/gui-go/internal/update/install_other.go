//go:build !darwin

package update

import (
	"errors"
	"os"
)

// ErrInstallUnsupported is returned where in-place installation is not implemented.
var ErrInstallUnsupported = errors.New("in-place update install is not implemented on this platform")

// BundleOf is only meaningful on macOS.
func BundleOf(executable string) (string, error) { return os.Executable() }

// Install is not implemented outside macOS.
func Install([]byte, string) error { return ErrInstallUnsupported }
