//go:build !e2e

package main

import "github.com/wailsapp/wails/v3/pkg/application"

func e2eServices(*HostService) []application.Service { return nil }

// profileBundleLoginItemAllowed: a named profile instance never changes its bundle's login item.
func profileBundleLoginItemAllowed() bool { return false }

func notifierServices(h *HostService) []application.Service {
	return []application.Service{application.NewService(h.notifier)}
}
