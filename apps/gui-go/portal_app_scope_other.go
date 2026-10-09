//go:build !linux

package main

// ensureAppScope is a Linux/Wayland concern (the portal learns the app id from the systemd scope).
func ensureAppScope() {}
