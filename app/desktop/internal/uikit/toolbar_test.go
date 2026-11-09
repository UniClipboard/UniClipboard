package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestToolbarButtons(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		sent, pinned := 0, 0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Children(func() {
				k.Toolbar(c, "Item actions", func() {
					k.ToolbarGroup(c, func() {
						if k.ToolbarButton(c.Key("send"), "Send", ToolbarButtonProps{Variant: Primary, Icon: IconSend}).Clicked() {
							sent++
						}
						if k.ToolbarButton(c.Key("pin"), "", ToolbarButtonProps{Icon: IconPin, Label: "Pin"}).Clicked() {
							pinned++
						}
					})
					k.ToolbarDivider(c)
					if k.ToolbarButton(c.Key("off"), "Disabled", ToolbarButtonProps{Disabled: true}).Clicked() {
						t.Error("disabled toolbar button clicked")
					}
				})
			})
		}
		tt := ui.NewTester(view, 600, 300)
		if err := tt.Click("Send"); err != nil || sent != 1 {
			t.Fatalf("send: %v %d", err, sent)
		}
		_ = tt.Click("Disabled")
		if _, ok := tt.AccessByRole(ui.RoleToolbar, "Item actions"); !ok {
			t.Fatal("toolbar not exposed with its label")
		}
		// roving focus: Right moves to the next item, Space presses it.
		_ = tt.Click("Send")
		tt.Key(0, ui.KeyRight)
		tt.Key(0, ui.KeySpace)
		if pinned != 1 {
			t.Fatalf("keyboard roving + activation failed: pinned=%d", pinned)
		}
	})
}
