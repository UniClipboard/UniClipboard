//go:build !darwin

package update

import "os"

// BundleOf is only meaningful on macOS.
func BundleOf(executable string) (string, error) { return os.Executable() }

// Install is not implemented outside macOS.
func Install(*os.File, string) error { return ErrInstallUnsupported }
