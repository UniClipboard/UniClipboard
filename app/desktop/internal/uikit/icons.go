package uikit

import (
	"fmt"
	"sync"

	"github.com/egoist/mygo/ui"
)

// 图标按 24 单位网格描边绘制。三套皮肤共用字形骨架（来自设计稿的线性图标），
// 各自的风格差异落在描边宽度/端点/拐角上：
//   - Linux：Adwaita symbolic，1.9 圆头；
//   - macOS：SF Symbols 风，1.8 圆头；
//   - Windows：Fluent 线性，1.5，圆头圆角更细。
//
// 个别字形（macOS 的 sidebar 汉堡线，Windows 的实心 pin 等）按皮肤替换。

// glyphs 是字形的 SVG 内部标记（不含 <svg> 外壳）。
var glyphs = map[string]string{
	"plus":          `<path d="M12 5v14M5 12h14"/>`,
	"minus":         `<path d="M5 12h14"/>`,
	"chevronLeft":   `<path d="m15 6-6 6 6 6"/>`,
	"chevronDown":   `<path d="m6 9 6 6 6-6"/>`,
	"chevronUp":     `<path d="m6 15 6-6 6 6"/>`,
	"chevronRight":  `<path d="m9 6 6 6-6 6"/>`,
	"close":         `<path d="M6 6l12 12M18 6 6 18"/>`,
	"check":         `<path d="m5 12 5 5 9-10"/>`,
	"message":       `<path d="M4.5 5.5h15v10h-8l-4.5 4v-4h-2.5z"/>`,
	"copy":          `<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>`,
	"trash":         `<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>`,
	"desktop":       `<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>`,
	"devices":       `<rect x="2" y="4" width="14" height="10" rx="1.5"/><path d="M6 18h6M9 14v4"/><rect x="17" y="8" width="5" height="12" rx="1.2"/>`,
	"download":      `<path d="M12 3.5v11M7.5 10.5 12 15l4.5-4.5M4.5 19.5h15"/>`,
	"pencil":        `<path d="M4 20h4L19 9l-4-4L4 16z"/>`,
	"doc":           `<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>`,
	"filter":        `<path d="M4 5h16l-6 7.5V19l-4 2v-8.5z"/>`,
	"globe":         `<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>`,
	"grid":          `<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>`,
	"history":       `<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>`,
	"info":          `<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 7.5v.5"/>`,
	"list":          `<path d="M4 6h16M4 12h16M4 18h16"/>`,
	"lock":          `<rect x="5" y="10.5" width="14" height="10" rx="2.5"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/>`,
	"lockOpen":      `<rect x="5" y="10.5" width="14" height="10" rx="2.5"/><path d="M8 10.5V7.5a4 4 0 0 1 7.7-1.5"/>`,
	"moon":          `<path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z"/>`,
	"more":          `<circle cx="5" cy="12" r="1" fill="currentColor"/><circle cx="12" cy="12" r="1" fill="currentColor"/><circle cx="19" cy="12" r="1" fill="currentColor"/>`,
	"box":           `<path d="M3.5 7.5 12 3l8.5 4.5v9L12 21l-8.5-4.5z"/><path d="M3.5 7.5 12 12l8.5-4.5M12 12v9"/>`,
	"phone":         `<rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18.5h2"/>`,
	"pin":           `<path d="M12 17v5M8 3h8l-1 6 3 4H6l3-4z"/>`,
	"retry":         `<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4.5V11h-6.5"/>`,
	"search":        `<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>`,
	"arrowRight":    `<path d="M4 12h14M13 6l6 6-6 6"/>`,
	"gear":          `<circle cx="12" cy="12" r="3"/><path d="M12 2.8v2.4M12 18.8v2.4M4.4 4.4l1.7 1.7M17.9 17.9l1.7 1.7M2.8 12h2.4M18.8 12h2.4M4.4 19.6l1.7-1.7M17.9 6.1l1.7-1.7"/>`,
	"shield":        `<path d="M12 3 4.5 6v5.5c0 4.6 3.2 8.4 7.5 9.5 4.3-1.1 7.5-4.9 7.5-9.5V6z"/>`,
	"sidebar":       `<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>`,
	"sun":           `<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>`,
	"tag":           `<path d="M3 12V4h8l10 10-8 8z"/><circle cx="7.5" cy="8.5" r="1.3"/>`,
	"arrowUpCircle": `<circle cx="12" cy="12" r="9"/><path d="M12 16.5v-9M8.5 11 12 7.5l3.5 3.5"/>`,
	"checkCircle":   `<circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/>`,
	"warning":       `<path d="M12 3 2 20h20z"/><path d="M12 10v4M12 17v.5"/>`,
	"xCircle":       `<circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/>`,
	// 菊花形进度字形，Spinner 用。
	"spinner": `<path d="M12 3v4"/><path d="M18.4 5.6l-2.8 2.8" opacity=".3"/><path d="M21 12h-4" opacity=".35"/><path d="M18.4 18.4l-2.8-2.8" opacity=".45"/><path d="M12 21v-4" opacity=".55"/><path d="M5.6 18.4l2.8-2.8" opacity=".65"/><path d="M3 12h4" opacity=".75"/><path d="M5.6 5.6l2.8 2.8" opacity=".9"/>`,
}

// iconGlyph maps the public vocabulary to glyph keys.
var iconGlyph = map[IconName]string{
	IconAdd: "plus", IconBack: "chevronLeft", IconChat: "message",
	IconChevronDown: "chevronDown", IconChevronRight: "chevronRight", IconCopy: "copy",
	IconDelete: "trash", IconDesktop: "desktop", IconDevices: "devices", IconDismiss: "close",
	IconDownload: "download", IconEdit: "pencil", IconFile: "doc", IconFilter: "filter",
	IconGlobe: "globe", IconGrid: "grid", IconHistory: "history", IconInfo: "info",
	IconList: "list", IconLock: "lock", IconMoon: "moon", IconMore: "more",
	IconPackage: "box", IconPhone: "phone", IconPin: "pin", IconRemove: "minus",
	IconRetry: "retry", IconSearch: "search", IconSend: "arrowRight", IconSettings: "gear",
	IconShield: "shield", IconSidebar: "sidebar", IconSun: "sun", IconTag: "tag",
	IconUnlock: "lockOpen", IconUpdate: "arrowUpCircle",
}

// 皮肤专属字形覆盖。
var skinGlyphs = map[SkinID]map[string]string{
	MacOS: {"sidebar": `<path d="M4 6h16M4 12h16M4 18h16"/>`},
}

// strokeWidth 是皮肤的默认描边宽度。
func (k Kit) iconStroke(size float32) float32 {
	w := float32(1.9)
	switch k.Skin {
	case MacOS:
		w = 1.8
	case Windows:
		w = 1.5
	}
	return w
}

var (
	iconMu    sync.Mutex
	iconCache = map[string]*ui.SVG{}
)

// glyphSVG parses (and caches) one glyph in one skin's stroke style. The stroke uses
// currentColor, so the SVG takes the colour of the surrounding text.
func (k Kit) glyphSVG(glyph string) *ui.SVG {
	key := string(k.Skin) + "/" + glyph
	iconMu.Lock()
	defer iconMu.Unlock()
	if s, ok := iconCache[key]; ok {
		return s
	}
	body, ok := skinGlyphs[k.Skin][glyph]
	if !ok {
		body = glyphs[glyph]
	}
	src := fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="%g" stroke-linecap="round" stroke-linejoin="round">%s</svg>`,
		k.iconStroke(0), body)
	s, err := ui.ParseSVG([]byte(src))
	if err != nil {
		panic("uikit: bad glyph " + glyph + ": " + err.Error())
	}
	iconCache[key] = s
	return s
}

// Icon draws the glyph of name in the skin's icon style, size DIPs square, in the current
// text colour. Decorative: it carries no accessible name.
func (k Kit) Icon(c *ui.Context, name IconName, size float32) ui.Element {
	g, ok := iconGlyph[name]
	if !ok {
		return ui.Box(c).Size(size, size)
	}
	return k.glyph(c, g, size)
}

// glyph draws a glyph by key (shared helpers such as check/close/spinner use it).
func (k Kit) glyph(c *ui.Context, g string, size float32) ui.Element {
	return ui.Icon(c, k.glyphSVG(g)).Size(size, size)
}
