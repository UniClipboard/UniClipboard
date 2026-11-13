//go:build darwin

package board

import (
	"os"
	"runtime"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"

	"github.com/uniclipboard/unic/sourceapp"
)

var appKit = sync.OnceValue(func() bool {
	_, err := purego.Dlopen("/System/Library/Frameworks/AppKit.framework/AppKit", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	return err == nil
})

func send(id objc.ID, sel string, args ...any) objc.ID {
	return id.Send(objc.RegisterName(sel), args...)
}
func nsString(s string) objc.ID {
	return send(objc.ID(objc.GetClass("NSString")), "stringWithUTF8String:", s)
}
func goString(s objc.ID) string {
	if s == 0 {
		return ""
	}
	p := objc.Send[*byte](s, objc.RegisterName("UTF8String"))
	if p == nil {
		return ""
	}
	b := make([]byte, 0, 64)
	for i := 0; i < 1024; i++ {
		c := *(*byte)(unsafe.Add(unsafe.Pointer(p), i))
		if c == 0 {
			return string(b)
		}
		b = append(b, c)
	}
	return ""
}
func pasteboard() objc.ID { return send(objc.ID(objc.GetClass("NSPasteboard")), "generalPasteboard") }
func nativeVersion() uint64 {
	if !appKit() {
		return 0
	}
	return uint64(objc.Send[int64](pasteboard(), objc.RegisterName("changeCount")))
}
func nativeSource() sourceState {
	if !appKit() {
		return sourceState{}
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := send(send(objc.ID(objc.GetClass("NSAutoreleasePool")), "alloc"), "init")
	defer send(pool, "drain")
	state := sourceState{token: nativeVersion()}
	ws := send(objc.ID(objc.GetClass("NSWorkspace")), "sharedWorkspace")
	marker := send(pasteboard(), "stringForType:", nsString("org.nspasteboard.source"))
	id := goString(marker)
	var app objc.ID
	attribution := "source_marker"
	if id != "" {
		apps := send(objc.ID(objc.GetClass("NSRunningApplication")), "runningApplicationsWithBundleIdentifier:", marker)
		if objc.Send[uint64](apps, objc.RegisterName("count")) > 0 {
			app = send(apps, "objectAtIndex:", uint64(0))
		}
	} else {
		app = send(ws, "frontmostApplication")
		id = goString(send(app, "bundleIdentifier"))
		attribution = "foreground"
		if app != 0 && objc.Send[int32](app, objc.RegisterName("processIdentifier")) == int32(os.Getpid()) {
			return state
		}
	}
	a := &sourceapp.Application{Platform: "darwin", Identifier: id, Name: goString(send(app, "localizedName")), Attribution: attribution}
	if a.Validate() != nil {
		return state
	}
	a.IconPNG = cachedIcon("darwin:"+id, func() []byte {
		icon := send(app, "icon")
		if icon == 0 {
			url := send(ws, "URLForApplicationWithBundleIdentifier:", nsString(id))
			path := send(url, "path")
			if path != 0 {
				icon = send(ws, "iconForFile:", path)
			}
		}
		if icon == 0 {
			return nil
		}
		tiff := send(icon, "TIFFRepresentation")
		rep := send(objc.ID(objc.GetClass("NSBitmapImageRep")), "imageRepWithData:", tiff)
		if rep == 0 {
			return nil
		}
		data := send(rep, "representationUsingType:properties:", uint64(4), send(objc.ID(objc.GetClass("NSDictionary")), "dictionary"))
		n := objc.Send[uint64](data, objc.RegisterName("length"))
		if n == 0 || n > 4<<20 {
			return nil
		}
		p := objc.Send[unsafe.Pointer](data, objc.RegisterName("bytes"))
		if p == nil {
			return nil
		}
		return smallPNG(unsafe.Slice((*byte)(p), int(n)))
	})
	state.app = a
	return state
}
