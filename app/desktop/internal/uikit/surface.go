package uikit

import (
	"github.com/egoist/mygo/plugins/glass"
	"github.com/egoist/mygo/ui"
)

// UiRoot installs the kit's theme and returns the window-sized root every view builds in:
//
//	k.UiRoot(c, func() { ... })
//
// The root paints the window background (transparent on macOS while the host shows a window
// material, so the sidebar Surface can reveal it) and sets the skin's base text.
func (k Kit) UiRoot(c *ui.Context, body func()) ui.Element {
	k.Apply(c)
	e := ui.Column(c).Fill().TextColor(k.Sem("foreground")).FontSize(k.baseFont())
	bg := k.Sem("background")
	if k.Skin == MacOS && c.Vibrancy() {
		bg = ui.Transparent
	}
	return e.Background(bg).Children(body)
}

// baseFont is the default text size of the skin.
func (k Kit) baseFont() float32 {
	if k.Skin == MacOS {
		return 13
	}
	return 14
}

// Surface returns the container of a window region; add content with .Children.
//
// Sidebar shows the platform's window material where there is one: on macOS it leaves the area
// transparent when the host applied a native material (Vibrancy) and otherwise draws the
// blurred, tinted glass fallback; Linux and Windows draw a solid colour. Content is always
// opaque (a rounded card on Windows).
func (k Kit) Surface(c *ui.Context, region SurfaceRegion) ui.Element {
	e := ui.Column(c).MinWidth(0).MinHeight(0)
	switch k.Skin {
	case MacOS:
		if region == RegionContent {
			return e.Background(k.Color("content"))
		}
		if c.Vibrancy() {
			return e // native material shows through
		}
		e.Material(sidebarGlass{fill: k.Color("glass-tinted"), blur: 14})
		return e.InsetShadow(-0.5, 0, 0, 0, k.Color("separator"))
	case Windows:
		if region == RegionContent {
			return e.Background(k.Color("w11-card")).Border(k.Px("strokeWidthThin"), k.Color("w11-card-stroke")).
				Radius(k.Px("borderRadiusXLarge"))
		}
		return e.Background(k.Color("w11-base"))
	}
	if region == RegionContent {
		return e.Background(k.Color("view"))
	}
	return e.Background(k.Color("sidebar"))
}

// sidebarGlass is the macOS sidebar fallback: a backdrop blur under a flat translucent tint
// (design T4, "tinted"). With Reduce Transparency it is an opaque blend with the window.
type sidebarGlass struct {
	fill ui.Color
	blur float32
}

// glassSurface applies the shared toolbar/search/action capsule material.
func (k Kit) glassSurface(e ui.Element) ui.Element {
	// Sample the native UI already painted in this frame. Native: true puts
	// NSGlassEffectView under the window's render surface and cannot sample
	// these controls; the maintained renderer plugin provides their lens,
	// refraction, reactive rim, press growth and accessibility fallbacks.
	return e.Material(glass.Glass{Style: glass.Regular, Interactive: true})
}

func (m sidebarGlass) PaintMaterial(p *ui.Painter, r ui.Rect, radii [4]float32) {
	fill := m.fill
	if p.Preferences().ReduceTransparency {
		bg := p.Theme().Background
		a := float32(fill.A) / 255
		mix := func(f, b uint8) uint8 { return uint8(float32(f)*a + float32(b)*(1-a) + .5) }
		fill = ui.RGB(mix(fill.R, bg.R), mix(fill.G, bg.G), mix(fill.B, bg.B))
	} else if m.blur > 0 {
		glass.Blur{Radius: m.blur}.PaintMaterial(p, r, radii)
	}
	p.Clip(r, radii[0], func() { p.Fill(r, fill, radii[0]) })
}
