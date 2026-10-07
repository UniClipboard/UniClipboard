//go:build e2e && !darwin

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// trayNativeState is only meaningful for the macOS tray implementation.
func trayNativeState(*application.SystemTray) map[string]any { return nil }

// trayEnableOpenMenu is only needed for the macOS tray.
func trayEnableOpenMenu(*application.SystemTray, *application.Menu) bool { return false }
