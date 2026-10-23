//go:build !linux || !gtk4

package main

import "github.com/wailsapp/wails/v3/pkg/application"

func setWindowZoom(w application.Window, scale float64) error {
	w.SetZoom(scale)
	return nil
}
