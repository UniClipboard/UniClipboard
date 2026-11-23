package pairnotice

import (
	"context"
	"crypto/rand"
	"errors"
	"runtime"
	"strings"
	"sync"

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

func Show(ctx context.Context, title, body string) error {
	if !Available() {
		return errors.New("notification center is unavailable")
	}
	centerClass := class("UNUserNotificationCenter")
	authorized := make(chan bool, 1)
	withPool(func() {
		center := send(centerClass, "currentNotificationCenter")
		cb := objc.NewBlock(func(_ objc.Block, granted bool, _ objc.ID) { authorized <- granted })
		defer cb.Release()
		send(center, "requestAuthorizationWithOptions:completionHandler:", uint64(1<<1|1<<2), cb)
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
