//go:build e2e && !darwin && !linux

package main

// No update marker exists outside macOS and the Linux AppImage (the Windows installer flow is verified by its own scenario).
func updateMarker() (string, bool) { return "", false }
