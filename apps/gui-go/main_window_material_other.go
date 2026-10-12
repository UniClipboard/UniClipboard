//go:build !darwin

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// withWindowMaterial: only the macOS host applies a window material behind the page.
func withWindowMaterial(o application.WebviewWindowOptions) application.WebviewWindowOptions {
	return o
}
