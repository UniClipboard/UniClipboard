//go:build !windows

package quickpanel

type unsupportedPlatform struct{}

func (unsupportedPlatform) Capture() (Target, error)   { return Target{}, ErrUnsupported }
func (unsupportedPlatform) Check(Target) error         { return ErrUnsupported }
func (unsupportedPlatform) Activate(Target) error      { return ErrUnsupported }
func (unsupportedPlatform) Ready(Target) (bool, error) { return false, ErrUnsupported }
func (unsupportedPlatform) Paste(Target) error         { return ErrUnsupported }
func (unsupportedPlatform) OwnsForeground() bool       { return false }
