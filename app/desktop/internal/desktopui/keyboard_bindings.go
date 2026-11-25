package desktopui

import (
	"maps"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/internal/desktopkeys"
	localconfig "github.com/uniclipboard/unic/internal/localconfig"
)

type shortcutStroke struct {
	mods ui.Modifiers
	key  ui.Key
}

var shortcutKeys = map[string]ui.Key{
	"enter": ui.KeyEnter, "escape": ui.KeyEscape, "backspace": ui.KeyBackspace, "tab": ui.KeyTab,
	"space": ui.KeySpace, "delete": ui.KeyDelete, "insert": ui.KeyInsert, "home": ui.KeyHome, "end": ui.KeyEnd,
	"pageup": ui.KeyPageUp, "pagedown": ui.KeyPageDown, "arrowleft": ui.KeyLeft, "arrowright": ui.KeyRight,
	"arrowup": ui.KeyUp, "arrowdown": ui.KeyDown, "minus": ui.KeyMinus, "equal": ui.KeyEqual, "comma": ui.KeyComma,
	"period": ui.KeyPeriod, "slash": ui.KeySlash, "semicolon": ui.KeySemicolon, "quote": ui.KeyQuote,
	"bracketleft": ui.KeyBracketLeft, "bracketright": ui.KeyBracketRight, "backslash": ui.KeyBackslash, "backquote": ui.KeyBackquote,
	"f1": ui.KeyF1, "f2": ui.KeyF2, "f3": ui.KeyF3, "f4": ui.KeyF4, "f5": ui.KeyF5, "f6": ui.KeyF6,
	"f7": ui.KeyF7, "f8": ui.KeyF8, "f9": ui.KeyF9, "f10": ui.KeyF10, "f11": ui.KeyF11, "f12": ui.KeyF12,
}

func init() {
	for i, key := range "abcdefghijklmnopqrstuvwxyz" {
		shortcutKeys[string(key)] = ui.KeyA + ui.Key(i)
	}
	for i, key := range "0123456789" {
		shortcutKeys[string(key)] = ui.Key0 + ui.Key(i)
	}
}

var shortcutMods = map[string]ui.Modifiers{"ctrl": ui.Ctrl, "alt": ui.Alt, "shift": ui.Shift, "super": ui.Super, "mod": ui.Cmd}

func parseStroke(value string) shortcutStroke {
	parts := strings.Split(value, "+")
	last := len(parts) - 1
	stroke := shortcutStroke{key: shortcutKeys[parts[last]]}
	for _, name := range parts[:last] {
		stroke.mods |= shortcutMods[name]
	}
	return stroke
}

func recordStroke(key ui.InputEvent) string {
	for name, code := range shortcutKeys {
		if key.Key != code {
			continue
		}
		var parts []string
		for _, mod := range []string{"ctrl", "alt", "shift", "super"} {
			if key.Mods&shortcutMods[mod] != 0 {
				parts = append(parts, mod)
			}
		}
		return strings.Join(append(parts, name), "+")
	}
	return ""
}

type shortcutSequence struct {
	first shortcutStroke
	until time.Time
}

type shortcutCommand struct {
	first, second shortcutStroke
	run           func()
}

func shortcutCommands(overrides map[string][]string, commands map[string]func()) []shortcutCommand {
	var result []shortcutCommand
	for _, action := range desktopkeys.ActionsForPlatform() {
		run := commands[action.ID]
		if run == nil {
			continue
		}
		for _, value := range desktopkeys.Bindings(overrides, action) {
			normalized, err := desktopkeys.Normalize(value)
			if err != nil {
				continue
			}
			first, second, _ := strings.Cut(normalized, " ")
			result = append(result, shortcutCommand{parseStroke(first), parseStroke(second), run})
		}
	}
	return result
}

func (s *shortcutSequence) take(stroke shortcutStroke, commands []shortcutCommand) bool {
	now := time.Now()
	first := s.first
	active := now.Before(s.until)
	*s = shortcutSequence{}
	for _, command := range commands {
		if active && first == command.first && stroke == command.second {
			command.run()
			return true
		}
	}
	for _, command := range commands {
		if command.first != stroke {
			continue
		}
		if command.second.key == ui.KeyUnknown {
			command.run()
		} else {
			s.first, s.until = stroke, now.Add(time.Second)
		}
		return true
	}
	return false
}

func formatShortcutBindings(values []string) string {
	labels := make([]string, 0, len(values))
	for _, value := range values {
		normalized, err := desktopkeys.Normalize(value)
		if err != nil {
			labels = append(labels, value)
			continue
		}
		strokes := strings.Fields(desktopkeys.Physical(normalized))
		for i, stroke := range strokes {
			stroke = desktopkeys.Accelerator(stroke)
			super := "Super+"
			if ui.Cmd == ui.Super {
				super = "⌘"
			}
			stroke = strings.NewReplacer("ctrl+", "Ctrl+", "alt+", "Alt+", "shift+", "Shift+", "super+", super).Replace(stroke)
			if last := stroke[len(stroke)-1:]; last >= "a" && last <= "z" && (len(stroke) == 1 || strings.HasSuffix(stroke[:len(stroke)-1], "+") || strings.HasSuffix(stroke[:len(stroke)-1], "⌘")) {
				stroke = stroke[:len(stroke)-1] + strings.ToUpper(last)
			}
			strokes[i] = stroke
		}
		labels = append(labels, strings.Join(strokes, " "))
	}
	return strings.Join(labels, " / ")
}

func shortcutHint(overrides map[string][]string, id string) string {
	for _, action := range desktopkeys.ActionsForPlatform() {
		if action.ID == id {
			return formatShortcutBindings(desktopkeys.Bindings(overrides, action))
		}
	}
	return ""
}

func zoomScale(step int) float32 { return 1 + float32(step)/10 }

func (d *viewState) bindKeyboardCommands(c *ui.Context) {
	var overrides map[string][]string
	if d.live != nil {
		overrides = d.live.config.Config.Desktop.Shortcuts
	}
	commands := map[string]func(){
		desktopkeys.Settings: func() {
			if d.page == "settings" {
				d.page = d.shortcutReturnPage
				if d.page == "" {
					d.page = "history"
				}
			} else {
				d.shortcutReturnPage, d.page = d.page, "settings"
			}
		},
		desktopkeys.ZoomIn:    func() { d.zoomStep = min(5, d.zoomStep+1) },
		desktopkeys.ZoomOut:   func() { d.zoomStep = max(-3, d.zoomStep-1) },
		desktopkeys.All:       func() { d.openHistoryLibrary("all") },
		desktopkeys.Favorites: func() { d.openHistoryLibrary("pinned") },
		desktopkeys.Devices:   func() { d.page = "devices" },
	}
	if d.page == "history" {
		commands[desktopkeys.Sidebar] = func() { d.sidebarHidden = !d.sidebarHidden }
		commands[desktopkeys.Search] = func() {
			d.narrowSearchOpen = true
			d.search.restore = true
			d.search.hadActivity = false
			d.filterOpen = false
		}
		commands[desktopkeys.Favorite] = func() {
			if item := d.current(); item != nil {
				d.toggleFavorites([]string{item.id})
			}
		}
		commands[desktopkeys.Tags] = d.openTagEditor
		commands[desktopkeys.Layout] = func() {
			if d.layout == "grid" {
				d.layout = "list"
			} else {
				d.layout = "grid"
			}
			d.inspector, d.detailOpen = false, false
		}
		commands[desktopkeys.Send] = func() { d.historySendMenu, d.historyActionsMenu = !d.historySendMenu, false }
		commands[desktopkeys.Actions] = func() { d.historyActionsMenu, d.historySendMenu = !d.historyActionsMenu, false }
	}
	bindings := shortcutCommands(overrides, commands)
	if d.page == "history" {
		bindings = append(bindings,
			shortcutCommand{first: shortcutStroke{ui.Cmd, ui.KeyP}, run: d.togglePin},
			shortcutCommand{first: shortcutStroke{key: ui.KeyC}, run: func() {
				if id := d.list.Hovered(d.layout == "grid"); id != "" {
					d.selectEntry(id)
					d.useEntry()
				}
			}},
			shortcutCommand{first: shortcutStroke{key: ui.KeyD}, run: func() {
				if id := d.list.Hovered(d.layout == "grid"); id != "" {
					d.requestDelete([]string{id})
				}
			}},
			shortcutCommand{first: shortcutStroke{key: ui.KeySpace}, run: func() {
				if item := d.current(); item != nil && (item.kind == desktopkit.KindImage || item.image != nil) {
					d.quickLook = true
				}
			}},
			shortcutCommand{first: shortcutStroke{ui.Cmd | ui.Alt, ui.KeyI}, run: func() {
				if d.layout == "grid" && d.current() != nil {
					d.inspector = !d.inspector
				}
			}},
		)
	}
	seen := map[shortcutStroke]bool{}
	for _, command := range bindings {
		for _, stroke := range []shortcutStroke{command.first, command.second} {
			if stroke.key == ui.KeyUnknown || seen[stroke] {
				continue
			}
			seen[stroke] = true
			c.OnWindowShortcut(stroke.mods, stroke.key, func() {
				if c.ShortcutRepeated() {
					return
				}
				if stroke.mods&(ui.Ctrl|ui.Alt|ui.Super) == 0 && c.FocusTakesText() {
					d.keyboardSequence = shortcutSequence{}
					return
				}
				d.keyboardSequence.take(stroke, bindings)
			})
		}
	}
}

func (d *viewState) openHistoryLibrary(id string) {
	d.page, d.nav.Value = "history", id
	d.tag, d.source, d.notice = "", "", ""
	if d.compact {
		d.sidebarHidden = true
	}
}

type shortcutEditor struct {
	id, value, error string
	confirm          bool
}

func (d *viewState) editShortcuts(c *ui.Context, k desktopkit.Kit, cfg *localconfig.Config) bool {
	editor := &d.shortcutEditor
	changed := false
	for _, scope := range []struct{ id, title string }{
		{desktopkeys.Global, "全局快捷键"}, {desktopkeys.Window, "主窗口"}, {desktopkeys.History, "历史记录"}, {desktopkeys.Panel, "快捷面板"},
	} {
		k.SettingsGroup(c.Key(scope.id), desktopkit.SettingsGroupProps{Title: d.tr(scope.title)}, func() {
			for _, action := range desktopkeys.ActionsForPlatform() {
				if action.Scope != scope.id {
					continue
				}
				keys := desktopkeys.Bindings(cfg.Desktop.Shortcuts, action)
				if _, ok := cfg.Desktop.Shortcuts[action.ID]; !ok && action.ID == desktopkeys.TogglePanel && cfg.Desktop.QuickPanelShortcut != "" {
					keys = []string{cfg.Desktop.QuickPanelShortcut}
				}
				k.SettingsRow(c.Key(action.ID), desktopkit.SettingsRowProps{Label: d.tr(action.Label)}, nil, func() {
					caption := formatShortcutBindings(keys)
					if caption == "" {
						caption = d.tr("关闭")
					}
					trigger := k.Button(c.Key("edit"), caption, desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Disabled: d.live.busy})
					if trigger.Clicked() {
						*editor = shortcutEditor{id: action.ID}
					}
					if editor.id == action.ID {
						changed = d.recordShortcut(c.Key("record"), k, cfg, trigger) || changed
					}
					_, command := k.ToolbarIconMenu(c.Key("binding-menu"), desktopkit.ToolbarIconMenuProps{
						Label: d.tr("更多操作"), Icon: desktopkit.IconMore, Disabled: d.live.busy,
						Items: []desktopkit.MenuEntry{
							{ID: "reset", Label: d.tr("恢复默认"), Disabled: d.live.busy || cfg.Desktop.Shortcuts[action.ID] == nil},
							{ID: "clear", Label: d.tr("清除绑定"), Disabled: d.live.busy || len(keys) == 0},
						},
					})
					switch command {
					case "reset":
						changed = d.applyShortcut(cfg, action.ID, nil) || changed
					case "clear":
						changed = d.applyShortcut(cfg, action.ID, []string{}) || changed
					}
				})
			}
		})
	}
	if k.Button(c.Key("reset-shortcuts"), d.tr("全部恢复默认"), desktopkit.ButtonProps{Disabled: d.live.busy || len(cfg.Desktop.Shortcuts) == 0}).Clicked() {
		cfg.Desktop.Shortcuts = nil
		*editor = shortcutEditor{}
		changed = true
	}
	return changed
}

func (d *viewState) recordShortcut(c *ui.Context, k desktopkit.Kit, cfg *localconfig.Config, trigger ui.Element) bool {
	editor := &d.shortcutEditor
	save := func() bool {
		editor.confirm = false
		if d.live.busy || editor.value == "" {
			return false
		}
		return d.applyShortcut(cfg, editor.id, []string{editor.value})
	}
	if editor.confirm && save() {
		trigger.Focus()
		return true
	}
	open, changed := true, false
	popup := ui.PopoverBase(c, trigger, &open, func(panel ui.Element) {
		panel.AttachTo(trigger, ui.AnchorBottomEnd, ui.AnchorTopEnd).MarginY(6).
			Width(284).MaxWidthPercent(92).Radius(10).Padding(10, 12).Gap(8).
			Background(k.Color("background-primary-default")).Border(1, k.Color("border-button-default")).
			Label(d.tr("录制快捷键"))
		target := ui.Row(c.Key("keys")).Wrap().Gap(6).AlignItems(ui.Center).MinHeight(30).
			Focusable().AutoFocus().Role(ui.RoleGroup).Label(d.tr("按下一组或两组组合键"))
		target.OnClick(func() { target.Focus() })
		target.HandleInput(func(key ui.InputEvent) bool {
			if key.Kind != ui.InputKeyDown || key.Key == ui.KeyTab && key.Mods&^ui.Shift == 0 {
				return false
			}
			if key.Repeat {
				return true
			}
			if key.Mods == 0 {
				switch key.Key {
				case ui.KeyEscape:
					*editor = shortcutEditor{}
					return true
				case ui.KeyEnter:
					editor.confirm = true
					return true
				}
			}
			if stroke := recordStroke(key); stroke != "" {
				if strings.Contains(editor.value, " ") {
					editor.value = ""
				}
				editor.value = strings.TrimSpace(editor.value + " " + stroke)
				editor.error = ""
			}
			return true
		})
		target.Children(func() {
			first, second, _ := strings.Cut(editor.value, " ")
			if first == "" {
				ui.Text(c, d.tr("按下组合键")).FontSize(12).TextColor(k.Color("text-secondary"))
				return
			}
			recordedShortcutKey(c.Key("first"), k, first)
			if second != "" {
				ui.Text(c.Key("sequence-arrow"), "→").FontSize(12).TextColor(k.Color("text-tertiary")).AccessHidden(true)
				recordedShortcutKey(c.Key("second"), k, second)
			}
		})
		if editor.error != "" {
			ui.Text(c.Key("error"), editor.error).Role(ui.RoleAlert).FontSize(12).TextColor(k.Color("text-error-primary"))
		}
		ui.Row(c.Key("actions")).Gap(8).AlignItems(ui.Center).Justify(ui.SpaceBetween).Children(func() {
			ui.Text(c.Key("hint"), d.tr("Enter 确认 · Esc 取消")).FontSize(11).TextColor(k.Color("text-secondary"))
			if k.Button(c.Key("save"), d.tr("确认"), desktopkit.ButtonProps{
				Variant: desktopkit.Primary, Size: desktopkit.SizeSmall,
				Disabled: d.live.busy || editor.value == "",
			}).Clicked() {
				changed = save()
			}
		})
	})
	if popup.Dismissed() {
		*editor = shortcutEditor{}
		trigger.Focus()
	}
	if changed {
		trigger.Focus()
	}
	return changed
}

func (d *viewState) applyShortcut(cfg *localconfig.Config, id string, keys []string) bool {
	next := make(map[string][]string, len(cfg.Desktop.Shortcuts)+1)
	maps.Copy(next, cfg.Desktop.Shortcuts)
	if keys == nil {
		delete(next, id)
	} else {
		next[id] = keys
	}
	if err := desktopkeys.Validate(next, cfg.Desktop.QuickPanelShortcut); err != nil {
		message := d.tr("此快捷键与现有绑定冲突。")
		if d.shortcutEditor.id == id {
			d.shortcutEditor.error = message
		} else {
			d.live.message = message
		}
		return false
	}
	cfg.Desktop.Shortcuts = next
	d.shortcutEditor = shortcutEditor{}
	return true
}

func recordedShortcutKey(c *ui.Context, k desktopkit.Kit, stroke string) {
	ui.Row(c).Height(26).PaddingX(8).Radius(5).Border(1, k.Color("border-button-default")).
		Background(k.SubtleCardSurface()).AlignItems(ui.Center).Children(func() {
		ui.Text(c, formatShortcutBindings([]string{stroke})).FontSize(12).FontWeight(600).SingleLine()
	})
}
