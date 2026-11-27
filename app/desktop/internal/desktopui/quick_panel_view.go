package desktopui

import (
	"fmt"
	"math"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/internal/desktopkeys"
)

type qpPalette struct{ background, foreground, muted, quiet, border, primary, white ui.Color }

func (p *quickPanelState) palette(c *ui.Context) qpPalette {
	dark := c.Theme().Dark
	if p.theme != "system" {
		dark = p.theme == "dark"
	}
	k := desktopkit.Kit{Dark: dark, Colors: p.colors}
	a := qpPalette{background: k.Color("background-primary-default"), foreground: k.Color("text-primary"), muted: k.Color("background-tertiary-default"), quiet: k.Color("text-secondary"), border: k.Color("separator-border"), primary: k.Color("accent-500"), white: k.Color("text-white")}

	return a
}
func (p *quickPanelState) commandLabel() string {
	if runtime.GOOS == "darwin" {
		return "⌘"
	}
	return "Ctrl+"
}
func qpKeycap(c *ui.Context, text string, a qpPalette) {
	ui.Row(c).MinWidth(18).Height(18).PaddingX(5).Radius(4).Background(a.muted).AlignItems(ui.Center).Justify(ui.Center).Children(func() { ui.Text(c, text).Font("monospace").FontSize(10.5).TextColor(a.quiet) })
}
func (p *quickPanelState) panel(c *ui.Context) {
	p.panelInvalidate = c.Invalidate
	a := p.palette(c)
	k := desktopkit.Kit{Dark: p.theme == "dark" || (p.theme == "system" && c.Theme().Dark), Colors: p.colors}
	k.Apply(c)
	prevMeasure := p.measureText
	p.measureText = c.MeasureText
	defer func() { p.measureText = prevMeasure }()
	if p.now.IsZero() {
		p.now = c.Now()
		p.previewDue = time.Time{}
		p.pendingPreview = ""
		p.schedulePreview()
	}
	p.tick(c)
	root := ui.Column(c).Fill().Background(a.background).TextColor(a.foreground).Radius(12).Border(1, a.border).Clip()
	root.Children(func() {
		if !p.visible {
			ui.Column(c).Fill().Gap(12).Justify(ui.Center).AlignItems(ui.Center).Children(func() {
				ui.Text(c, p.tr("面板已关闭")).FontSize(15 * zoomScale(p.textZoom))
				if ui.Button(c, p.tr("重新唤起")).Clicked() {
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
		p.searchRow(c, k, a)
		qpSlow := p.live != nil && p.live.d.live.showForegroundLoading()
		qpPending := !p.queryDue.IsZero() || (p.live != nil && p.live.d.live.foregroundLoading)
		scroll := k.Scroll(c.Key("history-scroll")).Grow(1).MinHeight(0)
		viewport := scroll.Bounds()
		p.listViewport = viewport
		first, last := len(p.rows), -1
		scroll.Children(func() {
			body := ui.Column(c).FillWidth().Padding(6)
			staleTarget := float32(1)
			if qpSlow && len(p.rows) > 0 {
				staleTarget = 0.76
			}
			staleA := body.AnimateTo("qp-stale-a", staleTarget, ui.Animation{
				Duration:  140 * time.Millisecond,
				Ease:      ui.EaseOut,
				NonMotion: true,
			})
			if staleA < 0.999 {
				body.Opacity(staleA)
			}
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
				case len(p.rows) == 0 && qpPending:
					p.anchor = ui.Rect{}
					if qpSlow || !p.queryDue.IsZero() {
						ui.Row(c.Key("qp-loading")).Padding(24).Gap(8).AlignItems(ui.Center).Children(func() {
							ui.Spinner(c.Key("spin")).Size(14, 14).TextColor(a.quiet).AccessHidden(true)
							ui.Text(c, p.tr("正在搜索…")).FontSize(13 * zoomScale(p.textZoom)).TextColor(a.quiet)
						})
					}
				case len(p.rows) == 0:
					p.anchor = ui.Rect{}
					p.emptyView(c, a)
				case p.kind == "图片":
					p.imageGrid(c, k, a)
					first, last = p.gridTop*3, min(len(p.rows)-1, p.gridTop*3+8)
				default:
					for i, e := range p.rows {
						row := p.resultRow(c.Key(e.id), k, a, i, e, qpPending).Bounds()
						if row.H > 0 && viewport.H > 0 && row.Y < viewport.Y+viewport.H && row.Y+row.H > viewport.Y {
							first, last = min(first, i), i
						}
					}
				}
			})
		})
		if p.live != nil && p.queryDue.IsZero() {
			p.live.d.live.prefetchDetails(p.live.d, p.live.d.entries, first, last)
		}
		if p.live != nil && p.live.system != nil {
			failure := p.live.system.FailureMessage()
			if failure == "" {
				failure = p.pasteFailure
			}
			if failure != "" {
				k.Text(c.Key("paste-failure"), p.tr(failure), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneCritical}).
					Role(ui.RoleAlert).FontSize(11*zoomScale(p.textZoom)).FillWidth().Padding(5, 12).Shrink(0)
			}
		}
		ui.Row(c).Height(34).Shrink(0).Padding(0, 10, 0, 12).BorderWidth(1, 0, 0, 0).BorderColor(a.border).AlignItems(ui.Center).Gap(7).Children(func() {
			ui.Row(c).Size(16, 16).Radius(4).Background(a.foreground).AlignItems(ui.Center).Justify(ui.Center).Children(func() { ui.Text(c, "T").FontSize(9 * zoomScale(p.textZoom)).TextColor(a.background) })
			ui.Text(c, p.pasteTargetLabel()).FontSize(12 * zoomScale(p.textZoom))
			shortcut := "⏎"
			if p.live != nil && (p.live.system == nil || !p.live.system.CanPaste) {
				shortcut = p.commandLabel() + "C"
			}
			qpKeycap(c, shortcut, a)
			ui.Spacer(c)
			b := ui.ButtonBase(c.Key("actions")).Label(p.tr("操作")).KeepFocus().Row().Gap(6).AlignItems(ui.Center)
			b.Children(func() {
				ui.Text(c, p.tr("操作")).FontSize(12 * zoomScale(p.textZoom)).TextColor(a.quiet)
				hint := p.commandLabel() + "K"
				if p.live != nil {
					hint = shortcutHint(p.live.d.live.config.Config.Desktop.Shortcuts, desktopkeys.PanelActions)
				}
				if hint != "" {
					qpKeycap(c, hint, a)
				}
			})
			b.OnClick(p.openActions)
		})
	})
	if p.visible && (p.menu != "" || p.previewEntry() != nil) && p.onUpdate != nil {
		scale := zoomScale(p.windowZoom)
		bounds := p.satelliteAnchorBounds(int(qpPanelWidth*scale), int(qpPanelHeight*scale))
		if w, h := p.satelliteSize(); w != p.satelliteWidth || h != p.satelliteHeight || bounds != p.satelliteAnchor {
			p.satelliteAnchor = bounds
			p.onUpdate()
		}
	}
	// A pending timer can be created by an input action after the prior build.
	if !p.queryDue.IsZero() {
		c.After(max(time.Millisecond, p.queryDue.Sub(c.Now())))
	}
	if !p.previewDue.IsZero() {
		c.After(max(time.Millisecond, p.previewDue.Sub(c.Now())))
	}
}
func (p *quickPanelState) searchRow(c *ui.Context, k desktopkit.Kit, a qpPalette) {
	qpSlow := p.live != nil && p.live.d.live.showForegroundLoading()
	qpPending := !p.queryDue.IsZero() || (p.live != nil && p.live.d.live.foregroundLoading)
	var overrides map[string][]string
	if p.live != nil {
		overrides = p.live.d.live.config.Config.Desktop.Shortcuts
	}
	ui.Row(c).Height(48).Shrink(0).PaddingX(14).Gap(8).AlignItems(ui.Center).BorderWidth(0, 0, 1, 0).BorderColor(a.border).Children(func() {
		if qpSlow {
			ui.Spinner(c.Key("search-spin")).Size(16, 16).TextColor(a.primary).Shrink(0).AccessHidden(true)
		} else {
			k.Icon(c, desktopkit.IconSearch, 16).TextColor(a.foreground)
		}
		for i, f := range p.chips {
			b := ui.ButtonBase(c.Key(f.dimension + f.value)).Label(p.tr("移除 ") + p.filterLabel(f)).KeepFocus().Height(24).PaddingX(8).Radius(6).Background(a.muted).Shrink(0)
			b.Children(func() {
				ui.Text(c, p.filterLabel(f)).FontSize(12.5 * zoomScale(p.textZoom)).TextColor(a.primary).SingleLine()
			})
			b.OnClick(func() {
				p.chips = append(p.chips[:i], p.chips[i+1:]...)
				p.queryChanged()
				p.suggestOpen = false
				p.suggestFocused = false
				p.input.Focus()
			})
		}
		in := ui.TextInputBase(c.Key("search"), &p.query).Label(p.tr("搜索快捷面板")).Placeholder(p.tr("搜索，或输入 # 标签  / 类型  @ 设备  on: 时间")).PlaceholderColor(a.quiet).ReadOnly(p.menu != "").FontSize(15 * zoomScale(p.textZoom)).Height(32).Grow(1).MinWidth(0).AutoFocus()
		p.queryStart, p.queryEnd = in.TextSelection()
		in.Bind(&p.input).OnChange(func() {
			if p.menu != "" {
				return
			}
			p.queryStart = len([]rune(p.query))
			p.queryEnd = p.queryStart
			p.queryChanged()
		})
		p.bindKeys(c, in)
		if p.hasActiveFilter() {
			cnt := ui.Text(c.Key("qp-count"), func() string {
				if len(p.suggestions()) > 0 {
					return fmt.Sprintf(p.tr("%d 条文字匹配"), len(p.rows))
				}
				return fmt.Sprintf(p.tr("%d 条"), len(p.rows))
			}()).FontSize(12 * zoomScale(p.textZoom)).TextColor(a.quiet).Shrink(0)
			cntTarget := float32(1)
			if qpPending {
				cntTarget = 0.45
			}
			cntA := cnt.AnimateTo("qp-cnt-a", cntTarget, ui.Animation{
				Duration:  140 * time.Millisecond,
				Ease:      ui.EaseOut,
				NonMotion: true,
			})
			if cntA < 0.999 {
				cnt.Opacity(cntA)
			}
		}
		favHint := shortcutHint(overrides, desktopkeys.PanelFavoritesFilter)
		favTip := p.tr("仅看已收藏")
		if favHint != "" {
			favTip += " (" + favHint + ")"
		}
		favBtn := ui.ButtonBase(c.Key("favorites-toggle")).Label(p.tr("仅看已收藏")).Tooltip(favTip).KeepFocus().Height(24).Radius(6).PaddingX(6).Row().Gap(4).AlignItems(ui.Center).Shrink(0)
		starInk := a.quiet
		if p.favoriteOnly {
			starInk = p.colors.Color(c.Theme().Dark, "status-yellow-text")
			favBtn.Background(a.muted)
		} else if favBtn.Hovered() {
			favBtn.Background(a.muted)
		}
		favBtn.Children(func() {
			ui.Text(c, "★").FontSize(12.5 * zoomScale(p.textZoom)).TextColor(starInk).SingleLine()
			if p.favoriteOnly {
				ui.Text(c, p.tr("已收藏")).FontSize(11.5 * zoomScale(p.textZoom)).FontWeight(600).TextColor(a.foreground).SingleLine()
			}
		})
		favBtn.OnClick(func() { p.toggleFavoriteFilter(); p.input.Focus() })

		b := ui.ButtonBase(c.Key("type")).Label(p.tr("切换类型")).KeepFocus().Row().Gap(6).AlignItems(ui.Center).Shrink(0)
		b.Children(func() {
			label := p.kind
			if label == "" {
				label = p.tr("全部")
			}
			ui.Text(c, p.tr(label)).FontSize(12 * zoomScale(p.textZoom)).TextColor(a.quiet)
			qpKeycap(c, "⇥", a)
		})
		b.OnClick(func() { p.tab(false); p.input.Focus() })
		if p.hasActiveFilter() {
			ui.ButtonBase(c.Key("clear")).Label(p.tr("清空")).KeepFocus().Size(18, 18).Children(func() { ui.Text(c, "×").FontSize(16 * zoomScale(p.textZoom)).TextColor(a.quiet) }).OnClick(func() { p.clear(); p.input.Focus() })
		}
		k.TopHairlineProgress(c.Key("qp-progress"), qpSlow || !p.queryDue.IsZero())
	})
}
func (p *quickPanelState) bindKeys(c *ui.Context, in ui.Element) {
	type binding struct {
		mods ui.Modifiers
		key  ui.Key
	}
	handlers := map[binding]func(){}
	var overrides map[string][]string
	if p.live != nil {
		overrides = p.live.d.live.config.Config.Desktop.Shortcuts
	}
	commands := shortcutCommands(overrides, map[string]func(){
		desktopkeys.PanelActions:         p.openActions,
		desktopkeys.PanelClear:           p.clear,
		desktopkeys.PanelFavoritesFilter: p.toggleFavoriteFilter,
		desktopkeys.PanelFavorite: func() {
			if p.active() != nil && p.queryDue.IsZero() {
				p.runAction("favorite")
			}
		},
		desktopkeys.PanelWindowIn:  func() { p.windowZoom = min(5, p.windowZoom+1) },
		desktopkeys.PanelWindowOut: func() { p.windowZoom = max(-2, p.windowZoom-1) },
		desktopkeys.PanelTextIn:    func() { p.textZoom = min(4, p.textZoom+1) },
		desktopkeys.PanelTextOut:   func() { p.textZoom = max(-2, p.textZoom-1) },
	})
	menuCommands := shortcutCommands(overrides, map[string]func(){
		desktopkeys.PanelActions: p.openActions,
	})
	bind := func(mods ui.Modifiers, key ui.Key, fn func()) { handlers[binding{mods, key}] = fn }
	command := ui.Cmd
	if runtime.GOOS != "darwin" {
		command = ui.Ctrl
	}
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
		if len(p.suggestions()) > 0 && !plain && !keep {
			options := p.suggestions()
			at := 0
			if p.suggestFocused && p.suggestCursor >= 0 && p.suggestCursor < len(options) {
				at = p.suggestCursor
			}
			p.accept(options[at])
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
	stepGridHorizontal := func(right bool) {
		if p.kind != "图片" || p.menu != "" || len(p.rows) == 0 {
			return
		}
		at := p.selected
		if !right && at%3 > 0 {
			at--
		}
		if right && at%3 < 2 {
			at++
		}
		p.selectRow(at)
	}
	selStart, selEnd := in.TextSelection()
	in.OnTextKey(func(key ui.TextKey) bool {
		if !p.visible {
			return false
		}
		p.query = key.Text()
		p.queryStart, p.queryEnd = selStart, selEnd
		before := p.query

		// When the action menu is open, handle menu navigation and lock the
		// search box so stray character keys never wipe out the menu.
		if p.menu != "" {
			if p.keySequence.take(shortcutStroke{key.Mods, key.Key}, menuCommands) {
				p.update()
				return true
			}
			switch {
			case key.Mods == 0 && key.Key == ui.KeyEscape:
				p.escape()
			case (key.Mods == 0 && key.Key == ui.KeyUp) || (key.Mods == ui.Ctrl && key.Key == ui.KeyP):
				p.navigate(false)
			case (key.Mods == 0 && key.Key == ui.KeyDown) || (key.Mods == ui.Ctrl && key.Key == ui.KeyN):
				p.navigate(true)
			case key.Mods == 0 && key.Key == ui.KeyEnter:
				enter(false, false)
			case key.Mods == 0 && key.Key == ui.KeyRight:
				rows := p.actions()
				if p.cursor >= 0 && p.cursor < len(rows) && rows[p.cursor].enabled && rows[p.cursor].id == "devices" {
					p.runAction("devices")
				}
			case key.Mods == 0 && (key.Key == ui.KeyLeft || key.Key == ui.KeyBackspace):
				if p.menu == "devices" {
					p.menu = "main"
					p.cursor = 0
					p.update()
				}
			case key.Mods == command && key.Key == ui.KeyQ:
				if p.live != nil && p.live.system != nil {
					p.live.system.Dismiss(true)
				}
				p.visible = false
				p.update()
			}
			return true
		}

		// Live query-dependent shortcuts evaluated against current key.Text().
		if before == "" {
			switch {
			case key.Mods == 0 && key.Key == ui.KeyBackspace:
				if len(p.chips) > 0 {
					p.chips = p.chips[:len(p.chips)-1]
					p.queryChanged()
					p.suggestOpen = false
					p.suggestFocused = false
					p.update()
					return true
				}
				if p.kind != "" {
					p.kind = ""
					p.queryChanged()
					p.suggestOpen = false
					p.suggestFocused = false
					p.update()
					return true
				}
				if p.favoriteOnly {
					p.favoriteOnly = false
					p.queryDue = time.Time{}
					p.applySearch()
					p.update()
					return true
				}
			case key.Mods == command && key.Key == ui.KeyV:
				p.restore(true, false, false)
				p.update()
				return true
			case key.Mods == (command|ui.Shift) && key.Key == ui.KeyV:
				p.restore(true, true, false)
				p.update()
				return true
			case key.Mods == 0 && key.Key == ui.KeyHome:
				if len(p.rows) > 0 {
					p.selectRow(0)
					return true
				}
			case key.Mods == 0 && key.Key == ui.KeyEnd:
				if len(p.rows) > 0 {
					p.selectRow(len(p.rows) - 1)
					return true
				}
			case key.Mods == 0 && (key.Key == ui.KeyLeft || key.Key == ui.KeyRight) && p.kind == "图片":
				stepGridHorizontal(key.Key == ui.KeyRight)
				return true
			}
		}
		if key.Mods == command && key.Key == ui.KeyC {
			if selStart == selEnd {
				p.restore(false, false, false)
				p.update()
				return true
			}
			return false
		}
		if key.Mods == command && key.Key == ui.Key0 {
			if p.toggleImageActualSize() {
				return true
			}
			if runtime.GOOS == "linux" {
				p.selectAndPaste(9, false)
				return true
			}
		}

		taken := p.keySequence.take(shortcutStroke{key.Mods, key.Key}, commands)
		if !taken {
			fn := handlers[binding{key.Mods, key.Key}]
			if fn == nil {
				return false
			}
			fn()
		}
		if before != p.query {
			key.SetText(p.query)
		}
		p.update()
		return true
	})
	bind(0, ui.KeyEscape, p.escape)
	bind(0, ui.KeyTab, func() { p.tab(false) })
	bind(ui.Shift, ui.KeyTab, func() { p.tab(true) })
	bind(0, ui.KeyUp, func() { p.navigate(false) })
	bind(0, ui.KeyDown, func() { p.navigate(true) })
	bind(ui.Ctrl, ui.KeyN, func() { p.navigate(true) })
	bind(ui.Ctrl, ui.KeyP, func() { p.navigate(false) })
	bind(0, ui.KeyPageUp, func() { p.pageNavigate(false) })
	bind(0, ui.KeyPageDown, func() { p.pageNavigate(true) })
	bind(ui.Alt, ui.KeyHome, func() {
		if len(p.rows) > 0 {
			p.selectRow(0)
		}
	})
	bind(ui.Alt, ui.KeyEnd, func() {
		if len(p.rows) > 0 {
			p.selectRow(len(p.rows) - 1)
		}
	})
	bind(command, ui.KeyHome, func() {
		if len(p.rows) > 0 {
			p.selectRow(0)
		}
	})
	bind(command, ui.KeyEnd, func() {
		if len(p.rows) > 0 {
			p.selectRow(len(p.rows) - 1)
		}
	})
	bind(ui.Alt, ui.KeyLeft, func() { stepGridHorizontal(false) })
	bind(ui.Alt, ui.KeyRight, func() { stepGridHorizontal(true) })
	bind(ui.Alt, ui.KeyUp, func() { p.scrollPreview(-120) })
	bind(ui.Alt, ui.KeyDown, func() { p.scrollPreview(120) })
	bind(ui.Shift, ui.KeyPageUp, func() { p.scrollPreview(-220) })
	bind(ui.Shift, ui.KeyPageDown, func() { p.scrollPreview(220) })
	bind(ui.Ctrl, ui.KeySpace, p.togglePreviewNow)
	bind(ui.Alt, ui.KeySpace, p.togglePreviewNow)
	bind(ui.Alt, ui.Key0, func() { p.toggleImageActualSize() })
	bind(0, ui.KeyEnter, func() { enter(false, false) })
	bind(ui.Shift, ui.KeyEnter, func() { enter(true, false) })
	bind(command, ui.KeyEnter, func() { enter(false, true) })
	bind(command|ui.Shift, ui.KeyEnter, func() { enter(false, true) })
	bind(command|ui.Shift, ui.KeyBackspace, func() { p.runAction("delete") })
	if runtime.GOOS == "linux" {
		bind(ui.Alt, ui.KeyBackspace, func() { p.runAction("delete") })
		bind(ui.Alt, ui.KeyEnter, func() { enter(true, false) })
		for i, key := range []ui.Key{ui.Key1, ui.Key2, ui.Key3, ui.Key4, ui.Key5, ui.Key6, ui.Key7, ui.Key8, ui.Key9, ui.Key0} {
			idx := i
			bind(command|ui.Alt, key, func() { p.selectAndPaste(idx, true) })
		}
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
		idx := i
		for _, plain := range []bool{false, true} {
			isPlain := plain
			mods := command
			if isPlain {
				mods |= ui.Shift
			}
			bind(mods, key, func() {
				if !isPlain && len(p.rows) == 0 && len(p.suggestions()) == 0 {
					opts := p.relaxations()
					if idx < len(opts) {
						p.relax(opts[idx].dimension)
						return
					}
				}
				p.selectAndPaste(idx, isPlain)
			})
		}
	}
	c.OnWindowShortcut(command, ui.KeyQ, func() {
		if p.live != nil && p.live.system != nil {
			p.live.system.Dismiss(true)
		}
		p.visible = false
		p.update()
	})
}
func (p *quickPanelState) selectAndPaste(index int, plain bool) {
	if p.kind == "图片" {
		index += p.gridTop * 3
	}
	if index < len(p.rows) {
		p.selectRow(index)
		p.restore(true, plain, false)
	}
}

func (p *quickPanelState) suggestionList(c *ui.Context, a qpPalette, options []qpFilter) {
	heading := func(title, note string) {
		ui.Row(c).Height(22).PaddingX(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, title).FontSize(11 * zoomScale(p.textZoom)).Bold().TextColor(a.quiet)
			ui.Spacer(c)
			ui.Text(c, note).FontSize(11 * zoomScale(p.textZoom)).TextColor(a.quiet)
		})
	}
	heading(p.tr("建议"), p.tr("↑↓ 选择 · ⏎/⇥ 接受"))
	cursor := 0
	if p.suggestFocused && p.suggestCursor >= 0 && p.suggestCursor < len(options) {
		cursor = p.suggestCursor
	}
	const maxVisible = 5
	start := max(0, min(cursor-maxVisible+1, max(0, len(options)-maxVisible)))
	end := min(len(options), start+maxVisible)
	labels := map[string]string{"favorite": "收藏", "type": "类型", "tag": "标签", "source": "设备", "time": "时间", "extension": "扩展名"}
	for i := start; i < end; i++ {
		f := options[i]
		b := ui.ButtonBase(c.Key("suggest-"+f.dimension+f.value)).Label(p.tr("建议：")+p.filterLabel(f)).KeepFocus().Height(30).FillWidth().Padding(0, 8, 0, 6).Radius(7).Row().AlignItems(ui.Center).Gap(8)
		if cursor == i {
			b.Background(a.muted)
		}
		b.Children(func() {
			ui.Column(c).Width(16)
			ui.Text(c, f.matched).Font("monospace").FontSize(11.5 * zoomScale(p.textZoom)).PaddingX(5).Radius(4).Background(a.muted).MaxLines(1)
			ui.Text(c, "→").FontSize(12.5 * zoomScale(p.textZoom)).TextColor(a.quiet)
			ui.Text(c, p.tr(labels[f.dimension])).FontSize(12.5 * zoomScale(p.textZoom)).TextColor(a.quiet)
			ui.Text(c, p.filterLabel(f)).FontSize(12.5*zoomScale(p.textZoom)).Bold().TextColor(a.primary).Padding(1, 7).Radius(5).Background(a.muted).Grow(1).MinWidth(0).MaxLines(1)
			if cursor == i {
				qpKeycap(c, "⏎", a)
			} else {
				qpKeycap(c, "⇥", a)
			}
		})
		b.OnClick(func() { p.accept(f); p.input.Focus() })
	}
	if hidden := len(options) - end; hidden > 0 {
		ui.Text(c, fmt.Sprintf(p.tr("还有 %d 条"), hidden)).FontSize(11*zoomScale(p.textZoom)).TextColor(a.quiet).Padding(4, 8)
	}
	heading(p.tr("文字匹配"), "")
}
func qpMarkedSpans(e qpEntry, query string, selected, staleQuery bool, ink, quiet ui.Color) []ui.Span {
	text := strings.NewReplacer("\n", " ", "\r", " ").Replace(e.title)
	q := strings.ToLower(strings.TrimSpace(query))
	lower := []rune(strings.ToLower(text))
	queryRunes := []rune(q)
	tint := ink.Alpha(.22)
	if selected {
		tint = ink.Alpha(.3)
	}
	end := 0
	spans := []ui.Span{}
	for at, r := range []rune(text) {
		if q != "" && strings.HasPrefix(string(lower[at:]), q) {
			end = at + len(queryRunes)
		}
		span := ui.Span{Text: string(r), Color: ink, Strikethrough: e.lost}
		matched := at < end
		if !staleQuery && len(e.highlights) > 0 {
			matched = false
			for _, range_ := range e.highlights {
				matched = matched || (at >= range_.Start && at < range_.End)
			}
		}
		if matched {
			span.Background = tint
			span.Radius = 3
			span.PaddingX = 1.5
		}
		if e.kind == "链接" {
			slash := slices.Index([]rune(text), '/')
			if slash < 0 || at < slash {
				span.Weight = 500
			} else {
				span.Color = quiet
			}
		}
		if n := len(spans); n > 0 {
			prev := &spans[n-1]
			if prev.Color == span.Color && prev.Background == span.Background && prev.Weight == span.Weight && prev.Strikethrough == span.Strikethrough && prev.Radius == span.Radius {
				prev.Text += span.Text
				continue
			}
		}
		spans = append(spans, span)
	}
	return spans
}
func qpSingleFileName(e qpEntry) string {
	lines := 0
	name := ""
	for _, line := range strings.Split(e.text, "\n") {
		if trimmed := strings.TrimSpace(line); trimmed != "" {
			lines++
			name = trimmed
		}
	}
	if lines > 1 {
		return ""
	}
	if name == "" {
		name = strings.TrimSpace(e.title)
	}
	return name
}

func qpIsSingleImageFile(e qpEntry) bool {
	if e.kind != "文件" || e.lost {
		return false
	}
	return isImageFilename(qpSingleFileName(e))
}

func qpFileExtensionBadge(e qpEntry) string {
	name := ""
	for _, line := range strings.Split(e.text, "\n") {
		if trimmed := strings.TrimSpace(line); trimmed != "" {
			name = trimmed
			break
		}
	}
	if name == "" {
		name = strings.TrimSpace(e.title)
	}
	if i := strings.LastIndexAny(name, `/\`); i >= 0 {
		name = name[i+1:]
	}
	dot := strings.LastIndexByte(name, '.')
	if dot <= 0 || dot == len(name)-1 {
		return ""
	}
	ext := name[dot+1:]
	if len(ext) > 5 {
		return ""
	}
	for i := 0; i < len(ext); i++ {
		b := ext[i]
		isLetter := (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z')
		isDigit := b >= '0' && b <= '9'
		if !isLetter && !isDigit {
			return ""
		}
	}
	return strings.ToUpper(ext)
}

func (p *quickPanelState) resultRow(c *ui.Context, k desktopkit.Kit, a qpPalette, i int, e qpEntry, staleQuery bool) ui.Element {
	selected := i == p.selected && !p.suggestFocused
	ink, quiet := a.foreground, a.quiet
	if selected {
		ink = a.white
		quiet = a.white.Alpha(.78)
	}
	b := ui.ButtonBase(c).Label(p.tr("历史：")+e.title).KeepFocus().FillWidth().Height(30).Shrink(0).Padding(0, 10, 0, 6).Radius(7).Row().Gap(8).AlignItems(ui.Center).TextColor(ink)
	if selected {
		b.Background(a.primary)
	} else if b.Hovered() && !p.keyboard {
		b.Background(a.muted)
	}
	b.Children(func() {
		ui.Row(c).Width(24).Shrink(0).Justify(ui.Center).Children(func() {
			if p.quickHasImagePreview(e) {
				if img := p.quickImage(e); img.source != nil {
					img.element(c).Size(24, 16).Radius(3).Fit(ui.Cover)
				} else {
					k.ShimmerBox(c.Key("thumb-skel"), 3).Size(24, 16)
				}
			} else {
				icons := map[string]desktopkit.IconName{"文本": desktopkit.IconList, "代码": desktopkit.IconPackage, "链接": desktopkit.IconGlobe, "文件": desktopkit.IconFile, "文件夹": desktopkit.IconFolder, "富文本": desktopkit.IconList}
				k.Icon(c, icons[e.kind], 14).TextColor(quiet)
			}
		})
		text := ui.RichText(c, qpMarkedSpans(e, p.query, selected, staleQuery, ink, quiet)...).FontSize(13 * zoomScale(p.textZoom)).Grow(1).MinWidth(0).MaxLines(1)
		if e.kind == "代码" {
			text.Font("monospace").FontSize(12 * zoomScale(p.textZoom))
		}
		if e.lost {
			text.Opacity(.5)
		}
		if e.kind == "富文本" || e.kind == "文件" || e.kind == "文件夹" {
			label := p.tr(e.kind)
			if e.kind == "文件" {
				if ext := qpFileExtensionBadge(e); ext != "" {
					label = ext
				}
			}
			ui.Text(c, label).FontSize(10.5*zoomScale(p.textZoom)).TextColor(quiet).Background(a.muted).Radius(4).Padding(1, 5).Shrink(0)
		}
		if e.favorite {
			ui.Text(c, "★").FontSize(12 * zoomScale(p.textZoom)).TextColor(p.colors.Color(c.Theme().Dark, "status-yellow-text"))
		}
		if e.source != "本机" && e.source != p.tr("本机") {
			desktopkit.For(false).Icon(c, desktopkit.IconDownload, 12).TextColor(quiet)
		}
		ui.Text(c, e.age).FontSize(11 * zoomScale(p.textZoom)).TextColor(quiet).Shrink(0)
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
	if i == p.activeIndex() && !p.suggestFocused {
		if rect := p.clampedAnchor(b.Bounds(), p.listViewport); rect.H > 0 {
			p.anchor = rect
		}
	}
	mods := b.ClickModifiers()
	b.OnClick(func() { p.selectRow(i); p.restore(true, mods&ui.Shift != 0, false) })
	if b.RightClicked() {
		p.selectRow(i)
		p.openActions()
	}
	return b
}
func (p *quickPanelState) imageGrid(c *ui.Context, k desktopkit.Kit, a qpPalette) {
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
					b := ui.ButtonBase(c.Key(e.id)).Label(p.tr("图片：")+e.title).KeepFocus().Grow(1).MinWidth(0).Height(86).Padding(2).Radius(7).Border(2, a.border).Clip()
					if at == p.selected {
						b.BorderColor(a.primary)
					}
					b.Children(func() {
						if img := p.quickImage(e); img.source != nil {
							img.element(c).Fill().Fit(ui.Cover).Radius(5)
						} else {
							k.ShimmerBox(c.Key("img-skel"), 5).Fill()
						}
						ui.Row(c).Absolute().Right(4).Bottom(4).Children(func() { qpKeycap(c, p.commandLabel()+fmt.Sprint(row*3+col+1), a) })
					})
					if b.Hovered() && !p.keyboard && p.menu == "" && p.hovered != at {
						p.hovered = at
						p.schedulePreview()
						p.update()
					}
					if at == p.activeIndex() {
						if rect := p.clampedAnchor(b.Bounds(), p.listViewport); rect.H > 0 {
							p.anchor = rect
						}
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
func (p *quickPanelState) emptyView(c *ui.Context, a qpPalette) {
	title := p.tr("还没有剪贴板历史")
	hint := p.tr("在任意设备上复制内容，都会出现在这里。")
	if p.mode == "disconnected" {
		title = p.tr("同步服务未响应")
		hint = p.tr("本次唤起期间恢复会自动刷新列表。")
	} else if p.live != nil && p.live.d.live.readError != "" {
		title = p.tr(p.live.d.live.readError)
	} else if p.query != "" {
		title = p.tr("没有匹配“") + p.query + p.tr("”的内容")
		hint = p.tr("试试其他关键词或筛选条件")
	} else if p.hasActiveFilter() {
		title = p.tr("没有符合条件的内容")
		hint = p.tr("试试其他关键词或筛选条件")
	}
	ui.Column(c).FillWidth().MinHeight(220).Gap(10).Justify(ui.Center).AlignItems(ui.Center).Children(func() {
		ui.Text(c, title).Role(ui.RoleHeading).FontSize(15 * zoomScale(p.textZoom)).Bold()
		ui.Text(c, hint).FontSize(12 * zoomScale(p.textZoom)).TextColor(a.quiet)
		if p.mode == "disconnected" {
			ui.Button(c, p.tr("立即重连")).KeepFocus().OnClick(func() { p.mode = "history"; p.applySearch(); p.update() })
		} else if p.hasActiveFilter() {
			options := p.relaxations()
			if len(options) > 0 {
				ui.Text(c, p.tr("试试放宽：")).FontSize(12 * zoomScale(p.textZoom)).TextColor(a.quiet)
				for i, r := range options {
					b := ui.ButtonBase(c.Key(r.dimension)).Label(r.label).KeepFocus().FillWidth().Height(30).Radius(7).PaddingX(10).Row().Gap(8).AlignItems(ui.Center)
					if i == p.relaxCursor {
						b.Background(a.muted)
					}
					b.Children(func() {
						qpKeycap(c, p.commandLabel()+fmt.Sprint(i+1), a)
						ui.Text(c, r.label).FontSize(13 * zoomScale(p.textZoom)).Grow(1)
						ui.Text(c, fmt.Sprintf(p.tr("%d 条"), r.count)).FontSize(12 * zoomScale(p.textZoom)).TextColor(a.quiet)
					})
					b.OnClick(func() { p.relax(r.dimension); p.input.Focus() })
				}
			}
			ui.Button(c, p.tr("清空筛选")).KeepFocus().OnClick(func() { p.clear(); p.input.Focus() })
		} else {
			ui.Text(c, p.tr("随时唤起  ")+p.commandLabel()+"Ctrl+V").FontSize(12 * zoomScale(p.textZoom)).TextColor(a.quiet)
		}
	})
}
func (p *quickPanelState) satellite(c *ui.Context) {
	p.satelliteInvalidate = c.Invalidate
	a := p.palette(c)
	k := desktopkit.Kit{Dark: p.theme == "dark" || (p.theme == "system" && c.Theme().Dark), Colors: p.colors}
	k.Apply(c)
	prevMeasure := p.measureText
	p.measureText = c.MeasureText
	defer func() { p.measureText = prevMeasure }()
	ui.Column(c).Fill().Radius(12).Border(qpSatelliteBorder, a.border).Background(a.background).TextColor(a.foreground).Clip().Children(func() {
		e := p.previewEntry()
		if e == nil {
			return
		}
		if p.menu == "" && p.quickHasImagePreview(*e) {
			p.imagePreview(c, k, a, *e)
			return
		}
		title := p.tr(e.kind) + " · " + e.age + p.tr(" · 来自 ") + e.source
		if p.menu != "" {
			title = p.tr("操作")
			if p.menu == "devices" {
				title = p.tr("发送到")
			}
		}
		header := ui.Text(c, title).FontSize(11 * zoomScale(p.textZoom)).TextColor(a.quiet).Shrink(0)
		if p.menu != "" {
			header.Padding(qpSatelliteMenuPad)
		} else {
			header.Padding(qpSatelliteHeaderPadTop, qpSatelliteHeaderPadSide, qpSatelliteHeaderPadBot, qpSatelliteHeaderPadSide)
		}
		if p.lastPreviewID != e.id {
			p.lastPreviewID = e.id
			p.satelliteScroll.Y = 0
		}
		scroll := k.Scroll(c.Key("satellite-scroll-" + p.menu + "-" + e.id)).TrackScroll(&p.satelliteScroll).Grow(1).MinHeight(0)
		scroll.Children(func() {
			if p.menu != "" {
				ui.Column(c).FillWidth().Padding(qpSatelliteMenuListPad).Gap(qpSatelliteMenuRowGap).Children(func() {
					for i, row := range p.actions() {
						b := ui.ButtonBase(c.Key(row.id)).Label(p.actionLabel(row)).KeepFocus().Disabled(!row.enabled).FillWidth().Height(qpSatelliteMenuRowHeight).PaddingX(10).Radius(7).Row().Gap(8).AlignItems(ui.Center)
						if i == p.cursor {
							b.Background(a.primary).TextColor(a.white)
						}
						if !row.enabled {
							b.Opacity(.45)
						}
						b.Children(func() {
							ui.Text(c, p.actionLabel(row)).FontSize(13 * zoomScale(p.textZoom)).Grow(1)
							if row.shortcut != "" {
								qpKeycap(c, row.shortcut, a)
							}
						})
						b.OnClick(func() { p.runAction(row.id) })
					}
				})
				return
			}
			ui.Column(c).FillWidth().Padding(qpSatelliteBodyPadTop, qpSatelliteBodyPadSide, qpSatelliteBodyPadBot, qpSatelliteBodyPadSide).Gap(qpSatelliteBodyGap).Children(func() {
				switch e.kind {
				case "图片":
					p.quickImage(*e).element(c).FillWidth().Height(230).Fit(ui.Contain).Label(e.title)
				case "富文本":
					if p.live != nil {
						ui.Text(c, e.text).FontSize(14 * zoomScale(p.textZoom)).FillWidth().Selectable()
						break
					}
					if p.richPreview == nil {
						p.richPreview = richtext.NewDocument(richtext.ParseHTML("<h2>设计评审记录</h2><p>确认列表高度与<strong>键盘操作</strong>。</p><ul><li>保留清晰的选中状态。</li><li>核对浅色和深色主题。</li></ul>"))
					}
					richtext.EditorBase(c, p.richPreview, richtext.EditorOptions{Label: p.tr("富文本预览"), ReadOnly: true}).FillWidth().Height(200)
				case "链接":
					ui.Text(c, e.title).FontSize(14 * zoomScale(p.textZoom)).Bold().FillWidth().Selectable()
					ui.Text(c, e.text).FontSize(12 * zoomScale(p.textZoom)).TextColor(a.quiet).FillWidth().Selectable()
				case "文件":
					for _, file := range strings.Split(e.text, "\n") {
						ui.Row(c).FillWidth().Gap(qpSatelliteFileRowGap).AlignItems(ui.Center).Children(func() {
							k.Icon(c, desktopkit.IconFile, qpSatelliteFileIconSize).Shrink(0)
							ui.Text(c, file).FontSize(13 * zoomScale(p.textZoom)).Grow(1).MinWidth(0)
						})
					}
				default:
					text := ui.Text(c, e.text).FontSize(14 * zoomScale(p.textZoom)).FillWidth().Selectable()
					if e.kind == "代码" {
						text.Font("monospace").FontSize(12 * zoomScale(p.textZoom))
					}
				}
			})
		})
		if p.menu != "" {
			ui.Text(c, p.tr("↑↓ 选择 · ⏎ 执行 · esc 返回")).FontSize(11*zoomScale(p.textZoom)).TextColor(a.quiet).Padding(10, 12).Shrink(0)
		}
	})
	if p.visible && (p.menu != "" || p.previewEntry() != nil) && p.onUpdate != nil {
		if w, h := p.satelliteSize(); w != p.satelliteWidth || h != p.satelliteHeight {
			p.onUpdate()
		}
	}
}

func (p *quickPanelState) pasteTargetLabel() string {
	if p.live != nil {
		if p.live.system != nil && p.live.system.CanPaste {
			return p.tr("粘贴到 ") + p.live.system.TargetName
		}
		return p.tr("复制到剪贴板")
	}
	return p.tr("粘贴到 TextEdit")
}
