package desktopkit

import (
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

type SegmentedItem struct {
	Value, Label string
	Icon         IconName
	IconOnly     bool
}
type SegmentedControlProps struct {
	Items   []SegmentedItem
	Value   *string
	Label   string
	Toolbar bool
}
type SegmentedResult struct {
	Element ui.Element
	Changed bool
}

type toolbarSegmentedSurface struct{ Group uint64 }

func (k Kit) SegmentedControl(c *ui.Context, p SegmentedControlProps) SegmentedResult {
	host := ui.Box(c).MinWidth(0)
	value := p.Value
	if value == nil {
		value = ui.Local(host, "value", func() string { return "" })
	}
	before := *value
	if p.Toolbar {
		var track ui.Element
		host.Children(func() {
			track = k.toolbarSegmentedControl(c.Key("segments"), value, p)
		})
		return SegmentedResult{Element: track, Changed: before != *value}
	}
	items := make([]boardui.SegmentedItem, len(p.Items))
	for i, it := range p.Items {
		items[i] = boardui.SegmentedItem{Value: it.Value, Label: it.Label, IconOnly: it.IconOnly}
		if it.Icon != "" {
			icon := it.Icon
			idx := i
			items[i].Leading = func(size float32) { k.Icon(c.Key(idx), icon, size).AccessHidden(true) }
		}
	}
	var track ui.Element
	host.Children(func() {
		track = k.native().SegmentedControl(c.Key("segments"), boardui.SegmentedProps{Value: value, Items: items, Label: p.Label})
	})
	return SegmentedResult{Element: track, Changed: before != *value}
}

// toolbarSegmentedControl renders the compact bordered BoardUI toolbar switcher
// (30 DIP tall, 2 DIP inset, 26×24 icon segments, hairline border and sliding thumb).
func (k Kit) toolbarSegmentedControl(c *ui.Context, value *string, p SegmentedControlProps) ui.Element {
	trackBg := ui.RGB(241, 241, 244)
	selBg := ui.RGB(255, 255, 255)
	if k.Dark {
		trackBg = ui.RGB(20, 20, 23)
		selBg = ui.RGB(44, 44, 50)
	}
	track := ui.Row(c).Label(p.Label).Role(ui.RoleGroup).FocusGroup(ui.Horizontal).
		Height(30).Padding(2).Gap(1).Radius(8).AlignItems(ui.Center).Shrink(0).
		Background(trackBg).Border(1, k.Color("separator-border"))

	dur := 200 * time.Millisecond
	if c.Preferences().ReduceMotion {
		dur = 0
	}
	track.Children(func() {
		rows := make([]ui.Element, len(p.Items))
		selectedIndex := -1
		for i, it := range p.Items {
			btn := ui.ButtonBase(c.Key(it.Value)).Role(ui.RoleToggleButton).Label(it.Label).Tooltip(it.Label).
				DimDisabled(false).AlignItems(ui.Center).Justify(ui.Center).Radius(6).Shrink(0)
			if it.IconOnly {
				btn.Size(26, 24)
			} else {
				btn.Height(24).Padding(0, 8).Gap(5)
			}
			if btn.Clicked() {
				*value = it.Value
			}
			rows[i] = btn
		}
		for i, it := range p.Items {
			if *value == it.Value {
				selectedIndex = i
			}
		}
		for i, it := range p.Items {
			e := rows[i]
			selected := i == selectedIndex
			entry := selected || (selectedIndex < 0 && i == 0)
			e.Checked(selected).TabStop(entry).FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(1)
			ink := k.Color("foreground-icon-secondary")
			if selected || e.Hovered() {
				ink = k.Color("text-primary")
			}
			e.TextColor(ink)
			if dur > 0 {
				e.Transition(ui.ElementTransition{Duration: dur, Ease: ui.CubicBezier(.25, .1, .25, 1), Colors: true})
			}
			e.Children(func() {
				if selected {
					thumb := ui.Box(c.Key("selection")).Absolute().Left(0).Top(0).Right(0).Bottom(0).
						Radius(6).Background(selBg).AccessHidden(true)
					if !k.Dark {
						thumb.Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, .06))
					}
					if dur > 0 {
						thumb.Transition(ui.ElementTransition{
							Duration: dur,
							Ease:     ui.CubicBezier(.25, .1, .25, 1),
							Position: true,
							Size:     true,
							Match:    toolbarSegmentedSurface{track.ID()},
						})
					}
				}
				ui.Row(c.Key("content")).AlignItems(ui.Center).Justify(ui.Center).Gap(5).ZIndex(1).Children(func() {
					if it.Icon != "" {
						k.Icon(c.Key("ic"), it.Icon, 14).TextColor(ink).AccessHidden(true)
					}
					if !it.IconOnly {
						k.Text(c.Key("lbl"), it.Label, TextProps{Role: RoleCallout, Emphasized: selected}).TextColor(ink).SingleLine()
					}
				})
			})
		}
	})
	return track
}
