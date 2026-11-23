// Package desktopkeys defines the persisted desktop shortcut actions.
package desktopkeys

import (
	"fmt"
	"runtime"
	"slices"
	"strings"
)

const (
	ZoomIn         = "global.zoomIn"
	ZoomOut        = "global.zoomOut"
	PanelWindowIn  = "quickPanel.windowIncrease"
	PanelWindowOut = "quickPanel.windowDecrease"
	PanelTextIn    = "quickPanel.textIncrease"
	PanelTextOut   = "quickPanel.textDecrease"
	Global         = "global"
	TogglePanel    = "global.toggleQuickPanel"
	ShowMain       = "global.showMainWindow"
	ShowFavorites  = "global.showFavoritePanel"
	Window         = "window"
	History        = "history"
	Panel          = "panel"

	Settings      = "nav.settings"
	Sidebar       = "nav.toggleSidebar"
	Favorite      = "clipboard.favorite"
	Search        = "clipboard.search"
	All           = "nav.allHistory"
	Favorites     = "nav.favorites"
	Devices       = "nav.devices"
	Layout        = "clipboard.toggleLayout"
	Tags          = "clipboard.editTags"
	Actions       = "clipboard.actions"
	Send          = "clipboard.send"
	PanelFavorite = "quickPanel.favorite"
	PanelActions  = "quickPanel.actions"
	PanelClear    = "quickPanel.clear"
)

type Action struct {
	ID, Scope, Label string
	Defaults         []string
}

func ActionsForPlatform() []Action {
	sidebar := "super+ctrl+s"
	if runtime.GOOS == "linux" {
		sidebar = "ctrl+b"
	}
	actions := []Action{
		{TogglePanel, Global, "打开快捷面板", nil},
		{ShowMain, Global, "打开主窗口", nil},
		{ShowFavorites, Global, "打开快捷面板并筛选已收藏", nil},
		{Settings, Window, "切换设置页", []string{"mod+comma"}},
		{ZoomIn, Window, "放大主界面", []string{"mod+equal", "mod+shift+equal"}},
		{ZoomOut, Window, "缩小主界面", []string{"mod+minus"}},
		{Sidebar, History, "切换侧栏", []string{sidebar}},
		{All, Window, "打开全部历史", []string{"mod+1"}},
		{Favorites, Window, "打开已收藏", []string{"mod+2"}},
		{Devices, Window, "管理设备…", []string{"mod+3"}},
		{Favorite, History, "切换收藏", []string{"f"}},
		{Search, History, "聚焦历史搜索框", []string{"mod+f"}},
		{Layout, History, "切换列表和网格", []string{"mod+shift+g"}},
		{Tags, History, "编辑标签", []string{"t"}},
		{Actions, History, "动作", []string{"mod+k"}},
		{Send, History, "发送到设备", []string{"mod+shift+d"}},
		{PanelFavorite, Panel, "切换收藏", []string{"mod+shift+f"}},
		{PanelActions, Panel, "打开动作菜单", []string{"mod+k"}},
		{PanelClear, Panel, "清空搜索和筛选", []string{"mod+backspace"}},
	}
	if runtime.GOOS == "linux" {
		actions = append(actions,
			Action{PanelWindowIn, Panel, "放大快捷面板窗口", []string{"ctrl+equal"}},
			Action{PanelWindowOut, Panel, "缩小快捷面板窗口", []string{"ctrl+minus"}},
			Action{PanelTextIn, Panel, "放大快捷面板文字", []string{"ctrl+shift+equal"}},
			Action{PanelTextOut, Panel, "缩小快捷面板文字", []string{"ctrl+shift+minus"}},
		)
	}
	return actions
}

func Bindings(overrides map[string][]string, action Action) []string {
	if keys, ok := overrides[action.ID]; ok {
		return keys
	}
	return action.Defaults
}

// NamedKeys matches the keyboard keys exposed by mygo's logical-key events.
var NamedKeys = []string{
	"enter", "escape", "backspace", "tab", "space", "delete", "insert", "home", "end", "pageup", "pagedown",
	"arrowleft", "arrowright", "arrowup", "arrowdown", "minus", "equal", "comma", "period", "slash", "semicolon",
	"quote", "bracketleft", "bracketright", "backslash", "backquote",
	"f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
}

var punctuationNames = map[string]string{
	"-": "minus", "=": "equal", ",": "comma", ".": "period", "/": "slash", ";": "semicolon", "'": "quote",
	"[": "bracketleft", "]": "bracketright", "\\": "backslash", "`": "backquote",
}

func Accelerator(stroke string) string {
	mods, key, ok := strings.Cut(stroke, "+")
	if ok {
		last := strings.LastIndexByte(stroke, '+')
		mods, key = stroke[:last+1], stroke[last+1:]
	} else {
		mods, key = "", stroke
	}
	for symbol, name := range punctuationNames {
		if key == name {
			key = symbol
			break
		}
	}
	return mods + strings.TrimPrefix(key, "arrow")
}

func Normalize(binding string) (string, error) {
	strokes := strings.Fields(strings.ToLower(binding))
	if len(strokes) < 1 || len(strokes) > 2 {
		return "", fmt.Errorf("shortcut requires one or two key combinations")
	}
	for i, stroke := range strokes {
		parts := strings.Split(stroke, "+")
		key := parts[len(parts)-1]
		if name, ok := punctuationNames[key]; ok {
			key = name
		}
		if !(len(key) == 1 && strings.Contains("abcdefghijklmnopqrstuvwxyz0123456789", key)) && !slices.Contains(NamedKeys, key) {
			return "", fmt.Errorf("unsupported shortcut key %q", key)
		}
		seen := map[string]bool{}
		for _, part := range parts[:len(parts)-1] {
			switch part {
			case "meta", "command", "cmd", "win":
				part = "super"
			case "control":
				part = "ctrl"
			case "option":
				part = "alt"
			case "cmdorctrl", "commandorcontrol":
				part = "mod"
			case "alt", "ctrl", "shift", "super", "mod":
			default:
				return "", fmt.Errorf("unsupported shortcut modifier %q", part)
			}
			if seen[part] {
				return "", fmt.Errorf("repeated shortcut modifier %q", part)
			}
			seen[part] = true
		}
		var normalized []string
		for _, mod := range []string{"ctrl", "alt", "shift", "super", "mod"} {
			if seen[mod] {
				normalized = append(normalized, mod)
			}
		}
		strokes[i] = strings.Join(append(normalized, key), "+")
	}
	return strings.Join(strokes, " "), nil
}

func Physical(binding string) string {
	mod := "ctrl"
	if runtime.GOOS == "darwin" {
		mod = "super"
	}
	strokes := strings.Fields(binding)
	for i, stroke := range strokes {
		parts := strings.Split(stroke, "+")
		for j := range len(parts) - 1 {
			if parts[j] == "mod" {
				parts[j] = mod
			}
		}
		normalized, _ := Normalize(strings.Join(parts, "+"))
		strokes[i] = normalized
	}
	return strings.Join(strokes, " ")
}

// Overlap reports whether normalized bindings can prevent one another
// from firing. The tray claims each step of a global sequence from the OS.
func Overlap(scope, key, otherScope, otherKey string) bool {
	if scope != otherScope && (scope == Global || otherScope == Global) {
		for _, stroke := range strings.Fields(key) {
			if slices.Contains(strings.Fields(otherKey), stroke) {
				return true
			}
		}
		return false
	}
	if (scope == Panel) != (otherScope == Panel) {
		return false
	}
	return key == otherKey || strings.HasPrefix(key, otherKey+" ") || strings.HasPrefix(otherKey, key+" ")
}

// Validate rejects ambiguous bindings within the main window or the panel.
// Commands in the two separate windows may intentionally share a binding.
func Validate(overrides map[string][]string, panelDefault string) error {
	actions := ActionsForPlatform()
	for id, values := range overrides {
		if !slices.ContainsFunc(actions, func(a Action) bool { return a.ID == id }) || len(values) > 8 {
			return fmt.Errorf("unsupported shortcut action or too many bindings for %q", id)
		}
		for _, value := range values {
			if _, err := Normalize(value); err != nil {
				return fmt.Errorf("%s: %w", id, err)
			}
		}
	}
	type binding struct{ scope, key, id string }
	var used []binding
	for _, action := range actions {
		values := Bindings(overrides, action)
		if _, ok := overrides[action.ID]; !ok && action.ID == TogglePanel && panelDefault != "" {
			values = []string{panelDefault}
		}
		for _, value := range values {
			normalized, _ := Normalize(value)
			key := Physical(normalized)
			if key == "" || strings.Contains(key, "  ") || strings.HasPrefix(key, " ") || strings.HasSuffix(key, " ") {
				return fmt.Errorf("repeated platform modifier for %s", action.ID)
			}
			for _, other := range used {
				if other.id != action.ID && Overlap(action.Scope, key, other.scope, other.key) {
					return fmt.Errorf("shortcut %q conflicts with %s", value, other.id)
				}
			}
			used = append(used, binding{action.Scope, key, action.ID})
		}
	}
	return nil
}
