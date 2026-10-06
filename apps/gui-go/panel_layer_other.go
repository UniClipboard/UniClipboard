//go:build !linux || !gtk3

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// The Wayland Layer Shell panel exists only on Linux; elsewhere the ordinary window path is the only one.
func attachLayerPanel(application.Window)                       {}
func layerPanelActive() bool                                    { return false }
func layerPrepareShow(application.Window, string, float64) bool { return false }
func layerShow(application.Window) bool                         { return false }
func layerSetLayout(application.Window, float64) bool           { return false }
