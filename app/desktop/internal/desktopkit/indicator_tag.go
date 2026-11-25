package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
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

// ToastStore, ToastOptions, ToastAction, ToasterProps, ToastStatus, and ToastPosition
// alias the public BoardUI Toast types for desktopkit callers.
type (
	ToastStore    = boardui.ToastStore
	ToastOptions  = boardui.ToastOptions
	ToastAction   = boardui.ToastAction
	ToasterProps  = boardui.ToasterProps
	ToastStatus   = boardui.NotificationStatus
	ToastPosition = boardui.NotificationPosition
)

const (
	ToastNeutral     = boardui.StatusNeutral
	ToastInformation = boardui.StatusInformation
	ToastSuccess     = boardui.StatusSuccess
	ToastWarning     = boardui.StatusWarning
	ToastError       = boardui.StatusError

	ToastPersistent      = boardui.ToastPersistent
	DefaultToastDuration = boardui.DefaultToastDuration
	DefaultToastLimit    = boardui.DefaultToastLimit

	ToastTopStart     = boardui.PositionTopStart
	ToastTopCenter    = boardui.PositionTopCenter
	ToastTopEnd       = boardui.PositionTopEnd
	ToastBottomStart  = boardui.PositionBottomStart
	ToastBottomCenter = boardui.PositionBottomCenter
	ToastBottomEnd    = boardui.PositionBottomEnd
)

// Toaster renders the floating BoardUI Toast stack in a top-level overlay.
func (k Kit) Toaster(c *ui.Context, store *ToastStore, p ToasterProps) {
	k.native().Toaster(c, store, p)
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
