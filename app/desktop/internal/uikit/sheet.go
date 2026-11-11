package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// SheetVariant selects the sheet width.
type SheetVariant int

const (
	// SheetManagement keeps the master-detail width (device browsing); the zero value.
	SheetManagement SheetVariant = iota
	// SheetTask is the compact surface of a focused task.
	SheetTask
)

// SheetProps mirrors contract.ts SheetProps. Keep calling Sheet while closed
// so the macOS overlay can finish its exit transition.
type SheetProps struct {
	Variant SheetVariant
	// Open shows the sheet; Esc and a click on the scrim set it false.
	Open  *bool
	Title string
	// Header adds controls beside the title, e.g. a filter field (may be nil).
	Header func()
	// Footer adds leading footer content, e.g. add/remove buttons and a status line (may be nil).
	Footer func()
	// Done is the label of the default button that closes the sheet ("Done"); "" hides it.
	Done string
	// OnClosed runs after the panel leaves. Clear transient content state here.
	OnClosed func()
}

// Sheet is a modal pane for a focused task: macOS sheet dropping from the title bar, Adwaita
// dialog, WinUI ContentDialog. Focus is trapped inside and returns to the opener on close.
// body builds the content; the result reports that Done was used (the caller usually also
// sets Open to false, which Sheet does not do for it, as in the contract).
func (k Kit) Sheet(c *ui.Context, p SheetProps, body func()) (done bool) {
	open := p.Open
	if open == nil {
		open = new(bool)
	}
	owner := ui.Box(c).Absolute().Size(0, 0)
	present := ui.Local(owner, "sheet-present", func() bool { return false })
	initialized := ui.Local(owner, "sheet-initialized", func() bool { return false })
	if !*initialized {
		owner.Jump("sheet-visibility", 0)
		*initialized = true
	}
	progress := float32(0)
	if *open {
		*present = true
		progress = 1
	}
	if k.Skin == MacOS {
		motion := ui.Animation{Duration: 300 * time.Millisecond, Ease: ui.CubicBezier(.2, 0, 0, 1)}
		if !*open {
			motion = ui.Animation{Duration: 220 * time.Millisecond, Ease: ui.EaseIn}
		}
		progress = owner.AnimateTo("sheet-visibility", progress, motion)
	}
	if *present && !*open && progress == 0 {
		*present = false
		if p.OnClosed != nil {
			p.OnClosed()
		}
	}
	if !*present {
		return false
	}
	// Keep the original tree and its rendering path until motion completes.
	// DialogBase owns modal input and restores focus when the live tree leaves.
	visible := true
	prim, sec, _ := k.fgTone()
	task := p.Variant == SheetTask
	ui.DialogBase(c, &visible, func(backdrop, panel ui.Element) {
		if !visible {
			*open = false
		}
		backdrop.Background(k.scrim())
		panel.Label(p.Title).Column().TextColor(prim)
		width := float32(760)
		if k.Skin == Windows {
			width = 800
		}
		if task {
			width = 576
			if k.Skin == Windows {
				width = 608
			}
		}
		panel.Width(width).MaxWidthPercent(94).MaxHeightPercent(92)
		titleSize := float32(20)
		switch k.Skin {
		case MacOS:
			// Animate the centered frame together with its size. Keep live content
			// laid out at each intermediate size instead of scaling its text.
			panel.Transition(ui.ElementTransition{
				Position: true, Size: true,
				Duration: 280 * time.Millisecond,
				Ease:     ui.CubicBezier(.2, 0, 0, 1),
			})
			scrim := k.scrim()
			scrim.A = uint8(float32(scrim.A) * progress)
			backdrop.Justify(ui.Start).Background(scrim)
			panel.TranslatePercent(0, -100*(1-progress))
			panel.Background(k.Color("window")).Padding(16, 20).Gap(14)
			panel.Radius(0, 0, 16, 16)
			k.shadowed(panel, "menu-shadow")
			titleSize = 17
		case Windows:
			k.dialogSurface(panel)
			panel.Gap(0).Clip()
			titleSize = k.Px("fontSizeBase500")
		default:
			k.dialogSurface(panel)
			panel.Padding(16, 20, 20).Gap(14)
		}

		head := ui.Row(c).Gap(14).AlignItems(ui.Center)
		if k.Skin == Windows {
			head.Padding(24, 24, 12)
		}
		head.Children(func() {
			ui.Text(c, p.Title).FontSize(titleSize).Bold()
			if p.Header != nil {
				p.Header()
			}
		})

		content := k.Scroll(c).Grow(1)
		switch k.Skin {
		case MacOS:
			if !task {
				content.Radius(10).Background(k.Color("content")).Border(.5, k.Color("separator"))
			}
		case Windows:
			content.Padding(0, 24, 20)
		default:
			if !task {
				content.Radius(12).Background(k.Color("card"))
				k.shadowed(content, "card-shadow")
			}
		}
		content.Children(func() {
			if body != nil {
				body()
			}
		})

		if p.Footer == nil && p.Done == "" {
			return
		}
		foot := ui.Row(c).Gap(8).AlignItems(ui.Center).TextColor(sec)
		switch k.Skin {
		case MacOS:
			foot.FontSize(12)
		case Windows:
			foot.Padding(20, 24).Gap(12).FontSize(k.Px("fontSizeBase200")).
				Background(k.Color("w11-base")).Border(0, ui.Color{})
		default:
			foot.FontSize(13)
		}
		foot.Children(func() {
			if p.Footer != nil {
				p.Footer()
			}
			ui.Spacer(c)
			if p.Done != "" {
				v := Primary
				if k.Skin == Windows {
					v = Standard
				}
				b := k.Button(c.Key("done"), p.Done, ButtonProps{Variant: v})
				if k.Skin == Windows {
					b.MinWidth(120)
				}
				if b.Clicked() {
					done = true
				}
			}
		})
	})
	return done
}
