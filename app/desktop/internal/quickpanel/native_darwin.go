//go:build darwin

package quickpanel

import (
	"math"
	"os"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

type macPlatform struct {
	trusted  func() bool
	secure   func() bool
	flags    func(int32) uint64
	keyDown  func(int32, uint16) bool
	create   func(uintptr, uint16, bool) uintptr
	setFlags func(uintptr, uint64)
	post     func(int32, uintptr)
	release  func(uintptr)
}

// NewNative creates the host adapter without requesting permissions or showing UI.
func NewNative() Platform {
	p := &macPlatform{}
	for _, lib := range []struct {
		path    string
		symbols map[string]any
	}{
		{"/System/Library/Frameworks/AppKit.framework/AppKit", nil},
		{"/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices", map[string]any{"AXIsProcessTrusted": &p.trusted}},
		{"/System/Library/Frameworks/Carbon.framework/Carbon", map[string]any{"IsSecureEventInputEnabled": &p.secure}},
		{"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics", map[string]any{"CGEventSourceFlagsState": &p.flags, "CGEventSourceKeyState": &p.keyDown, "CGEventCreateKeyboardEvent": &p.create, "CGEventSetFlags": &p.setFlags, "CGEventPostToPid": &p.post}},
		{"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", map[string]any{"CFRelease": &p.release}},
	} {
		handle, err := purego.Dlopen(lib.path, purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			return unsupportedPlatform{}
		}
		for name, dest := range lib.symbols {
			symbol, err := purego.Dlsym(handle, name)
			if err != nil {
				return unsupportedPlatform{}
			}
			purego.RegisterFunc(dest, symbol)
		}
	}
	return p
}
func macSend(id objc.ID, sel string, args ...any) objc.ID {
	return id.Send(objc.RegisterName(sel), args...)
}
func macApp(pid int) objc.ID {
	return macSend(objc.ID(objc.GetClass("NSRunningApplication")), "runningApplicationWithProcessIdentifier:", int32(pid))
}
func macFront() objc.ID {
	return macSend(macSend(objc.ID(objc.GetClass("NSWorkspace")), "sharedWorkspace"), "frontmostApplication")
}
func macIdentity(app objc.ID) (int, uint64) {
	if app == 0 || objc.Send[bool](app, objc.RegisterName("isTerminated")) {
		return 0, 0
	}
	pid := objc.Send[int32](app, objc.RegisterName("processIdentifier"))
	date := macSend(app, "launchDate")
	if date == 0 {
		return 0, 0
	}
	born := objc.Send[float64](date, objc.RegisterName("timeIntervalSince1970"))
	return int(pid), math.Float64bits(born)
}
func (p *macPlatform) Capture() (Target, error) {
	app := macFront()
	pid, born := macIdentity(app)
	if pid <= 0 || pid == os.Getpid() || born == 0 {
		return Target{}, ErrTarget
	}
	name := macSend(app, "localizedName")
	ptr := objc.Send[*byte](name, objc.RegisterName("UTF8String"))
	label := "原应用"
	if ptr != nil {
		var b []byte
		for i := uintptr(0); i < 4096; i++ {
			v := *(*byte)(unsafe.Add(unsafe.Pointer(ptr), i))
			if v == 0 {
				break
			}
			b = append(b, v)
		}
		if len(b) > 0 {
			label = string(b)
		}
	}
	return Target{ID: uintptr(pid), PID: pid, Born: born, Name: label}, nil
}
func (p *macPlatform) valid(t Target) bool {
	pid, born := macIdentity(macApp(t.PID))
	return pid == t.PID && pid != os.Getpid() && born == t.Born && t.Born != 0
}
func (p *macPlatform) Check(t Target) error {
	if !p.valid(t) {
		return ErrTarget
	}
	if p.secure() {
		return ErrProtected
	}
	if !p.trusted() {
		return ErrPermission
	}
	return nil
}
func (p *macPlatform) Activate(t Target) error {
	if !p.valid(t) {
		return ErrTarget
	}
	if !objc.Send[bool](macApp(t.PID), objc.RegisterName("activateWithOptions:"), uint64(1<<1)) {
		return ErrFocus
	}
	return nil
}
func (p *macPlatform) OwnsForeground() bool {
	pid, _ := macIdentity(macFront())
	return pid == os.Getpid()
}
func (p *macPlatform) Ready(t Target) (bool, error) {
	if err := p.Check(t); err != nil {
		return false, err
	}
	pid, born := macIdentity(macFront())
	if pid != t.PID || born != t.Born {
		if pid != os.Getpid() {
			return false, ErrFocus
		}
		return false, nil
	}
	// Never synthesize a chord while modifiers or the triggering keys remain held.
	if p.flags(1)&((1<<17)|(1<<18)|(1<<19)|(1<<20)|(1<<23)) != 0 {
		return false, nil
	}
	for _, key := range []uint16{9, 36, 76, 18, 19, 20, 21, 23, 22, 26, 28, 25} {
		if p.keyDown(1, key) {
			return false, nil
		}
	}
	return true, nil
}
func (p *macPlatform) Paste(t Target) error {
	ready, err := p.Ready(t)
	if err != nil {
		return err
	}
	if !ready {
		return ErrFocus
	}
	down, up := p.create(0, 9, true), p.create(0, 9, false)
	if down == 0 || up == 0 {
		if down != 0 {
			p.release(down)
		}
		if up != 0 {
			p.release(up)
		}
		return ErrInput
	}
	defer p.release(down)
	defer p.release(up)
	p.setFlags(down, 1<<20)
	p.setFlags(up, 1<<20)
	p.post(int32(t.PID), down)
	p.post(int32(t.PID), up)
	return nil
}
