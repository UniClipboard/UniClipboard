package desktopdemo

import (
	"fmt"
	"math"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

type qpPalette struct{ background, foreground, muted, quiet, border, primary, white ui.Color }

func (p *quickDemo) palette(c *ui.Context) qpPalette {
	dark := c.Theme().Dark
	if p.theme != "system" {
		dark = p.theme == "dark"
	}
	// The default GPUI preset in apps/quick-panel/assets/themes.json.
	a := qpPalette{background: ui.RGB(250, 250, 251), foreground: ui.RGBA(0, 0, 6, .8), muted: ui.RGBA(0, 0, 6, .08), quiet: ui.RGBA(0, 0, 6, .55), border: ui.RGBA(0, 0, 6, .09), primary: ui.RGB(53, 132, 228), white: ui.RGB(255, 255, 255)}
	if dark {
		a.background = ui.RGB(34, 34, 38)
		a.foreground = ui.RGB(255, 255, 255)
		a.muted = ui.RGBA(255, 255, 255, .1)
		a.quiet = ui.RGBA(255, 255, 255, .6)
		a.border = ui.RGBA(255, 255, 255, .09)
	}
	return a
}
func (p *quickDemo) commandLabel() string {
	if uikit.DefaultSkin() == uikit.MacOS {
		return "⌘"
	}
	return "Ctrl+"
}
func qpKeycap(c *ui.Context, text string, a qpPalette) {
	ui.Row(c).MinWidth(18).Height(18).PaddingX(5).Radius(4).Background(a.muted).AlignItems(ui.Center).Justify(ui.Center).Children(func() { ui.Text(c, text).Font("monospace").FontSize(10.5).TextColor(a.quiet) })
}
func (p *quickDemo) panel(c *ui.Context) {
	p.panelInvalidate = c.Invalidate
	if p.now.IsZero() {
		p.now = c.Now()
		p.previewDue = time.Time{}
		p.pendingPreview = ""
		p.schedulePreview()
	}
	p.tick(c)
	a := p.palette(c)
	k := uikit.For(p.theme == "dark" || (p.theme == "system" && c.Theme().Dark))
	k.Apply(c)
	root := ui.Column(c).Fill().Background(a.background).TextColor(a.foreground).Radius(12).Border(1, a.border).Clip()
	root.Children(func() {
		if !p.visible {
			ui.Column(c).Fill().Gap(12).Justify(ui.Center).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "面板已关闭").FontSize(15)
				if ui.Button(c, "重新唤起").Clicked() {
					p.visible = true
					p.query = ""
					p.kind = ""
					p.chips = nil
					p.applySearch()
					p.update()
				}
			})
			return
		}
		p.searchRow(c, a)
		scroll := k.Scroll(c.Key("history-scroll")).Grow(1).MinHeight(0)
		scroll.Children(func() {
			body := ui.Column(c).FillWidth().Padding(6)
			// Pointer motion re-enables hover. A stationary pointer cannot undo a key.
			x, y, over := body.PointerPosition()
			if over && (!p.pointerKnown || x != p.pointerX || y != p.pointerY) {
				p.keyboard = false
				p.pointerKnown = true
				p.pointerX = x
				p.pointerY = y
			}
			body.Children(func() {
				options := p.suggestions()
				if len(options) > 0 {
					p.suggestionList(c, a, options)
				}
				switch {
				case !p.queryDue.IsZero() || (p.live != nil && p.live.d.live.loading):
					ui.Text(c, "正在搜索…").FontSize(13).TextColor(a.quiet).Padding(24)
				case len(p.rows) == 0:
					p.emptyView(c, a)
				case p.kind == "图片":
					p.imageGrid(c, a)
				default:
					for i, e := range p.rows {
						p.resultRow(c.Key(e.id), a, i, e)
					}
				}
			})
		})
		if p.message != "" {
			ui.Text(c, p.message).Role(ui.RoleAlert).FontSize(11).TextColor(ui.RGB(224, 27, 36)).Padding(8, 12)
		}
		ui.Row(c).Height(34).Shrink(0).Padding(0, 10, 0, 12).BorderWidth(1, 0, 0, 0).BorderColor(a.border).AlignItems(ui.Center).Gap(7).Children(func() {
			ui.Row(c).Size(16, 16).Radius(4).Background(a.foreground).AlignItems(ui.Center).Justify(ui.Center).Children(func() { ui.Text(c, "T").FontSize(9).TextColor(a.background) })
			ui.Text(c, p.pasteTargetLabel()).FontSize(12)
			shortcut := "⏎"
			if p.live != nil {
				shortcut = p.commandLabel() + "C"
			}
			qpKeycap(c, shortcut, a)
			ui.Spacer(c)
			b := ui.ButtonBase(c.Key("actions")).Label("操作").Row().Gap(6).AlignItems(ui.Center)
			b.Children(func() { ui.Text(c, "操作").FontSize(12).TextColor(a.quiet); qpKeycap(c, p.commandLabel()+"K", a) })
			b.OnClick(p.openActions)
		})
	})
	// A pending timer can be created by an input action after the prior build.
	if !p.queryDue.IsZero() {
		c.After(max(time.Millisecond, p.queryDue.Sub(c.Now())))
	}
	if !p.previewDue.IsZero() {
		c.After(max(time.Millisecond, p.previewDue.Sub(c.Now())))
	}
}
func (p *quickDemo) searchRow(c *ui.Context, a qpPalette) {
	ui.Row(c).Height(48).Shrink(0).PaddingX(14).Gap(10).AlignItems(ui.Center).BorderWidth(0, 0, 1, 0).BorderColor(a.border).Children(func() {
		uikit.For(false).Icon(c, uikit.IconSearch, 16).TextColor(a.foreground)
		for i, f := range p.chips {
			b := ui.ButtonBase(c.Key(f.dimension + f.value)).Label("移除 " + f.label).Height(24).PaddingX(8).Radius(6).Background(a.muted)
			b.Children(func() { ui.Text(c, f.label).FontSize(12.5).TextColor(a.primary) })
			b.OnClick(func() {
				p.chips = append(p.chips[:i], p.chips[i+1:]...)
				p.queryChanged()
				p.suggestOpen = false
				p.input.Focus()
			})
		}
		in := ui.TextInputBase(c.Key("search"), &p.query).Label("搜索快捷面板").Placeholder("搜索，或输入 # 标签  @ 设备  / 类型").PlaceholderColor(a.quiet).FontSize(15).Height(32).Grow(1).MinWidth(0).AutoFocus()
		in.Bind(&p.input).OnChange(p.queryChanged)
		p.bindKeys(c, in)
		if p.query != "" || p.kind != "" || len(p.chips) > 0 {
			ui.Text(c, func() string {
				if len(p.suggestions()) > 0 {
					return fmt.Sprintf("%d 条文字匹配", len(p.rows))
				}
				return fmt.Sprintf("%d 条", len(p.rows))
			}()).FontSize(12).TextColor(a.quiet).Shrink(0)
		}
		b := ui.ButtonBase(c.Key("type")).Label("切换类型").Row().Gap(6).AlignItems(ui.Center).Shrink(0)
		b.Children(func() {
			label := p.kind
			if label == "" {
				label = "全部"
			}
			ui.Text(c, label).FontSize(12).TextColor(a.quiet)
			qpKeycap(c, "⇥", a)
		})
		b.OnClick(func() { p.tab(false); p.input.Focus() })
		if p.query != "" || p.kind != "" || len(p.chips) > 0 {
			ui.ButtonBase(c.Key("clear")).Label("清空").Size(18, 18).Children(func() { ui.Text(c, "×").FontSize(16).TextColor(a.quiet) }).OnClick(func() { p.clear(); p.input.Focus() })
		}
	})
}
func (p *quickDemo) bindKeys(c *ui.Context, in ui.Element) {
	type binding struct {
		mods ui.Modifiers
		key  ui.Key
	}
	handlers := map[binding]func(){}
	bind := func(mods ui.Modifiers, key ui.Key, fn func()) { handlers[binding{mods, key}] = fn }
	in.OnTextKey(func(key ui.TextKey) bool {
		fn, ok := handlers[binding{key.Mods, key.Key}]
		if !ok || !p.visible {
			return false
		}
		p.query = key.Text()
		before := p.query
		fn()
		if before != p.query {
			key.SetText(p.query)
		}
		p.update()
		return true
	})
	command := ui.Cmd
	if uikit.DefaultSkin() != uikit.MacOS {
		command = ui.Ctrl
	}
	bind(0, ui.KeyEscape, p.escape)
	bind(command, ui.KeyK, p.openActions)
	bind(0, ui.KeyTab, func() { p.tab(false) })
	bind(ui.Shift, ui.KeyTab, func() { p.tab(true) })
	bind(0, ui.KeyUp, func() { p.navigate(false) })
	bind(0, ui.KeyDown, func() { p.navigate(true) })
	bind(ui.Ctrl, ui.KeyN, func() { p.navigate(true) })
	bind(ui.Ctrl, ui.KeyP, func() { p.navigate(false) })
	enter := func(plain, keep bool) {
		if p.mode == "disconnected" {
			p.mode = "history"
			p.applySearch()
			return
		}
		if p.menu != "" && !plain && !keep {
			rows := p.actions()
			if len(rows) > p.cursor && rows[p.cursor].enabled {
				p.runAction(rows[p.cursor].id)
			}
			return
		}
		if p.suggestFocused && !plain && !keep {
			options := p.suggestions()
			if len(options) > p.suggestCursor {
				p.accept(options[p.suggestCursor])
			}
			return
		}
		if len(p.rows) == 0 && !plain && !keep {
			options := p.relaxations()
			if len(options) > 0 {
				p.relax(options[min(p.relaxCursor, len(options)-1)].dimension)
			}
			return
		}
		p.menu = ""
		p.restore(true, plain, keep)
	}
	bind(0, ui.KeyEnter, func() { enter(false, false) })
	bind(ui.Shift, ui.KeyEnter, func() { enter(true, false) })
	bind(command, ui.KeyEnter, func() { enter(false, true) })
	bind(command|ui.Shift, ui.KeyEnter, func() { enter(false, true) })
	if p.query == "" {
		bind(0, ui.KeyBackspace, func() {
			if p.menu == "devices" {
				p.menu = "main"
				p.cursor = 0
			} else if len(p.chips) > 0 {
				p.chips = p.chips[:len(p.chips)-1]
				p.queryChanged()
				p.suggestOpen = false
			} else if p.kind != "" {
				p.kind = ""
				p.queryChanged()
				p.suggestOpen = false
			}
		})
		bind(command, ui.KeyV, func() { p.restore(true, false, false) })
		bind(command|ui.Shift, ui.KeyV, func() { p.restore(true, true, false) })
	}
	bind(command, ui.KeyBackspace, p.clear)
	bind(command|ui.Shift, ui.KeyBackspace, func() { p.runAction("delete") })
	start, end := in.TextSelection()
	if start == end {
		bind(command, ui.KeyC, func() { p.restore(false, false, false) })
	}
	bind(command, ui.KeyO, func() {
		e := p.active()
		if e != nil && (e.kind == "链接" || e.kind == "文件") {
			p.runAction("open")
		}
	})
	bind(command|ui.Shift, ui.KeyO, func() { p.host("main") })
	bind(command, ui.KeyComma, func() { p.host("settings") })
	for i, key := range []ui.Key{ui.Key1, ui.Key2, ui.Key3, ui.Key4, ui.Key5, ui.Key6, ui.Key7, ui.Key8, ui.Key9} {
		for _, plain := range []bool{false, true} {
			mods := command
			if plain {
				mods |= ui.Shift
			}
			bind(mods, key, func() {
				at := i
				if p.kind == "图片" {
					at += p.gridTop * 3
				}
				if at < len(p.rows) {
					p.selectRow(at)
					p.restore(true, plain, false)
				}
			})
		}
	}
	for _, key := range []ui.Key{ui.KeyLeft, ui.KeyRight} {
		if p.menu == "devices" && key == ui.KeyLeft {
			bind(0, key, func() { p.menu = "main"; p.cursor = 0 })
		} else if p.kind == "图片" && p.query == "" && p.menu == "" {
			bind(0, key, func() {
				at := p.selected
				if key == ui.KeyLeft && at%3 > 0 {
					at--
				}
				if key == ui.KeyRight && at%3 < 2 {
					at++
				}
				p.selectRow(at)
			})
		}
	}
	c.OnWindowShortcut(command, ui.KeyQ, func() { p.visible = false; p.update() })
}
func (p *quickDemo) suggestionList(c *ui.Context, a qpPalette, options []qpFilter) {
	heading := func(title, note string) {
		ui.Row(c).Height(22).PaddingX(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, title).FontSize(11).Bold().TextColor(a.quiet)
			ui.Spacer(c)
			ui.Text(c, note).FontSize(11).TextColor(a.quiet)
		})
	}
	heading("建议", "按顺序接受 ⇥")
	cursor := 0
	if p.suggestFocused {
		cursor = p.suggestCursor
	}
	start := max(0, cursor-2)
	for i := start; i < min(len(options), start+3); i++ {
		f := options[i]
		b := ui.ButtonBase(c.Key("suggest-"+f.dimension+f.value)).Label("建议："+f.label).Height(30).FillWidth().Padding(0, 8, 0, 6).Radius(7).Row().AlignItems(ui.Center).Gap(8)
		if cursor == i {
			b.Background(a.muted)
		}
		b.Children(func() {
			ui.Column(c).Width(16)
			ui.Text(c, f.matched).Font("monospace").FontSize(11.5).PaddingX(5).Radius(4).Background(a.muted).MaxLines(1)
			ui.Text(c, "→").FontSize(12.5).TextColor(a.quiet)
			labels := map[string]string{"type": "类型", "tag": "标签", "source": "设备", "time": "时间"}
			ui.Text(c, labels[f.dimension]).FontSize(12.5).TextColor(a.quiet)
			ui.Text(c, f.label).FontSize(12.5).Bold().TextColor(a.primary).Padding(1, 7).Radius(5).Background(a.muted).Grow(1).MinWidth(0).MaxLines(1)
			qpKeycap(c, "⇥", a)
		})
		b.OnClick(func() { p.accept(f); p.input.Focus() })
	}
	if hidden := len(options) - min(len(options), start+3); hidden > 0 {
		ui.Text(c, fmt.Sprintf("还有 %d 条", hidden)).FontSize(11).TextColor(a.quiet).Padding(4, 8)
	}
	heading("文字匹配", "")
}
func qpMarkedSpans(e qpEntry, query string, selected bool, ink, quiet ui.Color) []ui.Span {
	text := strings.NewReplacer("\n", " ", "\r", " ").Replace(e.title)
	q := strings.ToLower(strings.TrimSpace(query))
	lower := []rune(strings.ToLower(text))
	queryRunes := []rune(q)
	tint := ui.RGBA(53, 132, 228, .22)
	if selected {
		tint = ui.RGBA(255, 255, 255, .3)
	}
	end := 0
	spans := []ui.Span{}
	for at, r := range []rune(text) {
		if q != "" && strings.HasPrefix(string(lower[at:]), q) {
			end = at + len(queryRunes)
		}
		span := ui.Span{Text: string(r), Color: ink, Strikethrough: e.lost}
		matched := at < end
		if len(e.highlights) > 0 {
			matched = false
			for _, range_ := range e.highlights {
				matched = matched || (at >= range_.Start && at < range_.End)
			}
		}
		if matched {
			span.Background = tint
		}
		if e.kind == "链接" {
			slash := slices.Index([]rune(text), '/')
			if slash < 0 || at < slash {
				span.Weight = 500
			} else {
				span.Color = quiet
			}
		}
		spans = append(spans, span)
	}
	return spans
}
func (p *quickDemo) resultRow(c *ui.Context, a qpPalette, i int, e qpEntry) {
	selected := i == p.selected && !p.suggestFocused
	ink, quiet := a.foreground, a.quiet
	if selected {
		ink = a.white
		quiet = ui.RGBA(255, 255, 255, .78)
	}
	b := ui.ButtonBase(c).Label("历史："+e.title).FillWidth().Height(30).Shrink(0).Padding(0, 10, 0, 6).Radius(7).Row().Gap(8).AlignItems(ui.Center).TextColor(ink)
	if selected {
		b.Background(a.primary)
	} else if b.Hovered() && !p.keyboard {
		b.Background(a.muted)
	}
	b.Children(func() {
		ui.Row(c).Width(24).Shrink(0).Justify(ui.Center).Children(func() {
			if e.kind == "图片" {
				ui.Image(c, p.quickImage(e)).Size(24, 16).Radius(3).Fit(ui.Cover)
			} else {
				icons := map[string]uikit.IconName{"文本": uikit.IconList, "代码": uikit.IconPackage, "链接": uikit.IconGlobe, "文件": uikit.IconFile, "富文本": uikit.IconList}
				uikit.For(false).Icon(c, icons[e.kind], 14).TextColor(quiet)
			}
		})
		text := ui.RichText(c, qpMarkedSpans(e, p.query, selected, ink, quiet)...).FontSize(13).Grow(1).MinWidth(0).MaxLines(1)
		if e.kind == "代码" {
			text.Font("monospace").FontSize(12)
		}
		if e.lost {
			text.Opacity(.5)
		}
		if e.kind == "富文本" || e.kind == "文件" {
			label := e.kind
			if e.kind == "文件" {
				label = "PDF"
				if e.lost {
					label = "ZIP"
				}
			}
			ui.Text(c, label).FontSize(10.5).TextColor(quiet).Background(a.muted).Radius(4).Padding(1, 5)
		}
		if e.favorite {
			ui.Text(c, "★").FontSize(12).TextColor(ui.RGB(251, 191, 36))
		}
		if e.source != "本机" {
			uikit.For(false).Icon(c, uikit.IconDownload, 12).TextColor(quiet)
		}
		ui.Text(c, e.age).FontSize(11).TextColor(quiet).Shrink(0)
		if i < 9 {
			qpKeycap(c, p.commandLabel()+fmt.Sprint(i+1), a)
		}
	})
	if selected && p.keyboard && p.revealSelection {
		b.ScrollIntoView()
		p.revealSelection = false
	}
	if b.Hovered() && !p.keyboard && p.menu == "" && p.hovered != i {
		p.hovered = i
		p.schedulePreview()
		p.update()
	}
	if (p.keyboard && selected) || (!p.keyboard && p.hovered == i) {
		p.anchor = b.Bounds()
	}
	mods := b.ClickModifiers()
	b.OnClick(func() { p.selectRow(i); p.restore(true, mods&ui.Shift != 0, false) })
	if b.RightClicked() {
		p.selectRow(i)
		p.openActions()
	}
}
func (p *quickDemo) imageGrid(c *ui.Context, a qpPalette) {
	grid := ui.Column(c.Key("image-wall")).FillWidth().HandleInput(func(ev ui.InputEvent) bool {
		if ev.Kind != ui.InputScroll {
			return false
		}
		rows := int(math.Round(float64(ev.DY) / 30))
		p.gridTop = max(0, min(p.gridTop+rows, max(0, (len(p.rows)+2)/3-3)))
		p.update()
		return true
	})
	grid.Children(func() {
		for row := 0; row < 3; row++ {
			ui.Row(c.Key(row)).FillWidth().Height(90).Gap(4).Children(func() {
				for col := 0; col < 3; col++ {
					at := p.gridTop*3 + row*3 + col
					if at >= len(p.rows) {
						ui.Column(c).Grow(1).Height(86)
						continue
					}
					e := p.rows[at]
					b := ui.ButtonBase(c.Key(e.id)).Label("图片："+e.title).Grow(1).MinWidth(0).Height(86).Padding(2).Radius(7).Border(2, a.border).Clip()
					if at == p.selected {
						b.BorderColor(a.primary)
					}
					b.Children(func() {
						ui.Image(c, p.quickImage(e)).Fill().Fit(ui.Cover).Radius(5)
						ui.Row(c).Absolute().Right(4).Bottom(4).Children(func() { qpKeycap(c, p.commandLabel()+fmt.Sprint(row*3+col+1), a) })
					})
					if b.Hovered() && !p.keyboard && p.menu == "" && p.hovered != at {
						p.hovered = at
						p.schedulePreview()
						p.update()
					}
					if at == p.selected {
						p.anchor = b.Bounds()
					}
					mods := b.ClickModifiers()
					b.OnClick(func() { p.selectRow(at); p.restore(true, mods&ui.Shift != 0, false) })
					if b.RightClicked() {
						p.selectRow(at)
						p.openActions()
					}
				}
			})
		}
	})
}
func (p *quickDemo) emptyView(c *ui.Context, a qpPalette) {
	title := "还没有剪贴板历史"
	hint := "在任意设备上复制内容，都会出现在这里。"
	if p.mode == "disconnected" {
		title = "同步服务未响应"
		hint = "本次唤起期间恢复会自动刷新列表。"
	} else if p.query != "" {
		title = "没有匹配“" + p.query + "”的内容"
		hint = "试试其他关键词或筛选条件"
	} else if p.kind != "" || len(p.chips) > 0 {
		title = "没有符合条件的内容"
		hint = "试试其他关键词或筛选条件"
	}
	ui.Column(c).FillWidth().MinHeight(220).Gap(10).Justify(ui.Center).AlignItems(ui.Center).Children(func() {
		ui.Text(c, title).Role(ui.RoleHeading).FontSize(15).Bold()
		ui.Text(c, hint).FontSize(12).TextColor(a.quiet)
		if p.mode == "disconnected" {
			ui.Button(c, "立即重连").OnClick(func() { p.mode = "history"; p.applySearch(); p.update() })
		} else if p.query != "" || p.kind != "" || len(p.chips) > 0 {
			options := p.relaxations()
			if len(options) > 0 {
				ui.Text(c, "试试放宽：").FontSize(12).TextColor(a.quiet)
				for i, r := range options {
					b := ui.ButtonBase(c.Key(r.dimension)).Label(r.label).FillWidth().Height(30).Radius(7).PaddingX(10).Row().Gap(8).AlignItems(ui.Center)
					if i == p.relaxCursor {
						b.Background(a.muted)
					}
					b.Children(func() {
						ui.Text(c, fmt.Sprint(i+1)).FontSize(12).TextColor(a.quiet)
						ui.Text(c, r.label).FontSize(13).Grow(1)
						ui.Text(c, fmt.Sprintf("%d 条", r.count)).FontSize(12).TextColor(a.quiet)
					})
					b.OnClick(func() { p.relax(r.dimension); p.input.Focus() })
				}
			}
			ui.Button(c, "清空筛选").OnClick(func() { p.clear(); p.input.Focus() })
		} else {
			ui.Text(c, "随时唤起  "+p.commandLabel()+"Ctrl+V").FontSize(12).TextColor(a.quiet)
		}
	})
}
func (p *quickDemo) satellite(c *ui.Context) {
	p.satelliteInvalidate = c.Invalidate
	a := p.palette(c)
	k := uikit.For(p.theme == "dark" || (p.theme == "system" && c.Theme().Dark))
	k.Apply(c)
	ui.Column(c).Fill().Radius(12).Border(1, a.border).Background(a.background).TextColor(a.foreground).Clip().Children(func() {
		e := p.active()
		if p.menu == "" {
			e = nil
			for i := range p.entries {
				if p.entries[i].id == p.previewID {
					e = &p.entries[i]
					break
				}
			}
		}
		if e == nil {
			return
		}
		if e.kind == "图片" && p.menu == "" {
			p.imagePreview(c, a, *e)
			return
		}
		title := e.kind + " · " + e.age + " · 来自 " + e.source
		if p.menu != "" {
			title = "操作"
			if p.menu == "devices" {
				title = "发送到"
			}
		}
		ui.Text(c, title).FontSize(11).TextColor(a.quiet).Padding(12).Shrink(0)
		scroll := k.Scroll(c.Key("satellite-scroll")).Grow(1).MinHeight(0)
		scroll.Children(func() {
			if p.menu != "" {
				ui.Column(c).FillWidth().Padding(6).Gap(2).Children(func() {
					for i, row := range p.actions() {
						b := ui.ButtonBase(c.Key(row.id)).Label(row.label).Disabled(!row.enabled).FillWidth().Height(30).PaddingX(10).Radius(7).Row().Gap(8).AlignItems(ui.Center)
						if i == p.cursor {
							b.Background(a.primary).TextColor(a.white)
						}
						if !row.enabled {
							b.Opacity(.45)
						}
						b.Children(func() {
							ui.Text(c, row.label).FontSize(13).Grow(1)
							if row.shortcut != "" {
								qpKeycap(c, row.shortcut, a)
							}
						})
						b.OnClick(func() { p.runAction(row.id) })
					}
				})
				return
			}
			ui.Column(c).FillWidth().Padding(16).Gap(8).Children(func() {
				switch e.kind {
				case "图片":
					ui.Image(c, p.quickImage(*e)).FillWidth().Height(230).Fit(ui.Contain).Label(e.title)
				case "富文本":
					if p.live != nil {
						ui.Text(c, e.text).FontSize(14).FillWidth().Selectable()
						break
					}
					if p.richPreview == nil {
						p.richPreview = richtext.NewDocument(richtext.ParseHTML("<h2>设计评审记录</h2><p>确认列表高度与<strong>键盘操作</strong>。</p><ul><li>保留清晰的选中状态。</li><li>核对浅色和深色主题。</li></ul>"))
					}
					richtext.EditorBase(c, p.richPreview, richtext.EditorOptions{Label: "富文本预览", ReadOnly: true}).FillWidth().Height(200)
				case "链接":
					ui.Text(c, e.title).FontSize(14).Bold().Selectable()
					ui.Text(c, e.text).FontSize(12).TextColor(a.quiet).Selectable()
				case "文件":
					for _, file := range strings.Split(e.text, "\n") {
						ui.Row(c).Gap(8).Children(func() { k.Icon(c, uikit.IconFile, 24); ui.Text(c, file).FontSize(13) })
					}
				default:
					text := ui.Text(c, e.text).FontSize(14).FillWidth().Selectable()
					if e.kind == "代码" {
						text.Font("monospace").FontSize(12)
					}
				}
			})
		})
		if p.menu != "" {
			ui.Text(c, "↑↓ 选择 · ⏎ 执行 · esc 返回").FontSize(11).TextColor(a.quiet).Padding(10, 12).Shrink(0)
		}
	})
}

func (p *quickDemo) pasteTargetLabel() string {
	if p.live != nil {
		return "复制到剪贴板"
	}
	return "粘贴到 TextEdit"
}
