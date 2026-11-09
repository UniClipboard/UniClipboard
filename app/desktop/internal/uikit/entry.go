package uikit

import "github.com/egoist/mygo/ui"

// EntryTone colours the status line of an EntryPage.
type EntryTone int

const (
	EntryNeutral EntryTone = iota
	EntryPositive
	EntryCritical
)

// EntryPageStatus is the status line above the title.
type EntryPageStatus struct {
	Text string // empty hides the line
	Tone EntryTone
	Icon IconName
	// Busy: work is in progress, a spinner replaces the icon.
	Busy bool
}

// EntryPageProps mirrors contract.ts EntryPageProps.
type EntryPageProps struct {
	// AppIcon is the app icon above the title (nil draws none).
	AppIcon     ui.ImageSource
	Status      EntryPageStatus
	Title       string
	Description string
	// BackLabel is the footer's leading Back button; empty hides it.
	BackLabel    string
	BackDisabled bool
}

// EntryPageResult carries the Back click of the frame.
type EntryPageResult struct {
	ui.Element
	BackClicked bool
}

// EntryPage is a full-window page shown before the main window content (first-run setup,
// startup, recovery, unlock): app icon, status line, title and description above the content
// (body), a footer with Back at the leading and actions (the primary one last) at the trailing
// end. macOS/Linux centre the column; Windows aligns it to the leading edge.
func (k Kit) EntryPage(c *ui.Context, p EntryPageProps, body func(), actions func()) EntryPageResult {
	t := k.layTok()
	windows := k.Skin == Windows
	var res EntryPageResult
	root := ui.Column(c).Fill().Background(t.Window).Role(ui.RoleRegion).Label(p.Title)
	res.Element = root
	maxW, iconSize := float32(420), float32(64)
	pad := []float32{0, 32}
	if windows {
		maxW, iconSize, pad = 480, 40, []float32{0, 40}
	}
	root.Children(func() {
		ui.Scroll(c.Key("scroll")).Grow(1).MinHeight(0).Children(func() {
			ui.Column(c.Key("align")).Padding(pad...).AlignItems(ui.Center).Justify(ui.Center).MinHeight(0).Children(func() {
				ui.Column(c.Key("column")).Width(maxW).MaxWidthPercent(100).Padding(24, 0, 28).Children(func() {
					align := ui.Center
					if windows {
						align = ui.Start
					}
					ui.Column(c.Key("header")).AlignItems(align).Children(func() {
						if p.AppIcon != nil {
							ui.Image(c.Key("icon"), p.AppIcon).Size(iconSize, iconSize).Margin(0, 0, 16).AccessHidden(true)
						}
						if p.Status.Text != "" {
							tone := ToneSecondary
							switch p.Status.Tone {
							case EntryPositive:
								tone = TonePositive
							case EntryCritical:
								tone = ToneCritical
							}
							ui.Row(c.Key("status")).Gap(5).AlignItems(ui.Center).Margin(0, 0, 6).Role(ui.RoleStatus).Children(func() {
								if p.Status.Busy {
									ui.Spinner(c.Key("spin")).Size(13, 13)
								} else if p.Status.Icon != "" {
									k.Icon(c.Key("icon"), p.Status.Icon, 14)
								}
								k.Text(c.Key("text"), p.Status.Text, TextProps{Role: RoleCaption, Tone: tone, Emphasized: true})
							})
						}
						k.Text(c.Key("title"), p.Title, TextProps{Role: RoleTitle, Emphasized: true}).
							Level(1).TextAlign(align)
						if p.Description != "" {
							ui.Box(c.Key("desc")).MaxWidth(380).Margin(6, 0, 0).Children(func() {
								k.Text(c, p.Description, TextProps{Role: RoleCallout, Tone: ToneSecondary}).TextAlign(align)
							})
						}
					})
					if body != nil {
						ui.Column(c.Key("body")).Margin(26, 0, 0).Gap(16).AlignItems(ui.Stretch).Children(body)
					}
				})
			})
		})
		if p.BackLabel != "" || actions != nil {
			ui.Row(c.Key("footer")).Shrink(0).MinHeight(64).Padding(0, 24).Gap(8).AlignItems(ui.Center).Children(func() {
				if p.BackLabel != "" {
					b := k.Button(c.Key("back"), p.BackLabel, ButtonProps{Size: SizeLarge, Icon: IconBack, Disabled: p.BackDisabled})
					res.BackClicked = b.Clicked()
				}
				ui.Spacer(c)
				if actions != nil {
					actions()
				}
			})
		}
	})
	return res
}
