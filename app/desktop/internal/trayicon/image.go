// Package trayicon owns v1 tray artwork, platform geometry and image updates.
package trayicon

import (
	"bytes"
	"context"
	"errors"
	"image"
	"image/color"
	"image/draw"
	"image/png"

	"github.com/egoist/mygo/ui"
	xdraw "golang.org/x/image/draw"

	"github.com/uniclipboard/unic/app/desktop/internal/appicons"
)

var glyph = ui.MustParseSVG(appicons.TrayGlyphSVG)

type spec struct {
	size, glyph, cat int
	foreground       color.NRGBA
	template         bool
}

// Render returns a PNG in the platform's tray geometry and current palette.
func Render(style string) ([]byte, bool, error) {
	if style != "glyph" && style != "cat" {
		return nil, false, errors.New("invalid tray icon style")
	}
	s := platformSpec()
	img := image.NewNRGBA(image.Rect(0, 0, s.size, s.size))
	template := s.template
	if style == "glyph" {
		source := glyph.Image(s.glyph, s.glyph, s.foreground)
		offset := (s.size - s.glyph) / 2
		draw.Draw(img, source.Bounds().Add(image.Pt(offset, offset)), source, image.Point{}, draw.Src)
	} else {
		source, err := png.Decode(bytes.NewReader(appicons.TrayPNG))
		if err != nil {
			return nil, false, err
		}
		offset := (s.size - s.cat) / 2
		xdraw.CatmullRom.Scale(img, image.Rect(offset, offset, offset+s.cat, offset+s.cat), source, source.Bounds(), xdraw.Src, nil)
		template = false
	}
	var out bytes.Buffer
	if err := png.Encode(&out, img); err != nil {
		return nil, false, err
	}
	return out.Bytes(), template, nil
}

// Icon serializes image preferences on its owner's thread. It never changes menus.
// The Desktop host invokes it on the mygo main thread.
type Icon struct {
	apply func([]byte, bool) error
	shown string
}

// New binds image updates to the native tray API (or an acceptance observer).
func New(apply func([]byte, bool) error) *Icon { return &Icon{apply: apply} }

// SetStyle avoids duplicate publication. Failed delivery leaves the shown style intact.
func (i *Icon) SetStyle(style string) error {
	if style == i.shown {
		return nil
	}
	return i.put(style)
}

// Refresh redraws the selected style after a platform theme change.
func (i *Icon) Refresh() error {
	style := i.shown
	if style == "" {
		style = "glyph"
	}
	return i.put(style)
}
func (i *Icon) put(style string) error {
	data, template, err := Render(style)
	if err != nil {
		return err
	}
	if err = i.apply(data, template); err != nil {
		return err
	}
	i.shown = style
	return nil
}

// WatchTheme observes platform-specific panel configuration. The caller marshals
// changed onto its UI thread. Cancel ctx to release polling resources.
func WatchTheme(ctx context.Context, changed func()) { watchTheme(ctx, changed) }
