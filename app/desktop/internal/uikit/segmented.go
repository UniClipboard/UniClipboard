package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// SegmentedItem mirrors contract.ts SegmentedItem.
type SegmentedItem struct {
	Value, Label string
	Icon         IconName
	// IconOnly shows only the icon; Label stays the accessible name and tooltip.
	IconOnly bool
}

// SegmentedControlProps mirrors contract.ts SegmentedControlProps: mutually exclusive options
// side by side with radio-group semantics (one tab stop; arrow keys move focus and selection
// together).
type SegmentedControlProps struct {
	Items []SegmentedItem
	// Value is the selected item's Value, updated when the user picks another segment.
	Value *string
	Label string // accessible name of the group
	// Toolbar uses the macOS glass capsule instead of the inset form control.
	Toolbar bool
}

// SegmentedResult is what SegmentedControl reports.
type SegmentedResult struct {
	Element ui.Element
	Changed bool // the selection changed in this frame
}

// SegmentedControl draws the segments: Adwaita linked buttons, a macOS capsule track with a
// raised thumb, a Win11 toolbar-style radio group.
func (k Kit) SegmentedControl(c *ui.Context, p SegmentedControlProps) SegmentedResult {
	pal := k.fldPal()
	idx := -1
	if p.Value != nil {
		for i, it := range p.Items {
			if it.Value == *p.Value {
				idx = i
			}
		}
	}
	sel := idx
	sg := ui.SegmentedBase(c, &sel, len(p.Items))
	track := sg.Track.Label(p.Label).AlignItems(ui.Stretch)
	switch k.Skin {
	case MacOS:
		track.Height(24).Padding(2).Radius(8).Background(pal.fill)
		if p.Toolbar {
			track.Height(32).Padding(3).Radius(16).Background(ui.Transparent)
			k.glassSurface(track)
		}
	case Windows:
		track.Padding(2).Gap(2).Radius(4).Background(pal.fill).Border(1, pal.edge)
	default:
		track.Height(34).Radius(6).Background(pal.bg).Clip()
	}
	track.Children(func() {
		for i, it := range p.Items {
			seg := sg.Segment(i).Label(it.Label).AlignItems(ui.Center).Justify(ui.Center).Gap(6).
				TextColor(pal.fg).FontSize(pal.size).Grow(1).MinWidth(0)
			on := i == sel
			iconOnly := it.IconOnly && it.Icon != ""
			if iconOnly {
				seg.Tooltip(it.Label)
			}
			switch k.Skin {
			case MacOS:
				seg.PaddingX(12).Radius(6).FontSize(12).TextColor(pal.fg2)
				if iconOnly {
					seg.PaddingX(9)
				}
				if on && !p.Toolbar {
					seg.Background(k.Color("ctrl")).Shadow(0, .5, 1, 0, ui.RGBA(0, 0, 0, .25)).FontWeight(600).TextColor(pal.fg)
				}
				if p.Toolbar {
					// Grow(0) retains the zero basis installed by Grow(1).
					// Restore the basis so the fixed-width segments cannot collapse.
					seg.Basis(34).Width(34).MinWidth(34).Padding(0).Radius(13).Grow(0).Shrink(0)
				}
			case Windows:
				seg.PaddingX(10).Height(28).Radius(4)
				switch {
				case on:
					seg.Background(pal.bg).Border(1, pal.edge)
				case seg.Hovered():
					seg.Background(pal.hover)
				}
			default:
				seg.PaddingX(10).MinWidth(34).FontWeight(700)
				switch {
				case on:
					seg.Background(pal.press)
				case seg.Hovered():
					seg.Background(pal.hover)
				}
				if i > 0 {
					seg.InsetShadow(1, 0, 0, 0, pal.edge)
				}
			}
			if k.Skin != Linux && seg.FocusVisible() {
				seg.Shadow(0, 0, 0, 3, pal.focus)
			}
			k.pressFeedback(c, seg, .92)
			seg.Children(func() {
				if it.Icon != "" {
					k.Icon(c.Key("icon"), it.Icon, 16)
				}
				if !iconOnly {
					ui.Text(c.Key("label"), it.Label).SingleLine()
				}
			})
		}
	})
	if k.Skin == MacOS && p.Toolbar && sel >= 0 {
		position := track.AnimateTo("uikit.selection", float32(sel), ui.Animation{
			Spring: ui.SpringOptions{Duration: 350 * time.Millisecond, Bounce: .12},
		})
		fill := k.Color("fill-2")
		track.Draw(func(p *ui.Painter, r ui.Rect) {
			p.Fill(ui.Rect{X: r.X + 3 + 34*position, Y: r.Y + 3, W: 34, H: r.H - 6}, fill, 13)
		})
	}
	changed := sel != idx
	if changed && sel >= 0 && sel < len(p.Items) && p.Value != nil {
		*p.Value = p.Items[sel].Value
	}
	return SegmentedResult{Element: track, Changed: changed}
}
