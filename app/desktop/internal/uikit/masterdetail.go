package uikit

import "github.com/egoist/mygo/ui"

// MasterDetailProps mirrors contract.ts MasterDetailProps.
type MasterDetailProps struct {
	// DetailKey: changing the selected object's identity resets the detail and its scroll position.
	DetailKey string
	// DetailLabel is the accessible name of the detail region.
	DetailLabel string
}

// MasterDetail is a list column beside a detail column inside a large sheet. The skin owns the
// column widths and the divider; the detail scrolls on its own.
func (k Kit) MasterDetail(c *ui.Context, p MasterDetailProps, list func(), detail func()) ui.Element {
	t := k.layTok()
	root := ui.Row(c).Height(480).MinHeight(0).AlignItems(ui.Stretch)
	switch k.Skin {
	case MacOS:
		root.Clip()
	case Windows:
		root.Height(480).Clip().Radius(k.Px("borderRadiusMedium")).Border(1, t.CardStroke)
	default:
		root.Clip()
	}
	return root.Children(func() {
		listCol := ui.Column(c.Key("master")).Width(240).Shrink(0).MinHeight(0)
		switch k.Skin {
		case MacOS:
			listCol.InsetShadow(-0.5, 0, 0, 0, t.Border)
		case Windows:
			listCol.Background(k.Color("w11-base")).InsetShadow(-1, 0, 0, 0, t.Border)
		default:
			listCol.Background(k.Sem("sidebar")).InsetShadow(-1, 0, 0, 0, t.Border)
		}
		listCol.Children(func() {
			if list != nil {
				list()
			}
		})
		dc := c.Key("detail-" + p.DetailKey)
		pad := []float32{20, 28, 28}
		gap := float32(18)
		if k.Skin == Linux {
			pad = []float32{20, 28, 28}
		}
		k.Scroll(dc).Role(ui.RoleRegion).Label(p.DetailLabel).Grow(1).MinWidth(0).Children(func() {
			ui.Column(dc.Key("body")).MaxWidth(760).Padding(pad...).Gap(gap).Children(func() {
				if detail != nil {
					detail()
				}
			})
		})
	})
}
