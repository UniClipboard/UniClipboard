package uikit

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// 浮层（菜单、对话框、提示、预览）共用的外观：按皮肤取材质、阴影、遮罩。

// shadowLayer 是 CSS box-shadow 的一层。
type shadowLayer struct {
	x, y, blur, spread float32
	col                ui.Color
	inset              bool
}

// splitTop 按顶层逗号切分（忽略括号内的逗号）。
func splitTop(s string) []string {
	var out []string
	depth, start := 0, 0
	for i, r := range s {
		switch r {
		case '(':
			depth++
		case ')':
			depth--
		case ',':
			if depth == 0 {
				out = append(out, strings.TrimSpace(s[start:i]))
				start = i + 1
			}
		}
	}
	return append(out, strings.TrimSpace(s[start:]))
}

// parseShadow 解析 "0 0 0 1px rgba(...), 0 10px 30px rgba(...)"。
func parseShadow(s string) []shadowLayer {
	var out []shadowLayer
	for _, part := range splitTop(s) {
		if part == "" {
			continue
		}
		var l shadowLayer
		if strings.HasPrefix(part, "inset") {
			l.inset = true
			part = strings.TrimSpace(strings.TrimPrefix(part, "inset"))
		}
		cut := len(part)
		if i := strings.Index(part, "rgb"); i >= 0 {
			cut = i
		} else if i := strings.Index(part, "#"); i >= 0 {
			cut = i
		}
		l.col, _ = parseColor(part[cut:])
		var n []float32
		for _, f := range strings.Fields(part[:cut]) {
			n = append(n, parsePx(f))
		}
		if len(n) >= 2 {
			l.x, l.y = n[0], n[1]
		}
		if len(n) >= 3 {
			l.blur = n[2]
		}
		if len(n) >= 4 {
			l.spread = n[3]
		}
		out = append(out, l)
	}
	return out
}

// shadowed 把皮肤阴影 token 套到元素上。
func (k Kit) shadowed(e ui.Element, token string) ui.Element {
	ls := parseShadow(k.Raw(token))
	for i := len(ls) - 1; i >= 0; i-- { // CSS 先写的层在上面
		l := ls[i]
		if l.inset {
			e.InsetShadow(l.x, l.y, l.blur, l.spread, l.col)
		} else {
			e.Shadow(l.x, l.y, l.blur, l.spread, l.col)
		}
	}
	return e
}

// scrim 是模态遮罩色。
func (k Kit) scrim() ui.Color {
	if k.Skin == Windows {
		return k.Color("colorBackgroundOverlay")
	}
	return k.Color("scrim")
}

// fgTone 返回正文、次要、三级文字色。
func (k Kit) fgTone() (primary, secondary, tertiary ui.Color) {
	switch k.Skin {
	case MacOS:
		return k.Color("label"), k.Color("label-2"), k.Color("label-3")
	case Windows:
		return k.Color("colorNeutralForeground1"), k.Color("colorNeutralForeground2"), k.Color("colorNeutralForeground3")
	}
	return k.Color("fg"), k.Color("fg2"), k.Color("fg3")
}

// dangerText 是破坏性文字色。
func (k Kit) dangerText() ui.Color {
	switch k.Skin {
	case MacOS:
		return k.Color("red")
	case Windows:
		return k.Color("colorPaletteRedForeground1")
	}
	return k.Color("destructive-text")
}

// hoverFill 是透明控件悬停的填充。
func (k Kit) hoverFill() ui.Color {
	switch k.Skin {
	case MacOS:
		return k.Color("fill-3")
	case Windows:
		return k.Color("colorSubtleBackgroundHover")
	}
	return k.Color("hover")
}

// dividerColor 是分隔线颜色。
func (k Kit) dividerColor() ui.Color {
	switch k.Skin {
	case MacOS:
		return k.Color("separator")
	case Windows:
		return k.Color("w11-divider")
	}
	return k.Color("border")
}

// swatchColor 解析 TagColor：调色板名或 #rrggbb；空串为中性灰。
func (k Kit) swatchColor(t TagColor) ui.Color {
	if t == "" {
		_, _, tert := k.fgTone()
		return tert
	}
	if c, ok := parseColor(string(t)); ok && strings.HasPrefix(string(t), "#") {
		return c
	}
	if c := k.Color(string(t)); c.A != 0 {
		return c
	}
	fallback := map[TagColor]string{
		"orange": "#ff9500", "blue": "#007aff", "green": "#28cd41", "purple": "#af52de", "gray": "#8e8e93",
	}
	c, _ := parseColor(fallback[t])
	return c
}

// popupSurface 给下拉菜单面板上皮肤材质（圆角/底色/边/阴影）。
func (k Kit) popupSurface(panel ui.Element) ui.Element {
	switch k.Skin {
	case MacOS:
		// 原型是毛玻璃；此处用不透明弹出色近似。
		panel.Padding(5).Radius(13).Background(k.Color("popup-opaque")).MinWidth(250)
		k.shadowed(panel, "menu-shadow")
		panel.InsetShadow(0, 0, 0, .5, k.Color("glass-rim"))
	case Windows:
		panel.Padding(4).Radius(k.Px("borderRadiusXLarge")).MinWidth(208).
			Background(k.Color("colorNeutralBackground1")).Border(1, k.Color("w11-stroke-elevation"))
		k.shadowed(panel, "shadow16")
	default:
		panel.Padding(6).Radius(12).Background(k.Color("popover")).MinWidth(240)
		k.shadowed(panel, "pop-shadow")
	}
	p, _, _ := k.fgTone()
	return panel.TextColor(p)
}

// dialogSurface 给居中的对话框面板上皮肤材质；radius 随皮肤。
func (k Kit) dialogSurface(panel ui.Element) ui.Element {
	switch k.Skin {
	case MacOS:
		panel.Radius(26).Background(k.Color("glass-tinted"))
		k.shadowed(panel, "win-shadow")
		panel.InsetShadow(0, 0, 0, .5, k.Color("glass-rim"))
	case Windows:
		panel.Radius(k.Px("borderRadiusXLarge")).Background(k.Color("colorNeutralBackground1")).
			Border(1, k.Color("w11-stroke-elevation"))
		k.shadowed(panel, "shadow64")
	default:
		panel.Radius(15).Background(k.Color("dialog"))
		k.shadowed(panel, "dialog-shadow")
	}
	p, _, _ := k.fgTone()
	return panel.TextColor(p)
}

// drawCheck 画一个对勾（菜单已选标记），颜色随当前文字色传入。
func drawCheck(e ui.Element, col ui.Color, w float32) ui.Element {
	return e.Draw(func(p *ui.Painter, r ui.Rect) {
		var path ui.Path
		cx, cy := r.X+r.W/2, r.Y+r.H/2
		path.MoveTo(cx-4, cy)
		path.LineTo(cx-1, cy+3)
		path.LineTo(cx+4.5, cy-3.5)
		p.StrokePath(&path, w, col)
	})
}

func itoa(n int) string { return strconv.Itoa(n) }
