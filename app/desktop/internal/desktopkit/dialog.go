package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// AlertSuppression adds an application-owned suppression choice.
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

type AlertDialogProps struct {
	// Open is shown while true. Esc, a click outside and Cancel set it false; Confirm sets
	// it false too unless KeepOpenOnConfirm.
	Open                      *bool
	Title                     string
	Description               string
	ConfirmLabel, CancelLabel string
	// Destructive uses the source danger button and initially focuses Cancel.
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
	host := ui.Box(c).Size(0, 0).Absolute()
	st := ui.Local(host, "desktopkit.alert", func() alertMemo { return alertMemo{} })
	if !*open && st.open && st.busy && busy {
		*open = true
	}
	st.open, st.busy = *open, busy
	var initial ui.Element
	k.native().Dialog(c, boardui.DialogProps{Open: open, Title: p.Title, Description: p.Description, Width: 480}, func() {
		if body != nil {
			ui.Column(c.Key("body")).Gap(8).Children(body)
		}
		if s := p.Suppression; s != nil && s.Checked != nil {
			k.Checkbox(c.Key("suppression"), s.Checked, CheckboxProps{Label: s.Label, Disabled: busy})
		}
		if f := p.Field; f != nil && f.Value != nil {
			initial = k.native().Input(c.Key("field"), f.Value, boardui.InputProps{Label: f.Label, Placeholder: f.Placeholder, Disabled: busy})
		}
		if p.Error != "" {
			k.Text(c.Key("error"), p.Error, TextProps{Tone: ToneCritical}).Role(ui.RoleAlert)
		}
		var cancel, confirm ui.Element
		variant := Primary
		if p.Destructive {
			variant = Destructive
		}
		label := p.ConfirmLabel
		if busy {
			label = p.BusyLabel
		}
		ui.Row(c.Key("actions")).Justify(ui.End).Gap(12).Children(func() {
			cancel = k.Button(c.Key("cancel"), p.CancelLabel, ButtonProps{Disabled: busy})
			confirm = k.Button(c.Key("confirm"), label, ButtonProps{Variant: variant, Disabled: p.ConfirmDisabled, Busy: busy})
		})
		if p.Field == nil {
			if p.Destructive {
				initial = cancel
			} else {
				initial = confirm
			}
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
	// Run after DialogBase records the opener, preserving native scope restoration.
	if !busy {
		initial.AutoFocus()
	}
	if busy && !*open {
		*open = true
		c.Invalidate()
	}
	return res
}

type alertMemo struct{ open, busy bool }
