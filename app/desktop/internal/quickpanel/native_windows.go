//go:build windows

package quickpanel

import (
	"os"
	"unsafe"

	"golang.org/x/sys/windows"
)

var user32 = windows.NewLazySystemDLL("user32.dll")
var getForeground = user32.NewProc("GetForegroundWindow")
var getWindowPID = user32.NewProc("GetWindowThreadProcessId")
var setForeground = user32.NewProc("SetForegroundWindow")
var showWindow = user32.NewProc("ShowWindow")
var getKeyState = user32.NewProc("GetAsyncKeyState")
var sendInput = user32.NewProc("SendInput")
var isWindow = user32.NewProc("IsWindow")

type windowsPlatform struct{}

func NewNative() Platform { return windowsPlatform{} }
func winIdentity(hwnd uintptr) (int, uint64) {
	valid, _, _ := isWindow.Call(hwnd)
	if valid == 0 {
		return 0, 0
	}
	var pid uint32
	_, _, _ = getWindowPID.Call(hwnd, uintptr(unsafe.Pointer(&pid)))
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return 0, 0
	}
	defer func() { _ = windows.CloseHandle(h) }()
	var created, exited, kernel, user windows.Filetime
	if windows.GetProcessTimes(h, &created, &exited, &kernel, &user) != nil {
		return 0, 0
	}
	return int(pid), uint64(created.HighDateTime)<<32 | uint64(created.LowDateTime)
}
func (windowsPlatform) Capture() (Target, error) {
	hwnd, _, _ := getForeground.Call()
	pid, born := winIdentity(hwnd)
	if hwnd == 0 || pid == 0 || pid == os.Getpid() || born == 0 {
		return Target{}, ErrTarget
	}
	return Target{ID: hwnd, PID: pid, Born: born, Name: "原应用"}, nil
}
func (windowsPlatform) Check(t Target) error {
	pid, born := winIdentity(t.ID)
	if pid != t.PID || pid == 0 || pid == os.Getpid() || born != t.Born || born == 0 {
		return ErrTarget
	}
	return nil
}
func (p windowsPlatform) Activate(t Target) error {
	if err := p.Check(t); err != nil {
		return err
	}
	// SW_RESTORE also restores a minimized target without creating another instance.
	_, _, _ = showWindow.Call(t.ID, 9)
	ok, _, _ := setForeground.Call(t.ID)
	if ok == 0 {
		return ErrFocus
	}
	return nil
}
func (windowsPlatform) OwnsForeground() bool {
	h, _, _ := getForeground.Call()
	pid, _ := winIdentity(h)
	return pid == os.Getpid()
}
func (p windowsPlatform) Ready(t Target) (bool, error) {
	if err := p.Check(t); err != nil {
		return false, err
	}
	h, _, _ := getForeground.Call()
	if h != t.ID {
		pid, _ := winIdentity(h)
		if pid != os.Getpid() {
			return false, ErrFocus
		}
		return false, nil
	}
	for _, key := range []uintptr{0x10, 0x11, 0x12, 0x5b, 0x5c, 0x0d, 0x56, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39} {
		state, _, _ := getKeyState.Call(key)
		if state&0x8000 != 0 {
			return false, nil
		}
	}
	return true, nil
}
func (p windowsPlatform) Paste(t Target) error {
	ready, err := p.Ready(t)
	if err != nil {
		return err
	}
	if !ready {
		return ErrFocus
	}
	// INPUT is DWORD type + an architecture-aligned 32/24-byte union.
	size, offset := 40, 8
	if unsafe.Sizeof(uintptr(0)) == 4 {
		size, offset = 28, 4
	}
	input := make([]byte, size*4)
	for i, key := range []uint16{0x11, 0x56, 0x56, 0x11} {
		base := unsafe.Pointer(&input[i*size])
		*(*uint32)(base) = 1
		*(*uint16)(unsafe.Add(base, offset)) = key
		if i >= 2 {
			*(*uint32)(unsafe.Add(base, offset+4)) = 2
		}
	}
	sent, _, _ := sendInput.Call(4, uintptr(unsafe.Pointer(&input[0])), uintptr(size))
	if sent != 4 {
		// Release only keys from the submitted chord. Never repeat key-down input.
		releases := input[size*2:]
		_, _, _ = sendInput.Call(2, uintptr(unsafe.Pointer(&releases[0])), uintptr(size))
		return ErrInput
	}
	return nil
}
