package boardui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

type SegmentedItem struct {
	Value, Label, Icon string
	IconOnly, Disabled bool
	Leading            func(size float32)
}

// SegmentedProps is the fixed source single-select group. Variant is solid or plain.
type SegmentedProps struct {
	Value          *string
	Items          []SegmentedItem
	Label, Variant string
	Disabled       bool
}

// SegmentedControl delegates arrows, Home/End and Tab to FocusGroup. ButtonBase
// owns Enter/Space. Arrow navigation alone does not commit or clear selection.
func (k Kit) SegmentedControl(c *ui.Context, p SegmentedProps) ui.Element {
	if p.Value == nil {
		panic("boardui: SegmentedControl requires Value")
	}
	if p.Variant != "" && p.Variant != "solid" && p.Variant != "plain" {
		panic("boardui: unsupported Segmented variant")
	}
	plain := p.Variant == "plain"
	track := ui.Row(c).Label(p.Label).Role(ui.RoleGroup).FocusGroup(ui.Horizontal).AlignItems(ui.Start).Gap(2).Radius(10)
	if !plain {
		track.Padding(4).Background(Token(k.Dark, "segmented-control-background"))
	}
	track.Children(func() {
		rows := make([]ui.Element, len(p.Items))
		selectedIndex := -1
		firstEnabled := -1
		for i, it := range p.Items {
			rows[i] = ui.ButtonBase(c.Key(it.Value)).Role(ui.RoleToggleButton).Label(it.Label).Disabled(p.Disabled || it.Disabled).DimDisabled(false).AlignItems(ui.Center).Justify(ui.Center).Padding(4, 10).Radius(6).MinWidth(0)
			if firstEnabled < 0 && !it.Disabled {
				firstEnabled = i
			}
			if rows[i].Clicked() && !p.Disabled && !it.Disabled {
				*p.Value = it.Value
			}
		}
		for i, it := range p.Items {
			if *p.Value == it.Value {
				selectedIndex = i
			}
		}
		for i, it := range p.Items {
			e := rows[i]
			selected := i == selectedIndex
			entry := selected || selectedIndex < 0 && i == firstEnabled
			e.Checked(selected).TabStop(entry)
			style, ink := "body-regular", "text-secondary"
			if selected {
				style, ink = "body-medium", "text-primary"
			} else if e.Hovered() {
				ink = "text-primary"
			}
			dressType(e, style)
			e.TextColor(Token(k.Dark, ink)).FocusRingColor(Token(k.Dark, "border-focus-ring")).FocusRingOffset(0)
			if p.Disabled || it.Disabled {
				e.Opacity(.5)
			}
			e.Transition(ui.ElementTransition{Duration: 200 * time.Millisecond, Ease: ui.CubicBezier(.25, .1, .25, 1), Colors: true})
			e.Children(func() {
				if selected && !plain {
					// Native matched geometry preserves the source's shared sliding
					// surface across item parents, including different label widths.
					thumb := ui.Box(c.Key("selection")).Absolute().Left(0).Top(0).Right(0).Bottom(0).Radius(6).Background(Token(k.Dark, "segmented-control-selected-background")).AccessHidden(true)
					k.shadows(thumb, "2xs")
					thumb.Transition(ui.ElementTransition{Duration: 200 * time.Millisecond, Ease: ui.CubicBezier(.25, .1, .25, 1), Position: true, Size: true, Match: segmentedSurface{track.ID()}})
				}
				ui.Row(c.Key("content")).AlignItems(ui.Center).ZIndex(1).Children(func() {
					if it.Leading != nil {
						it.Leading(16)
					} else if it.Icon != "" {
						ui.Icon(c.Key("icon"), IconSVG(it.Icon)).Size(16, 16).AccessHidden(true)
					}
					if !it.IconOnly {
						k.Text(c.Key("label"), it.Label, style).TextColor(Token(k.Dark, ink)).SingleLine()
					}
				})
			})
		}
	})
	return track
}

type segmentedSurface struct{ Group uint64 }
