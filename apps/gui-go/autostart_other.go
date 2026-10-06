//go:build !linux

package main

func (h *HostService) loginItem() osAutostart { return h.app.Autostart }

func (p loginItemPolicy) sweepLegacyDesktopEntry() error { return nil }
