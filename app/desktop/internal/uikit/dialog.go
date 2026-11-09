package uikit

import "github.com/egoist/mygo/ui"

// AlertSuppression is a "do not ask again" checkbox (NSAlert suppression button).
type AlertSuppression struct {
	Label   string
	Checked *bool
}

// AlertField is a text field the user must fill before confirming, e.g. "Type RESET to
// confirm"; it takes the initial focus.
type AlertField struct {
	Label       string
	Value       *string
	Placeholder string
}

// AlertDialogProps mirrors contract.ts AlertDialogProps. Callbacks become the AlertResult
// returned by AlertDialog.
type AlertDialogProps struct {
	// Open is shown while true. Esc, a click outside and Cancel set it false; Confirm sets
	// it false too unless KeepOpenOnConfirm.
	Open                      *bool
	Title                     string
	Description               string
	ConfirmLabel, CancelLabel string
	// Destructive: the confirmed action destroys data; each skin applies its platform's
	// destructive styling and initial focus goes to Cancel so Return never confirms by accident.
	Destructive bool
	// KeepOpenOnConfirm is the asynchronous confirmation (onConfirm returning a promise): the
	// alert stays open, the caller sets BusyLabel / Error and closes it on success.
	KeepOpenOnConfirm bool
	Suppression       *AlertSuppression
	Field             *AlertField
	ConfirmDisabled   bool
	// BusyLabel (non-empty) means the confirmed work is running: it replaces the confirm
	// label, disables both buttons and the alert cannot be dismissed.
	BusyLabel string
	Error     string
}

// AlertResult reports what the user did this frame.
type AlertResult struct {
	Confirmed bool // the confirm button was used
	Cancelled bool // the Cancel button was used (not Esc / click outside)
}

// AlertDialog shows a modal alert; body adds content under the description (may be nil).
// Focus is trapped inside while open, Esc closes it and focus returns to the opener
// (DialogBase).
func (k Kit) AlertDialog(c *ui.Context, p AlertDialogProps, body func()) (res AlertResult) {
	open := p.Open
	if open == nil {
		open = new(bool)
	}
	busy := p.BusyLabel != ""
	// A running confirmation cannot be dismissed: reopen what Esc / the backdrop closed.
	host := ui.Box(c).Size(0, 0).Absolute()
	st := ui.Local(host, "uikit.alert", func() alertMemo { return alertMemo{} })
	if !*open && st.open && st.busy {
		*open = true
	}
	st.open, st.busy = *open, busy

	prim, sec, _ := k.fgTone()
	ui.DialogBase(c, open, func(backdrop, panel ui.Element) {
		backdrop.Background(k.scrim())
		k.dialogSurface(panel)
		panel.Label(p.Title).Description(p.Description).Column()
		switch k.Skin {
		case MacOS:
			panel.Width(316).MaxWidthPercent(92).Padding(20, 18, 16).Gap(8).AlignItems(ui.Center)
		case Windows:
			panel.Width(448).MaxWidthPercent(92).Clip()
		default:
			panel.Width(330).MaxWidthPercent(92).Padding(24, 22, 18).Gap(10).AlignItems(ui.Center)
		}
		winPad := func(e ui.Element) {
			if k.Skin == Windows {
				e.Padding(0, 24).AlignSelf(ui.Stretch)
			}
		}

		if k.Skin == MacOS && p.Destructive {
			ui.Row(c).Size(52, 52).Radius(12).Background(k.Color("fill-2")).Justify(ui.Center).AlignItems(ui.Center).
				TextColor(sec).Children(func() { k.Icon(c, IconDelete, 26) })
		}
		switch k.Skin {
		case MacOS:
			ui.Text(c, p.Title).FontSize(13).Bold().TextColor(prim)
			if p.Description != "" {
				ui.Text(c, p.Description).FontSize(11).TextColor(sec).Center()
			}
		case Windows:
			winPad(ui.Text(c, p.Title).FontSize(k.Px("fontSizeBase500")).Bold().Margin(24, 0, 12))
			if p.Description != "" {
				winPad(ui.Text(c, p.Description).FontSize(k.Px("fontSizeBase300")).TextColor(prim))
			}
		default:
			ui.Text(c, p.Title).FontSize(20).Bold()
			if p.Description != "" {
				ui.Text(c, p.Description).FontSize(14).TextColor(sec)
			}
		}
		if body != nil {
			col := ui.Column(c).Gap(6)
			winPad(col)
			col.Children(body)
		}
		if s := p.Suppression; s != nil && s.Checked != nil {
			cb := ui.Checkbox(c, s.Checked, s.Label).FontSize(13)
			if k.Skin == Windows {
				winPad(cb)
				cb.Margin(12, 0, 0)
			} else {
				cb.AlignSelf(ui.Start)
			}
		}
		if f := p.Field; f != nil && f.Value != nil {
			col := ui.Column(c).Gap(6).AlignSelf(ui.Stretch).Margin(4, 0, 0)
			if k.Skin == Windows {
				col.Margin(16, 24, 0)
			}
			col.Children(func() {
				ui.Text(c, f.Label).FontSize(13).Bold()
				in := ui.TextInput(c, f.Value).Placeholder(f.Placeholder).Label(f.Label).
					Padding(0, 8).Height(30).Radius(k.ControlRadius()).
					Border(1, k.dividerColor()).Disabled(busy)
				in.AutoFocus()
			})
		}
		if p.Error != "" {
			e := ui.Text(c, p.Error).FontSize(13).TextColor(k.dangerText()).Label(p.Error)
			if k.Skin == Windows {
				winPad(e)
				e.Margin(12, 24, 0)
			}
		}

		confirmV := Primary
		if p.Destructive && k.Skin != Windows {
			confirmV = Destructive
		}
		size := SizeMedium
		if k.Skin == MacOS {
			size = SizeLarge
		}
		confirmLabel := p.ConfirmLabel
		if busy {
			confirmLabel = p.BusyLabel
		}
		var confirm, cancel ui.Element
		mkCancel := func() {
			cancel = k.Button(c.Key("cancel"), p.CancelLabel, ButtonProps{Size: size, Disabled: busy})
			cancel.Grow(1)
			if p.Destructive && p.Field == nil {
				cancel.AutoFocus() // Return must not confirm data loss by accident
			}
		}
		mkConfirm := func() {
			confirm = k.Button(c.Key("confirm"), confirmLabel,
				ButtonProps{Variant: confirmV, Size: size, Disabled: p.ConfirmDisabled || busy})
			confirm.Grow(1)
			if !p.Destructive && p.Field == nil {
				confirm.AutoFocus()
			}
		}
		actions := func() {
			if k.Skin == Linux { // Linux: Cancel | Confirm; others: Confirm, then Cancel
				mkCancel()
				mkConfirm()
			} else {
				mkConfirm()
				mkCancel()
			}
		}
		switch k.Skin {
		case MacOS:
			ui.Column(c).AlignSelf(ui.Stretch).Padding(6, 0, 0).Gap(6).Children(actions)
		case Windows:
			ui.Row(c).AlignSelf(ui.Stretch).Margin(24, 0, 0).Padding(24).Gap(8).
				Background(k.Color("w11-base")).Children(actions)
		default:
			ui.Row(c).AlignSelf(ui.Stretch).Padding(8, 0, 0).Gap(10).Children(actions)
		}
		if confirm.Clicked() && !p.ConfirmDisabled && !busy {
			res.Confirmed = true
			if !p.KeepOpenOnConfirm {
				*open = false
			}
		}
		if cancel.Clicked() && !busy {
			res.Cancelled = true
			*open = false
		}
	})
	return res
}

type alertMemo struct{ open, busy bool }
