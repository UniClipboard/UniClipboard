//go:build linux

package quickpanel

import (
	"context"
	"encoding/binary"
	"errors"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"
	"unsafe"

	"github.com/ebitengine/purego"
)

const (
	// Linux evdev keycodes (input-event-codes.h) for base-layout Ctrl+V.
	linuxKeyLeftCtrl uint32 = 29
	linuxKeyV        uint32 = 47

	wlDisplayID uint32 = 1
	wlOpSync    uint16 = 0
	wlOpGetReg  uint16 = 1

	wlRegistryOpBind uint16 = 0
	wlRegistryEvGlob uint16 = 0

	wlCallbackEvDone uint16 = 0

	vkMgrOpCreate  uint16 = 0
	vkOpKeymap     uint16 = 0
	vkOpKey        uint16 = 1
	vkOpModifiers  uint16 = 2
	vkOpDestroy    uint16 = 3
	vkKeymapFormat uint32 = 1 // WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1

	vkModControl uint32 = 0x04
)

// minimalCtrlVXKBKeymap is the fallback XKB v1 keymap when libxkbcommon cannot
// be loaded dynamically. Prefer defaultWaylandXKBKeymap() which compiles a
// standard evdev/us keymap via libxkbcommon.so.0 so GTK/Qt/Chromium match
// Control+V bindings identically to the compositor's real seat keymap.
const minimalCtrlVXKBKeymap = `xkb_keymap {
  xkb_keycodes "unic_paste" {
    minimum = 8;
    maximum = 255;
    <LCTL> = 37;
    <LFSH> = 50;
    <AB04> = 55;
  };
  xkb_types "unic_paste" {
    type "ONE_LEVEL" {
      modifiers = none;
      level_name[Level1] = "Base";
    };
    type "TWO_LEVEL" {
      modifiers = Shift;
      map[Shift] = Level2;
      level_name[Level1] = "Base";
      level_name[Level2] = "Shift";
    };
  };
  xkb_compatibility "unic_paste" {
    interpret Any+AnyOf(all) { action = SetMods(modifiers=modMapMods); };
  };
  xkb_symbols "unic_paste" {
    key <LCTL> { type="ONE_LEVEL", [ Control_L ] };
    key <LFSH> { type="ONE_LEVEL", [ Shift_L ] };
    key <AB04> { type="TWO_LEVEL", [ v, V ] };
    modifier_map Shift { <LFSH> };
    modifier_map Control { <LCTL> };
  };
};
` + "\x00"

var (
	waylandKeymapOnce  sync.Once
	waylandKeymapBytes []byte
)

func defaultWaylandXKBKeymap() []byte {
	waylandKeymapOnce.Do(func() {
		if km := loadSystemXKBKeymap(); len(km) > 0 {
			waylandKeymapBytes = km
			return
		}
		waylandKeymapBytes = []byte(minimalCtrlVXKBKeymap)
	})
	return waylandKeymapBytes
}

func loadSystemXKBKeymap() []byte {
	libXKB, err := purego.Dlopen("libxkbcommon.so.0", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	if err != nil {
		return nil
	}
	var (
		xkbContextNew         func(uint32) uintptr
		xkbContextUnref       func(uintptr)
		xkbKeymapNewFromNames func(uintptr, uintptr, uint32) uintptr
		xkbKeymapGetAsString  func(uintptr, uint32) uintptr
		xkbKeymapUnref        func(uintptr)
		libcFree              func(uintptr)
	)
	purego.RegisterLibFunc(&xkbContextNew, libXKB, "xkb_context_new")
	purego.RegisterLibFunc(&xkbContextUnref, libXKB, "xkb_context_unref")
	purego.RegisterLibFunc(&xkbKeymapNewFromNames, libXKB, "xkb_keymap_new_from_names")
	purego.RegisterLibFunc(&xkbKeymapGetAsString, libXKB, "xkb_keymap_get_as_string")
	purego.RegisterLibFunc(&xkbKeymapUnref, libXKB, "xkb_keymap_unref")
	if libC, cerr := purego.Dlopen("libc.so.6", purego.RTLD_NOW); cerr == nil {
		purego.RegisterLibFunc(&libcFree, libC, "free")
	}

	ctx := xkbContextNew(0)
	if ctx == 0 {
		return nil
	}
	defer xkbContextUnref(ctx)

	km := xkbKeymapNewFromNames(ctx, 0, 0)
	if km == 0 {
		return nil
	}
	defer xkbKeymapUnref(km)

	ptr := xkbKeymapGetAsString(km, 1) // XKB_KEYMAP_FORMAT_TEXT_V1
	if ptr == 0 {
		return nil
	}
	if libcFree != nil {
		defer libcFree(ptr)
	}
	// Copy NUL-terminated C string up to 1 MiB safety cap.
	const maxKeymap = 1 << 20
	raw := unsafe.Slice((*byte)(unsafe.Pointer(ptr)), maxKeymap)
	for i, b := range raw {
		if b == 0 {
			out := make([]byte, i+1)
			copy(out, raw[:i+1])
			return out
		}
	}
	return nil
}

type waylandGlobal struct {
	name    uint32
	version uint32
}

type waylandVirtualKeyboardBackend struct {
	socketPath string
}

func waylandSocketPath() string {
	display := strings.TrimSpace(os.Getenv("WAYLAND_DISPLAY"))
	if display == "" {
		return ""
	}
	if filepath.IsAbs(display) {
		return display
	}
	runtimeDir := strings.TrimSpace(os.Getenv("XDG_RUNTIME_DIR"))
	if runtimeDir == "" {
		return ""
	}
	return filepath.Join(runtimeDir, display)
}

func probeWaylandVirtualKeyboard(socketPath string) bool {
	if socketPath == "" {
		return false
	}
	conn, err := dialWayland(socketPath)
	if err != nil {
		return false
	}
	defer conn.Close()
	globals, err := conn.discoverGlobals()
	if err != nil {
		return false
	}
	_, hasSeat := globals["wl_seat"]
	_, hasVK := globals["zwp_virtual_keyboard_manager_v1"]
	return hasSeat && hasVK
}

func (b *waylandVirtualKeyboardBackend) Prepare(context.Context) error {
	return nil
}

func (b *waylandVirtualKeyboardBackend) Close() {}

func (b *waylandVirtualKeyboardBackend) PasteCtrlV() error {
	conn, err := dialWayland(b.socketPath)
	if err != nil {
		return ErrInput
	}
	defer conn.Close()

	globals, err := conn.discoverGlobals()
	if err != nil {
		return ErrInput
	}
	seat, okSeat := globals["wl_seat"]
	mgr, okMgr := globals["zwp_virtual_keyboard_manager_v1"]
	if !okSeat || !okMgr {
		return ErrUnsupported
	}

	seatVer := min(seat.version, uint32(7))
	if seatVer == 0 {
		seatVer = 1
	}
	regID := uint32(2)
	seatID := conn.allocID()
	mgrID := conn.allocID()
	vkID := conn.allocID()
	if err := conn.bindGlobal(regID, seat.name, "wl_seat", seatVer, seatID); err != nil {
		return ErrInput
	}
	if err := conn.bindGlobal(regID, mgr.name, "zwp_virtual_keyboard_manager_v1", 1, mgrID); err != nil {
		return ErrInput
	}
	// zwp_virtual_keyboard_manager_v1.create_virtual_keyboard(seat, new_id)
	if err := conn.send(mgrID, vkMgrOpCreate, encodeU32s(seatID, vkID), nil); err != nil {
		return ErrInput
	}

	// Supply the XKB keymap via an unlinked private file descriptor over SCM_RIGHTS.
	keymapData := defaultWaylandXKBKeymap()
	keymapFile, err := createKeymapFD(keymapData)
	if err != nil {
		return ErrInput
	}
	defer keymapFile.Close()
	rights := syscall.UnixRights(int(keymapFile.Fd()))
	if err := conn.send(vkID, vkOpKeymap, encodeU32s(vkKeymapFormat, uint32(len(keymapData))), rights); err != nil {
		return ErrInput
	}
	if err := conn.roundtrip(); err != nil {
		return ErrInput
	}

	t := uint32(time.Now().UnixMilli() & 0x7fffffff)
	// Press LeftCtrl, assert Control modifier, press V, hold briefly, release V and LeftCtrl.
	if err := conn.send(vkID, vkOpKey, encodeU32s(t, linuxKeyLeftCtrl, 1), nil); err != nil {
		return ErrInput
	}
	if err := conn.send(vkID, vkOpModifiers, encodeU32s(vkModControl, 0, 0, 0), nil); err != nil {
		return ErrInput
	}
	if err := conn.roundtrip(); err != nil {
		return ErrInput
	}
	if err := conn.send(vkID, vkOpKey, encodeU32s(t+5, linuxKeyV, 1), nil); err != nil {
		return ErrInput
	}
	if err := conn.roundtrip(); err != nil {
		_ = conn.releaseCtrlV(vkID, t+10)
		return ErrInput
	}
	time.Sleep(15 * time.Millisecond)
	if err := conn.releaseCtrlV(vkID, t+20); err != nil {
		return ErrInput
	}
	_ = conn.send(vkID, vkOpDestroy, nil, nil)
	_ = conn.roundtrip()
	return nil
}

func (c *waylandWireConn) releaseCtrlV(vkID, t uint32) error {
	if err := c.send(vkID, vkOpKey, encodeU32s(t, linuxKeyV, 0), nil); err != nil {
		return err
	}
	if err := c.send(vkID, vkOpKey, encodeU32s(t+5, linuxKeyLeftCtrl, 0), nil); err != nil {
		return err
	}
	if err := c.send(vkID, vkOpModifiers, encodeU32s(0, 0, 0, 0), nil); err != nil {
		return err
	}
	return c.roundtrip()
}

func createKeymapFD(data []byte) (*os.File, error) {
	f, err := os.CreateTemp("", "unic-vk-keymap-*")
	if err != nil {
		return nil, err
	}
	_ = os.Remove(f.Name())
	if _, err := f.Write(data); err != nil {
		_ = f.Close()
		return nil, err
	}
	if _, err := f.Seek(0, 0); err != nil {
		_ = f.Close()
		return nil, err
	}
	return f, nil
}

type waylandWireConn struct {
	uc     *net.UnixConn
	nextID uint32
	buf    []byte
}

func dialWayland(path string) (*waylandWireConn, error) {
	conn, err := net.DialTimeout("unix", path, time.Second)
	if err != nil {
		return nil, err
	}
	uc, ok := conn.(*net.UnixConn)
	if !ok {
		_ = conn.Close()
		return nil, errors.New("wayland: non-unix socket")
	}
	_ = uc.SetDeadline(time.Now().Add(2 * time.Second))
	return &waylandWireConn{uc: uc, nextID: 2}, nil
}

func (c *waylandWireConn) allocID() uint32 {
	id := c.nextID
	c.nextID++
	return id
}

func (c *waylandWireConn) Close() {
	if c.uc != nil {
		_ = c.uc.Close()
	}
}

func (c *waylandWireConn) send(objectID uint32, opcode uint16, payload []byte, oob []byte) error {
	size := 8 + len(payload)
	if size > 0xffff {
		return errors.New("wayland: message too large")
	}
	msg := make([]byte, size)
	binary.LittleEndian.PutUint32(msg[0:4], objectID)
	binary.LittleEndian.PutUint32(msg[4:8], (uint32(size)<<16)|uint32(opcode))
	copy(msg[8:], payload)
	n, oobn, err := c.uc.WriteMsgUnix(msg, oob, nil)
	if err != nil {
		return err
	}
	if n != len(msg) || oobn != len(oob) {
		return errors.New("wayland: short write")
	}
	return nil
}

func (c *waylandWireConn) bindGlobal(regID, name uint32, iface string, version, newID uint32) error {
	s := encodeWaylandString(iface)
	payload := make([]byte, 4+len(s)+8)
	binary.LittleEndian.PutUint32(payload[0:4], name)
	copy(payload[4:], s)
	off := 4 + len(s)
	binary.LittleEndian.PutUint32(payload[off:off+4], version)
	binary.LittleEndian.PutUint32(payload[off+4:off+8], newID)
	return c.send(regID, wlRegistryOpBind, payload, nil)
}

func (c *waylandWireConn) discoverGlobals() (map[string]waylandGlobal, error) {
	regID := c.allocID()
	syncID := c.allocID()
	if err := c.send(wlDisplayID, wlOpGetReg, encodeU32s(regID), nil); err != nil {
		return nil, err
	}
	if err := c.send(wlDisplayID, wlOpSync, encodeU32s(syncID), nil); err != nil {
		return nil, err
	}
	globals := map[string]waylandGlobal{}
	err := c.readUntilCallback(syncID, func(oid uint32, opcode uint16, payload []byte) {
		if oid == regID && opcode == wlRegistryEvGlob {
			if name, iface, ver, ok := decodeGlobalEvent(payload); ok {
				globals[iface] = waylandGlobal{name: name, version: ver}
			}
		}
	})
	return globals, err
}

func (c *waylandWireConn) roundtrip() error {
	cbID := c.nextID
	c.nextID++
	if err := c.send(wlDisplayID, wlOpSync, encodeU32s(cbID), nil); err != nil {
		return err
	}
	return c.readUntilCallback(cbID, nil)
}

func (c *waylandWireConn) readUntilCallback(cbID uint32, onEvent func(uint32, uint16, []byte)) error {
	tmp := make([]byte, 4096)
	oob := make([]byte, 256)
	for {
		for len(c.buf) >= 8 {
			oid := binary.LittleEndian.Uint32(c.buf[0:4])
			szOp := binary.LittleEndian.Uint32(c.buf[4:8])
			size := int(szOp >> 16)
			opcode := uint16(szOp & 0xffff)
			if size < 8 {
				return errors.New("wayland: invalid message size")
			}
			if len(c.buf) < size {
				break
			}
			payload := c.buf[8:size]
			c.buf = c.buf[size:]
			if oid == wlDisplayID && opcode == 0 {
				return errors.New("wayland: protocol error")
			}
			if oid == cbID && opcode == wlCallbackEvDone {
				return nil
			}
			if onEvent != nil {
				onEvent(oid, opcode, payload)
			}
		}
		n, _, _, _, err := c.uc.ReadMsgUnix(tmp, oob)
		if err != nil {
			return err
		}
		if n == 0 {
			return errors.New("wayland: eof")
		}
		c.buf = append(c.buf, tmp[:n]...)
	}
}

func encodeU32s(vals ...uint32) []byte {
	out := make([]byte, 4*len(vals))
	for i, v := range vals {
		binary.LittleEndian.PutUint32(out[4*i:4*i+4], v)
	}
	return out
}

func encodeWaylandString(s string) []byte {
	n := len(s) + 1
	padded := (n + 3) &^ 3
	out := make([]byte, 4+padded)
	binary.LittleEndian.PutUint32(out[0:4], uint32(n))
	copy(out[4:], s)
	return out
}

func decodeGlobalEvent(payload []byte) (name uint32, iface string, version uint32, ok bool) {
	if len(payload) < 12 {
		return 0, "", 0, false
	}
	name = binary.LittleEndian.Uint32(payload[0:4])
	slen := int(binary.LittleEndian.Uint32(payload[4:8]))
	if slen <= 0 {
		return 0, "", 0, false
	}
	padded := (slen + 3) &^ 3
	if 8+padded+4 > len(payload) {
		return 0, "", 0, false
	}
	iface = strings.TrimRight(string(payload[8:8+slen]), "\x00")
	version = binary.LittleEndian.Uint32(payload[8+padded : 12+padded])
	return name, iface, version, true
}
