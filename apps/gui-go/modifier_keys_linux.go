//go:build linux

package main

/*
#cgo pkg-config: x11
#include <X11/Xlib.h>
#include <X11/keysym.h>
#include <stdlib.h>

static Display *uc_open_display(void) { return XOpenDisplay(NULL); }

// uc_selected_keycodes writes the keycodes of the selected modifier family (0 alt, 1 control, 2 meta) into out.
static int uc_selected_keycodes(Display *d, int family, unsigned char *out) {
	KeySym syms[4];
	int n = 0;
	switch (family) {
	case 0: syms[0] = XK_Alt_L; syms[1] = XK_Alt_R; n = 2; break;
	case 1: syms[0] = XK_Control_L; syms[1] = XK_Control_R; n = 2; break;
	case 2: syms[0] = XK_Super_L; syms[1] = XK_Super_R; syms[2] = XK_Meta_L; syms[3] = XK_Meta_R; n = 4; break;
	}
	int count = 0;
	for (int i = 0; i < n; i++) {
		KeyCode code = XKeysymToKeycode(d, syms[i]);
		if (code != 0) out[count++] = code;
	}
	return count;
}
*/
import "C"

import (
	"errors"
	"os"
	"unsafe"
)

// modifierDoubleTapAvailable: the trigger needs a native X11 session, as in the Tauri shell
// (`modifier_double_tap_platform`, linux module): DISPLAY set and no WAYLAND_DISPLAY. Under Wayland (XWayland
// included) the X server cannot see keys pressed in other clients, so it is reported unsupported rather than
// pretending to work.
var modifierDoubleTapAvailable = os.Getenv("DISPLAY") != "" && os.Getenv("WAYLAND_DISPLAY") == ""

type x11KeyState struct{ display *C.Display }

func newPlatformKeyState() (modifierKeyState, error) {
	if !modifierDoubleTapAvailable {
		return nil, errors.New("modifier double-tap requires a native X11 session")
	}
	display := C.uc_open_display()
	if display == nil {
		return nil, errors.New("failed to open the X11 display for modifier double-tap")
	}
	return &x11KeyState{display: display}, nil
}

// snapshot reads the physical key state with XQueryKeymap: the selected modifier's keys, and any other key
// (keycodes 8..255).
func (s *x11KeyState) snapshot(modifier string) (selectedDown, otherDown bool) {
	family := map[string]C.int{"alt": 0, "control": 1, "meta": 2}
	var keymap [32]C.char
	C.XQueryKeymap(s.display, &keymap[0])
	down := func(code int) bool { return byte(keymap[code/8])&(1<<(code%8)) != 0 }
	var selected [4]C.uchar
	count := 0
	if f, ok := family[modifier]; ok {
		count = int(C.uc_selected_keycodes(s.display, f, (*C.uchar)(unsafe.Pointer(&selected[0]))))
	}
	isSelected := func(code int) bool {
		for i := 0; i < count; i++ {
			if int(selected[i]) == code {
				return true
			}
		}
		return false
	}
	for i := 0; i < count; i++ {
		if down(int(selected[i])) {
			selectedDown = true
		}
	}
	for code := 8; code <= 255; code++ {
		if !isSelected(code) && down(code) {
			otherDown = true
			break
		}
	}
	return selectedDown, otherDown
}

// Close releases the display connection; the worker that owns the state calls it when it stops.
func (s *x11KeyState) Close() error {
	C.XCloseDisplay(s.display)
	return nil
}
