//go:build (!darwin || !private_mac_apis) && !production && !release

package main

import "github.com/wailsapp/wails/v3/pkg/application"

func openDevelopmentTools(window application.Window) { window.OpenDevTools() }
