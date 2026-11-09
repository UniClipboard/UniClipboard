package uikit

import "github.com/egoist/mygo/ui"

// StepListStep 对应 contract.ts StepListStep。
type StepListStep struct {
	ID          string
	Title       string
	Description string
	// Status 是描述下的实时状态；Active 表示进行中（强调色）。
	Status *StepStatus
	// Action 是该步的命令，Primary 为主按钮。
	Action *StepAction
	Done   bool
}

// StepStatus 是步骤的实时状态文字。
type StepStatus struct {
	Text   string
	Active bool
}

// StepAction 是步骤的命令按钮。
type StepAction struct {
	Label   string
	Primary bool
}

// StepListProps 对应 contract.ts StepListProps。
type StepListProps struct {
	Label string
	Steps []StepListStep
}

// StepListResult 报告本帧被点击的步骤命令。
type StepListResult struct {
	ui.Element
	// Clicked 是被点击命令所在步骤的 id（"" 无）。
	Clicked string
}

// StepList 是带编号的上手步骤：macOS 为编号徽标卡片，Windows/Linux 为设置向导列表行。
// 完成的步骤编号换成对勾。
func (k Kit) StepList(c *ui.Context, p StepListProps) StepListResult {
	res := StepListResult{}
	col := ui.Column(c).Label(p.Label).Role(ui.RoleList).Gap(8)
	col.Children(func() {
		for i, s := range p.Steps {
			if id := k.stepRow(c.Key(s.ID), s, i+1); id != "" {
				res.Clicked = id
			}
		}
	})
	res.Element = col
	return res
}

func (k Kit) stepRow(c *ui.Context, s StepListStep, n int) (clicked string) {
	rad := float32(10)
	switch k.Skin {
	case Windows:
		rad = k.Px("borderRadiusMedium")
	case Linux:
		rad = 12
	}
	card := ui.Row(c).AlignItems(ui.Start).Gap(12).Padding(12).Radius(rad).Role(ui.RoleListItem).
		Label(s.Title).Background(k.surfaceSolid()).Border(1, k.Sem("border"))
	card.Children(func() {
		badge := ui.Row(c).Size(24, 24).Justify(ui.Center).AlignItems(ui.Center).Shrink(0).Radius(12)
		if s.Done {
			badge.Background(k.stepPositive())
		} else {
			badge.Background(k.Sem("secondary"))
		}
		badge.Children(func() {
			if s.Done {
				ui.Box(c).Size(24, 24).Draw(func(p *ui.Painter, r ui.Rect) {
					path := (&ui.Path{}).MoveTo(r.X+7, r.Y+12.5).LineTo(r.X+10.5, r.Y+16).LineTo(r.X+17, r.Y+8.5)
					p.StrokePath(path, 1.8, ui.RGB(255, 255, 255))
				})
				return
			}
			ui.Text(c, itoaStep(n)).FontSize(12).FontWeight(600).TextColor(k.Sem("foreground"))
		})
		body := ui.Column(c).Gap(3).Grow(1).MinWidth(0)
		body.Children(func() {
			ui.Text(c, s.Title).FontSize(k.navFont()).FontWeight(600).TextColor(k.Sem("foreground"))
			if s.Description != "" {
				k.Text(c, s.Description, TextProps{Tone: ToneSecondary, Role: RoleCaption})
			}
			if s.Status != nil {
				col := k.Sem("mutedForeground")
				if s.Status.Active {
					col = k.Sem("primary")
				}
				ui.Text(c, s.Status.Text).FontSize(12).TextColor(col)
			}
			if a := s.Action; a != nil {
				v := Standard
				if a.Primary {
					v = Primary
				}
				if k.Button(c.Key("act"), a.Label, ButtonProps{Variant: v, Size: SizeSmall}).Clicked() {
					clicked = s.ID
				}
			}
		})
	})
	return clicked
}

func (k Kit) stepPositive() ui.Color {
	if k.Skin == Windows {
		return k.Color("w11-status-positive")
	}
	return k.Color("green")
}

func itoaStep(n int) string {
	if n < 10 {
		return string(rune('0' + n))
	}
	return itoaStep(n/10) + string(rune('0'+n%10))
}
