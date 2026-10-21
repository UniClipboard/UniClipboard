//go:build !darwin

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// Other GUI hosts use opaque native windows; their surfaces are controlled by the skin.
func applyGUIEffectsMaterial(_ *application.WebviewWindow, _ bool) {}
