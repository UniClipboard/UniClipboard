//go:build !windows

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// suppressKeyboardMenu only matters on Windows, where a plain Alt press enters the system keyboard-menu mode.
func suppressKeyboardMenu(*application.WebviewWindow) {}
