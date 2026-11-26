package headless

import (
	"context"
	"errors"
	"runtime"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func init() {
	matchClipboardDuringHandoff = matchAfterTransientLock
}

// Temporarily hold the clipboard in the external E2E test process while the
// daemon verifies the paste mark. The synthetic content remains unchanged.
func matchAfterTransientLock(client *api.Client, ctx context.Context, mark string) (bool, error) {
	user32 := windows.NewLazySystemDLL("user32.dll")
	open := user32.NewProc("OpenClipboard")
	closeBoard := user32.NewProc("CloseClipboard")
	create := user32.NewProc("CreateWindowExW")
	destroy := user32.NewProc("DestroyWindow")
	held := make(chan bool, 1)
	release := make(chan struct{})
	released := make(chan struct{})
	defer func() { close(release); <-released }()
	go func() {
		defer close(released)
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		class, _ := windows.UTF16PtrFromString("STATIC")
		window, _, _ := create.Call(0, uintptr(unsafe.Pointer(class)), 0, 0, 0, 0, 0, 0, ^uintptr(2), 0, 0, 0)
		if window == 0 {
			held <- false
			return
		}
		defer func() { _, _, _ = destroy.Call(window) }()
		deadline := time.Now().Add(time.Second)
		for {
			result, _, _ := open.Call(window)
			if result != 0 {
				break
			}
			if time.Now().After(deadline) || ctx.Err() != nil {
				held <- false
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		defer func() { _, _, _ = closeBoard.Call() }()
		held <- true
		select {
		case <-time.After(125 * time.Millisecond):
		case <-release:
		}
	}()
	if !<-held {
		return false, errors.New("external test failed to acquire clipboard lock")
	}
	start := time.Now()
	match, err := client.MatchDesktopClipboard(ctx, mark)
	if err == nil && match && time.Since(start) < 70*time.Millisecond {
		return false, errors.New("paste mark was accepted before the clipboard lock cleared")
	}
	return match, err
}
