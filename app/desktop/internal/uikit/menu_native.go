package uikit

import (
	"fmt"
	"sync"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

var menuImageCache sync.Map

// menuEntryImage reuses Kit glyphs as cached system-menu images. Template
// images follow the system highlight; tag swatches retain their own color.
func menuEntryImage(e MenuEntry) *mygo.MenuIcon {
	if e.Icon == "" && e.Swatch == "" {
		return nil
	}
	key := string(e.Icon) + ":" + string(e.Swatch)
	if cached, ok := menuImageCache.Load(key); ok {
		return cached.(*mygo.MenuIcon)
	}
	k := Kit{Skin: MacOS}
	var svg *ui.SVG
	if e.Swatch != "" {
		col := k.swatchColor(e.Swatch)
		var err error
		svg, err = ui.ParseSVG([]byte(fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><circle cx="8" cy="8" r="4" fill="#%02x%02x%02x"/></svg>`, col.R, col.G, col.B)))
		if err != nil {
			panic(err)
		}
	} else {
		glyph, ok := iconGlyph[e.Icon]
		if !ok {
			return nil
		}
		svg = k.glyphSVG(glyph)
	}
	icon, err := mygo.NewMenuIconSVG(svg, e.Swatch == "")
	if err != nil {
		panic(err)
	}
	cached, _ := menuImageCache.LoadOrStore(key, icon)
	return cached.(*mygo.MenuIcon)
}
