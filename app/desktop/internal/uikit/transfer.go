package uikit

import (
	"math"

	"github.com/egoist/mygo/ui"
)

// TransferStatusItem is the presentation data of one file or content unit in a transfer.
type TransferStatusItem struct {
	ID     string
	Label  string
	Status string
	Tone   StatusTone
	// Value is progress 0-100; nil is indeterminate.
	Value  *float64
	Bytes  string
	Reason string
}

// TransferStatusProps mirrors contract.ts TransferStatusProps.
type TransferStatusProps struct {
	Label   string // accessible name of the trigger
	Title   string // heading of the details popup
	Status  string
	Summary string
	Tone    StatusTone
	// Value is progress 0-100 for StatusInfo; nil is indeterminate.
	Value *float64
	// Retryable makes a click on the trigger report Retry (unless Disabled). Inspecting the
	// details by hover or focus is never prevented.
	Retryable bool
	Disabled  bool
	Items     []TransferStatusItem
}

// TransferStatusResult carries the retry request of the frame.
type TransferStatusResult struct {
	ui.Element
	Retry bool
}

func (k Kit) layTransferColor(t StatusTone) ui.Color {
	switch t {
	case StatusInfo:
		return k.layTok().Accent
	case StatusNeutral:
		return k.layTok().Fg2
	}
	return k.layStatusColor(t)
}

// TransferIndicator is the 18 px mark of a transfer: a progress ring (a spinner when value is
// nil) while the tone is StatusInfo, otherwise a check, warning or info glyph.
func (k Kit) TransferIndicator(c *ui.Context, tone StatusTone, value *float64, label string) ui.Element {
	col := k.layTransferColor(tone)
	e := ui.Box(c).Size(18, 18).Shrink(0)
	if tone == StatusInfo {
		e.Role(ui.RoleProgress).Label(label)
		if value == nil {
			return e.Children(func() { ui.Spinner(c.Key("spin")).Size(18, 18) })
		}
		v := math.Max(0, math.Min(100, *value))
		e.Range(0, 100, v)
		return e.Draw(func(p *ui.Painter, r ui.Rect) {
			cx, cy, rad := r.X+r.W/2, r.Y+r.H/2, r.W/2-1.5
			p.Stroke(ui.Rect{X: cx - rad, Y: cy - rad, W: 2 * rad, H: 2 * rad}, col.Alpha(.16), rad, 2.2)
			if v <= 0 {
				return
			}
			path := &ui.Path{}
			n := int(math.Max(2, v*0.6))
			for i := 0; i <= n; i++ {
				a := -math.Pi/2 + 2*math.Pi*(v/100)*float64(i)/float64(n)
				x, y := cx+rad*float32(math.Cos(a)), cy+rad*float32(math.Sin(a))
				if i == 0 {
					path.MoveTo(x, y)
				} else {
					path.LineTo(x, y)
				}
			}
			p.StrokePath(path.Cap(ui.RoundCap), 2.2, col)
		})
	}
	e.Role(ui.RoleImage).Label(label)
	return e.Draw(func(p *ui.Painter, r ui.Rect) {
		cx, cy, rad := r.X+r.W/2, r.Y+r.H/2, r.W/2-1
		p.Stroke(ui.Rect{X: cx - rad, Y: cy - rad, W: 2 * rad, H: 2 * rad}, col, rad, 1.6)
		path := &ui.Path{}
		switch tone {
		case StatusPositive:
			path.MoveTo(cx-3.5, cy+.2).LineTo(cx-1, cy+2.8).LineTo(cx+3.6, cy-2.6)
		case StatusNeutral, StatusInfo:
			path.MoveTo(cx, cy-4).LineTo(cx, cy-3.9).MoveTo(cx, cy-1.5).LineTo(cx, cy+4)
		default: // warning, critical: exclamation mark
			path.MoveTo(cx, cy-4.5).LineTo(cx, cy+1).MoveTo(cx, cy+3.6).LineTo(cx, cy+3.7)
		}
		p.StrokePath(path.Cap(ui.RoundCap).Join(ui.RoundJoin), 1.6, col)
	})
}

// TransferItem is one file row of the transfer details: completed files stay compact (a check
// mark), only active files expose byte progress, failures show their reason.
func (k Kit) TransferItem(c *ui.Context, item TransferStatusItem) ui.Element {
	t := k.layTok()
	return ui.Column(c).Padding(10, 8).Role(ui.RoleListItem).Label(item.Label+", "+item.Status).
		InsetShadow(0, 1, 0, 0, t.Border).Gap(4).Children(func() {
		ui.Row(c.Key("head")).Gap(10).AlignItems(ui.Center).Children(func() {
			ui.Box(c.Key("icon")).TextColor(t.Fg3).AccessHidden(true).Children(func() { k.Icon(c, IconFile, 16) })
			name := TextProps{}
			if item.Tone == StatusPositive {
				name.Tone = ToneSecondary
			}
			name.Truncate = true
			ui.Row(c.Key("name")).Grow(1).MinWidth(0).Children(func() {
				k.Text(c, item.Label, name).Tooltip(item.Label)
			})
			stTone := ToneSecondary
			if item.Tone == StatusCritical {
				stTone = ToneCritical
			}
			k.Text(c.Key("status"), item.Status, TextProps{Role: RoleCaption, Tone: stTone}).Shrink(0)
		})
		if item.Tone == StatusInfo {
			ui.Column(c.Key("progress")).Margin(4, 0, 0, 26).Gap(6).Children(func() {
				v := -1.0
				if item.Value != nil {
					v = math.Max(0, math.Min(1, *item.Value/100))
				}
				ui.Progress(c.Key("bar"), v).Height(4).Radius(2).Label(item.Label).
					Background(t.Shade).TextColor(t.Accent)
				if item.Bytes != "" {
					k.Text(c.Key("bytes"), item.Bytes, TextProps{Role: RoleCaption, Tone: ToneSecondary})
				}
			})
		}
		if item.Reason != "" && item.Tone != StatusPositive {
			ui.Box(c.Key("reason")).Margin(0, 0, 0, 26).Children(func() {
				k.Text(c, item.Reason, TextProps{Role: RoleCaption, Tone: ToneCritical})
			})
		}
	})
}

type transferHover struct{ open, panelHot, dismissed bool }

// TransferStatus is the transfer inspector: an indicator button whose hover / keyboard-focus
// popup lists the title, status, summary and one TransferItem per file.
func (k Kit) TransferStatus(c *ui.Context, p TransferStatusProps) TransferStatusResult {
	t := k.layTok()
	radius := float32(8)
	if k.Skin == Windows {
		radius = k.Px("borderRadiusMedium")
	}
	trigger := ui.ButtonBase(c).Size(32, 32).Shrink(0).Radius(radius).Label(p.Label).
		AccessDisabled(p.Disabled).AlignItems(ui.Center).Justify(ui.Center)
	if trigger.Hovered() || trigger.Focused() {
		trigger.Background(t.Hover)
	}
	trigger.Children(func() { k.TransferIndicator(c.Key("ind"), p.Tone, p.Value, p.Label) })
	res := TransferStatusResult{Element: trigger}
	if trigger.Clicked() && p.Retryable && !p.Disabled {
		res.Retry = true
	}

	st := ui.Local(trigger, "hover", func() transferHover { return transferHover{} })
	hot := trigger.Hovered() || trigger.Focused() || st.panelHot
	switch {
	case !hot:
		st.open, st.dismissed = false, false
	case !st.dismissed:
		st.open = true
	}
	pop := ui.PopoverBase(c.Key("details"), trigger, &st.open, func(panel ui.Element) {
		panel.Width(368).MaxWidthPercent(92).Radius(12).Clip().Label(p.Title).Role(ui.RoleDialog).
			Background(k.Sem("popover")).Border(1, t.CardStroke).
			Shadow(0, 8, 24, 0, ui.RGBA(0, 0, 0, .22)).MarginY(8)
		st.panelHot = panel.Hovered() || panel.FocusWithin()
		ui.Column(c.Key("hdr")).Padding(16).Gap(6).Children(func() {
			ui.Row(c.Key("heading")).Gap(12).AlignItems(ui.Baseline).Children(func() {
				ui.Row(c.Key("name")).Grow(1).MinWidth(0).Children(func() {
					k.Text(c, p.Title, TextProps{Emphasized: true, Truncate: true})
				})
				tone := ToneSecondary
				if p.Tone == StatusCritical {
					tone = ToneCritical
				}
				k.Text(c.Key("st"), p.Status, TextProps{Role: RoleCaption, Tone: tone}).Shrink(0)
			})
			if p.Summary != "" {
				k.Text(c.Key("sum"), p.Summary, TextProps{Role: RoleCaption, Tone: ToneSecondary})
			}
		})
		if len(p.Items) > 0 {
			ui.Scroll(c.Key("files")).Role(ui.RoleList).Label(p.Title).MaxHeight(304).Padding(0, 8, 8).Children(func() {
				for _, it := range p.Items {
					k.TransferItem(c.Key(it.ID), it)
				}
			})
		}
	})
	if pop.Dismissed() {
		st.dismissed = true
	}
	return res
}
