//go:build !darwin

package main

// The Dock icon and the reopen event exist only on macOS.
func (h *HostService) setDockIconVisible(bool) {}
func (h *HostService) routeDockReopen()        {}
