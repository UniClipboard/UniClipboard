package uikit

import "github.com/egoist/mygo/ui"

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
				if on {
					seg.Background(k.Color("ctrl")).Shadow(0, .5, 1, 0, ui.RGBA(0, 0, 0, .25)).FontWeight(600).TextColor(pal.fg)
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
			if k.Skin != Linux && seg.Focused() {
				seg.Shadow(0, 0, 0, 3, pal.focus)
			}
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
	changed := sel != idx
	if changed && sel >= 0 && sel < len(p.Items) && p.Value != nil {
		*p.Value = p.Items[sel].Value
	}
	return SegmentedResult{Element: track, Changed: changed}
}
