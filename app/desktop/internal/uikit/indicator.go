package uikit

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// ProgressBarProps mirrors contract.ts ProgressBarProps.
type ProgressBarProps struct {
	// Label is the accessible name.
	Label string
	// Value is 0–100 for a determinate bar; nil shows indeterminate activity.
	Value *float64
}

// ProgressBar is a thin track with an accent indicator (WinUI ProgressBar, AppKit bar).
func (k Kit) ProgressBar(c *ui.Context, p ProgressBarProps) ui.Element {
	v := -1.0
	if p.Value != nil {
		v = min(max(*p.Value, 0), 100) / 100
	}
	var h float32 = 6
	var track, fill ui.Color
	switch k.Skin {
	case MacOS:
		track, fill = k.Color("fill-2"), k.Color("accent")
	case Windows:
		h = 4
		track, fill = k.Color("colorNeutralBackground6"), k.Color("colorCompoundBrandBackground")
	default:
		track, fill = k.Color("btn"), k.Color("accent")
	}
	return ui.Progress(c, v).Label(p.Label).Height(h).Radius(h / 2).Background(track).TextColor(fill)
}

// BadgeProps mirrors contract.ts BadgeProps.
type BadgeProps struct {
	Count int
	// Label is the accessible name, e.g. "3 new items"; a bare number means nothing to a
	// screen reader.
	Label string
}

// Badge is a small count capsule.
func (k Kit) Badge(c *ui.Context, p BadgeProps) ui.Element {
	var h, padX, size float32
	var bg, fg ui.Color
	weight := 700
	switch k.Skin {
	case MacOS:
		h, padX, size, weight = 18, 5, 11, 600
		bg, fg = k.Color("red"), ui.RGB(255, 255, 255)
	case Windows:
		h, padX, size, weight = 16, 4, k.Px("fontSizeBase100"), 600
		bg, fg = k.Color("colorBrandBackground"), k.Color("colorNeutralForegroundOnBrand")
	default:
		h, padX, size = 20, 6, 12
		bg, fg = k.Color("accent"), k.Color("accent-fg")
	}
	return ui.Row(c).Height(h).MinWidth(h).PaddingX(padX).Radius(h / 2).Center().Background(bg).
		Label(p.Label).Children(func() {
		ui.Text(c, strconv.Itoa(p.Count)).FontSize(size).FontWeight(weight).TextColor(fg).AccessHidden(true)
	})
}

// TagTint returns the background and ink of a tag colour in the current skin and scheme:
// a palette name ("orange", "blue", "green", "purple", "gray"), a custom "#rrggbb" (16% tint
// of the colour with the colour as ink) or "" for neutral.
func (k Kit) TagTint(color TagColor) (bg, ink ui.Color) {
	if strings.HasPrefix(string(color), "#") {
		col, ok := parseColor(string(color))
		if ok {
			return col.Alpha(0x29 / 255.0), col
		}
	}
	name := string(color)
	switch name {
	case "orange", "blue", "green", "purple", "gray":
	default:
		switch k.Skin {
		case MacOS:
			return k.Color("fill-2"), k.Color("label-2")
		case Windows:
			return k.Color("w11-tag-neutral-bg"), k.Color("w11-tag-neutral-ink")
		}
		return k.Color("btn"), k.Color("fg2")
	}
	if k.Skin == Windows {
		return k.Color("w11-tag-" + name + "-bg"), k.Color("w11-tag-" + name + "-ink")
	}
	return k.Color("tag-" + name + "-bg"), k.Color("tag-" + name + "-ink")
}

// SwatchColor is the solid colour of a tag's dot or square; "" is gray.
func (k Kit) SwatchColor(color TagColor) ui.Color {
	if strings.HasPrefix(string(color), "#") {
		if col, ok := parseColor(string(color)); ok {
			return col
		}
	}
	if k.Skin == Windows {
		switch color {
		case "orange":
			return ui.RGB(0xE0, 0x70, 0x3A)
		case "blue":
			return ui.RGB(0x3A, 0x7B, 0xD5)
		case "purple":
			return ui.RGB(0xA8, 0x55, 0xC8)
		case "green":
			return ui.RGB(0x2E, 0x9E, 0x5B)
		}
		return ui.RGB(0x8A, 0x8A, 0x8A)
	}
	switch color {
	case "orange", "blue", "green", "purple":
		return k.Color(string(color))
	}
	return k.Color("gray")
}

// ColorDotProps mirrors contract.ts ColorDotProps.
type ColorDotProps struct {
	Color TagColor
	// Label names the dot for assistive technology; without it the dot is decorative.
	Label string
}

// ColorDot is a colour mark, e.g. a tag's colour beside its name.
func (k Kit) ColorDot(c *ui.Context, p ColorDotProps) ui.Element {
	var d float32 = 10
	if k.Skin == MacOS {
		d = 8
	}
	col := k.SwatchColor(p.Color)
	if p.Color == "" && k.Skin == Windows {
		col = k.Color("w11-status-neutral")
	}
	e := ui.Box(c).Size(d, d).Radius(d / 2).Background(col)
	if p.Label != "" {
		e.Label(p.Label)
	} else {
		e.AccessHidden(true)
	}
	return e
}

// KindVariant selects the form of a KindBadge.
type KindVariant int

const (
	// KindTile is the square leading visual of a list row.
	KindTile KindVariant = iota
	// KindPill is the capsule naming the kind in a detail header.
	KindPill
)

// KindBadgeProps mirrors contract.ts KindBadgeProps.
type KindBadgeProps struct {
	Kind ContentKind
	// Label is the visible text of the mark (TXT, PDF, Code); it is also the accessible name.
	Label   string
	Variant KindVariant
	// Image is the thumbnail an image tile shows.
	Image ui.ImageSource
}

// kindColors returns the background and ink for a content kind.
func (k Kit) kindColors(kind ContentKind) (bg, ink ui.Color) {
	switch kind {
	case KindCode:
		switch k.Skin {
		case MacOS:
			return ui.RGB(0x1d, 0x1d, 0x1f), ui.RGB(0x7e, 0xe7, 0x87)
		case Windows:
			return ui.RGB(0x1f, 0x1f, 0x1f), ui.RGB(0x7e, 0xe7, 0x87)
		}
		return k.Color("code"), k.Color("code-ink")
	case KindLink:
		return k.TagTint("blue")
	case KindFolder:
		return k.TagTint("purple")
	case KindFile, KindImage:
		return k.TagTint("orange")
	}
	if k.Skin == Linux {
		return k.Color("shade"), k.Color("fg2")
	}
	return k.TagTint("")
}

// kindIcon is the glyph that names a content kind.
func kindIcon(kind ContentKind) IconName {
	switch kind {
	case KindCode:
		return IconCode
	case KindLink:
		return IconLink
	case KindImage:
		return IconImage
	case KindFile:
		return IconFile
	case KindFolder:
		return IconFolder
	}
	return IconText
}

// KindBadge is the type mark of a clipboard entry: a square tile or a capsule label.
func (k Kit) KindBadge(c *ui.Context, p KindBadgeProps) ui.Element {
	bg, ink := k.kindColors(p.Kind)
	if p.Variant == KindPill {
		var h, padX, size float32
		var r float32
		weight := 700
		switch k.Skin {
		case MacOS:
			h, padX, size, r, weight = 20, 8, 11, 10, 600
		case Windows:
			h, padX, size, r, weight = 20, 8, k.Px("fontSizeBase200"), k.Px("borderRadiusCircular"), 600
		default:
			h, padX, size, r = 22, 9, 12, 11
		}
		return ui.Row(c).Height(h).PaddingX(padX).Radius(min(r, h/2)).Center().Gap(4).Background(bg).TextColor(ink).
			Label(p.Label).Children(func() {
			k.Icon(c.Key("icon"), kindIcon(p.Kind), size+3).AccessHidden(true)
			ui.Text(c, p.Label).FontSize(size).FontWeight(weight).AccessHidden(true)
		})
	}
	var w, h, r float32
	switch k.Skin {
	case MacOS:
		w, h, r = 30, 30, 7
		if p.Kind == KindImage {
			w, r = 40, 6
		}
	case Windows:
		w, h, r = 32, 30, k.Px("borderRadiusMedium")
	default:
		w, h, r = 40, 32, 6
	}
	e := ui.Row(c).Size(w, h).Radius(r).Center().Clip().Background(bg).TextColor(ink).Label(p.Label)
	if p.Kind == KindImage {
		switch k.Skin {
		case MacOS:
			e.Background(k.Color("thumb")).InsetShadow(0, 0, 0, 0.5, k.Color("separator"))
		case Windows:
			e.Background(k.Color("colorNeutralBackground3")).Border(k.Px("strokeWidthThin"), k.Color("w11-card-stroke"))
		default:
			e.Background(k.Color("thumb"))
		}
		return e.Children(func() {
			if p.Image != nil {
				ui.Image(c, p.Image).Size(w, h).Fit(ui.Cover)
				return
			}
			k.Icon(c.Key("icon"), IconImage, 16).AccessHidden(true)
		})
	}
	return e.Children(func() {
		k.Icon(c.Key("icon"), kindIcon(p.Kind), 16).AccessHidden(true)
	})
}
