package desktopkit

import (
	"github.com/egoist/mygo/ui"
)

type TagProps struct {
	Color TagColor
	// Detail is secondary text after the name, e.g. "2 of 3".
	Detail string
	// Partial: only some of the selected items carry the tag (drawn dashed).
	Partial bool
	// RemoveLabel shows a remove button named by it, e.g. "Remove #finance".
	RemoveLabel string
	// ActionLabel makes the tag itself a button, e.g. "Add #docker to all 3".
	ActionLabel string
}

// TagResult is what Tag returns: the element, and which of its buttons fired this frame.
type TagResult struct {
	ui.Element
	Removed   bool // the remove button was clicked
	Activated bool // the tag itself (ActionLabel) was clicked
}

// Tag is a coloured tag chip, optionally removable and/or itself a button.
func (k Kit) Tag(c *ui.Context, name string, p TagProps) TagResult {
	bg, ink := k.TagTint(p.Color)
	var h, padX, padR, r, size float32
	nameWeight := 700
	{

		h, padX, padR, r, size = 22, 9, 6, 11, 12
	}

	if p.RemoveLabel == "" {
		padR = padX
	}
	var res TagResult
	e := ui.Row(c).Height(h).Padding(0, padR, 0, padX).Radius(r).Gap(4).Center().Background(bg).TextColor(ink)
	if p.Partial {
		e.DrawOver(func(pt *ui.Painter, rc ui.Rect) {
			pt.StrokeDashed(ui.Rect{X: rc.X + .5, Y: rc.Y + .5, W: rc.W - 1, H: rc.H - 1}, ink, r, 1)
		})
	}
	content := func() {
		ui.Text(c, name).FontSize(size).FontWeight(nameWeight)
		if p.Detail != "" {
			ui.Text(c, p.Detail).FontSize(size).FontWeight(400).Opacity(.8)
		}
	}
	e.Children(func() {
		if p.ActionLabel != "" {
			b := ui.ButtonBase(c).Label(p.ActionLabel).Gap(4).Radius(r).Children(content)
			res.Activated = b.Clicked()
		} else {
			ui.Row(c).Gap(4).Center().Children(content)
		}
		if p.RemoveLabel != "" {
			res.Removed = k.RemoveButton(c, p.RemoveLabel).Clicked()
		}
	})
	res.Element = e
	return res
}

type NoticeProps struct {
	Tone  NoticeTone
	Title string
	Text  string
	// ActionLabel shows a secondary command button.
	ActionLabel string
	// DismissLabel shows a close button named by it.
	DismissLabel string
}

// NoticeResult reports which button of an InlineNotice fired this frame.
type NoticeResult struct {
	ui.Element
	Acted     bool
	Dismissed bool
}

// InlineNotice is a status message pinned in the content, not a floating toast.
func (k Kit) InlineNotice(c *ui.Context, p NoticeProps) NoticeResult {
	var bg, iconColor ui.Color
	glyph := [...]string{ToneInfo: "info", ToneSuccess: "checkCircle", ToneWarning: "warning", ToneError: "xCircle"}[p.Tone]
	var r, titleSize, textSize float32 = 12, 14, 13
	titleWeight := 700
	var textColor ui.Color
	{

		bg = k.Color([...]string{ToneInfo: "notification-information-background", ToneSuccess: "notification-success-background",
			ToneWarning: "status-yellow-background", ToneError: "background-tertiary-error"}[p.Tone])
		iconColor = k.Color([...]string{ToneInfo: "notification-information-foreground", ToneSuccess: "notification-success-foreground",
			ToneWarning: "status-yellow-text", ToneError: "text-error-primary"}[p.Tone])
		textColor = k.Color("text-secondary")
		if p.Tone == ToneInfo {
			textColor = k.Color("text-primary")
		}
	}

	var res NoticeResult
	e := ui.Row(c).Padding(10, 12).Radius(r).Gap(10).AlignItems(ui.Center).Background(bg).Label(p.Title)

	e.Children(func() {
		k.glyph(c, glyph, 16).TextColor(iconColor)
		ui.Text(c, p.Title).FontSize(titleSize).FontWeight(titleWeight).NoWrap()
		if p.Text != "" {
			ui.Text(c, p.Text).FontSize(textSize).TextColor(textColor)
		}
		if p.ActionLabel != "" || p.DismissLabel != "" {
			ui.Box(c).Grow(1)
			if p.ActionLabel != "" {
				res.Acted = k.Button(c.Key("action"), p.ActionLabel, ButtonProps{Size: SizeSmall}).Clicked()
			}
			if p.DismissLabel != "" {
				res.Dismissed = k.RemoveButton(c.Key("dismiss"), p.DismissLabel).Clicked()
			}
		}
	})
	res.Element = e
	return res
}

func (k Kit) Disclosure(c *ui.Context, label string, open *bool, body func()) ui.Element {

	size := float32(13)
	fg := k.Color("text-secondary")

	return ui.Column(c).Gap(8).Children(func() {
		hd := ui.ButtonBase(c.Key("hd")).Label(label).Expanded(*open).Gap(6).Padding(2, 0).Radius(6).
			TextColor(fg).AlignSelf(ui.Start)
		hd.Children(func() {
			if *open {
				k.glyph(c, "chevronDown", 11)
			} else {
				k.glyph(c, "chevronRight", 11)
			}
			ui.Text(c, label).FontSize(size)
		})
		if hd.Clicked() {
			*open = !*open
		}
		if *open {
			ui.Column(c.Key("body")).Children(body)
		}
	})
}
