package uikit

import "github.com/egoist/mygo/ui"

// TaskPanelProps mirrors contract.ts TaskPanelProps: a focused task inside a management
// sheet. Each skin owns header, form and action layout.
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
	if k.Skin == MacOS {
		// The sheet is already task-width: content and footer fill it.
		panel.Gap(16).Padding(4, 0, 0)
	}
	panel.Children(func() {
		centered := k.Skin != Windows
		head := ui.Column(c).Gap(8)
		if centered {
			head.AlignItems(ui.Center).MaxWidth(k.taskWidth()).AlignSelf(ui.Center)
			if k.Skin == MacOS {
				head.MaxWidth(10000).Gap(4)
			}
		} else {
			head.MaxWidth(544)
		}
		head.Children(func() {
			stage := func() {
				if p.Stage != "" {
					e := k.Text(c, p.Stage, TextProps{Role: RoleCaption, Tone: ToneSecondary})
					if k.Skin == Windows {
						e.TextColor(k.Color("colorBrandForeground1"))
					}
				}
			}
			if k.Skin == Windows || k.Skin == MacOS {
				stage()
			}
			k.Text(c, p.Title, TextProps{Role: RoleTitle, Emphasized: true})
			if p.Description != "" {
				k.Text(c, p.Description, TextProps{Tone: ToneSecondary})
			}
			if k.Skin == Linux {
				stage()
			}
		})

		content := k.Scroll(c).Grow(1).Padding(4)
		if centered {
			content.MaxWidth(k.taskWidth()).AlignSelf(ui.Center).WidthPercent(100)
			if k.Skin == MacOS {
				content.MaxWidth(10000)
			}
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

		if k.Skin == Windows {
			ui.Box(c).Height(1).Background(k.Color("w11-card-stroke"))
		}
		foot := ui.Row(c).Gap(16).AlignItems(ui.Center).Padding(16, 0, 0)
		if k.Skin == MacOS {
			foot.Padding(4, 0, 0)
		}
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
	if k.Skin == Linux {
		return 480
	}
	return 416
}
