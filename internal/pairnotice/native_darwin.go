//go:build darwin

package pairnotice

import (
	"context"
	"crypto/rand"
	"errors"
	"os/exec"
	"runtime"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"

	"github.com/uniclipboard/unic/internal/nativeboard"
)

var loadNotifications = sync.OnceValue(func() bool {
	_, err := purego.Dlopen("/System/Library/Frameworks/UserNotifications.framework/UserNotifications", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	return err == nil
})

func send(receiver objc.ID, selector string, args ...any) objc.ID {
	return receiver.Send(objc.RegisterName(selector), args...)
}

func class(name string) objc.ID { return objc.ID(objc.GetClass(name)) }

func withPool(work func()) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := send(send(class("NSAutoreleasePool"), "alloc"), "init")
	defer send(pool, "drain")
	work()
}

func callObjCBlock(block uintptr, args ...uintptr) {
	lit := *(**[3]uintptr)(unsafe.Pointer(&block))
	var a [8]uintptr
	a[0] = block
	n := copy(a[1:], args)
	purego.SyscallN(lit[2], a[:n+1]...)
}

var (
	delegateOnce sync.Once
	delegateObj  objc.ID
)

// ensureDelegate attaches a UNUserNotificationCenterDelegate so macOS still
// presents banners when the app bundle is active in the foreground.
func ensureDelegate() {
	delegateOnce.Do(func() {
		if objc.GetClass("UNUserNotificationCenter") == 0 {
			return
		}
		cls, err := objc.RegisterClass(
			"UnicPairNoticeDelegate",
			objc.GetClass("NSObject"),
			[]*objc.Protocol{objc.GetProtocol("UNUserNotificationCenterDelegate")},
			nil,
			[]objc.MethodDef{
				{
					Cmd: objc.RegisterName("userNotificationCenter:willPresentNotification:withCompletionHandler:"),
					Fn: func(self objc.ID, _ objc.SEL, center, n objc.ID, handler uintptr) {
						// UNNotificationPresentationOptionSound (1<<1) | List (1<<3) | Banner (1<<4)
						callObjCBlock(handler, 1<<1|1<<3|1<<4)
					},
				},
			},
		)
		if err != nil || cls == 0 {
			return
		}
		delegateObj = send(send(objc.ID(cls), "alloc"), "init")
		center := send(class("UNUserNotificationCenter"), "currentNotificationCenter")
		send(center, "setDelegate:", delegateObj)
	})
}

func Available() bool {
	if !nativeboard.ActiveConsoleUser() || !loadNotifications() {
		return false
	}
	supported := false
	withPool(func() {
		bundle := send(class("NSBundle"), "mainBundle")
		path := send(bundle, "bundlePath")
		appSuffix := send(class("NSString"), "stringWithUTF8String:", ".app")
		supported = bundle != 0 && send(bundle, "bundleIdentifier") != 0 &&
			objc.Send[bool](path, objc.RegisterName("hasSuffix:"), appSuffix)
	})
	return supported
}

// PermissionStatus passively reads the macOS notification authorization state
// without prompting the user. Returns one of granted, denied, not_determined,
// unsupported or unknown.
func PermissionStatus(ctx context.Context) string {
	if !Available() {
		return "unsupported"
	}
	ch := make(chan int64, 1)
	withPool(func() {
		center := send(class("UNUserNotificationCenter"), "currentNotificationCenter")
		cb := objc.NewBlock(func(_ objc.Block, settings objc.ID) {
			if settings == 0 {
				ch <- -1
				return
			}
			status := objc.Send[int64](settings, objc.RegisterName("authorizationStatus"))
			ch <- status
		})
		defer cb.Release()
		send(center, "getNotificationSettingsWithCompletionHandler:", cb)
	})
	select {
	case <-ctx.Done():
		return "unknown"
	case st := <-ch:
		switch st {
		case 0: // UNAuthorizationStatusNotDetermined
			return "not_determined"
		case 1: // UNAuthorizationStatusDenied
			return "denied"
		case 2, 3, 4: // Authorized, Provisional, Ephemeral
			return "granted"
		default:
			return "unknown"
		}
	}
}

// OpenSettings opens macOS System Settings directly to the Notifications pane.
func OpenSettings(ctx context.Context) error {
	return exec.CommandContext(ctx, "open", "x-apple.systempreferences:com.apple.Notifications-Settings.extension").Run()
}

func Show(ctx context.Context, title, body string) error {
	if !Available() {
		return errors.New("notification center is unavailable")
	}
	centerClass := class("UNUserNotificationCenter")
	authorized := make(chan bool, 1)
	withPool(func() {
		ensureDelegate()
		center := send(centerClass, "currentNotificationCenter")
		cb := objc.NewBlock(func(_ objc.Block, granted bool, _ objc.ID) { authorized <- granted })
		defer cb.Release()
		send(center, "requestAuthorizationWithOptions:completionHandler:", uint64(1<<0|1<<1|1<<2), cb)
	})
	select {
	case <-ctx.Done():
		return ctx.Err()
	case granted := <-authorized:
		if !granted {
			return errors.New("notifications are not authorized")
		}
	}
	if !nativeboard.ActiveConsoleUser() {
		return errors.New("desktop session is unavailable")
	}
	done := make(chan bool, 1)
	withPool(func() {
		center := send(centerClass, "currentNotificationCenter")
		str := func(s string) objc.ID {
			return send(class("NSString"), "stringWithUTF8String:", strings.ReplaceAll(s, "\x00", ""))
		}
		content := send(send(class("UNMutableNotificationContent"), "alloc"), "init")
		defer send(content, "release")
		send(content, "setTitle:", str(title))
		send(content, "setBody:", str(body))
		send(content, "setSound:", send(class("UNNotificationSound"), "defaultSound"))
		request := send(class("UNNotificationRequest"), "requestWithIdentifier:content:trigger:", str(rand.Text()), content, objc.ID(0))
		cb := objc.NewBlock(func(_ objc.Block, err objc.ID) { done <- err == 0 })
		defer cb.Release()
		send(center, "addNotificationRequest:withCompletionHandler:", request, cb)
	})
	select {
	case <-ctx.Done():
		return ctx.Err()
	case ok := <-done:
		if !ok {
			return errors.New("notification request failed")
		}
	}
	return nil
}
