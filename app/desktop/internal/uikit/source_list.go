package uikit

import "github.com/egoist/mygo/ui"

// SourceListItem 对应 contract.ts SourceListItem。
type SourceListItem struct {
	ID       string
	Title    string
	Subtitle string
	// SubtitleTone 给副标题着色（如信任问题）；nil 用次要色。
	SubtitleTone *StatusTone
	Icon         IconName
	// Status 非 nil 时在图标上画状态点。
	Status *StatusTone
	// Dimmed 画淡（如离线设备）；Pending 表示还不是对象（待接受的邀请），图标描边。
	Dimmed  bool
	Pending bool
}

// SourceListAction 是分区标题上的图标按钮（如刷新）。
type SourceListAction struct {
	Label    string
	Icon     IconName
	Busy     bool
	Disabled bool
}

// SourceListSection 对应 contract.ts SourceListSection。Extra 在条目后构建额外内容。
type SourceListSection struct {
	ID    string
	Title string
	Items []SourceListItem
	// Action 是标题命令，Placeholder 是没有条目时的按钮标签（如 "Invite Device"）。
	Action      *SourceListAction
	Placeholder string
	Extra       func()
}

// SourceListProps 对应 contract.ts SourceListProps。选中值在 *SourceState.Value。
type SourceListProps struct {
	Label    string
	Sections []SourceListSection
	// Header 画在分区上方（如连接提示），Footer 是固定在底部的状态文字。
	Header func()
	Footer string
}

// SourceCommandKind 区分 SourceState.Command 报告的命令。
type SourceCommandKind int

const (
	SourceNoCommand SourceCommandKind = iota
	SourceSectionAction
	SourcePlaceholder
)

// SourceState 是管理视图对象列表的持久状态。
type SourceState struct {
	Value string

	changed bool
	cmd     SourceCommandKind
	cmdSec  string
}

// Changed 报告本帧 Value 被点击或键盘改变。
func (s *SourceState) Changed() bool { return s.changed }

// Command 返回本帧触发的命令与分区 id。
func (s *SourceState) Command() (SourceCommandKind, string) { return s.cmd, s.cmdSec }

func (s *SourceState) pick(id string) {
	if s.Value != id {
		s.Value, s.changed = id, true
	}
}

// SourceList 是单选对象列表：一个 Tab 停靠点，Up/Down 跨分区移动选中，Home/End 跳转。
func (k Kit) SourceList(c *ui.Context, st *SourceState, p SourceListProps) ui.Element {
	st.changed, st.cmd, st.cmdSec = false, SourceNoCommand, ""
	var order []string
	for _, s := range p.Sections {
		for _, it := range s.Items {
			order = append(order, it.ID)
		}
	}
	lk := k.navLook()
	shell := ui.Column(c).Gap(8).Padding(8, 8, 10)
	shell.Children(func() {
		if p.Header != nil {
			p.Header()
		}
		list := k.Scroll(c).Grow(1).MinHeight(0).Gap(10).Label(p.Label).Role(ui.RoleList).Focusable()
		list.Children(func() {
			for _, s := range p.Sections {
				k.sourceSection(c, st, s, lk)
			}
		})
		k.sourceKeys(list, st, order)
		if p.Footer != "" {
			k.Text(c, p.Footer, TextProps{Tone: ToneSecondary, Role: RoleCaption}).Padding(0, lk.padX)
		}
	})
	return shell
}

func (k Kit) sourceKeys(list ui.Element, st *SourceState, order []string) {
	if len(order) == 0 {
		return
	}
	idx := -1
	for i, id := range order {
		if id == st.Value {
			idx = i
		}
	}
	switch {
	case list.Shortcut(0, ui.KeyDown):
		st.pick(order[min(idx+1, len(order)-1)])
	case list.Shortcut(0, ui.KeyUp):
		st.pick(order[max(idx-1, 0)])
	case list.Shortcut(0, ui.KeyHome):
		st.pick(order[0])
	case list.Shortcut(0, ui.KeyEnd):
		st.pick(order[len(order)-1])
	}
}

func (k Kit) sourceSection(c *ui.Context, st *SourceState, s SourceListSection, lk navLook) {
	sec := ui.Column(c.Key(s.ID)).Gap(2).Shrink(0)
	sec.Children(func() {
		if s.Title != "" || s.Action != nil {
			head := ui.Row(c).AlignItems(ui.Center).Padding(0, lk.padX).Gap(4)
			head.Children(func() {
				ui.Text(c, s.Title).FontSize(lk.heading).FontWeight(600).TextColor(k.Sem("mutedForeground")).
					SingleLine().Grow(1)
				if a := s.Action; a != nil {
					b := k.Button(c.Key("action"), "", ButtonProps{Variant: Subtle, Size: SizeSmall, Icon: a.Icon,
						Label: a.Label, Disabled: a.Disabled, Busy: a.Busy})
					if b.Clicked() {
						st.cmd, st.cmdSec = SourceSectionAction, s.ID
					}
				}
			})
		}
		for _, it := range s.Items {
			k.sourceRow(c, st, it)
		}
		if len(s.Items) == 0 && s.Placeholder != "" {
			if k.Button(c.Key("ph"), s.Placeholder, ButtonProps{Variant: Subtle}).Clicked() {
				st.cmd, st.cmdSec = SourcePlaceholder, s.ID
			}
		}
		if s.Extra != nil {
			s.Extra()
		}
	})
}

func (k Kit) sourceRow(c *ui.Context, st *SourceState, it SourceListItem) {
	selected := st.Value == it.ID
	lk := k.navLook()
	radius := lk.radius
	row := ui.Row(c.Key(it.ID)).AlignItems(ui.Center).Gap(10).Padding(6, lk.padX).MinHeight(44).Shrink(0).
		Radius(radius).Label(it.Title).Role(ui.RoleListItem)
	row.AccessSelected(selected)
	if selected {
		row.Background(k.navSelectedFill())
	} else if row.Hovered() {
		row.Background(k.navHoverFill())
	}
	if it.Dimmed {
		row.Opacity(0.55)
	}
	row.DrawOver(func(p *ui.Painter, rc ui.Rect) {
		if k.Skin == Windows && selected {
			p.Fill(ui.Rect{X: rc.X, Y: rc.Y + rc.H/2 - 8, W: 3, H: 16}, k.Sem("primary"), 1.5)
		}
	})
	row.Children(func() {
		tile := ui.Row(c).Size(32, 32).Justify(ui.Center).AlignItems(ui.Center).Shrink(0).Radius(8)
		if it.Pending {
			tile.Border(1, k.Sem("mutedForeground"))
		} else {
			tile.Background(k.Sem("secondary"))
		}
		tile.Children(func() { k.Icon(c, it.Icon, 18).TextColor(k.Sem("foreground")) })
		tile.DrawOver(func(p *ui.Painter, rc ui.Rect) {
			if it.Status != nil {
				cx, cy := rc.X+rc.W-4, rc.Y+rc.H-4
				p.Fill(ui.Rect{X: cx - 6, Y: cy - 6, W: 12, H: 12}, k.surfaceSolid(), 6)
				p.Fill(ui.Rect{X: cx - 4, Y: cy - 4, W: 8, H: 8}, k.navStatusColor(*it.Status), 4)
			}
		})
		col := ui.Column(c).Gap(1).Grow(1).MinWidth(0)
		col.Children(func() {
			ui.Text(c, it.Title).FontSize(k.navFont()).TextColor(k.Sem("foreground")).SingleLine()
			if it.Subtitle != "" {
				sub := k.Sem("mutedForeground")
				if it.SubtitleTone != nil {
					sub = k.navStatusColor(*it.SubtitleTone)
				}
				ui.Text(c, it.Subtitle).FontSize(lk.heading + 0.5).TextColor(sub).SingleLine()
			}
		})
	})
	if row.Clicked() {
		st.pick(it.ID)
	}
}
