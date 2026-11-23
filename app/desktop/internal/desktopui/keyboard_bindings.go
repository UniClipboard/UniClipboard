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

func recordStroke(key ui.TextKey) string {
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
		commands[desktopkeys.Search] = func() { d.search.restore = true; d.filterOpen = false }
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
		for _, cmd := range []shortcutCommand{
			{first: shortcutStroke{ui.Cmd, ui.KeyP}, run: d.togglePin},
			{first: shortcutStroke{key: ui.KeyC}, run: func() {
				if id := d.list.Hovered(d.layout == "grid"); id != "" {
					d.selectEntry(id)
					d.useEntry()
				}
			}},
			{first: shortcutStroke{key: ui.KeyD}, run: func() {
				if id := d.list.Hovered(d.layout == "grid"); id != "" {
					d.requestDelete([]string{id})
				}
			}},
			{first: shortcutStroke{key: ui.KeySpace}, run: func() {
				if item := d.current(); item != nil && (item.kind == desktopkit.KindImage || item.image != nil) {
					d.quickLook = true
				}
			}},
			{first: shortcutStroke{ui.Cmd | ui.Alt, ui.KeyI}, run: func() {
				if d.layout == "grid" && d.current() != nil {
					d.inspector = !d.inspector
				}
			}},
		} {
			bindings = append(bindings, cmd)
		}
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
					if k.Button(c.Key("edit"), caption, desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Disabled: d.live.busy}).Clicked() {
						*editor = shortcutEditor{id: action.ID}
					}
				})
				if editor.id == action.ID {
					changed = d.recordShortcut(c.Key("record-"+action.ID), k, cfg, action.ID) || changed
				}
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

func (d *viewState) recordShortcut(c *ui.Context, k desktopkit.Kit, cfg *localconfig.Config, id string) bool {
	editor := &d.shortcutEditor
	display := editor.value
	field := ui.TextInputBase(c.Key("keys"), &display).Label(d.tr("录制快捷键")).Placeholder(d.tr("按下一组或两组组合键")).Height(30).AutoFocus()
	field.OnTextKey(func(key ui.TextKey) bool {
		if key.Key == ui.KeyEscape {
			*editor = shortcutEditor{}
			return true
		}
		if stroke := recordStroke(key); stroke != "" {
			if strings.Count(editor.value, " ") == 1 {
				editor.value = ""
			}
			editor.value = strings.TrimSpace(editor.value + " " + stroke)
			editor.error = ""
			key.SetText(editor.value)
		}
		return true
	})
	changed := false
	settingsFooterBar(c, k, editor.error, editor.error != "", func() {
		for _, button := range []struct{ id, label string }{
			{"cancel", "取消"}, {"disable", "关闭"}, {"reset", "恢复默认"}, {"save", "保存"},
		} {
			if !k.Button(c.Key(button.id), d.tr(button.label), desktopkit.ButtonProps{
				Size: desktopkit.SizeSmall, Disabled: d.live.busy || button.id == "save" && editor.value == "",
			}).Clicked() {
				continue
			}
			if button.id == "cancel" {
				*editor = shortcutEditor{}
				return
			}
			next := make(map[string][]string, len(cfg.Desktop.Shortcuts)+1)
			maps.Copy(next, cfg.Desktop.Shortcuts)
			switch button.id {
			case "disable":
				next[id] = []string{}
			case "reset":
				delete(next, id)
			case "save":
				value, err := desktopkeys.Normalize(editor.value)
				if err != nil {
					editor.error = err.Error()
					return
				}
				next[id] = []string{value}
			}
			if err := desktopkeys.Validate(next, cfg.Desktop.QuickPanelShortcut); err != nil {
				editor.error = d.tr("此快捷键与现有绑定冲突。")
				return
			}
			cfg.Desktop.Shortcuts = next
			*editor = shortcutEditor{}
			changed = true
		}
	})
	return changed
}
