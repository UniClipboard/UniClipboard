package desktopkit

import "github.com/egoist/mygo/ui"

type TaskPanelProps struct {
	Title       string
	Description string
	// Stage is the step caption, e.g. "Step 2 of 3".
	Stage string
	// BackLabel shows a Back button at the start of the footer ("" hides it).
	BackLabel    string
	BackDisabled bool
}

// TaskPanel lays out one task: header (title, description, stage), scrolling content (body)
// and a footer with Back at the start and the primary actions (actions, may be nil) at the end.
// It reports whether Back was used this frame.
func (k Kit) TaskPanel(c *ui.Context, p TaskPanelProps, body, actions func()) (back bool) {
	panel := ui.Column(c).Gap(24).Padding(12, 4, 4).Grow(1).Label(p.Title)

	// The sheet is already task-width: content and footer fill it.

	panel.Children(func() {
		centered := true
		head := ui.Column(c).Gap(8)
		if centered {
			head.AlignItems(ui.Center).MaxWidth(k.taskWidth()).AlignSelf(ui.Center)

		} else {
			head.MaxWidth(544)
		}
		head.Children(func() {
			stage := func() {
				if p.Stage != "" {
					k.Text(c, p.Stage, TextProps{Role: RoleCaption, Tone: ToneSecondary})

				}
			}

			k.Text(c, p.Title, TextProps{Role: RoleTitle, Emphasized: true})
			if p.Description != "" {
				k.Text(c, p.Description, TextProps{Tone: ToneSecondary})
			}
			{
				stage()
			}
		})

		content := k.Scroll(c).Grow(1).Padding(4)
		if centered {
			content.MaxWidth(k.taskWidth()).AlignSelf(ui.Center).WidthPercent(100)

		} else {
			content.MaxWidth(544)
		}
		content.Children(func() {
			ui.Column(c).Gap(16).Children(func() {
				if body != nil {
					body()
				}
			})
		})

		foot := ui.Row(c).Gap(16).AlignItems(ui.Center).Padding(16, 0, 0)

		foot.Children(func() {
			if p.BackLabel != "" {
				if k.Button(c.Key("back"), p.BackLabel, ButtonProps{Disabled: p.BackDisabled}).Clicked() && !p.BackDisabled {
					back = true
				}
			}
			ui.Spacer(c)
			if actions != nil {
				ui.Row(c).Gap(8).Justify(ui.End).Children(actions)
			}
		})
	})
	return back
}

func (k Kit) taskWidth() float32 {
	return 480
}
