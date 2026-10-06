//go:build windows

package main

import "github.com/wailsapp/wails/v3/pkg/w32"

// modifierDoubleTapAvailable: reading the keyboard with GetAsyncKeyState needs no permission on Windows
// (Tauri `modifier_double_tap_platform`, windows module: always Supported).
const modifierDoubleTapAvailable = true

// Virtual-key codes (winuser.h) the snapshot distinguishes.
const (
	vkLButton  = 0x01
	vkXButton2 = 0x06
	vkControlG = 0x11 // VK_CONTROL
	vkMenu     = 0x12 // VK_MENU (Alt)
	vkLWin     = 0x5b
	vkRWin     = 0x5c
	vkLControl = 0xa2
	vkRControl = 0xa3
	vkLMenu    = 0xa4
	vkRMenu    = 0xa5
)

type windowsKeyState struct{}

func newPlatformKeyState() (modifierKeyState, error) { return windowsKeyState{}, nil }

func selectedKeys(modifier string) []int {
	switch modifier {
	case "alt":
		return []int{vkMenu, vkLMenu, vkRMenu}
	case "control":
		return []int{vkControlG, vkLControl, vkRControl}
	case "meta":
		return []int{vkLWin, vkRWin}
	}
	return nil
}

// snapshot samples every key with GetAsyncKeyState (the physical state, whichever window has focus), exactly as the
// Tauri shell does: the selected modifier's keys, and any other key in 0x07..0xFE. The mouse buttons
// (0x01..0x06) are not keyboard activity and are skipped.
func (windowsKeyState) snapshot(modifier string) (selectedDown, otherDown bool) {
	selected := selectedKeys(modifier)
	isSelected := func(vk int) bool {
		for _, s := range selected {
			if s == vk {
				return true
			}
		}
		return false
	}
	for _, vk := range selected {
		if keyDown(vk) {
			selectedDown = true
			break
		}
	}
	for vk := 0x07; vk <= 0xfe; vk++ {
		if !isSelected(vk) && keyDown(vk) {
			otherDown = true
			break
		}
	}
	return selectedDown, otherDown
}

func keyDown(vk int) bool {
	if vk >= vkLButton && vk <= vkXButton2 {
		return false
	}
	return w32.GetAsyncKeyState(vk)&0x8000 != 0
}
