//go:build !e2e

package main

import "github.com/wailsapp/wails/v3/pkg/application"

func e2eServices(*HostService) []application.Service { return nil }

// profileBundleLoginItemAllowed: a named profile instance never changes its bundle's login item.
func profileBundleLoginItemAllowed() bool { return false }

func notifierServices(h *HostService) []application.Service {
	return []application.Service{application.NewService(h.notifier)}
}

// The e2e evidence hooks of the single-instance path do nothing in normal builds.
func e2eLaunch(*HostService) {}

func e2eSecondInstance(*HostService, application.SecondInstanceData, secondLaunchAction) {}

func e2eBootstrapped(*HostService, bool) {}

// scriptedKeyState: normal builds only read the real keyboard.
func scriptedKeyState() bool { return false }

func modifierKeyStateFactory() func() (modifierKeyState, error) { return newPlatformKeyState }

func e2eModifierTriggered() {}

// e2eTrayLanguage records tray language calls in the e2e build only.
func e2eTrayLanguage(string) {}

// e2eTrayLanguageGap is the scheduling hook of the e2e build; it does nothing here.
func e2eTrayLanguageGap() {}

// e2eTrayPublish times a tray menu publish in the e2e build; it returns a no-op here.
func e2eTrayPublish() func() { return func() {} }

// e2eInvoke marks the entry and return of selected host commands in the e2e build; it returns a no-op here.
func e2eInvoke(string) func() { return func() {} }
