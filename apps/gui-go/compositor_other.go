//go:build !linux

package main

// usesCompositorShortcuts is a Linux/Wayland concept; the other platforms register the shortcut with the OS.
func usesCompositorShortcuts() bool { return false }
