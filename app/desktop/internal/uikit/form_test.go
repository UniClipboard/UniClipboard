package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestFormField(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		user, pass, submits := "", "", 0

		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(16).Gap(10).Children(func() {
				k.FormField(c.Key("user"), FormFieldProps{Label: "Device name", Value: &user, Hint: "Shown to peers"})
				r := k.FormField(c.Key("pass"), FormFieldProps{Label: "Passphrase", Value: &pass, Password: true,
					RevealShow: "Show passphrase", RevealHide: "Hide passphrase", Error: "Too short"})
				if r.Submitted {
					submits++
				}
			})
		}
		tt := ui.NewTester(view, 400, 400)
		for _, s := range []string{"Device name", "Shown to peers", "Too short"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		tt.Key(0, ui.KeyTab)
		tt.Type("Mac")
		if user != "Mac" {
			t.Fatalf("user=%q", user)
		}
		tt.Key(0, ui.KeyTab)
		tt.Type("secret")
		tt.Key(0, ui.KeyEnter)
		if pass != "secret" || submits != 1 {
			t.Fatalf("pass=%q submits=%d", pass, submits)
		}
		if tt.HasText("secret") {
			t.Fatal("password must be masked")
		}
		if err := tt.Click("Show passphrase"); err != nil {
			t.Fatal(err)
		}
		if _, ok := tt.AccessByRole(ui.RoleButton, "Hide passphrase"); !ok {
			t.Fatal("reveal button did not flip its name")
		}
	})
}
