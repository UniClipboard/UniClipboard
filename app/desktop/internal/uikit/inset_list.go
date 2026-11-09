package uikit

import "github.com/egoist/mygo/ui"

// InsetListRow 对应 contract.ts InsetListRow。
type InsetListRow struct {
	ID    string
	Title string
	// Status 自定义尾部状态，提供时取代 Detail。
	Status func()
	// Detail 是尾部状态文字，按 Tone 着色。
	Detail string
	Tone   StatusTone
	// Action 是行尾的链接命令。
	Action string
}

// InsetListProps 对应 contract.ts InsetListProps。
type InsetListProps struct {
	Label string
	Rows  []InsetListRow
}

// InsetListResult 报告本帧被点击命令所在行 id。
type InsetListResult struct {
	ui.Element
	Clicked string
}

// InsetList 是成组（inset）的短行表，每行有标题、状态文字/点与可选命令，如各设备的送达情况。
func (k Kit) InsetList(c *ui.Context, p InsetListProps) InsetListResult {
	res := InsetListResult{}
	rad := float32(10)
	switch k.Skin {
	case Windows:
		rad = k.Px("borderRadiusMedium")
	case Linux:
		rad = 12
	}
	box := ui.Column(c).Label(p.Label).Role(ui.RoleList).Radius(rad).Clip().Background(k.surfaceSolid()).
		Border(1, k.Sem("border"))
	if k.Skin == MacOS {
		box.Background(k.Color("fill-4")).Border(.5, k.Color("separator")).Radius(12)
	}
	box.Children(func() {
		for i, r := range p.Rows {
			row := ui.Row(c.Key(r.ID)).AlignItems(ui.Center).Gap(10).Padding(0, 12).MinHeight(40).Role(ui.RoleListItem).
				Label(r.Title)
			if k.Skin == MacOS {
				row.MinHeight(34)
			}
			first := i == 0
			row.DrawOver(func(pt *ui.Painter, rc ui.Rect) {
				if !first {
					pt.Fill(ui.Rect{X: rc.X + 12, Y: rc.Y, W: rc.W - 12, H: 1}, k.Sem("border"), 0)
				}
			})
			row.Children(func() {
				ui.Text(c, r.Title).FontSize(k.navFont()).TextColor(k.Sem("foreground")).SingleLine().Grow(1).MinWidth(0)
				switch {
				case r.Status != nil:
					r.Status()
				case r.Detail != "":
					ui.Box(c).Size(8, 8).Radius(4).Background(k.navStatusColor(r.Tone)).Shrink(0)
					ui.Text(c, r.Detail).FontSize(k.navFont() - 1).TextColor(k.insetTone(r.Tone)).SingleLine().Shrink(0)
				}
				if r.Action != "" {
					if k.Button(c.Key("act"), r.Action, ButtonProps{Variant: Link, Size: SizeSmall}).Clicked() {
						res.Clicked = r.ID
					}
				}
			})
		}
	})
	res.Element = box
	return res
}

func (k Kit) insetTone(t StatusTone) ui.Color {
	if t == StatusNeutral {
		return k.Sem("mutedForeground")
	}
	return k.navStatusColor(t)
}
