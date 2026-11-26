//go:build linux

package quickpanel

import (
	"context"
	"os"
	"sync"
	"syscall"
	"time"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/uniclipboard/unic/internal/waylandwire"
)

const (
	// Linux evdev keycodes (input-event-codes.h) for base-layout Ctrl+V.
	linuxKeyLeftCtrl uint32 = 29
	linuxKeyV        uint32 = 47

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
		xkbKeymapGetAsString  func(uintptr, uint32) *byte
		xkbKeymapUnref        func(uintptr)
		libcFree              func(*byte)
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
	if ptr == nil {
		return nil
	}
	if libcFree != nil {
		defer libcFree(ptr)
	}
	// Copy NUL-terminated C string up to 1 MiB safety cap.
	const maxKeymap = 1 << 20
	raw := unsafe.Slice(ptr, maxKeymap)
	for i, b := range raw {
		if b == 0 {
			out := make([]byte, i+1)
			copy(out, raw[:i+1])
			return out
		}
	}
	return nil
}

type waylandVirtualKeyboardBackend struct {
	socketPath string
}

func probeWaylandVirtualKeyboard(socketPath string) bool {
	if socketPath == "" {
		return false
	}
	conn, err := waylandwire.Dial(socketPath)
	if err != nil {
		return false
	}
	defer conn.Close()
	globals, err := conn.DiscoverGlobals()
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
	conn, err := waylandwire.Dial(b.socketPath)
	if err != nil {
		return ErrInput
	}
	defer conn.Close()

	globals, err := conn.DiscoverGlobals()
	if err != nil {
		return ErrInput
	}
	seat, okSeat := globals["wl_seat"]
	mgr, okMgr := globals["zwp_virtual_keyboard_manager_v1"]
	if !okSeat || !okMgr {
		return ErrUnsupported
	}

	seatVer := min(seat.Version, uint32(7))
	if seatVer == 0 {
		seatVer = 1
	}
	regID := uint32(2)
	seatID := conn.AllocateID()
	mgrID := conn.AllocateID()
	vkID := conn.AllocateID()
	if err := conn.BindGlobal(regID, seat.Name, "wl_seat", seatVer, seatID); err != nil {
		return ErrInput
	}
	if err := conn.BindGlobal(regID, mgr.Name, "zwp_virtual_keyboard_manager_v1", 1, mgrID); err != nil {
		return ErrInput
	}
	// zwp_virtual_keyboard_manager_v1.create_virtual_keyboard(seat, new_id)
	if err := conn.Send(mgrID, vkMgrOpCreate, waylandwire.EncodeU32s(seatID, vkID), nil); err != nil {
		return ErrInput
	}

	// Supply the XKB keymap via an unlinked private file descriptor over SCM_RIGHTS.
	keymapData := defaultWaylandXKBKeymap()
	keymapFile, err := createKeymapFD(keymapData)
	if err != nil {
		return ErrInput
	}
	defer func() { _ = keymapFile.Close() }()
	rights := syscall.UnixRights(int(keymapFile.Fd()))
	if err := conn.Send(vkID, vkOpKeymap, waylandwire.EncodeU32s(vkKeymapFormat, uint32(len(keymapData))), rights); err != nil {
		return ErrInput
	}
	if err := conn.Roundtrip(); err != nil {
		return ErrInput
	}

	t := uint32(time.Now().UnixMilli() & 0x7fffffff)
	// Press LeftCtrl, assert Control modifier, press V, hold briefly, release V and LeftCtrl.
	if err := conn.Send(vkID, vkOpKey, waylandwire.EncodeU32s(t, linuxKeyLeftCtrl, 1), nil); err != nil {
		return ErrInput
	}
	if err := conn.Send(vkID, vkOpModifiers, waylandwire.EncodeU32s(vkModControl, 0, 0, 0), nil); err != nil {
		return ErrInput
	}
	if err := conn.Roundtrip(); err != nil {
		return ErrInput
	}
	if err := conn.Send(vkID, vkOpKey, waylandwire.EncodeU32s(t+5, linuxKeyV, 1), nil); err != nil {
		return ErrInput
	}
	if err := conn.Roundtrip(); err != nil {
		_ = releaseCtrlV(conn, vkID, t+10)
		return ErrInput
	}
	time.Sleep(15 * time.Millisecond)
	if err := releaseCtrlV(conn, vkID, t+20); err != nil {
		return ErrInput
	}
	_ = conn.Send(vkID, vkOpDestroy, nil, nil)
	_ = conn.Roundtrip()
	return nil
}

func releaseCtrlV(c *waylandwire.Conn, vkID, t uint32) error {
	if err := c.Send(vkID, vkOpKey, waylandwire.EncodeU32s(t, linuxKeyV, 0), nil); err != nil {
		return err
	}
	if err := c.Send(vkID, vkOpKey, waylandwire.EncodeU32s(t+5, linuxKeyLeftCtrl, 0), nil); err != nil {
		return err
	}
	if err := c.Send(vkID, vkOpModifiers, waylandwire.EncodeU32s(0, 0, 0, 0), nil); err != nil {
		return err
	}
	return c.Roundtrip()
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
