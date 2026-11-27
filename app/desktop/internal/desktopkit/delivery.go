package desktopkit

import (
	"math"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
)

const (
	deliveryHeaderHeight   float32 = 50
	deliveryRowHeight      float32 = 40
	deliveryHairlineHeight float32 = 2.5
)

// DeliveryTargetItem is the presentation data of one device or external channel
// inside a DeliveryCard.
type DeliveryTargetItem struct {
	ID            string
	Label         string
	Icon          IconName
	External      bool
	ChannelLabel  string
	Status        string
	Tone          StatusTone
	Confirmed     bool     // true when remote receipt is confirmed (full positive segment)
	Active        bool     // true while transferring or awaiting confirmation (draws bottom hairline)
	Value         *float64 // determinate progress 0..100 when Active; nil draws indeterminate hairline
	Bytes         string   // inline byte counter shown when Active && !Compact
	Reason        string   // detailed error/discard explanation shown via hover tooltip
	Retryable     bool
	RetryDisabled bool
	RetryLabel    string
	RetryTarget   string
}

// DeliveryCardProps configures the collapsible B1 delivery summary and fixed-height
// per-target list.
type DeliveryCardProps struct {
	EntryID        string
	ListLabel      string
	Summary        string
	Tone           StatusTone
	DefaultOpen    bool
	ExpandLabel    string
	CollapseLabel  string
	Compact        bool
	InDrawer       bool
	Items          []DeliveryTargetItem
	FallbackPrefix string
	FallbackValue  string
	FallbackTone   StatusTone
}

// DeliveryCardResult reports interactions for the current frame.
type DeliveryCardResult struct {
	ui.Element
	RetryTarget string
	Open        bool
}

type deliveryCardState struct {
	entryID string
	open    bool
}

// DeliveryCard renders a collapsible delivery summary strip with a segmented
// micro-bar and strictly fixed-height (40 DIP) target rows so state transitions
// between failed, queued, transferring, awaiting confirmation, and delivered
// never cause vertical layout shift.
func (k Kit) DeliveryCard(c *ui.Context, p DeliveryCardProps) DeliveryCardResult {
	border := k.Color("separator-border")
	headerBg := k.SubtleCardSurface()
	if p.InDrawer {
		headerBg = k.DrawerInnerSurface()
	}

	if len(p.Items) == 0 {
		strip := ui.Row(c.Key("fallback")).Height(deliveryRowHeight).Shrink(0).Padding(0, 12).Gap(8).
			Radius(10).AlignItems(ui.Center).Background(headerBg).Border(1, border).
			Children(func() {
				ui.Box(c.Key("dot")).Size(6.5, 6.5).Radius(3.25).Shrink(0).
					Background(k.deliveryDotColor(p.FallbackTone)).AccessHidden(true)
				ui.Row(c.Key("text")).Gap(0).AlignItems(ui.Center).Grow(1).MinWidth(0).Children(func() {
					if p.FallbackPrefix != "" {
						ui.Text(c.Key("prefix"), p.FallbackPrefix).FontSize(12).TextColor(k.Color("text-secondary")).SingleLine().Shrink(0)
					}
					ui.Text(c.Key("val"), p.FallbackValue).FontSize(12).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine().MinWidth(0).Shrink(1)
				})
			})
		return DeliveryCardResult{Element: strip}
	}

	card := ui.Column(c).Radius(10).Clip().Background(headerBg).Border(1, border)
	st := ui.Local(card, "delivery-open", func() deliveryCardState {
		return deliveryCardState{entryID: p.EntryID, open: p.DefaultOpen}
	})
	if st.entryID != p.EntryID {
		st.entryID = p.EntryID
		st.open = p.DefaultOpen
	}

	res := DeliveryCardResult{Element: card, Open: st.open}

	previewNames := make([]string, 0, len(p.Items))
	for _, it := range p.Items {
		if it.Label != "" {
			previewNames = append(previewNames, it.Label)
		}
	}
	previewText := strings.Join(previewNames, " · ")

	expandLabel := p.ExpandLabel
	if expandLabel == "" {
		expandLabel = "明细"
	}
	collapseLabel := p.CollapseLabel
	if collapseLabel == "" {
		collapseLabel = "收起"
	}

	card.Children(func() {
		// Strictly fixed 50 DIP collapsible summary header.
		trig := ui.ButtonBase(c.Key("trigger")).Height(deliveryHeaderHeight).Shrink(0).FillWidth().
			Padding(0, 12).Label(p.Summary)
		if trig.Hovered() {
			trig.Background(k.Color("background-secondary-hover"))
		}
		if trig.Clicked() {
			st.open = !st.open
			res.Open = st.open
		}
		trig.Children(func() {
			ui.Column(c.Key("inner")).FillWidth().Justify(ui.Center).Gap(7).Children(func() {
				ui.Row(c.Key("top")).Height(20).Gap(7).AlignItems(ui.Center).FillWidth().Children(func() {
					k.deliveryHeaderBadge(c.Key("badge"), p.Tone)
					ui.Text(c.Key("sum"), p.Summary).FontSize(12.5).FontWeight(600).
						TextColor(k.Color("text-primary")).SingleLine().Shrink(0)
					if !st.open && previewText != "" {
						ui.Text(c.Key("preview"), previewText).FontSize(11.5).
							TextColor(k.Color("text-tertiary")).SingleLine().Grow(1).MinWidth(0)
					} else {
						ui.Box(c.Key("spacer")).Grow(1).MinWidth(0)
					}
					toggleText := expandLabel
					if st.open {
						toggleText = collapseLabel
					}
					ui.Row(c.Key("chev-wrap")).Gap(2).AlignItems(ui.Center).Shrink(0).Children(func() {
						ui.Text(c.Key("lbl"), toggleText).FontSize(11).TextColor(k.Color("text-secondary")).SingleLine()
						k.deliveryChevron(c.Key("ic"), st.open)
					})
				})

				// Segmented micro-bar (one equal-width segment per target).
				ui.Row(c.Key("meter")).Height(4).Gap(3).FillWidth().Shrink(0).AccessHidden(true).Children(func() {
					for i, it := range p.Items {
						k.deliveryMeterSegment(c.Key(i), it)
					}
				})
			})
		})

		if !st.open {
			return
		}

		bodyBg := k.CardSurface()
		if k.Dark {
			bodyBg = k.workspaceCanvas()
		}
		list := ui.Column(c.Key("targets")).Role(ui.RoleList).Label(p.ListLabel).
			Background(bodyBg).FillWidth()
		list.DrawOver(func(pt *ui.Painter, rc ui.Rect) {
			pt.Fill(ui.Rect{X: rc.X, Y: rc.Y, W: rc.W, H: 1}, border, 0)
		})
		list.Children(func() {
			for i, it := range p.Items {
				if k.deliveryRow(c.Key(it.ID), i, it, p.Compact, border) {
					res.RetryTarget = it.RetryTarget
				}
			}
		})
	})

	return res
}

func (k Kit) deliveryDotColor(t StatusTone) ui.Color {
	switch t {
	case StatusPositive:
		return k.Color("notification-success-foreground")
	case StatusInfo:
		return k.Color("accent-500")
	case StatusWarning:
		return k.Color("status-orange-text")
	case StatusCritical:
		return k.Color("foreground-icon-error")
	default:
		return k.Color("text-tertiary")
	}
}

func (k Kit) deliveryHeaderBadge(c *ui.Context, tone StatusTone) ui.Element {
	var bg, fg ui.Color
	switch tone {
	case StatusPositive:
		bg = k.Color("notification-success-background")
		fg = k.Color("notification-success-foreground")
	case StatusInfo:
		bg = k.Color("notification-information-background")
		fg = k.Color("notification-information-foreground")
	case StatusWarning:
		bg = k.Color("status-yellow-background")
		fg = k.Color("status-yellow-text")
	case StatusCritical:
		bg = k.Color("notification-error-background")
		fg = k.Color("notification-error-foreground")
	default:
		bg = k.Color("background-tertiary-default")
		fg = k.Color("text-secondary")
	}
	e := ui.Box(c).Size(18, 18).Radius(9).Shrink(0).Background(bg).Center().AccessHidden(true)
	if tone == StatusInfo {
		return e.Children(func() { ui.Spinner(c.Key("spin")).Size(12, 12).TextColor(fg) })
	}
	return e.Draw(func(p *ui.Painter, r ui.Rect) {
		cx, cy := r.X+r.W/2, r.Y+r.H/2
		path := &ui.Path{}
		switch tone {
		case StatusPositive:
			path.MoveTo(cx-3.2, cy+0.2).LineTo(cx-0.9, cy+2.5).LineTo(cx+3.4, cy-2.4)
		default:
			path.MoveTo(cx, cy-3.6).LineTo(cx, cy+0.8).MoveTo(cx, cy+3.1).LineTo(cx, cy+3.2)
		}
		p.StrokePath(path.Cap(ui.RoundCap).Join(ui.RoundJoin), 1.7, fg)
	})
}

func (k Kit) deliveryChevron(c *ui.Context, open bool) ui.Element {
	col := k.Color("text-secondary")
	return ui.Box(c).Size(12, 12).Shrink(0).AccessHidden(true).Draw(func(p *ui.Painter, r ui.Rect) {
		cx, cy := r.X+r.W/2, r.Y+r.H/2
		path := &ui.Path{}
		if open {
			path.MoveTo(cx-3.2, cy+1.5).LineTo(cx, cy-1.8).LineTo(cx+3.2, cy+1.5)
		} else {
			path.MoveTo(cx-3.2, cy-1.5).LineTo(cx, cy+1.8).LineTo(cx+3.2, cy-1.5)
		}
		p.StrokePath(path.Cap(ui.RoundCap).Join(ui.RoundJoin), 1.5, col)
	})
}

func (k Kit) deliveryMeterSegment(c *ui.Context, it DeliveryTargetItem) ui.Element {
	seg := ui.Box(c).Grow(1).Basis(0).Height(4).Radius(2).Clip()
	if it.Active {
		track := k.Color("notification-information-background")
		fill := k.Color("accent-500")
		frac := float32(0.88)
		if it.Value != nil {
			frac = float32(math.Max(0.06, math.Min(1, *it.Value/100)))
		}
		return seg.Draw(func(p *ui.Painter, r ui.Rect) {
			p.Fill(r, track, r.H/2)
			p.Fill(ui.Rect{X: r.X, Y: r.Y, W: r.W * frac, H: r.H}, fill, r.H/2)
		})
	}
	var col ui.Color
	switch {
	case it.Tone == StatusPositive && it.Confirmed:
		col = k.Color("notification-success-foreground")
	case it.Tone == StatusPositive:
		col = k.Color("notification-success-foreground").MixOklab(k.Color("separator-border-strong"), 0.45)
	case it.Tone == StatusCritical:
		col = k.Color("foreground-icon-error")
	case it.Tone == StatusWarning:
		col = k.Color("status-orange-text")
	default:
		col = k.Color("border-button-hover")
	}
	return seg.Background(col)
}

func (k Kit) deliveryRow(c *ui.Context, index int, it DeliveryTargetItem, compact bool, border ui.Color) (retried bool) {
	row := ui.Row(c).Height(deliveryRowHeight).Shrink(0).FillWidth().Padding(0, 12).Gap(8).
		AlignItems(ui.Center).Role(ui.RoleListItem).Label(it.Label + ", " + it.Status)

	if it.Tone == StatusCritical {
		alpha := float32(0.045)
		if k.Dark {
			alpha = 0.075
		}
		row.Background(k.Color("foreground-icon-error").Alpha(alpha))
	}

	accent := k.Color("accent-500")
	ease := ui.CubicBezier(.4, 0, .2, 1)
	row.DrawOver(func(pt *ui.Painter, rc ui.Rect) {
		if index > 0 {
			pt.Fill(ui.Rect{X: rc.X, Y: rc.Y, W: rc.W, H: 1}, border, 0)
		}
		if !it.Active || rc.W <= 0 {
			return
		}
		barRect := ui.Rect{X: rc.X, Y: rc.Y + rc.H - deliveryHairlineHeight, W: rc.W, H: deliveryHairlineHeight}
		pt.Fill(barRect, accent.Alpha(0.16), 0)
		if it.Value != nil {
			frac := float32(math.Max(0, math.Min(1, *it.Value/100)))
			if frac > 0 {
				pt.Fill(ui.Rect{X: barRect.X, Y: barRect.Y, W: barRect.W * frac, H: barRect.H}, accent, 0)
			}
			return
		}
		if pt.Preferences().ReduceMotion {
			pt.Fill(ui.Rect{X: barRect.X, Y: barRect.Y, W: barRect.W * 0.72, H: barRect.H}, accent.Alpha(0.75), 0)
			return
		}
		const period = 1150 * time.Millisecond
		phase := float32(pt.Now().UnixNano()%period.Nanoseconds()) / float32(period.Nanoseconds())
		eased := ease(phase)
		segW := barRect.W * 0.38
		x := barRect.X - segW + (barRect.W+segW)*eased
		seg := ui.Rect{X: x, Y: barRect.Y, W: segW, H: barRect.H}
		pt.Clip(barRect, 0, func() {
			pt.FillGradient(seg, ui.LinearGradient{
				Angle: 90,
				Stops: []ui.GradientStop{
					{Color: accent.Alpha(0), At: 0},
					{Color: accent.Alpha(0.95), At: 0.45},
					{Color: accent.Alpha(0.95), At: 0.65},
					{Color: accent.Alpha(0), At: 1},
				},
			}, barRect.H/2)
		})
		pt.AnimationFrame()
	})

	icon := it.Icon
	if icon == "" {
		icon = IconDesktop
	}
	iconColor := k.Color("text-tertiary")
	switch it.Tone {
	case StatusCritical:
		iconColor = k.Color("foreground-icon-error")
	case StatusWarning:
		iconColor = k.Color("status-orange-text")
	case StatusInfo:
		iconColor = k.Color("accent-500")
	}

	statusColor := k.Color("text-secondary")
	statusWeight := 400
	switch it.Tone {
	case StatusPositive:
		if it.Confirmed {
			statusColor = k.Color("status-lime-text")
		}
	case StatusInfo:
		if k.Dark {
			statusColor = k.Color("accent-300")
		} else {
			statusColor = k.Color("accent-600")
		}
		statusWeight = 600
	case StatusWarning:
		statusColor = k.Color("status-orange-text")
		statusWeight = 600
	case StatusCritical:
		statusColor = k.Color("text-error-primary")
		statusWeight = 600
	}

	row.Children(func() {
		k.Icon(c.Key("icon"), icon, 14).TextColor(iconColor).Shrink(0).AccessHidden(true)

		ui.Row(c.Key("name-group")).Grow(1).MinWidth(0).Gap(5).AlignItems(ui.Center).Children(func() {
			ui.Text(c.Key("name"), it.Label).FontSize(12.5).FontWeight(550).
				TextColor(k.Color("text-primary")).SingleLine().MinWidth(0).Shrink(1).Tooltip(it.Label)
			if it.External && it.ChannelLabel != "" {
				chBg := k.Color("background-tertiary-default")
				if k.Dark {
					chBg = k.Color("text-primary").Alpha(0.09)
				}
				ui.Box(c.Key("ch")).Padding(1, 5).Radius(4).Shrink(0).
					Background(chBg).Children(func() {
					ui.Text(c.Key("ch-t"), it.ChannelLabel).FontSize(10).FontWeight(600).
						TextColor(k.Color("text-tertiary")).SingleLine()
				})
			}
		})

		ui.Row(c.Key("trailing")).Shrink(0).Gap(6).AlignItems(ui.Center).Children(func() {
			if it.Active && it.Bytes != "" && !compact {
				ui.Text(c.Key("bytes"), it.Bytes).FontSize(11).TextColor(k.Color("text-tertiary")).SingleLine().Shrink(0)
			}

			statusEl := ui.Box(c.Key("st-wrap")).Padding(1, 0).Shrink(0).Children(func() {
				ui.Text(c.Key("st"), it.Status).FontSize(11.5).FontWeight(statusWeight).
					TextColor(statusColor).SingleLine()
			})
			if it.Reason != "" {
				dashCol := statusColor.Alpha(0.45)
				statusEl.DrawOver(func(pt *ui.Painter, rc ui.Rect) {
					y := rc.Y + rc.H - 0.5
					for x := rc.X; x < rc.X+rc.W; x += 4 {
						w := float32(2.2)
						if x+w > rc.X+rc.W {
							w = rc.X + rc.W - x
						}
						pt.Fill(ui.Rect{X: x, Y: y, W: w, H: 1}, dashCol, 0)
					}
				})
				k.WithTooltip(c.Key("reason-tip"), statusEl, it.Reason)
			}

			if it.Retryable {
				retryText := it.RetryLabel
				if retryText == "" {
					retryText = "重试"
				}
				btn := ui.ButtonBase(c.Key("retry")).Height(22).Padding(0, 7).Radius(6).Shrink(0).
					Center().Disabled(it.RetryDisabled).
					Background(k.CardSurface()).Border(1, k.Color("border-button-default")).
					Label(retryText + " " + it.Label)
				if btn.Hovered() && !it.RetryDisabled {
					btn.Background(k.Color("background-secondary-hover")).Border(1, k.Color("border-button-hover"))
				}
				fg := k.Color("text-primary")
				if it.RetryDisabled {
					fg = k.Color("foreground-disabled")
				}
				btn.Children(func() {
					ui.Row(c.Key("rb")).Gap(3).AlignItems(ui.Center).Children(func() {
						k.Icon(c.Key("ric"), IconRetry, 11).TextColor(fg)
						ui.Text(c.Key("rl"), retryText).FontSize(11).FontWeight(600).TextColor(fg).SingleLine()
					})
				})
				if btn.Clicked() && !it.RetryDisabled {
					retried = true
				}
			}
		})
	})

	return retried
}
