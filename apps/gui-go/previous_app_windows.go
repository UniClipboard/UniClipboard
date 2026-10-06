//go:build windows

package main

import (
	"fmt"
	"sync"
	"syscall"
	"time"
	"unicode/utf16"
	"unsafe"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/w32"
	"golang.org/x/sys/windows"
)

// previousAppInputSupported reports that this platform can restore the previous window and send keys to it.
const previousAppInputSupported = true

// Windows restricts SetForegroundWindow to the process that last received input. The workaround the Tauri shell
// uses (and PowerToys Run before it) is to join the foreground thread's input queue with AttachThreadInput around
// the call. Wails beta.28 exposes SetForegroundWindow/SetFocus/GetAsyncKeyState (pkg/w32) and x/sys/windows has
// GetForegroundWindow/GetGUIThreadInfo, but neither has AttachThreadInput, and the SendInput in pkg/w32 is a
// commented-out cgo remnant; those two are the only native calls bridged here.
var (
	user32Lib             = windows.NewLazySystemDLL("user32.dll")
	procAttachThreadInput = user32Lib.NewProc("AttachThreadInput")
	procSendInput         = user32Lib.NewProc("SendInput")
)

const (
	restoreSettle    = 40 * time.Millisecond // let the restored window take focus before keys are sent
	altReleaseWait   = 120 * time.Millisecond
	altReleasePoll   = 10 * time.Millisecond
	inputKeyboard    = 1
	keyEventKeyUp    = 0x0002
	keyEventUnicode  = 0x0004
	vkControl, vkAlt = 0x11, 0x12
	vkV              = 0x56
)

// previousTarget is the window that had the foreground before the quick panel opened, plus the child window
// inside it that held keyboard focus. Chromium/Electron/WebView2 apps keep focus in a nested render widget, so
// restoring only the top-level window would send the keystroke to the frame instead of the input box.
type previousTarget struct{ top, focus windows.HWND }

var previous struct {
	sync.Mutex
	target *previousTarget
}

// rememberPreviousForeground records the foreground window before the panel takes focus.
func rememberPreviousForeground(panel application.Window) {
	panelHWND := windowHWND(panel)
	top := windows.GetForegroundWindow()
	if top == 0 || top == panelHWND {
		return
	}
	var focus windows.HWND
	if thread, err := windows.GetWindowThreadProcessId(top, nil); err == nil && thread != 0 {
		info := windows.GUIThreadInfo{Size: uint32(unsafe.Sizeof(windows.GUIThreadInfo{}))}
		if windows.GetGUIThreadInfo(thread, &info) == nil {
			focus = info.Focus
		}
	}
	previous.Lock()
	previous.target = &previousTarget{top: top, focus: focus}
	previous.Unlock()
}

// restorePreviousForeground returns focus to the recorded window. The record is consumed, so a second
// restore without a new show reports an error instead of acting on a stale window.
func restorePreviousForeground() error {
	previous.Lock()
	target := previous.target
	previous.target = nil
	previous.Unlock()
	if target == nil {
		return fmt.Errorf("No previous foreground window captured")
	}
	return activateWindow(target.top, target.focus, "previous app")
}

// forceForegroundWindow gives the panel keyboard focus despite the foreground lock.
func forceForegroundWindow(panel application.Window) {
	_ = activateWindow(windowHWND(panel), 0, "quick panel")
}

func windowHWND(w application.Window) windows.HWND { return windows.HWND(uintptr(w.NativeWindow())) }

// activateWindow runs on the main (UI) thread: AttachThreadInput joins this thread's input queue, so the thread
// that calls SetForegroundWindow must be the one that owns the application's windows.
func activateWindow(top, focus windows.HWND, label string) error {
	if !windows.IsWindow(top) {
		return fmt.Errorf("Stored %s window handle is no longer valid", label)
	}
	if w32.IsWindowMinimised(uintptr(top)) {
		w32.ShowWindow(w32.HWND(top), w32.SW_RESTORE)
	}
	current := windows.GetCurrentThreadId()
	foregroundThread, _ := windows.GetWindowThreadProcessId(windows.GetForegroundWindow(), nil)
	attached := foregroundThread != 0 && foregroundThread != current
	if attached {
		attachThreadInput(foregroundThread, current, true)
	}
	ok := w32.SetForegroundWindow(w32.HWND(top)) != 0
	w32.SetFocus(w32.HWND(top))
	if attached {
		attachThreadInput(foregroundThread, current, false)
	}
	if !ok {
		return fmt.Errorf("SetForegroundWindow failed while restoring %s", label)
	}
	if focus != 0 && focus != top && windows.IsWindow(focus) {
		if thread, err := windows.GetWindowThreadProcessId(focus, nil); err == nil && thread != 0 {
			innerAttached := thread != current
			if innerAttached {
				attachThreadInput(thread, current, true)
			}
			w32.SetFocus(w32.HWND(focus))
			if innerAttached {
				attachThreadInput(thread, current, false)
			}
		}
	}
	return nil
}

func attachThreadInput(from, to uint32, attach bool) {
	var flag uintptr
	if attach {
		flag = 1
	}
	_, _, _ = procAttachThreadInput.Call(uintptr(from), uintptr(to), flag)
}

// runOnMainThread runs fn on the application's UI thread and returns its error.
func runOnMainThread(fn func() error) error { return application.InvokeSyncWithError(fn) }

// simulatePaste sends Ctrl+V to the window restored by restorePreviousForeground; the clipboard already
// holds the selected item (the panel copies it first), so nothing is written here.
func simulatePaste() error {
	time.Sleep(restoreSettle)
	// A still-held Alt (the quick panel shortcut is Ctrl+Alt+V) would turn Ctrl+V into Ctrl+Alt+V in the target;
	// wait briefly for its release and neutralize it around the keystroke when it stays down.
	altDown := !waitForAltRelease()
	keys := []keyEvent{}
	if altDown {
		keys = append(keys, keyEvent{vk: vkAlt, up: true})
	}
	keys = append(keys,
		keyEvent{vk: vkControl}, keyEvent{vk: vkV}, keyEvent{vk: vkV, up: true}, keyEvent{vk: vkControl, up: true})
	if altDown {
		keys = append(keys, keyEvent{vk: vkAlt})
	}
	return sendKeys(keys)
}

// simulateTextInput types text as Unicode key events without touching the clipboard.
func simulateTextInput(text string) error {
	if text == "" {
		return fmt.Errorf("Cannot enter empty text")
	}
	time.Sleep(restoreSettle)
	var keys []keyEvent
	for _, unit := range utf16.Encode([]rune(text)) {
		keys = append(keys, keyEvent{scan: unit, unicode: true}, keyEvent{scan: unit, unicode: true, up: true})
	}
	return sendKeys(keys)
}

func waitForAltRelease() bool {
	altIsDown := func() bool { return w32.GetAsyncKeyState(vkAlt)&0x8000 != 0 }
	if !altIsDown() {
		return true
	}
	for deadline := time.Now().Add(altReleaseWait); time.Now().Before(deadline); {
		time.Sleep(altReleasePoll)
		if !altIsDown() {
			return true
		}
	}
	return false
}

type keyEvent struct {
	vk, scan uint16
	up       bool
	unicode  bool
}

// keyboardInput is the Win32 INPUT structure for a keyboard event; the trailing pad makes it as large as the
// union's biggest member (MOUSEINPUT), which SendInput checks through its size argument.
type keyboardInput struct {
	kind uint32
	ki   struct {
		vk, scan  uint16
		flags     uint32
		timestamp uint32
		extra     uintptr
	}
	_ [8]byte
}

func sendKeys(events []keyEvent) error {
	inputs := make([]keyboardInput, len(events))
	for i, event := range events {
		inputs[i].kind = inputKeyboard
		inputs[i].ki.vk, inputs[i].ki.scan = event.vk, event.scan
		if event.unicode {
			inputs[i].ki.flags |= keyEventUnicode
		}
		if event.up {
			inputs[i].ki.flags |= keyEventKeyUp
		}
	}
	sent, _, callErr := procSendInput.Call(uintptr(len(inputs)), uintptr(unsafe.Pointer(&inputs[0])), unsafe.Sizeof(inputs[0]))
	if int(sent) != len(inputs) {
		if callErr != syscall.Errno(0) {
			return fmt.Errorf("SendInput sent %d events, expected %d: %v", sent, len(inputs), callErr)
		}
		return fmt.Errorf("SendInput sent %d events, expected %d", sent, len(inputs))
	}
	return nil
}
