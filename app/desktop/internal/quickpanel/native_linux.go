//go:build linux

package quickpanel

import (
	"os"
	"strconv"
	"strings"

	"github.com/jezek/xgb"
	"github.com/jezek/xgb/xproto"
	"github.com/jezek/xgb/xtest"
)

type x11Platform struct {
	conn        *xgb.Conn
	root        xproto.Window
	active, pid xproto.Atom
	control, v  byte
}

func NewNative() Platform {
	// XWayland must not be mistaken for authority over native Wayland windows.
	if os.Getenv("WAYLAND_DISPLAY") != "" || strings.EqualFold(os.Getenv("XDG_SESSION_TYPE"), "wayland") {
		return unsupportedPlatform{}
	}
	display := os.Getenv("DISPLAY")
	if !strings.HasPrefix(display, ":") && !strings.HasPrefix(display, "unix:") {
		return unsupportedPlatform{}
	}
	conn, err := xgb.NewConn()
	if err != nil {
		return unsupportedPlatform{}
	}
	fail := func() Platform { conn.Close(); return unsupportedPlatform{} }
	if xtest.Init(conn) != nil {
		return fail()
	}
	p := &x11Platform{conn: conn, root: xproto.Setup(conn).DefaultScreen(conn).Root}
	for name, dest := range map[string]*xproto.Atom{"_NET_ACTIVE_WINDOW": &p.active, "_NET_WM_PID": &p.pid} {
		out, err := xproto.InternAtom(conn, true, uint16(len(name)), name).Reply()
		if err != nil || out.Atom == 0 {
			return fail()
		}
		*dest = out.Atom
	}
	setup := xproto.Setup(conn)
	count := byte(int(setup.MaxKeycode) - int(setup.MinKeycode) + 1)
	mapping, err := xproto.GetKeyboardMapping(conn, setup.MinKeycode, count).Reply()
	if err != nil || mapping.KeysymsPerKeycode == 0 {
		return fail()
	}
	// Use base-layout keys. Never synthesize extra layout-switch modifiers.
	for i := 0; i < int(count); i++ {
		sym := mapping.Keysyms[i*int(mapping.KeysymsPerKeycode)]
		if sym == 0xffe3 {
			p.control = byte(setup.MinKeycode) + byte(i)
		}
		if sym == 'v' {
			p.v = byte(setup.MinKeycode) + byte(i)
		}
	}
	if p.control == 0 || p.v == 0 {
		return fail()
	}
	return p
}
func (p *x11Platform) Close() { p.conn.Close() }
func (p *x11Platform) property(window xproto.Window, atom xproto.Atom) (uint32, error) {
	out, err := xproto.GetProperty(p.conn, false, window, atom, xproto.AtomAny, 0, 1).Reply()
	if err != nil || out == nil || out.Format != 32 || len(out.Value) != 4 {
		return 0, ErrTarget
	}
	return xgb.Get32(out.Value), nil
}
func processBirth(pid int) uint64 {
	b, err := os.ReadFile("/proc/" + strconv.Itoa(pid) + "/stat")
	if err != nil {
		return 0
	}
	text := string(b)
	end := strings.LastIndexByte(text, ')')
	if end < 0 {
		return 0
	}
	fields := strings.Fields(text[end+1:])
	if len(fields) <= 19 {
		return 0
	}
	born, _ := strconv.ParseUint(fields[19], 10, 64)
	return born
}
func (p *x11Platform) foreground() (xproto.Window, int, error) {
	window, err := p.property(p.root, p.active)
	if err != nil || window == 0 {
		return 0, 0, ErrTarget
	}
	pid, err := p.property(xproto.Window(window), p.pid)
	return xproto.Window(window), int(pid), err
}
func (p *x11Platform) Capture() (Target, error) {
	window, pid, err := p.foreground()
	born := processBirth(pid)
	if err != nil || pid == 0 || pid == os.Getpid() || born == 0 {
		return Target{}, ErrTarget
	}
	return Target{ID: uintptr(window), PID: pid, Born: born, Name: "原应用"}, nil
}
func (p *x11Platform) Check(t Target) error {
	pid, err := p.property(xproto.Window(t.ID), p.pid)
	if err != nil || int(pid) != t.PID || t.PID == os.Getpid() || t.Born == 0 || processBirth(t.PID) != t.Born {
		return ErrTarget
	}
	return nil
}
func (p *x11Platform) OwnsForeground() bool {
	_, pid, err := p.foreground()
	return err == nil && pid == os.Getpid()
}
func (p *x11Platform) Activate(t Target) error {
	if err := p.Check(t); err != nil {
		return err
	}
	current, _, _ := p.foreground()
	event := xproto.ClientMessageEvent{Format: 32, Window: xproto.Window(t.ID), Type: p.active, Data: xproto.ClientMessageDataUnionData32New([]uint32{2, uint32(xproto.TimeCurrentTime), uint32(current), 0, 0})}
	if xproto.SendEventChecked(p.conn, false, p.root, xproto.EventMaskSubstructureRedirect|xproto.EventMaskSubstructureNotify, string(event.Bytes())).Check() != nil {
		return ErrFocus
	}
	return nil
}
func (p *x11Platform) Ready(t Target) (bool, error) {
	if err := p.Check(t); err != nil {
		return false, err
	}
	window, pid, err := p.foreground()
	if err != nil {
		return false, ErrFocus
	}
	if uintptr(window) != t.ID {
		if pid != os.Getpid() {
			return false, ErrFocus
		}
		return false, nil
	}
	focus, err := xproto.GetInputFocus(p.conn).Reply()
	if err != nil {
		return false, ErrFocus
	}
	// A focused widget is allowed only inside the captured client window.
	at := focus.Focus
	for depth := 0; at != window && depth < 64; depth++ {
		if at == 0 || at == p.root || at == 1 {
			return false, ErrFocus
		}
		tree, e := xproto.QueryTree(p.conn, at).Reply()
		if e != nil || tree.Parent == at {
			return false, ErrFocus
		}
		at = tree.Parent
	}
	if at != window {
		return false, ErrFocus
	}
	keys, err := xproto.QueryKeymap(p.conn).Reply()
	if err != nil {
		return false, ErrInput
	}
	for _, b := range keys.Keys {
		if b != 0 {
			return false, nil
		}
	}
	return true, nil
}
func (p *x11Platform) Paste(t Target) error {
	ready, err := p.Ready(t)
	if err != nil {
		return err
	}
	if !ready {
		return ErrFocus
	}
	for i, key := range []byte{p.control, p.v, p.v, p.control} {
		kind := byte(xproto.KeyPress)
		if i >= 2 {
			kind = xproto.KeyRelease
		}
		if xtest.FakeInputChecked(p.conn, kind, key, 0, p.root, 0, 0, 0).Check() != nil {
			_ = xtest.FakeInputChecked(p.conn, xproto.KeyRelease, p.v, 0, p.root, 0, 0, 0).Check()
			_ = xtest.FakeInputChecked(p.conn, xproto.KeyRelease, p.control, 0, p.root, 0, 0, 0).Check()
			return ErrInput
		}
	}
	return nil
}
