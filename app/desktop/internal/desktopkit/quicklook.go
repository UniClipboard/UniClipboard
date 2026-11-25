package desktopkit

import "github.com/egoist/mygo/ui"

type QuickLookProps struct {
	// Open shows the preview; Esc, Space, the close button and a click outside set it false.
	Open *bool
	// Title is e.g. "2 of 21".
	Title string
	// CloseLabel is the accessible name of the close button.
	CloseLabel string
	// Src is the picture to show (nil draws an empty stage); Alt is its accessible name.
	Src           ui.ImageSource
	Alt           string
	ImageDrawOver func(*ui.Painter, ui.Rect)
	// PreviousLabel / NextLabel name the step buttons ("" hides one); Left / Right arrows step.
	PreviousLabel, NextLabel string
	// PrimaryLabel names the main command, e.g. copy and close; Return runs it ("" hides it).
	PrimaryLabel string
}

// QuickLookResult reports what the user did this frame.
type QuickLookResult struct {
	Previous, Next, Primary bool
}

func (k Kit) QuickLook(c *ui.Context, p QuickLookProps, actions func()) (res QuickLookResult) {
	open := p.Open
	if open == nil {
		open = new(bool)
	}
	ui.DialogBase(c, open, func(backdrop, panel ui.Element) {
		backdrop.Background(k.scrim())
		panel.Label(p.Title).Column().Clip()
		fg := k.Color("text-primary")
		k.dialogSurface(panel).WidthPercent(88).HeightPercent(88)

		// Keys: a focused button keeps its own Space/Return (the innermost handler wins).
		if panel.Shortcut(0, ui.KeySpace) {
			*open = false
		}
		if p.PreviousLabel != "" && panel.Shortcut(0, ui.KeyLeft) {
			res.Previous = true
		}
		if p.NextLabel != "" && panel.Shortcut(0, ui.KeyRight) {
			res.Next = true
		}
		if p.PrimaryLabel != "" && panel.Shortcut(0, ui.KeyEnter) {
			res.Primary = true
		}

		closeBtn := func() {
			b := ui.ButtonBase(c.Key("close")).Label(p.CloseLabel).Tooltip(p.CloseLabel)

			{
				b.Size(32, 32).Radius(k.ControlRadius()).Justify(ui.Center).AlignItems(ui.Center)
				if b.Hovered() {
					b.Background(k.Color("background-primary-hover"))
				}
			}
			b.Children(func() {

				{
					k.Icon(c, IconDismiss, 16)
				}
			})
			if b.Clicked() {
				*open = false
			}
		}
		bar := ui.Row(c).AlignItems(ui.Center).Gap(10)

		{
			bar.Height(40).Padding(0, 12)
		}
		bar.Children(func() {

			ui.Text(c, p.Title).FontSize(13).Bold().Shrink(0)
			ui.Spacer(c)
			if actions != nil {
				actions()
			}

			{
				closeBtn()
			}
		})

		stage := ui.Row(c).Grow(1).Shrink(1).MinHeight(0).Clip().Justify(ui.Center).AlignItems(ui.Center).Padding(12, 56)

		stage.Children(func() {
			if p.Src != nil {
				image := ui.Image(c, p.Src).Fit(ui.Contain).WidthPercent(100).HeightPercent(100).Label(p.Alt).Radius(6)
				if p.ImageDrawOver != nil {
					image.DrawOver(p.ImageDrawOver)
				}
			}
			step := func(label string, left bool) bool {
				b := ui.ButtonBase(c.Key(label)).Label(label).Tooltip(label).Absolute().TopPercent(50).
					Size(34, 34).Margin(-17, 0, 0).Radius(17).Justify(ui.Center).AlignItems(ui.Center)

				{
					b.Background(k.Color("background-secondary-default"))
				}
				if left {
					b.Left(12)
				} else {
					b.Right(12)
				}
				b.Draw(func(pt *ui.Painter, r ui.Rect) {
					var path ui.Path
					cx, cy := r.X+r.W/2, r.Y+r.H/2
					d := float32(1)
					if left {
						d = -1
					}
					path.MoveTo(cx-2.5*d, cy-6)
					path.LineTo(cx+3.5*d, cy)
					path.LineTo(cx-2.5*d, cy+6)
					pt.StrokePath(&path, 2, fg)
				})
				return b.Clicked()
			}
			if p.PreviousLabel != "" && step(p.PreviousLabel, true) {
				res.Previous = true
			}
			if p.NextLabel != "" && step(p.NextLabel, false) {
				res.Next = true
			}
		})

		if p.PrimaryLabel != "" {
			ui.Row(c).Padding(10, 14).Justify(ui.End).Children(func() {
				if k.Button(c.Key("primary"), p.PrimaryLabel, ButtonProps{Variant: Primary}).Clicked() {
					res.Primary = true
				}
			})
		}
	})
	return res
}
