//go:build !linux || !gtk3

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// Layer Shell is implemented for Linux GTK3. GTK4 and other platforms use ordinary windows.
func attachLayerPanel(application.Window)                       {}
func layerPanelActive() bool                                    { return false }
func layerPrepareShow(application.Window, string, float64) bool { return false }
func layerShow(application.Window) bool                         { return false }
func layerSetLayout(application.Window, float64) bool           { return false }
