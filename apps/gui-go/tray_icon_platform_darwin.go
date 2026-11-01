package main

import "github.com/wailsapp/wails/v3/pkg/application"

func trayIconSpec() iconSpec {
	return macTrayIconSpec()
}

// macOS colours the template from its alpha alone, including on theme changes.
func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetTemplateIcon(data) }

// catTrayImage derives a smaller, padded template without changing the shipped asset.
func catTrayImage() ([]byte, error) { return macCatTrayImage() }

// watchSystemTheme: macOS recolours a template image itself, so nothing to watch.
func watchSystemTheme(*application.App, func()) {}
