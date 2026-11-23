// Package showcase composes the independent BoardUI fixture. It owns synthetic
// form data only and never connects to a service or system clipboard.
package showcase

import (
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// App owns all domain state. Native controls own only ephemeral focus/editor state.
type App struct {
	Dark                                            bool
	Colors                                          *boardui.ColorTheme
	Open, RoleOpen                                  bool
	Name, Password, ReadOnly, Disabled, Role, Error string
	Saved                                           int
}

func New() *App {
	if err := boardui.LoadFonts(); err != nil {
		panic(err)
	}
	return &App{Name: "Maya Chen", Password: "secret", ReadOnly: "Managed by workspace", Disabled: "Not available", Role: "member"}
}

// View is shared by the standalone native demo and the complete software E2E.
func (a *App) View(c *ui.Context) {
	k := boardui.Kit{Dark: a.Dark, Colors: a.Colors}
	k.Apply(c)
	ui.Column(c).Fill().Center().Background(k.Color("background-full")).Children(func() {
		if k.Button(c, "Open settings", boardui.ButtonProps{}).Clicked() {
			a.Open = true
		}
	})
	k.Dialog(c, boardui.DialogProps{Open: &a.Open, Title: "Workspace settings", Description: "Manage your profile and workspace access.", Width: 480}, func() {
		ui.Column(c).WidthPercent(100).Gap(16).Children(func() {
			if a.Error != "" && strings.TrimSpace(a.Name) != "" {
				a.Error = ""
			}
			k.Input(c, &a.Name, boardui.InputProps{Label: "Name", Description: "Your display name.", Error: a.Error, Required: true})
			k.Input(c, &a.Password, boardui.InputProps{Label: "Password", Password: true})
			k.Input(c, &a.ReadOnly, boardui.InputProps{Label: "Read-only", ReadOnly: true})
			k.Input(c, &a.Disabled, boardui.InputProps{Label: "Disabled", Disabled: true})
			k.Select(c, boardui.SelectProps{Label: "Role", Value: &a.Role, Open: &a.RoleOpen, Items: []boardui.SelectItem{{Value: "member", Label: "Member"}, {Value: "admin", Label: "Administrator"}, {Value: "viewer", Label: "Viewer"}}})
			ui.Row(c).WidthPercent(100).Justify(ui.End).Gap(12).Margin(8, 0, 0).Children(func() {
				if k.Button(c, "Cancel", boardui.ButtonProps{Variant: "secondary"}).Clicked() {
					a.Open = false
					a.RoleOpen = false
				}
				if k.Button(c, "Save changes", boardui.ButtonProps{}).Clicked() {
					if strings.TrimSpace(a.Name) == "" {
						a.Error = "Name is required."
					} else {
						a.Saved++
						a.Open = false
						a.RoleOpen = false
					}
				}
			})
		})
	})
	if !a.Open {
		a.RoleOpen = false
	}
}
