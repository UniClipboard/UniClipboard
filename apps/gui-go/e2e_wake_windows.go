//go:build e2e && windows

package main

import (
	"errors"
	"os"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

const (
	wmPowerBroadcast     = 0x0218
	pbtAPMResumeAutomaic = 0x0012
)

// postSystemWake delivers the message Windows sends when the machine resumes (WM_POWERBROADCAST /
// PBT_APMRESUMEAUTOMATIC) to this process's own windows, so Wails' window procedure -> Windows.APMResumeAutomatic ->
// Common.SystemDidWake chain delivers it to the scheduler. The machine does not sleep: it is the message-level
// stand-in for a real resume.
func postSystemWake() error {
	sendMessage := windows.NewLazySystemDLL("user32.dll").NewProc("SendMessageTimeoutW")
	pid := uint32(os.Getpid())
	var delivered int
	cb := syscall.NewCallback(func(hwnd uintptr, _ uintptr) uintptr {
		var owner uint32
		_, _ = windows.GetWindowThreadProcessId(windows.HWND(hwnd), &owner)
		if owner == pid {
			var result uintptr
			if r, _, _ := sendMessage.Call(hwnd, wmPowerBroadcast, pbtAPMResumeAutomaic, 0, 0x2 /* SMTO_ABORTIFHUNG */, 2000, uintptr(unsafe.Pointer(&result))); r != 0 {
				delivered++
			}
		}
		return 1
	})
	if err := windows.EnumWindows(cb, nil); err != nil {
		return err
	}
	if delivered == 0 {
		return errors.New("no window of this process accepted the resume message")
	}
	return nil
}
