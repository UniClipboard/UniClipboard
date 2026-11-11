// Package desktopdemo restores the main branch desktop composition with local fixtures.
// It has no daemon client and never writes the system clipboard or application settings.
package desktopdemo

import (
	"errors"
	"flag"
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

const Reference = "8dd3796f8bbb6b1e1e07ee352a4e65e4341f56e7"

type entry struct {
	id, title, text, source, date, size string
	kind                                uikit.ContentKind
	tags                                []string
	pinned                              bool
}

type devicePath struct {
	name, state, lastSync string
	failed                bool
}

// device is a fixture. Latency, link, direction and content-type policy have no Engine
// field yet; the demo keeps them locally.
type device struct {
	id, name, platform string
	icon               uikit.IconName
	online             bool
	latency            int
	link, lastSeen     string
	fingerprint        string
	direction          string // both, send, recv, pause
	text, image, file  bool
	paths              []devicePath
}

type pendingDevice struct {
	id, name, platform, via, fingerprint string
	icon                                 uikit.IconName
}

type deviceCandidate struct {
	id, name, introducer string
	icon                 uikit.IconName
}

type demo struct {
	openQuickPanel                                                         func()
	quick                                                                  *quickDemo
	tags                                                                   []demoTag
	tagManager                                                             tagManagerState
	assignTagsOpen                                                         bool
	search                                                                 demoSearch
	page, theme, query, kind, source, tag, timeRange, layout               string
	nav, settings                                                          uikit.NavState
	list                                                                   uikit.ListState
	deviceList                                                             uikit.SourceState
	entries                                                                []entry
	devices                                                                []device
	selected                                                               string
	confirmEntries                                                         []string
	sidebarHidden, detailOpen, inspector, devicesOpen, tagsOpen, quickLook bool
	compact                                                                bool
	gridSize                                                               float64
	deviceTask, fieldError, notice, confirm, confirmID                     string
	deviceName, passphrase, confirmation, invitation                       string
	pending                                                                []pendingDevice
	candidates                                                             []deviceCandidate
	inviteCode, inviteMode, connectInput, renameValue, deviceNote          string
	renaming, deviceDetailOpen                                             bool
	options                                                                map[string]bool
	choices                                                                map[string]string
}

func newDemo(page, theme string) *demo {
	d := &demo{
		page: page, theme: theme, layout: "list", selected: "note",
		nav: uikit.NavState{Value: "all"}, settings: uikit.NavState{Value: "general"},
		deviceList: uikit.SourceState{Value: "local"}, gridSize: .4,
		deviceName: "MacBook Pro",
		options:    map[string]bool{"startup": true, "tray": true, "sound": true, "sync": true, "autoSync": true, "text": true, "image": true, "file": true, "relay": true, "quickPanel": true, "encryption": true},
		choices:    map[string]string{"language": "简体中文", "appearance": theme, "font": "标准", "fileLimit": "50 MB", "retention": "30 天", "proxy": "系统代理", "shortcut": "⌘⇧V"},
	}
	d.entries = []entry{
		{id: "note", title: "周一产品讨论", text: "周一产品讨论\n\n1. 确认桌面端布局与导航。\n2. 核对设备连接状态。\n3. 整理本周设计评审记录。\n\n下次评审：周三 14:00。", source: "local", date: "今天 10:42", size: "96 个字符", kind: uikit.KindText, tags: []string{"工作"}, pinned: true},
		{id: "code", title: "HTTP 服务示例", text: "func main() {\n    http.HandleFunc(\"/health\", handler)\n    http.ListenAndServe(\":8080\", nil)\n}", source: "linux", date: "今天 10:35", size: "Go · 4 行", kind: uikit.KindCode, tags: []string{"开发"}},
		{id: "link", title: "UniClipboard", text: "https://github.com/UniClipboard/UniClipboard", source: "windows", date: "今天 10:28", size: "链接", kind: uikit.KindLink, tags: []string{"工作"}},
		{id: "image", title: "桌面布局.png", text: "桌面布局", source: "phone", date: "今天 10:16", size: "1600 × 1000 · PNG · 286 KB", kind: uikit.KindImage, tags: []string{"设计"}},
		{id: "files", title: "设计交付 · 3 个文件", text: "界面规范.pdf\n组件清单.csv\n桌面布局.png", source: "windows", date: "今天 09:50", size: "3 个文件 · 12.8 MB", kind: uikit.KindFile, tags: []string{"设计", "工作"}},
		{id: "text", title: "会议室预订", text: "已预订 3 楼会议室。\n时间：14:00—15:00。\n参会人数：6 人。", source: "local", date: "昨天 17:20", size: "42 个字符", kind: uikit.KindText},
		{id: "offline", title: "发布检查清单", text: "确认版本号\n确认安装包\n核对更新说明", source: "linux", date: "昨天 16:05", size: "21 个字符", kind: uikit.KindText, tags: []string{"开发"}},
	}
	// Match the reference's eight current-day rows and fourteen older rows.
	// Use fabricated content rather than the user's clipboard data.
	d.entries = slices.Insert(d.entries, 5,
		entry{id: "schedule", title: "下午设计评审：确认三栏布局、字体和控件间距", text: "下午 14:00 开始设计评审。请准备界面规范与组件清单。", source: "local", date: "今天 09:30", kind: uikit.KindText},
		entry{id: "notes", title: "本周迭代安排", text: "完成历史窗口、设备管理和设置页面的布局核对。", source: "local", date: "今天 09:15", kind: uikit.KindText},
		entry{id: "thanks", title: "收到，评审记录已经整理完成。", text: "收到，评审记录已经整理完成。", source: "windows", date: "今天 08:50", kind: uikit.KindText},
	)
	older := []string{"确认设备名称与连接状态的显示方式", "整理桌面端快捷键说明", "历史内容预览与图片缩放", "组件清单与界面规范", "发送文件后的进度显示", "调整列表选中状态与悬停状态", "检查窄窗口中的返回操作", "核对深色模式下的文本对比度", "新增设备的指纹核对步骤", "统一设置页面中的分组间距", "更新设计评审结论", "明天继续核对安装包"}
	for i, title := range older {
		d.entries = append(d.entries, entry{id: fmt.Sprintf("older-%d", i), title: title, text: title, source: "local", date: fmt.Sprintf("昨天 %02d:20", 15-i), kind: uikit.KindText})
	}
	d.devices = []device{
		{id: "local", name: "MacBook Pro", platform: "macOS 15.6", icon: uikit.IconDesktop, online: true},
		{id: "windows", name: "工作电脑", platform: "Windows 11", icon: uikit.IconDesktop, online: true, latency: 15, link: "局域网", lastSeen: "刚刚", direction: "both", text: true, image: true, file: true,
			fingerprint: "8A42 7C91 2F60 B3D8 41E5 9A0C 7D12 5F3E", paths: []devicePath{{name: "tailcat · 默认", state: "已连接", lastSync: "1 分钟前"}}},
		{id: "linux", name: "Linux 工作站", platform: "Fedora 43", icon: uikit.IconDesktop, lastSeen: "昨天 18:02", direction: "both", text: true,
			fingerprint: "E310 4B6A 92CF 7D05 18A3 CC42 6F9B 20D7", paths: []devicePath{{name: "tailcat · 默认", state: "不可达", lastSync: "昨天 18:02", failed: true}}},
		{id: "phone", name: "我的手机", platform: "Android 16", icon: uikit.IconPhone, online: true, latency: 180, link: "加密中继", lastSeen: "刚刚", direction: "recv", text: true, image: true,
			fingerprint: "5D08 AA13 7E42 9B6C F1D4 30E7 82A5 C619", paths: []devicePath{{name: "tailcat · 默认", state: "已连接（经中继）", lastSync: "12 分钟前"}, {name: "tailcat · 工作仓库", state: "对方未批准", lastSync: "—", failed: true}}},
	}
	d.pending = []pendingDevice{{id: "p1", name: "新电脑", platform: "Windows 11", via: "配对码 482-913", icon: uikit.IconDesktop, fingerprint: "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D"}}
	d.candidates = []deviceCandidate{{id: "c1", name: "Alice 的平板", introducer: "工作电脑 介绍", icon: uikit.IconPhone}}
	d.inviteCode, d.inviteMode = "482-913", "code"
	d.initTags()
	d.list.Select(d.selected)
	return d
}

// Run opens an isolated fixture window. Parse all options before starting mygo.
func Run(args []string) error {
	flags := flag.NewFlagSet("gui --demo", flag.ContinueOnError)
	page := flags.String("page", "history", "history, devices, settings, setup, unlock, startup, recovery, quick-panel")
	theme := flags.String("theme", "system", "system, light, dark")
	quickState := flags.String("quick-state", "history", "history, locked, disconnected, empty")
	if err := flags.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	if flags.NArg() != 0 {
		return fmt.Errorf("unexpected demo argument: %s", flags.Arg(0))
	}
	if !slices.Contains([]string{"history", "devices", "settings", "setup", "unlock", "startup", "recovery", "quick-panel"}, *page) {
		return fmt.Errorf("unknown demo page: %s", *page)
	}
	if !slices.Contains([]string{"system", "light", "dark"}, *theme) {
		return fmt.Errorf("unknown demo theme: %s", *theme)
	}
	if !slices.Contains([]string{"history", "locked", "disconnected", "empty"}, *quickState) {
		return fmt.Errorf("unknown quick panel state: %s", *quickState)
	}
	if *page == "quick-panel" {
		return runQuickDemo(*theme, *quickState)
	}
	d := newDemo(*page, *theme)
	if *page == "devices" {
		d.page, d.devicesOpen = "history", true
	}
	mygo.App.SetName("unic UI Demo")
	mygo.App.WhenReady(func() {
		width, height := 1100, 720
		opts := mygo.WindowOptions{Title: "unic · 界面演示", Width: width, Height: height, MinWidth: 600, MinHeight: 480, Content: ui.View(d.view)}
		if uikit.DefaultSkin() == uikit.MacOS {
			opts.TitleBarStyle = mygo.TitleBarHidden
			opts.TrafficLightPosition = &mygo.Point{X: 19, Y: 19}
			opts.Vibrancy = mygo.VibrancySidebar
		}
		mygo.NewWindow(opts)
		var quickWindow *mygo.Window
		var quick *quickDemo
		d.openQuickPanel = func() {
			if quickWindow == nil {
				quick = newQuickDemo(d.theme, "history")
				quickWindow = openQuickDemoWindow(quick, func() { quickWindow = nil })
			} else {
				quick.visible = true
				quick.expanded = false
				quick.menu = ""
				quick.applySearch()
				quickWindow.Show()
				quickWindow.Invalidate()
			}
		}
	})
	return mygo.App.Run()
}

func (d *demo) view(c *ui.Context) {
	dark := c.Theme().Dark
	if d.theme != "system" {
		dark = d.theme == "dark"
	}
	k := uikit.For(dark)
	k.Apply(c)
	k.UiRoot(c, func() {
		w, _ := c.Size()
		// main's library collapses its sidebar below 1100 px. The Kit separately
		// switches list/detail navigation below SinglePaneWidth.
		compact := w < 1100
		if compact != d.compact {
			d.sidebarHidden, d.compact = compact, compact
		}
		c.OnShortcut(ui.Cmd, ui.KeyQ, mygo.App.Quit)
		if !d.devicesOpen && !d.tagsOpen && !d.assignTagsOpen && d.confirm == "" {
			if d.page == "history" {
				c.OnShortcut(ui.Cmd, ui.KeyComma, func() { d.page = "settings" })
				c.OnShortcut(ui.Cmd, ui.KeyP, d.togglePin)
			}
			if d.page == "settings" {
				c.OnShortcut(0, ui.KeyEscape, func() { d.page = "history" })
			}
		}
		switch d.page {
		case "history":
			d.history(c, k)
		case "settings":
			d.settingsPage(c, k)
		case "quick-panel":
			d.quickPanel(c, k)
		default:
			d.entryPage(c, k)
		}
		d.deviceSheet(c.Key("devices"), k)
		d.tagSheet(c.Key("tags"), k)
		d.assignTagSheet(c.Key("assign-tags"), k)
		d.confirmDialog(c.Key("confirm"), k)
	})
}

func (d *demo) sourceName(id string) string {
	for _, v := range d.devices {
		if v.id == id {
			if id == "local" {
				return "本机"
			}
			return v.name
		}
	}
	return "已移除设备"
}

func kindName(kind uikit.ContentKind) string {
	return []string{"文本", "代码", "链接", "图片", "文件"}[kind]
}

func (d *demo) filtered() []entry {
	var out []entry
	for _, v := range d.entries {
		if d.nav.Value == "pinned" && !v.pinned {
			continue
		}
		if !d.matchesSearch(v) {
			continue
		}
		out = append(out, v)
	}
	return out
}

func (d *demo) current() *entry {
	for i := range d.entries {
		if d.entries[i].id == d.selected {
			return &d.entries[i]
		}
	}
	return nil
}

func (d *demo) selectEntry(id string) { d.selected = id; d.list.Select(id); d.notice = "" }

func (d *demo) togglePin() {
	if v := d.current(); v != nil {
		on := !v.pinned
		ids := d.list.Selection()
		for i := range d.entries {
			if slices.Contains(ids, d.entries[i].id) {
				d.entries[i].pinned = on
			}
		}
	}
}

func (d *demo) requestDelete(ids []string) {
	if len(ids) == 0 {
		return
	}
	d.confirm, d.confirmID = fmt.Sprintf("删除 %d 条历史？", len(ids)), ""
	d.confirmEntries = slices.Clone(ids)
}

func (d *demo) confirmDialog(c *ui.Context, k uikit.Kit) {
	if d.confirm == "" {
		return
	}
	open := true
	desc := "此操作只修改本次演示中的假数据。"
	if strings.HasPrefix(d.confirmID, "device:") {
		desc = "撤销此设备的全部同步路径。对方需要重新配对并重新批准。已同步到对方的内容无法撤回。"
	}
	r := k.AlertDialog(c, uikit.AlertDialogProps{Open: &open, Title: d.confirm, Description: desc, ConfirmLabel: "移除", CancelLabel: "取消", Destructive: true}, nil)
	if r.Confirmed {
		if strings.HasPrefix(d.confirmID, "device:") {
			id := strings.TrimPrefix(d.confirmID, "device:")
			d.devices = slices.DeleteFunc(d.devices, func(v device) bool { return v.id == id })
			d.deviceList.Value, d.deviceDetailOpen = "local", false
		} else {
			d.entries = slices.DeleteFunc(d.entries, func(v entry) bool { return slices.Contains(d.confirmEntries, v.id) })
			d.selected = ""
		}
	}
	if !open {
		d.confirm, d.confirmID = "", ""
		d.confirmEntries = nil
	}
}
