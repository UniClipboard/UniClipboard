//go:build windows

package scenarios

import (
	"fmt"
	"sync"
	"unsafe"

	"golang.org/x/sys/windows"
)

const (
	swMinimize = 6
	swRestore  = 9
)

var (
	hideUser32            = windows.NewLazySystemDLL("user32.dll")
	procHideEnumWindows   = hideUser32.NewProc("EnumWindows")
	procHideGetWindowPID  = hideUser32.NewProc("GetWindowThreadProcessId")
	procHideIsVisible     = hideUser32.NewProc("IsWindowVisible")
	procHideShowWindow    = hideUser32.NewProc("ShowWindow")
	procHideSetForeground = hideUser32.NewProc("SetForegroundWindow")

	hideMu        sync.Mutex
	hideOnce      sync.Once
	hideCB        uintptr
	hideTargetPID uint32
	hideHWNDs     []uintptr
)

func getHideEnumCallback() uintptr {
	hideOnce.Do(func() {
		hideCB = windows.NewCallback(func(hwnd, _ uintptr) uintptr {
			var owner uint32
			_, _, _ = procHideGetWindowPID.Call(hwnd, uintptr(unsafe.Pointer(&owner)))
			if owner == hideTargetPID {
				vis, _, _ := procHideIsVisible.Call(hwnd)
				if vis != 0 {
					hideHWNDs = append(hideHWNDs, hwnd)
				}
			}
			return 1
		})
	})
	return hideCB
}

func findProcessHWNDs(pid uint32) []uintptr {
	hideMu.Lock()
	defer hideMu.Unlock()
	hideTargetPID = pid
	hideHWNDs = nil
	_, _, _ = procHideEnumWindows.Call(getHideEnumCallback(), 0)
	out := make([]uintptr, len(hideHWNDs))
	copy(out, hideHWNDs)
	return out
}

func hideGUIProcess(pid int) error {
	if pid <= 0 {
		return fmt.Errorf("invalid gui pid %d", pid)
	}
	hwnds := findProcessHWNDs(uint32(pid))
	if len(hwnds) == 0 {
		return fmt.Errorf("no visible HWND found for gui pid %d", pid)
	}
	for _, hwnd := range hwnds {
		_, _, _ = procHideShowWindow.Call(hwnd, swMinimize)
	}
	return nil
}

func restoreGUIProcess(pid int) error {
	if pid <= 0 {
		return fmt.Errorf("invalid gui pid %d", pid)
	}
	hwnds := findProcessHWNDs(uint32(pid))
	if len(hwnds) == 0 {
		return fmt.Errorf("no HWND found to restore for gui pid %d", pid)
	}
	for _, hwnd := range hwnds {
		_, _, _ = procHideShowWindow.Call(hwnd, swRestore)
		_, _, _ = procHideSetForeground.Call(hwnd)
	}
	return nil
}
