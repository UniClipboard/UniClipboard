package uikit

import "github.com/egoist/mygo/ui"

// SheetVariant selects the sheet width.
type SheetVariant int

const (
	// SheetManagement keeps the master-detail width (device browsing); the zero value.
	SheetManagement SheetVariant = iota
	// SheetTask is the compact surface of a focused task.
	SheetTask
)

// SheetProps mirrors contract.ts SheetProps. The exit animation (and so onClosed) is not
// reproduced: the sheet disappears when Open turns false, so a caller can unmount at once.
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
	prim, sec, _ := k.fgTone()
	task := p.Variant == SheetTask
	ui.DialogBase(c, open, func(backdrop, panel ui.Element) {
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
			backdrop.Justify(ui.Start)
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

		content := ui.Scroll(c).Grow(1)
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
