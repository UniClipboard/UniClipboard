package desktopkit

import (
	"sync"

	"github.com/egoist/mygo/ui"
)

// iconGlyph maps application semantics to the finite official Remix vocabulary.
var iconGlyph = map[IconName]string{
	IconAdd: "plus", IconBack: "chevronLeft", IconChat: "message",
	IconChevronDown: "chevronDown", IconChevronRight: "chevronRight", IconCopy: "copy",
	IconMaximize: "maximize",
	IconDelete:   "trash", IconDesktop: "desktop", IconDevices: "devices", IconDismiss: "close",
	IconDownload: "download", IconEdit: "pencil", IconFile: "doc", IconFolder: "folder", IconText: "textLines", IconCode: "codeBrackets", IconLink: "chain", IconImage: "picture", IconFilter: "filter",
	IconGlobe: "globe", IconGrid: "grid", IconHistory: "history", IconInfo: "info", IconKeyboard: "keyboard",
	IconList: "list", IconLock: "lock", IconMerge: "merge", IconMoon: "moon", IconMore: "more",
	IconPackage: "box", IconPhone: "phone", IconPin: "pin", IconQuickPanel: "flash", IconRemove: "minus",
	IconRetry: "retry", IconSearch: "search", IconSend: "arrowRight", IconSettings: "gear",
	IconShield: "shield", IconSidebar: "sidebar", IconSun: "sun", IconTag: "tag",
	IconUnlock: "lockOpen", IconUpdate: "arrowUpCircle", IconWarning: "warning",
}

var iconOnce sync.Once
var iconCache map[string]*ui.SVG

// glyphSVG parses the fixed generated dictionary once. The cache is bounded by
// that dictionary and does not depend on an OS, theme, widget ID or row ID.
func (k Kit) glyphSVG(glyph string) *ui.SVG {
	iconOnce.Do(func() {
		iconCache = make(map[string]*ui.SVG, len(glyphs))
		for name, source := range glyphs {
			svg, err := ui.ParseSVG([]byte(source))
			if err != nil {
				panic("desktopkit: invalid generated Remix glyph " + name + ": " + err.Error())
			}
			iconCache[name] = svg
		}
	})
	if svg, ok := iconCache[glyph]; ok {
		return svg
	}
	panic("desktopkit: unknown glyph " + glyph)
}

// Icon renders an application glyph. The action or field owns its accessible
// name; the SVG is decorative. An empty optional icon renders an empty box.
func (k Kit) Icon(c *ui.Context, name IconName, size float32) ui.Element {
	if name == "" {
		return ui.Box(c).Size(size, size).AccessHidden(true)
	}
	glyph, ok := iconGlyph[name]
	if !ok {
		panic("desktopkit: unknown icon " + string(name))
	}
	return k.glyph(c, glyph, size)
}

func (k Kit) glyph(c *ui.Context, name string, size float32) ui.Element {
	return ui.Icon(c, k.glyphSVG(name)).Size(size, size).AccessHidden(true)
}
