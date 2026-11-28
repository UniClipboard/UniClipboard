//go:build darwin

package scenarios

import (
	"fmt"
	"os/exec"
	"runtime"
	"strings"
	"sync"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

var (
	hideAppKitOnce sync.Once
	hideAppKitErr  error
)

func ensureHideAppKit() error {
	hideAppKitOnce.Do(func() {
		if _, err := purego.Dlopen("/System/Library/Frameworks/Foundation.framework/Foundation", purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
			hideAppKitErr = fmt.Errorf("dlopen Foundation: %w", err)
			return
		}
		if _, err := purego.Dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
			hideAppKitErr = fmt.Errorf("dlopen AppKit: %w", err)
			return
		}
	})
	return hideAppKitErr
}

func hideGUIProcess(pid int) error {
	if pid <= 0 {
		return fmt.Errorf("invalid gui pid %d", pid)
	}
	if err := setProcessVisibleAppleScript(pid, false); err == nil {
		return nil
	}
	return setRunningAppHidden(pid, true)
}

func restoreGUIProcess(pid int) error {
	if pid <= 0 {
		return fmt.Errorf("invalid gui pid %d", pid)
	}
	if err := setProcessVisibleAppleScript(pid, true); err == nil {
		return nil
	}
	return setRunningAppHidden(pid, false)
}

func setProcessVisibleAppleScript(pid int, visible bool) error {
	visStr := "false"
	if visible {
		visStr = "true"
	}
	script := fmt.Sprintf(`tell application "System Events" to set visible of (first process whose unix id is %d) to %s`, pid, visStr)
	out, err := exec.Command("/usr/bin/osascript", "-e", script).CombinedOutput()
	if err != nil {
		return fmt.Errorf("osascript visible=%s pid=%d: %v (%s)", visStr, pid, err, strings.TrimSpace(string(out)))
	}
	return nil
}

type voidType = struct{}

func setRunningAppHidden(pid int, hide bool) error {
	if err := ensureHideAppKit(); err != nil {
		return err
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	poolCls := objc.ID(objc.GetClass("NSAutoreleasePool"))
	pool := objc.Send[objc.ID](objc.Send[objc.ID](poolCls, objc.RegisterName("alloc")), objc.RegisterName("init"))
	defer objc.Send[voidType](pool, objc.RegisterName("drain"))

	cls := objc.ID(objc.GetClass("NSRunningApplication"))
	if cls == 0 {
		return fmt.Errorf("NSRunningApplication unavailable")
	}
	app := objc.Send[objc.ID](cls, objc.RegisterName("runningApplicationWithProcessIdentifier:"), int32(pid))
	if app == 0 {
		return fmt.Errorf("NSRunningApplication not found for pid %d", pid)
	}
	if hide {
		// NSApplicationActivateIgnoringOtherApps = 1 << 1
		_ = objc.Send[bool](app, objc.RegisterName("activateWithOptions:"), uint64(1<<1))
		if !objc.Send[bool](app, objc.RegisterName("hide")) {
			return fmt.Errorf("NSRunningApplication.hide(%d) returned false", pid)
		}
		return nil
	}
	if !objc.Send[bool](app, objc.RegisterName("unhide")) {
		return fmt.Errorf("NSRunningApplication.unhide(%d) returned false", pid)
	}
	return nil
}
