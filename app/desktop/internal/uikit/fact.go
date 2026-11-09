package uikit

import "github.com/egoist/mygo/ui"

// Fact is one label/value pair of a FactList.
type Fact struct {
	ID        string
	Label     string
	Value     string
	Monospace bool
}

// FactListProps mirrors contract.ts FactListProps.
type FactListProps struct {
	Label string // accessible name of the list
	Facts []Fact
}

// FactList shows small facts side by side, e.g. an image's dimensions and format: wrapping
// tiles with the label above the value.
func (k Kit) FactList(c *ui.Context, p FactListProps) ui.Element {
	t := k.layTok()
	r := k.layRadius()
	return ui.Row(c).Wrap().Gap(8).Role(ui.RoleGroup).Label(p.Label).Children(func() {
		for _, f := range p.Facts {
			tile := ui.Column(c.Key(f.ID)).MinWidth(120).Padding(8, 12).Radius(r).Gap(2).Background(t.Card)
			if k.Skin == MacOS {
				tile.InsetShadow(0, 0, 0, 0.5, t.CardStroke)
			} else {
				tile.Border(1, t.CardStroke)
			}
			tile.Children(func() {
				k.Text(c.Key("l"), f.Label, TextProps{Role: RoleCaption, Tone: ToneSecondary})
				v := k.Text(c.Key("v"), f.Value, TextProps{Role: RoleCallout, Emphasized: true, Truncate: true})
				if f.Monospace {
					v.Font("monospace")
				}
			})
		}
	})
}
