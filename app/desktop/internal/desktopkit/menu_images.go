package desktopkit

import (
	"fmt"
	"sync"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

var menuIcons struct {
	sync.Mutex
	items map[string]*mygo.MenuIcon
}

// menuEntryImage supplies SVGs to mygo's maintained in-window menu adapter.
// Named glyph/palette resources have a fixed upper bound. Custom tag colors are
// short-lived menu resources owned by mygo and are never retained in this cache.
func menuEntryImage(e MenuEntry) *mygo.MenuIcon {
	if e.Icon == "" && e.Swatch == "" {
		return nil
	}
	k := Kit{}
	var svg *ui.SVG
	key := ""
	if e.Swatch != "" {
		col := k.SwatchColor(e.Swatch)
		var err error
		svg, err = ui.ParseSVG([]byte(fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="4" fill="#%02x%02x%02x"/></svg>`, col.R, col.G, col.B)))
		if err != nil {
			panic(err)
		}
		switch e.Swatch {
		case "orange", "blue", "green", "purple", "gray":
			key = "swatch/" + string(e.Swatch)
		}
	} else {
		g, ok := iconGlyph[e.Icon]
		if !ok {
			return nil
		}
		svg = k.glyphSVG(g)
		key = "glyph/" + g
	}
	menuIcons.Lock()
	defer menuIcons.Unlock()
	if icon := menuIcons.items[key]; key != "" && icon != nil {
		return icon
	}
	icon, err := mygo.NewMenuIconSVG(svg, e.Swatch == "")
	if err != nil {
		panic(err)
	}
	if key != "" {
		if menuIcons.items == nil {
			menuIcons.items = map[string]*mygo.MenuIcon{}
		}
		menuIcons.items[key] = icon
	}
	return icon
}
