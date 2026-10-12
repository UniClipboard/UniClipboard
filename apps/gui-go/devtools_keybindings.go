//go:build !production && !release

package main

import (
	"github.com/wailsapp/wails/v3/pkg/application"
	"log"
)

// The native binding remains available when the frontend error boundary is showing.
func devToolsKeyBindings() map[string]func(application.Window) {
	open := func(window application.Window) {
		log.Print("opening development tools")
		openDevelopmentTools(window)
	}
	return map[string]func(application.Window){"F12": open, "Ctrl+Shift+I": open}
}
