//go:build linux

package board

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"os"
	"strings"

	"github.com/jezek/xgb"
	"github.com/jezek/xgb/xproto"

	"github.com/uniclipboard/unic/sourceapp"
)

// Native Wayland does not expose another client's clipboard application.
// Do not misattribute its XWayland helper as the source application.
func sourceX11() *xgb.Conn {
	if os.Getenv("WAYLAND_DISPLAY") != "" || strings.EqualFold(os.Getenv("XDG_SESSION_TYPE"), "wayland") {
		return nil
	}
	display := os.Getenv("DISPLAY")
	if !strings.HasPrefix(display, ":") && !strings.HasPrefix(display, "unix:") {
		return nil
	}
	c, err := xgb.NewConn()
	if err != nil {
		return nil
	}
	return c
}
func sourceAtom(c *xgb.Conn, name string) xproto.Atom {
	a, err := xproto.InternAtom(c, true, uint16(len(name)), name).Reply()
	if err != nil {
		return 0
	}
	return a.Atom
}
func selectionOwner(c *xgb.Conn) xproto.Window {
	atom := sourceAtom(c, "CLIPBOARD")
	if atom == 0 {
		return 0
	}
	r, err := xproto.GetSelectionOwner(c, atom).Reply()
	if err != nil {
		return 0
	}
	return r.Owner
}
func nativeVersion() uint64 {
	c := sourceX11()
	if c == nil {
		return 0
	}
	defer c.Close()
	return uint64(selectionOwner(c))
}
func nativeSource() sourceState {
	c := sourceX11()
	if c == nil {
		return sourceState{}
	}
	defer c.Close()
	owner := selectionOwner(c)
	state := sourceState{token: uint64(owner)}
	if owner == 0 {
		return state
	}
	// WM_CLASS is an application identity, unlike WM_NAME (a window title).
	r, err := xproto.GetProperty(c, false, owner, xproto.AtomWmClass, xproto.AtomString, 0, 128).Reply()
	if err != nil || r == nil || r.Format != 8 {
		return state
	}
	names := strings.Split(strings.TrimRight(string(r.Value), "\x00"), "\x00")
	id := names[len(names)-1]
	a := &sourceapp.Application{Platform: "linux", Identifier: id, Name: id, Attribution: "clipboard_owner"}
	if a.Validate() != nil {
		return state
	}
	a.IconPNG = cachedIcon("linux:"+id, func() []byte {
		atom := sourceAtom(c, "_NET_WM_ICON")
		if atom == 0 {
			return nil
		}
		r, err := xproto.GetProperty(c, false, owner, atom, xproto.AtomCardinal, 0, 1<<20).Reply()
		if err != nil || r == nil || r.Format != 32 || r.BytesAfter != 0 {
			return nil
		}
		data := r.Value
		for len(data) >= 8 {
			w, h := uint64(xgb.Get32(data)), uint64(xgb.Get32(data[4:]))
			data = data[8:]
			if w == 0 || h == 0 || w*h > uint64(len(data)/4) {
				return nil
			}
			if w <= 128 && h <= 128 {
				img := image.NewNRGBA(image.Rect(0, 0, int(w), int(h)))
				for y := 0; y < int(h); y++ {
					for x := 0; x < int(w); x++ {
						v := xgb.Get32(data[(y*int(w)+x)*4:])
						img.SetNRGBA(x, y, color.NRGBA{byte(v >> 16), byte(v >> 8), byte(v), byte(v >> 24)})
					}
				}
				var b bytes.Buffer
				if png.Encode(&b, img) == nil {
					return smallPNG(b.Bytes())
				}
				return nil
			}
			data = data[int(w*h)*4:]
		}
		return nil
	})
	state.app = a
	return state
}
