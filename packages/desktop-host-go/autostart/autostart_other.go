//go:build !darwin

package autostart

func (Registration) Enable() error          { return ErrUnsupported }
func (Registration) Disable() error         { return ErrUnsupported }
func (Registration) Enabled() (bool, error) { return false, ErrUnsupported }
func (Registration) Reconcile(bool) error   { return ErrUnsupported }
