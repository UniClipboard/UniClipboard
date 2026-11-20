package desktopkit

import (
	"strconv"

	"github.com/egoist/mygo/ui"
)

type ProgressBarProps struct {
	// Label is the accessible name.
	Label string
	// Value is 0–100 for a determinate bar; nil shows indeterminate activity.
	Value *float64
}

func (k Kit) ProgressBar(c *ui.Context, p ProgressBarProps) ui.Element {
	v := -1.0
	if p.Value != nil {
		v = min(max(*p.Value, 0), 100) / 100
	}
	var h float32 = 6
	var track, fill ui.Color
	{

		track, fill = k.Color("background-tertiary-default"), k.Color("accent-500")
	}

	return ui.Progress(c, v).Label(p.Label).Height(h).Radius(h / 2).Background(track).TextColor(fill)
}

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
	{

		h, padX, size = 20, 6, 12
		bg, fg = k.Color("accent-500"), k.Color("text-white")
	}

	return ui.Row(c).Height(h).MinWidth(h).PaddingX(padX).Radius(h / 2).Center().Background(bg).
		Label(p.Label).Children(func() {
		ui.Text(c, strconv.Itoa(p.Count)).FontSize(size).FontWeight(weight).TextColor(fg).AccessHidden(true)
	})
}

func (k Kit) TagTint(color TagColor) (bg, ink ui.Color) {
	if col, ok := parseCustomColor(string(color)); ok {
		return col.Alpha(0x29 / 255.0), col
	}
	if color == "" || color == "gray" {
		return k.Color("background-tertiary-default"), k.Color("text-secondary")
	}
	name := string(color)
	if name == "green" {
		name = "lime"
	}
	switch name {
	case "orange", "blue", "lime", "purple":
		return k.Color("status-" + name + "-background"), k.Color("status-" + name + "-text")
	}
	return k.Color("background-tertiary-default"), k.Color("text-secondary")
}

// SwatchColor preserves user-defined tag colors and maps named tags to BoardUI statuses.
func (k Kit) SwatchColor(color TagColor) ui.Color {
	if col, ok := parseCustomColor(string(color)); ok {
		return col
	}
	if color == "" || color == "gray" {
		return k.Color("text-tertiary")
	}
	_, ink := k.TagTint(color)
	return ink
}

type ColorDotProps struct {
	Color TagColor
	// Label names the dot for assistive technology; without it the dot is decorative.
	Label string
}

// ColorDot is a colour mark, e.g. a tag's colour beside its name.
func (k Kit) ColorDot(c *ui.Context, p ColorDotProps) ui.Element {
	var d float32 = 10

	col := k.SwatchColor(p.Color)

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
		{
		}

		return k.Color("accent-950"), k.Color("text-white")
	case KindLink:
		return k.TagTint("blue")
	case KindFolder:
		return k.TagTint("purple")
	case KindFile, KindImage:
		return k.TagTint("orange")
	}
	return k.Color("background-tertiary-default"), k.Color("text-secondary")
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
		{

			h, padX, size, r = 22, 9, 12, 11
		}

		return ui.Row(c).Height(h).PaddingX(padX).Radius(min(r, h/2)).Center().Gap(4).Background(bg).TextColor(ink).
			Label(p.Label).Children(func() {
			k.Icon(c.Key("icon"), kindIcon(p.Kind), size+3).AccessHidden(true)
			ui.Text(c, p.Label).FontSize(size).FontWeight(weight).AccessHidden(true)
		})
	}
	var w, h, r float32
	{

		w, h, r = 40, 32, 6
	}

	e := ui.Row(c).Size(w, h).Radius(r).Center().Clip().Background(bg).TextColor(ink).Label(p.Label)
	if p.Kind == KindImage {
		{

			e.Background(k.Color("text-white"))
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
