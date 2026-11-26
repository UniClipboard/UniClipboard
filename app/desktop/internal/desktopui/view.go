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

	"github.com/uniclipboard/unic/app/desktop/boardui"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
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
	backendSync                         []api.HistoryBackendSync
	createdAt                           int64
	id, title, text, source, date, size string
	kind                                desktopkit.ContentKind
	tags                                []string
	pinned                              bool
	folder                              *folderInfo
	linkPreview                         *linkPreviewInfo
}

// linkPreviewInfo holds the resolved link_preview.v1 annotation and decoded assets.
type linkPreviewInfo struct {
	siteName    string
	title       string
	description string
	canonical   string
	favicon     ui.ImageSource
	thumbnail   *historyImage
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

type syncChannel struct {
	id               string
	implementationID string
	transportKind    string
	state            string // running, restarting, unavailable, suspended, disabled
	pairing          bool
	externalChannels bool
	restart          bool
	policy           string
	noText           bool
	noImages         bool
	noFiles          bool
}

type deviceBinding struct {
	id               string
	instanceID       string
	implementationID string
	state            string // authorized, pending
	syncEnabled      bool
	policy           engine.BindingSyncPolicy
	connection       string // connected, connecting, disconnected
	connectionPath   string // direct, derp, peer_relay, unknown, ""
	latencyMs        int64
	lastChecked      string
	revision         int64
	lastActive       string
	lastError        string
}

// device holds unified live and preview display state backed by Engine device/binding facts.
type device struct {
	id           string
	name         string
	userName     string
	reportedName string
	deviceID     string
	icon         desktopkit.IconName
	online       bool
	lastSeen     string
	fingerprint  string
	revision     int64
	bindings     []deviceBinding
}

func (v device) displayName() string {
	if s := strings.TrimSpace(v.name); s != "" {
		return s
	}
	if s := strings.TrimSpace(v.userName); s != "" {
		return s
	}
	if s := strings.TrimSpace(v.reportedName); s != "" {
		return s
	}
	return "未命名设备"
}

type externalChannel struct {
	id               string
	name             string
	backendID        string
	enabled          bool
	autoApply        bool
	remoteRevocation bool
	revision         int64
	lastSeen         string
	lastError        string
}

type pendingPath struct {
	id         string
	instanceID string
	state      string
	revision   int64
}

type pendingDevice struct {
	id          string
	name        string
	via         string
	byCode      bool
	fingerprint string
	revision    int64
	icon        desktopkit.IconName
	paths       []pendingPath
}

type deviceCandidate struct {
	id, name, introducer string
	backendID            string
	icon                 desktopkit.IconName
}

type viewState struct {
	colors          *boardui.ColorTheme
	toolbarTrailing func(*ui.Context)
	windowFrame     *bool
	pluginSettings  builtinPluginSettings
	about           aboutState
	storage         storageSettingsState
	diagnostics     diagnosticsState
	backup          backupSettingsState
	shortcuts       shortcutSettingsState
	notifications   notificationSettingsState

	reference                                                             string
	previewImage                                                          ui.ImageSource
	language                                                              string
	quickShortcutStatus                                                   func() (string, string)
	retryQuickShortcut                                                    func()
	live                                                                  *liveState
	tagEditor                                                             tagEditorState
	actionResult                                                          actionResultState
	toasts                                                                desktopkit.ToastStore
	dismissedRestart, dismissedCfgErr, dismissedApply, dismissedPending   string
	pendingCountChanged                                                   func(int)
	lastPendingCount                                                      int
	openURL                                                               func(string) error
	openQuickPanel                                                        func()
	announcement                                                          announcementState
	quick                                                                 *quickPanelState
	tags                                                                  []historyTag
	tagManager                                                            tagManagerState
	search                                                                historySearchState
	page, theme, query, kind, source, tag, timeRange, layout              string
	nav, settings                                                         desktopkit.NavState
	list                                                                  desktopkit.ListState
	deviceList                                                            desktopkit.SourceState
	entries                                                               []entry
	syncChannels                                                          []syncChannel
	devices                                                               []device
	expandedBindings                                                      map[string]bool
	refreshingDevices                                                     bool
	refreshingDeviceID                                                    string
	channels                                                              []externalChannel
	selected                                                              string
	confirmEntries                                                        []string
	drawerSwitchDir                                                       int
	sidebarHidden, detailOpen, inspector, tagsOpen, quickLook, filterOpen bool
	narrowSearchOpen, wasNarrow                                           bool
	zoomStep                                                              int
	shortcutReturnPage                                                    string
	shortcutEditor                                                        shortcutEditor
	keyboardSequence                                                      shortcutSequence
	historySendMenu, historyActionsMenu                                   bool
	compact                                                               bool
	gridSize                                                              float64
	deviceTask, deviceTaskForID, pairingChannelID                         string
	fieldError, notice, confirm, confirmDesc, confirmLabel, confirmID     string
	deviceName                                                            string
	pending                                                               []pendingDevice
	candidates                                                            []deviceCandidate
	inviteCode, inviteMode, connectInput, renameValue, deviceNote         string
	pairingQR                                                             pairingQRCache
	renaming, deviceDetailOpen, restartPending                            bool
	options                                                               map[string]bool
	choices                                                               map[string]string
}

// SetPendingCountChanged registers a host callback notified whenever the number
// of pending device requests changes (used to update OS Dock/app badges).
func (s *Desktop) SetPendingCountChanged(fn func(int)) {
	s.d.pendingCountChanged = fn
	s.d.lastPendingCount = -1
}

func (d *viewState) view(c *ui.Context) {
	if d.live != nil {
		d.live.bindWake(c)
		d.live.poll(d)
	}
	if d.pendingCountChanged != nil && len(d.pending) != d.lastPendingCount {
		d.lastPendingCount = len(d.pending)
		d.pendingCountChanged(d.lastPendingCount)
	}
	dark := c.Theme().Dark
	if d.theme != "system" {
		dark = d.theme == "dark"
	}
	k := desktopkit.Kit{Dark: dark, Colors: d.colors}
	k.Apply(c)
	scale := zoomScale(d.zoomStep)
	k.ScaledUiRoot(c, scale, func() {
		w, _ := c.Size()
		// Collapse the sidebar into an overlay below SidebarOverlayWidth (900 DIP)
		// so three-column browsing and inline toolbar search remain comfortable.
		// The Kit separately switches list/detail navigation below SinglePaneWidth.
		compact := w/scale < desktopkit.SidebarOverlayWidth
		if compact != d.compact {
			d.sidebarHidden, d.compact = compact, compact
		}
		c.OnWindowShortcut(ui.Cmd, ui.KeyQ, mygo.App.Quit)
		if !d.quickLook && !d.tagsOpen && !d.tagEditor.open && !d.actionResult.open && !d.announcement.detailOpen && d.deviceTask == "" && d.confirm == "" && !d.about.alphaWarningOpen && !d.about.manualUpdateOpen && !d.backup.confirmOpen {
			if d.shortcutEditor.id == "" || d.page != "settings" {
				d.bindKeyboardCommands(c)
			}
			if d.page == "settings" && d.shortcutEditor.id == "" && (!d.compact || d.sidebarHidden) {
				// A narrow sidebar drawer owns Escape before page navigation.
				c.OnShortcut(0, ui.KeyEscape, func() { d.page = "history" })
			}
			if d.page == "devices" {
				singlePane := w < desktopkit.SinglePaneWidth
				c.OnWindowShortcut(0, ui.KeyEscape, func() {
					if c.ShortcutRepeated() {
						return
					}
					d.devicesBack(singlePane)
				})
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
		d.actionResultDialog(c.Key("action-result"), k)
		d.confirmDialog(c.Key("confirm"), k)
		d.aboutDialogs(c.Key("about"), k)
		d.announcementDetailSheet(c.Key("announcement-detail"), k)
		d.toasterOverlay(c, k)
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
	desc := d.confirmDesc
	if desc == "" {
		desc = d.tr("此操作只修改本次演示中的假数据。")
		if d.live != nil {
			desc = d.tr("删除历史记录及应用管理的内容。此操作无法撤销。")
		}
		if strings.HasPrefix(d.confirmID, "device:") {
			desc = d.tr("将撤销此设备的信任并移除全部同步路径。对方需要重新配对并批准。已同步到对方的内容无法撤回。")
		}
	}
	confirmLabel := d.confirmLabel
	if confirmLabel == "" {
		confirmLabel = d.tr("移除")
	}
	r := k.AlertDialog(c, desktopkit.AlertDialogProps{Open: &open, Title: d.confirm, Description: desc, ConfirmLabel: confirmLabel, CancelLabel: d.tr("取消"), Destructive: true}, nil)
	if r.Confirmed && d.live != nil {
		switch {
		case strings.HasPrefix(d.confirmID, "device:"):
			id := strings.TrimPrefix(d.confirmID, "device:")
			client := d.live.client
			rev := int64(0)
			if peer, ok := d.live.peers[id]; ok {
				rev = peer.Revision
			} else if dev := d.deviceByID(id); dev != nil {
				rev = dev.revision
			}
			d.mutate(func(ctx context.Context) error {
				if rev > 0 {
					return client.UnpairAt(ctx, id, rev)
				}
				return client.Unpair(ctx, id)
			}, "设备已移除")
		case strings.HasPrefix(d.confirmID, "binding:"):
			rest := strings.TrimPrefix(d.confirmID, "binding:")
			devID, bindID, _ := strings.Cut(rest, ":")
			var rev int64
			if dev := d.deviceByID(devID); dev != nil {
				for _, b := range dev.bindings {
					if b.id == bindID {
						rev = b.revision
						break
					}
				}
			}
			client := d.live.client
			d.mutate(func(ctx context.Context) error { return client.RemoveBinding(ctx, bindID, rev) }, "同步路径已移除")
		case strings.HasPrefix(d.confirmID, "channel:"):
			id := strings.TrimPrefix(d.confirmID, "channel:")
			var rev int64
			if ch := d.channelByID(id); ch != nil {
				rev = ch.revision
			}
			client := d.live.client
			d.mutate(func(ctx context.Context) error { return client.RemoveChannel(ctx, id, rev) }, "外部渠道已移除")
		default:
			d.deleteLiveEntries(d.confirmEntries)
		}
	} else if r.Confirmed {
		switch {
		case strings.HasPrefix(d.confirmID, "device:"):
			id := strings.TrimPrefix(d.confirmID, "device:")
			d.devices = slices.DeleteFunc(d.devices, func(v device) bool { return v.id == id })
			d.pending = slices.DeleteFunc(d.pending, func(p pendingDevice) bool { return p.id == id })
			d.deviceList.Value, d.deviceDetailOpen = "local", false
			d.ensureDeviceSelection()
		case strings.HasPrefix(d.confirmID, "binding:"):
			rest := strings.TrimPrefix(d.confirmID, "binding:")
			devID, bindID, _ := strings.Cut(rest, ":")
			if dev := d.deviceByID(devID); dev != nil {
				dev.bindings = slices.DeleteFunc(dev.bindings, func(b deviceBinding) bool { return b.id == bindID })
			}
		case strings.HasPrefix(d.confirmID, "channel:"):
			id := strings.TrimPrefix(d.confirmID, "channel:")
			d.channels = slices.DeleteFunc(d.channels, func(ch externalChannel) bool { return ch.id == id })
			d.deviceDetailOpen = false
			d.ensureDeviceSelection()
		default:
			d.entries = slices.DeleteFunc(d.entries, func(v entry) bool { return slices.Contains(d.confirmEntries, v.id) })
			d.selected = ""
		}
	}
	if !open {
		d.confirm, d.confirmDesc, d.confirmLabel, d.confirmID = "", "", "", ""
		d.confirmEntries = nil
	}
}

func (d *viewState) quickPanel(c *ui.Context, _ desktopkit.Kit) {
	if d.quick == nil {
		return
	}
	d.quick.colors = d.colors
	d.quick.panel(c)
}
