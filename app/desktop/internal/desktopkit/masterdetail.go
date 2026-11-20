package desktopkit

import "github.com/egoist/mygo/ui"

type MasterDetailProps struct {
	// DetailKey: changing the selected object's identity resets the detail and its scroll position.
	DetailKey string
	// DetailLabel is the accessible name of the detail region.
	DetailLabel string
	// DetailOpen enables the single-pane layout: below SinglePaneWidth the list shows while it
	// is false and the detail, under a back button labelled BackLabel, while it is true. Nil
	// keeps both columns at every width.
	DetailOpen *bool
	BackLabel  string
}

func (k Kit) MasterDetail(c *ui.Context, p MasterDetailProps, list func(), detail func()) ui.Element {
	t := k.layTok()
	if w, _ := c.Size(); p.DetailOpen != nil && w < SinglePaneWidth {
		return k.masterDetailSingle(c, p, list, detail)
	}
	root := ui.Row(c).Height(480).MinHeight(0).AlignItems(ui.Stretch)
	{

		root.Clip()
	}

	return root.Children(func() {
		listCol := ui.Column(c.Key("master")).Width(240).Shrink(0).MinHeight(0)
		{

			listCol.Background(k.Color("background-secondary-default")).InsetShadow(-1, 0, 0, 0, t.Border)
		}

		listCol.Children(func() {
			if list != nil {
				list()
			}
		})
		dc := c.Key("detail-" + p.DetailKey)
		pad := []float32{20, 28, 28}
		gap := float32(18)
		k.Scroll(dc).Role(ui.RoleRegion).Label(p.DetailLabel).Grow(1).MinWidth(0).Children(func() {
			ui.Column(dc.Key("body")).MaxWidth(760).Padding(pad...).Gap(gap).Children(func() {
				if detail != nil {
					detail()
				}
			})
		})
	})
}

// masterDetailSingle shows either the list or the detail in the full width of the sheet.
func (k Kit) masterDetailSingle(c *ui.Context, p MasterDetailProps, list func(), detail func()) ui.Element {
	root := ui.Column(c).Height(480).MinHeight(0).AlignItems(ui.Stretch).Clip()

	return root.Children(func() {
		if !*p.DetailOpen {
			ui.Column(c.Key("master")).Grow(1).MinHeight(0).Children(func() {
				if list != nil {
					list()
				}
			})
			return
		}
		ui.Row(c.Key("detail-bar")).Padding(8, 12, 0).Children(func() {
			if k.Button(c.Key("detail-back"), p.BackLabel, ButtonProps{Variant: Subtle, Icon: IconBack}).Clicked() {
				*p.DetailOpen = false
			}
		})
		dc := c.Key("detail-" + p.DetailKey)
		k.Scroll(dc).Role(ui.RoleRegion).Label(p.DetailLabel).Grow(1).MinHeight(0).Children(func() {
			ui.Column(dc.Key("body")).Padding(8, 28, 28).Gap(18).Children(func() {
				if detail != nil {
					detail()
				}
			})
		})
	})
}
