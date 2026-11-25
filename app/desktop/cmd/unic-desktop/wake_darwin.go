//go:build darwin

package main

import (
	"context"
	"sync"
	"time"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"

	api "github.com/uniclipboard/unic/internal/localapi"
)

var loadWakeAppKit = sync.OnceValue(func() bool {
	_, err := purego.Dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	return err == nil
})

// watchSystemWake subscribes to macOS NSWorkspaceDidWakeNotification on the
// existing AppKit main loop and notifies unicd so peer links warm up or heal
// immediately after deep sleep instead of waiting for the first clipboard copy.
func watchSystemWake(ctx context.Context, socket string) func() {
	if !loadWakeAppKit() {
		return func() {}
	}
	ws := objc.ID(objc.GetClass("NSWorkspace")).Send(objc.RegisterName("sharedWorkspace"))
	if ws == 0 {
		return func() {}
	}
	nc := ws.Send(objc.RegisterName("notificationCenter"))
	if nc == 0 {
		return func() {}
	}
	name := objc.ID(objc.GetClass("NSString")).Send(objc.RegisterName("stringWithUTF8String:"), "NSWorkspaceDidWakeNotification")
	client := api.NewClient(socket)
	block := objc.NewBlock(func(_ objc.Block, _ objc.ID) {
		go func() {
			nctx, cancel := context.WithTimeout(ctx, 5*time.Second)
			defer cancel()
			_ = client.NotifyNetworkChanged(nctx)
		}()
	})
	observer := nc.Send(
		objc.RegisterName("addObserverForName:object:queue:usingBlock:"),
		name,
		objc.ID(0),
		objc.ID(0),
		block,
	)
	var once sync.Once
	return func() {
		once.Do(func() {
			if observer != 0 {
				nc.Send(objc.RegisterName("removeObserver:"), observer)
			}
			block.Release()
		})
	}
}
