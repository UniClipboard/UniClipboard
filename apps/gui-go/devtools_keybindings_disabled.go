//go:build production || release

package main

import "github.com/wailsapp/wails/v3/pkg/application"

func devToolsKeyBindings() map[string]func(application.Window) { return nil }
