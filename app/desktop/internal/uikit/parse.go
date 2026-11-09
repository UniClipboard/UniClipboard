package uikit

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// parseColor reads "#rgb", "#rrggbb", "#rrggbbaa", "rgb(...)", "rgba(...)" and "transparent".
func parseColor(s string) (ui.Color, bool) {
	s = strings.TrimSpace(s)
	switch {
	case s == "transparent":
		return ui.Transparent, true
	case strings.HasPrefix(s, "#"):
		h := s[1:]
		if len(h) == 3 || len(h) == 4 {
			var b strings.Builder
			for _, r := range h {
				b.WriteRune(r)
				b.WriteRune(r)
			}
			h = b.String()
		}
		if len(h) != 6 && len(h) != 8 {
			return ui.Color{}, false
		}
		v, err := strconv.ParseUint(h, 16, 32)
		if err != nil {
			return ui.Color{}, false
		}
		if len(h) == 6 {
			return ui.RGB(uint8(v>>16), uint8(v>>8), uint8(v)), true
		}
		return ui.Color{R: uint8(v >> 24), G: uint8(v >> 16), B: uint8(v >> 8), A: uint8(v)}, true
	case strings.HasPrefix(s, "rgb"):
		open, end := strings.IndexByte(s, '('), strings.LastIndexByte(s, ')')
		if open < 0 || end < open {
			return ui.Color{}, false
		}
		f := strings.FieldsFunc(s[open+1:end], func(r rune) bool { return r == ',' || r == ' ' || r == '/' })
		if len(f) < 3 {
			return ui.Color{}, false
		}
		var n [3]float64
		for i := range n {
			p := strings.TrimSuffix(f[i], "%")
			v, err := strconv.ParseFloat(p, 64)
			if err != nil {
				return ui.Color{}, false
			}
			if p != f[i] {
				v = v * 255 / 100
			}
			n[i] = v
		}
		a := 1.0
		if len(f) > 3 {
			p := strings.TrimSuffix(f[3], "%")
			v, err := strconv.ParseFloat(p, 64)
			if err != nil {
				return ui.Color{}, false
			}
			if p != f[3] {
				v /= 100
			}
			a = v
		}
		return ui.RGBA(uint8(n[0]+.5), uint8(n[1]+.5), uint8(n[2]+.5), float32(a)), true
	}
	return ui.Color{}, false
}

// parsePx reads "12px" / "12" / "0.5rem"(×16).
func parsePx(s string) float32 {
	s = strings.TrimSpace(s)
	mul := 1.0
	switch {
	case strings.HasSuffix(s, "px"):
		s = strings.TrimSuffix(s, "px")
	case strings.HasSuffix(s, "rem"):
		s, mul = strings.TrimSuffix(s, "rem"), 16
	}
	v, err := strconv.ParseFloat(s, 64)
	if err != nil {
		return 0
	}
	return float32(v * mul)
}
