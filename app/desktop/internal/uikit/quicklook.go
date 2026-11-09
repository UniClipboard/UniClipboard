package uikit

import "github.com/egoist/mygo/ui"

// QuickLookProps mirrors contract.ts QuickLookProps. The optional commands are plain labels:
// QuickLook reports which one was used in QuickLookResult.
type QuickLookProps struct {
	// Open shows the preview; Esc, Space, the close button and a click outside set it false.
	Open *bool
	// Title is e.g. "2 of 21".
	Title string
	// CloseLabel is the accessible name of the close button.
	CloseLabel string
	// Src is the picture to show (nil draws an empty stage); Alt is its accessible name.
	Src ui.ImageSource
	Alt string
	// PreviousLabel / NextLabel name the step buttons ("" hides one); Left / Right arrows step.
	PreviousLabel, NextLabel string
	// PrimaryLabel names the main command, e.g. copy and close; Return runs it ("" hides it).
	PrimaryLabel string
}

// QuickLookResult reports what the user did this frame.
type QuickLookResult struct {
	Previous, Next, Primary bool
}

// QuickLook is a large preview over the window (design ME4; the macOS QLPreviewPanel is not
// used). Focus is trapped inside; Esc and Space close it, the arrow keys step, Return runs
// the primary command. actions adds commands to the title bar (may be nil).
func (k Kit) QuickLook(c *ui.Context, p QuickLookProps, actions func()) (res QuickLookResult) {
	open := p.Open
	if open == nil {
		open = new(bool)
	}
	ui.DialogBase(c, open, func(backdrop, panel ui.Element) {
		backdrop.Background(ui.RGBA(0, 0, 0, .35))
		panel.Label(p.Title).Column().Clip()
		var fg ui.Color
		switch k.Skin {
		case Windows:
			k.dialogSurface(panel)
			panel.Width(1024).MaxWidthPercent(94).Height(704).MaxHeightPercent(92)
			fg = k.Color("colorNeutralForeground1")
		default:
			r := float32(16)
			if k.Skin == Linux {
				r = 15
			}
			panel.Radius(r).Background(ui.RGBA(30, 30, 32, .92)).WidthPercent(88).HeightPercent(88)
			k.shadowed(panel, map[SkinID]string{MacOS: "win-shadow", Linux: "dialog-shadow"}[k.Skin])
			fg = ui.RGBA(255, 255, 255, 1)
		}
		panel.TextColor(fg)

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
			if k.Skin == MacOS {
				b.Size(12, 12).Radius(6).Background(ui.RGBA(255, 95, 87, 1)).Justify(ui.Center).AlignItems(ui.Center)
			} else {
				b.Size(32, 32).Radius(k.ControlRadius()).Justify(ui.Center).AlignItems(ui.Center)
				if b.Hovered() {
					b.Background(ui.RGBA(128, 128, 128, .25))
				}
			}
			b.Children(func() {
				if k.Skin == MacOS {
					k.Icon(c, IconDismiss, 9).TextColor(ui.RGBA(0, 0, 0, .6))
				} else {
					k.Icon(c, IconDismiss, 16)
				}
			})
			if b.Clicked() {
				*open = false
			}
		}
		bar := ui.Row(c).AlignItems(ui.Center).Gap(10)
		if k.Skin == Windows {
			bar.MinHeight(48).Padding(0, 8, 0, 16).Border(0, ui.Color{})
		} else {
			bar.Height(40).Padding(0, 12)
		}
		bar.Children(func() {
			if k.Skin == MacOS {
				closeBtn()
			}
			ui.Text(c, p.Title).FontSize(13).Bold().Shrink(0)
			ui.Spacer(c)
			if actions != nil {
				actions()
			}
			if k.Skin == Windows && p.PrimaryLabel != "" {
				if k.Button(c.Key("primary"), p.PrimaryLabel, ButtonProps{Variant: Primary}).Clicked() {
					res.Primary = true
				}
			}
			if k.Skin != MacOS {
				closeBtn()
			}
		})

		stage := ui.Row(c).Grow(1).Shrink(1).MinHeight(0).Clip().Justify(ui.Center).AlignItems(ui.Center).Padding(12, 56)
		if k.Skin == Windows {
			stage.Background(k.Color("w11-code-bg"))
		}
		stage.Children(func() {
			if p.Src != nil {
				ui.Image(c, p.Src).Fit(ui.Contain).WidthPercent(100).HeightPercent(100).Label(p.Alt).Radius(6)
			}
			step := func(label string, left bool) bool {
				b := ui.ButtonBase(c.Key(label)).Label(label).Tooltip(label).Absolute().TopPercent(50).
					Size(34, 34).Margin(-17, 0, 0).Radius(17).Justify(ui.Center).AlignItems(ui.Center)
				if k.Skin == Windows {
					b.Background(k.Color("colorNeutralBackground1")).TextColor(fg)
				} else {
					b.Background(ui.RGBA(255, 255, 255, .12))
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

		if k.Skin != Windows && p.PrimaryLabel != "" {
			ui.Row(c).Padding(10, 14).Justify(ui.End).Children(func() {
				if k.Button(c.Key("primary"), p.PrimaryLabel, ButtonProps{Variant: Primary}).Clicked() {
					res.Primary = true
				}
			})
		}
	})
	return res
}
