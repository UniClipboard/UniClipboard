package desktopkit

import (
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// SheetVariant selects the sheet width.
type SheetVariant int

const (
	// SheetManagement keeps the master-detail width (device browsing); the zero value.
	SheetManagement SheetVariant = iota
	// SheetTask is the compact surface of a focused task.
	SheetTask
)

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

// Sheet keeps its native modal scope and live body until source exit motion ends.
// The application owns the task state and clears it only through OnClosed.
func (k Kit) Sheet(c *ui.Context, p SheetProps, body func()) (done bool) {
	open := p.Open
	if open == nil {
		open = new(bool)
	}
	owner := ui.Box(c).Absolute().Size(0, 0)
	state := ui.Local(owner, "sheet-presentation", func() sheetPresentation { return sheetPresentation{} })
	if !state.initialized {
		owner.Jump("sheet-visibility", 0)
		owner.Jump("sheet-backdrop", 0)
		state.initialized = true
	}
	target := float32(0)
	if *open {
		state.present = true
		target = 1
	}
	duration := time.Duration(boardui.Number("dialog-ms")) * time.Millisecond
	progress := owner.AnimateTo("sheet-visibility", target, ui.Animation{Duration: duration, Ease: ui.CubicBezier(.32, .72, 0, 1)})
	scrimProgress := owner.AnimateTo("sheet-backdrop", target, ui.Animation{Duration: duration, Ease: ui.CubicBezier(0, 0, .2, 1)})
	if state.present && !*open && progress == 0 && scrimProgress == 0 {
		state.present = false
		if p.OnClosed != nil {
			p.OnClosed()
		}
		c.Invalidate()
	}
	if !state.present {
		return false
	}
	// Keep DialogBase's original live tree through the last exit frame. Its scope
	// owns keyboard isolation and restores the opener when that tree is removed.
	visible := true
	task := p.Variant == SheetTask
	w, h := c.Size()
	panel := ui.DialogBase(c, &visible, func(backdrop, panel ui.Element) {
		if !visible {
			*open = false
			c.Invalidate()
		}
		backdrop.Background(k.scrim()).Opacity(scrimProgress)
		width := float32(760)
		if task {
			width = 576
		}
		panel.Label(p.Title).TabStop(false).FocusRing(false).Column().TextColor(k.Color("text-primary")).
			Width(width).MaxWidth(w - 32).MaxHeight(h - 32).Padding(boardui.Number("dialog-padding")).Gap(boardui.Number("dialog-header-body-gap")).
			Radius(boardui.Number("dialog-radius")).Background(k.Color("background-full"))
		k.shadowed(panel, "xs")
		// Native layout transitions resize live content. Position and size share the
		// same curve, so the centered parent keeps both axes centered during resize.
		panel.Transition(ui.ElementTransition{Duration: duration, Ease: ui.CubicBezier(.32, .72, 0, 1), Position: true, Size: true})
		scale := float32(.85) + .15*progress
		panel.Scale(scale, scale).Opacity(progress).Blur(4 * (1 - progress))
		if p.Title != "" || p.Header != nil {
			ui.Row(c.Key("heading")).Gap(14).AlignItems(ui.Center).Children(func() {
				if p.Title != "" {
					k.native().Text(c.Key("title"), p.Title, "title-2-medium").Role(ui.RoleHeading).Level(2)
				}
				if p.Header != nil {
					p.Header()
				}
			})
		}
		content := k.Scroll(c.Key("content")).Grow(1).MinHeight(0)
		content.Children(func() {
			if body != nil {
				body()
			}
		})
		if p.Footer != nil || p.Done != "" {
			ui.Row(c.Key("footer")).Gap(8).AlignItems(ui.Center).TextColor(k.Color("text-secondary")).Children(func() {
				if p.Footer != nil {
					p.Footer()
				}
				ui.Spacer(c)
				if p.Done != "" && k.Button(c.Key("done"), p.Done, ButtonProps{Variant: Primary}).Clicked() {
					done = true
				}
			})
		}
	})
	// DialogBase records its opener after the body callback. Focus the shell only
	// after that point, matching the public BoardUI Dialog's native lifecycle.
	panel.AutoFocus()
	if done {
		c.Invalidate()
	}
	return done
}

type sheetPresentation struct{ initialized, present bool }
