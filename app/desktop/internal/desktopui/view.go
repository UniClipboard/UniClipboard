// Package desktopui owns the desktop renderer and production service binding.
package desktopui

import (
	"context"
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
)

type entry struct {
	generatedTitle                      bool
	rich                                *richtext.Document
	preview                             string
	highlights                          []engine.HistorySearchRange
	picture                             *historyImage
	image                               ui.ImageSource
	sourceAppName, sourceAppID          string
	sourceAppIcon                       ui.ImageSource
	contentError, syncState             string
	createdAt                           int64
	id, title, text, source, date, size string
	kind                                desktopkit.ContentKind
	tags                                []string
	pinned                              bool
	folder                              *folderInfo
}

// folderInfo is the display model of a folder entry. Rows exist only for snapshots.
type folderInfo struct {
	names     []string
	reference bool
	files     int
	dirs      int
	bytes     int64
	rows      []folderRow
	truncated bool
	listError bool // the live folder could not be read
}
type folderRow struct {
	depth int
	name  string
	dir   bool
	size  int64
}

func humanBytes(n int64) string {
	const u = "KMGT"
	if n < 1024 {
		return fmt.Sprintf("%d B", n)
	}
	f, i := float64(n)/1024, 0
	for f >= 1024 && i < len(u)-1 {
		f /= 1024
		i++
	}
	return fmt.Sprintf("%.1f %cB", f, u[i])
}

type devicePath struct {
	name, state, lastSync string
	failed                bool
}

// device holds shared display state. Preview-only latency, link, direction and
// content-type policy remain separate from the Engine-backed fields.
type device struct {
	id, name, platform string
	icon               desktopkit.IconName
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
	icon                                 desktopkit.IconName
}

type deviceCandidate struct {
	id, name, introducer string
	backendID            string
	icon                 desktopkit.IconName
}

type viewState struct {
	toolbarTrailing func(*ui.Context)
	windowFrame     *bool
	pluginSettings  builtinPluginSettings

	reference                                                              string
	previewImage                                                           ui.ImageSource
	language                                                               string
	quickShortcutStatus                                                    func() (string, string)
	retryQuickShortcut                                                     func()
	live                                                                   *liveState
	tagEditor                                                              tagEditorState
	openQuickPanel                                                         func()
	quick                                                                  *quickPanelState
	tags                                                                   []historyTag
	tagManager                                                             tagManagerState
	search                                                                 historySearchState
	page, theme, query, kind, source, tag, timeRange, layout               string
	nav, settings                                                          desktopkit.NavState
	list                                                                   desktopkit.ListState
	deviceList                                                             desktopkit.SourceState
	entries                                                                []entry
	devices                                                                []device
	selected                                                               string
	confirmEntries                                                         []string
	drawerSwitchDir                                                                     int
	sidebarHidden, detailOpen, inspector, devicesOpen, tagsOpen, quickLook, filterOpen bool
	compact                                                                bool
	gridSize                                                               float64
	deviceTask, fieldError, notice, confirm, confirmID                     string
	deviceName                                                             string
	pending                                                                []pendingDevice
	candidates                                                             []deviceCandidate
	inviteCode, inviteMode, connectInput, renameValue, deviceNote          string
	renaming, deviceDetailOpen                                             bool
	options                                                                map[string]bool
	choices                                                                map[string]string
}

func (d *viewState) view(c *ui.Context) {
	if d.live != nil {
		d.live.bindWake(c)
		d.live.poll(d)
	}
	dark := c.Theme().Dark
	if d.theme != "system" {
		dark = d.theme == "dark"
	}
	k := desktopkit.For(dark)
	k.Apply(c)
	k.UiRoot(c, func() {
		w, _ := c.Size()
		// main's library collapses its sidebar below 1100 px. The Kit separately
		// switches list/detail navigation below SinglePaneWidth.
		compact := w < 1100
		if compact != d.compact {
			d.sidebarHidden, d.compact = compact, compact
		}
		c.OnWindowShortcut(ui.Cmd, ui.KeyQ, mygo.App.Quit)
		if !d.tagsOpen && !d.tagEditor.open && d.deviceTask == "" && d.confirm == "" {
			if d.page == "history" {
				c.OnShortcut(ui.Cmd, ui.KeyComma, func() { d.page = "settings" })
				c.OnShortcut(ui.Cmd, ui.KeyP, d.togglePin)
				c.OnShortcut(0, ui.KeyT, d.openTagEditor)
			}
			if d.page == "settings" && (!d.compact || d.sidebarHidden) {
				// A narrow sidebar drawer owns Escape before page navigation.
				c.OnShortcut(0, ui.KeyEscape, func() { d.page = "history" })
			}
			if d.page == "devices" && !d.renaming {
				c.OnShortcut(0, ui.KeyEscape, func() { d.page = "history" })
			}
		}
		switch d.page {
		case "history":
			d.history(c, k)
		case "devices":
			d.devicesPage(c, k)
		case "settings":
			d.settingsPage(c, k)
		case "quick-panel":
			d.quickPanel(c, k)
		default:
			d.page = "history"
			d.history(c, k)
		}
		d.deviceTaskSheet(c.Key("devices"), k)
		d.tagSheet(c.Key("tags"), k)
		d.confirmDialog(c.Key("confirm"), k)
	})
}

func (d *viewState) sourceName(id string) string {
	if d.live != nil {
		for _, v := range d.live.catalogue["source"] {
			if v.Value == id {
				return v.Label
			}
		}
	}
	if id == "migration" {
		return d.tr("从旧版导入")
	}
	if id == "external" {
		return d.tr("外部来源")
	}
	for _, v := range d.devices {
		if v.id == id {
			if id == "local" {
				return d.tr("本机")
			}
			return v.name
		}
	}
	return d.tr("已移除设备")
}

func kindName(kind desktopkit.ContentKind) string {
	return []string{"文本", "代码", "链接", "图片", "文件", "文件夹"}[kind]
}

func (d *viewState) filtered() []entry {
	if d.live != nil {
		if d.unsupportedSearch() {
			return nil
		}
		return d.entries
	}
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

func (d *viewState) current() *entry {
	for i := range d.entries {
		if d.entries[i].id == d.selected {
			return &d.entries[i]
		}
	}
	if d.live != nil && d.live.retained != nil && d.live.retained.id == d.selected {
		return d.live.retained
	}
	return nil
}

func (d *viewState) selectEntry(id string) {
	if d.selected != id {
		d.tagEditor = tagEditorState{}
	}
	d.selected = id
	d.list.Select(id)
	d.notice = ""
}

func (d *viewState) togglePin() {
	if d.live != nil {
		d.live.message = "置顶功能尚不可用。"
		return
	}
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

func (d *viewState) requestDelete(ids []string) {
	if len(ids) == 0 {
		return
	}
	d.confirm, d.confirmID = fmt.Sprintf(d.tr("删除 %d 条历史？"), len(ids)), ""
	d.confirmEntries = slices.Clone(ids)
}

func (d *viewState) confirmDialog(c *ui.Context, k desktopkit.Kit) {
	if d.confirm == "" {
		return
	}
	open := true
	desc := "此操作只修改本次演示中的假数据。"
	if d.live != nil {
		desc = "删除历史记录及应用管理的内容。此操作无法撤销。"
	}
	if strings.HasPrefix(d.confirmID, "device:") {
		desc = "撤销此设备的全部同步路径。对方需要重新配对并重新批准。已同步到对方的内容无法撤回。"
	}
	r := k.AlertDialog(c, desktopkit.AlertDialogProps{Open: &open, Title: d.confirm, Description: d.tr(desc), ConfirmLabel: d.tr("移除"), CancelLabel: d.tr("取消"), Destructive: true}, nil)
	if r.Confirmed && d.live != nil {
		if strings.HasPrefix(d.confirmID, "device:") {
			id := strings.TrimPrefix(d.confirmID, "device:")
			client := d.live.client
			d.mutate(func(ctx context.Context) error { return client.Unpair(ctx, id) }, "设备已移除")
		} else {
			d.deleteLiveEntries(d.confirmEntries)
		}
	} else if r.Confirmed {
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

func (d *viewState) quickPanel(c *ui.Context, _ desktopkit.Kit) {
	if d.quick == nil {
		return
	}
	d.quick.panel(c)
}
