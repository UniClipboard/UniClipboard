package desktopkit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

type ActionBarProps struct {
	// Hint is an optional display-only hint when used inline.
	Hint string
	// Status displays transient action feedback in a pill above the floating dock
	// without resizing or mutating primary action buttons.
	Status string
	// Floating overlays the centered pill dock above the parent pane's bottom
	// edge with a soft content fade that clears the scrollbar lane.
	Floating bool
}

func (k Kit) dockSurface(e ui.Element, radius float32) {
	if k.Dark {
		e.Background(ui.RGBA(34, 34, 34, .94)).
			Border(1, ui.RGBA(255, 255, 255, .10)).
			Shadow(0, 12, 28, 0, ui.RGBA(0, 0, 0, .48)).
			Shadow(0, 1, 4, 0, ui.RGBA(0, 0, 0, .32)).
			Radius(radius)
		return
	}
	e.Background(k.Color("background-primary-default").Alpha(.95)).
		Border(1, ui.RGBA(10, 10, 10, .09)).
		Shadow(0, 8, 24, 0, ui.RGBA(0, 0, 0, .10)).
		Shadow(0, 1, 3, 0, ui.RGBA(0, 0, 0, .06)).
		Radius(radius)
}

func (k Kit) ActionBar(c *ui.Context, p ActionBarProps, body func()) ui.Element {
	if p.Floating {
		base := k.Color("background-full")
		ui.Box(c.Key("fade")).Absolute().Left(0).Right(10).Bottom(0).Height(72).ZIndex(1).PassThrough().
			LinearGradient(ui.LinearGradient{
				Angle: 180,
				Stops: []ui.GradientStop{
					{Color: base.Alpha(0), At: 0},
					{Color: base.Alpha(.92), At: .72},
					{Color: base, At: 1},
				},
			})
	}
	wrap := ui.Column(c).AlignItems(ui.Center).Gap(8).Shrink(0)
	if p.Floating {
		wrap.Absolute().Left(0).Right(0).Bottom(16).ZIndex(2).PassThrough()
	}
	wrap.Children(func() {
		if p.Status != "" {
			toast := ui.Row(c.Key("status")).Height(26).Padding(0, 11).Gap(5).AlignItems(ui.Center).Shrink(0).PassThrough()
			k.dockSurface(toast, 13)
			toast.Children(func() {
				k.glyph(c, "checkCircle", 13).TextColor(k.Color("notification-success-foreground")).Shrink(0)
				k.native().Text(c, p.Status, "caption-1-medium").TextColor(k.Color("text-primary")).
					Label(p.Status).SingleLine()
			})
		}
		dock := ui.Row(c.Key("dock")).Padding(4).Gap(4).AlignItems(ui.Center).Shrink(0)
		k.dockSurface(dock, 20)
		dock.Children(func() {
			if body != nil {
				body()
			}
			if !p.Floating && p.Hint != "" {
				k.native().Text(c, p.Hint, "caption-1-regular").TextColor(k.Color("text-secondary")).
					Padding(0, 8, 0, 4).Label(p.Hint).SingleLine()
			}
		})
	})
	return wrap
}

// ActionDockPrimary draws the primary capsule button inside a floating ActionBar dock.
func (k Kit) ActionDockPrimary(c *ui.Context, label string, icon IconName, shortcut string) ui.Element {
	b := ui.ButtonBase(c).Label(label).Height(32).Padding(0, 13).Radius(16).Gap(6).
		Justify(ui.Center).AlignItems(ui.Center).Shrink(0).
		FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
	from, to := k.Color("accent-500"), k.Color("accent-600")
	pressed := b.Pressed() || (b.Focused() && c.KeyDown(ui.KeySpace))
	if pressed {
		from, to = k.Color("accent-600"), k.Color("accent-700")
	} else if b.Hovered() {
		from, to = k.Color("accent-400"), k.Color("accent-500")
	}
	b.Gradient(from, to, 180)
	scale := float32(1)
	if pressed && !c.Preferences().ReduceMotion {
		scale = .98
	}
	b.AnimatePose(ui.Pose{Scale: scale}, ui.Animation{Duration: 100 * time.Millisecond, Ease: ui.EaseOut})
	ink := k.Color("text-white")
	b.Children(func() {
		if icon != "" {
			k.Icon(c, icon, 15).TextColor(ink).Shrink(0)
		}
		k.native().Text(c, label, "body-2-medium").TextColor(ink).SingleLine()
		if shortcut != "" {
			k.native().Text(c, shortcut, "caption-2-medium").TextColor(ink.Alpha(.74)).SingleLine()
		}
	})
	return b
}

// ActionDockSeparator draws a hairline vertical divider between actions inside the floating dock.
func (k Kit) ActionDockSeparator(c *ui.Context) ui.Element {
	col := k.Color("separator-border-strong")
	if !k.Dark {
		col = k.Color("border-button-hover")
	}
	return ui.Box(c).Size(1, 16).Margin(0, 2).Shrink(0).Background(col)
}

// ActionDockMenu draws a quiet, borderless capsule menu trigger inside the floating dock.
func (k Kit) ActionDockMenu(c *ui.Context, p MenuProps) (trigger ui.Element, chosen string) {
	if p.Open == nil {
		host := ui.Row(c)
		open := ui.Local(host, "desktopkit.dockmenu.open", func() bool { return false })
		host.Children(func() {
			p.Open = open
			trigger, chosen = k.ActionDockMenu(c, p)
		})
		return trigger, chosen
	}
	m := ui.MenuBase(c, p.Open)
	trigger = m.Trigger
	trigger.Label(p.Label).Height(32).Padding(0, 12).Radius(16).Gap(6).
		Justify(ui.Center).AlignItems(ui.Center).Shrink(0).
		FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
	if trigger.Pressed() || *p.Open {
		trigger.Background(k.Color("background-tertiary-default"))
	} else if trigger.Hovered() {
		trigger.Background(k.navHoverFill())
	}
	ink := k.Color("text-primary")
	trigger.Children(func() {
		if p.Icon != "" {
			k.Icon(c, p.Icon, 15).TextColor(ink).Shrink(0)
		}
		k.native().Text(c, p.Label, "body-2-medium").TextColor(ink).SingleLine()
	})
	m.Popup(func(panel ui.Element) {
		k.popupSurface(panel)
		k.menuEntriesInto(c, m, p.Items, &chosen)
	})
	return trigger, chosen
}
