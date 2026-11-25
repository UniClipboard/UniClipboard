package desktopui

import (
	"context"
	"fmt"
	"math/rand/v2"
	"regexp"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

var (
	pairCodePattern   = regexp.MustCompile(`^\d{3}-?\d{3}$`)
	pairTicketPattern = regexp.MustCompile(`^unic-pair:\S+$`)
)

// connectKind names the input kind, or "" when the input is not recognised.
func connectKind(s string) string {
	s = strings.TrimSpace(s)
	switch {
	case pairCodePattern.MatchString(s):
		return "配对码"
	case pairTicketPattern.MatchString(s):
		return "完整票据"
	}
	return ""
}

func newPairCode() string { return fmt.Sprintf("%03d-%03d", 100+rand.IntN(900), 100+rand.IntN(900)) }

// ---- Unified Surface Palette Helpers (BoardUI Tokens & desktopkit Surfaces) ----

func deviceToolbarBg(k desktopkit.Kit) ui.Color {
	return k.WorkspaceSurface()
}

func deviceCanvasBg(k desktopkit.Kit) ui.Color {
	return k.Color("background-secondary-default")
}

func deviceInspectorBg(k desktopkit.Kit) ui.Color {
	return k.WorkspaceSurface()
}

func deviceCardBg(k desktopkit.Kit) ui.Color {
	return k.CardSurface()
}

func deviceInspectorCardBg(k desktopkit.Kit) ui.Color {
	return k.SubtleCardSurface()
}

func deviceCardBorder(k desktopkit.Kit) ui.Color {
	if k.Dark {
		return k.Color("border-button-default")
	}
	return k.Color("separator-border")
}

func deviceCardBorderHover(k desktopkit.Kit) ui.Color {
	return k.Color("border-button-hover")
}

func deviceInsetBg(k desktopkit.Kit, inInspector bool) ui.Color {
	if k.Dark {
		return k.WorkspaceSurface()
	}
	if inInspector {
		return k.Color("background-primary-default")
	}
	return k.Color("background-secondary-default")
}

func deviceAccentSurface(k desktopkit.Kit) (bg, edge ui.Color) {
	if k.Dark {
		return k.Color("accent-500").Alpha(0.12), k.Color("accent-400").Alpha(0.32)
	}
	return k.Color("accent-50"), k.Color("accent-200")
}

func deviceWarnBannerSurface(k desktopkit.Kit) (bg, edge, fg ui.Color) {
	fg = k.Color("status-yellow-text")
	if k.Dark {
		return fg.Alpha(0.11), fg.Alpha(0.28), fg
	}
	bg = k.Color("background-primary-default").MixOklab(k.Color("status-yellow-background"), 0.24)
	edge = k.Color("background-primary-default").MixOklab(fg, 0.28)
	return bg, edge, fg
}

// ---- Unified Derived Data Helpers (Live + Demo) ----

func (d *viewState) channelDisplayName(id string) string {
	if id == "builtin.tailcat" {
		return d.tr("Tailcat 同步")
	}
	return id
}

func (d *viewState) channelStateLabel(state string) string {
	switch state {
	case "running":
		return d.tr("运行中")
	case "restarting":
		return d.tr("正在重启")
	case "unavailable":
		return d.tr("不可用")
	case "suspended":
		return d.tr("已暂停")
	case "disabled":
		return d.tr("已停用")
	default:
		return state
	}
}

func (d *viewState) channelSelectLabel(ch syncChannel) string {
	name := d.channelDisplayName(ch.id)
	if ch.implementationID != "" && ch.implementationID != ch.id {
		return fmt.Sprintf("%s (%s)", name, ch.implementationID)
	}
	return name
}

func (d *viewState) bindingErrorLabel(code string) string {
	switch code {
	case "unreachable":
		return d.tr("对端不可达，正在等待重连")
	case "auth_expired":
		return d.tr("授权已失效，请重新配对")
	case "backend_stopped":
		return d.tr("对应同步渠道未运行")
	case "timeout":
		return d.tr("连接超时")
	case "remove_failed":
		return d.tr("退出远端群组失败，渠道仍保留在本地，可稍后重试移除。")
	default:
		return code
	}
}

func authorizedBindings(v device) []deviceBinding {
	var out []deviceBinding
	for _, b := range v.bindings {
		if b.state == "authorized" {
			out = append(out, b)
		}
	}
	return out
}

func pendingBindings(v device) []deviceBinding {
	var out []deviceBinding
	for _, b := range v.bindings {
		if b.state == "pending" {
			out = append(out, b)
		}
	}
	return out
}

func (d *viewState) bindingPathLabel(path string) string {
	switch path {
	case "direct":
		return d.tr("直连")
	case "derp":
		return d.tr("中继")
	case "peer_relay":
		return d.tr("节点中继")
	default:
		return ""
	}
}

func (d *viewState) deviceAggregateStatus(v device) (text string, tone desktopkit.StatusTone, online bool) {
	if v.id == "local" {
		return d.tr("本机"), desktopkit.StatusPositive, true
	}
	auth := authorizedBindings(v)
	if len(auth) == 0 {
		return d.tr("无路径"), desktopkit.StatusNeutral, false
	}
	enabled := 0
	connected := 0
	connecting := 0
	knownPath := ""
	for _, b := range auth {
		if b.syncEnabled {
			enabled++
			switch b.connection {
			case "connected":
				connected++
				if knownPath == "" {
					knownPath = d.bindingPathLabel(b.connectionPath)
				}
			case "connecting":
				connecting++
			}
		}
	}
	if enabled == 0 {
		return d.tr("已暂停"), desktopkit.StatusNeutral, false
	}
	if connected > 0 {
		if len(auth) > 1 && connected < len(auth) {
			return fmt.Sprintf(d.tr("在线 · %d/%d"), connected, len(auth)), desktopkit.StatusPositive, true
		}
		if knownPath != "" {
			return fmt.Sprintf(d.tr("在线 · %s"), knownPath), desktopkit.StatusPositive, true
		}
		return d.tr("在线"), desktopkit.StatusPositive, true
	}
	if connecting > 0 {
		return d.tr("重连中"), desktopkit.StatusWarning, false
	}
	return d.tr("离线"), desktopkit.StatusNeutral, false
}

func (d *viewState) deviceSubtitle(v device) string {
	auth := authorizedBindings(v)
	pend := pendingBindings(v)
	if len(auth) == 0 && len(pend) == 0 {
		return d.tr("已信任 · 无同步路径")
	}
	if len(pend) > 0 {
		return fmt.Sprintf(d.tr("%d 条路径 · %d 条待批准"), len(auth)+len(pend), len(pend))
	}
	if len(auth) == 1 {
		return d.channelDisplayName(auth[0].instanceID)
	}
	return fmt.Sprintf(d.tr("%d 条同步路径"), len(auth))
}

func isDefaultBindingPolicy(p engine.BindingSyncPolicy) bool {
	return p.Normalized() == engine.DefaultBindingSyncPolicy()
}

func (d *viewState) bindingPolicyShortTag(b deviceBinding) string {
	p := b.policy.Normalized()
	if isDefaultBindingPolicy(p) {
		return ""
	}
	if p.Text == p.Images && p.Images == p.Files {
		switch p.Text {
		case engine.SyncDirectionSendOnly:
			return d.tr("仅发送")
		case engine.SyncDirectionReceiveOnly:
			return d.tr("仅接收")
		case engine.SyncDirectionDisabled:
			return d.tr("不同步")
		}
	}
	return d.tr("自定义")
}

func (d *viewState) bindingDirectionShortLabel(dir engine.SyncDirection) string {
	switch dir {
	case engine.SyncDirectionSendOnly:
		return d.tr("仅发")
	case engine.SyncDirectionReceiveOnly:
		return d.tr("仅收")
	case engine.SyncDirectionDisabled:
		return d.tr("关闭")
	default:
		return d.tr("双向")
	}
}

func (d *viewState) bindingPolicySummary(b deviceBinding) string {
	if !b.syncEnabled {
		return d.tr("已暂停")
	}
	p := b.policy.Normalized()
	if isDefaultBindingPolicy(p) {
		return d.tr("全部双向")
	}
	if p.Text == p.Images && p.Images == p.Files {
		switch p.Text {
		case engine.SyncDirectionSendOnly:
			return d.tr("仅发送")
		case engine.SyncDirectionReceiveOnly:
			return d.tr("仅接收")
		case engine.SyncDirectionDisabled:
			return d.tr("不同步")
		}
	}
	return fmt.Sprintf("%s %s · %s %s · %s %s",
		d.tr("文本"), d.bindingDirectionShortLabel(p.Text),
		d.tr("图片"), d.bindingDirectionShortLabel(p.Images),
		d.tr("文件"), d.bindingDirectionShortLabel(p.Files),
	)
}

func (d *viewState) bindingExpanded(b deviceBinding, total int) bool {
	if d.expandedBindings != nil {
		if open, ok := d.expandedBindings[b.id]; ok {
			return open
		}
	}
	return total == 1
}

func (d *viewState) toggleBindingExpanded(b deviceBinding, total int) {
	if d.expandedBindings == nil {
		d.expandedBindings = map[string]bool{}
	}
	d.expandedBindings[b.id] = !d.bindingExpanded(b, total)
}

func (d *viewState) globalFilesEnabled() bool {
	if d.live != nil && d.live.loaded {
		return d.live.config.Config.Sync.Files
	}
	if on, ok := d.options["syncFiles"]; ok {
		return on
	}
	return true
}

func (d *viewState) bindingTypeLockedReason(ch *syncChannel, kind string) (string, bool) {
	if !d.globalSyncEnabled() {
		return d.tr("全局同步已关闭"), true
	}
	if kind == "files" && !d.globalFilesEnabled() {
		return d.tr("全局文件已关闭"), true
	}
	if ch != nil {
		switch kind {
		case "text":
			if ch.noText {
				return d.tr("渠道已关闭"), true
			}
		case "images":
			if ch.noImages {
				return d.tr("渠道已关闭"), true
			}
		case "files":
			if ch.noFiles {
				return d.tr("渠道已关闭"), true
			}
		}
	}
	return "", false
}

func (d *viewState) applyBindingPolicyUpdate(v *device, b deviceBinding, enabled *bool, next engine.BindingSyncPolicy) {
	next = next.Normalized()
	if d.live != nil {
		d.setLiveBindingPolicy(b.id, b.revision, enabled, next)
		return
	}
	for j := range v.bindings {
		if v.bindings[j].id == b.id {
			v.bindings[j].policy = next
			if enabled != nil {
				v.bindings[j].syncEnabled = *enabled
			}
			break
		}
	}
}

func (d *viewState) deviceFootPaths(v device) string {
	auth := authorizedBindings(v)
	if len(auth) == 0 {
		return d.tr("点击右侧添加同步路径")
	}
	parts := make([]string, 0, len(auth))
	for _, b := range auth {
		name := d.channelDisplayName(b.instanceID)
		if !b.syncEnabled {
			name = fmt.Sprintf(d.tr("%s(暂停)"), name)
		} else if tag := d.bindingPolicyShortTag(b); tag != "" {
			name = fmt.Sprintf("%s(%s)", name, tag)
		}
		parts = append(parts, name)
	}
	return strings.Join(parts, " · ")
}

func (d *viewState) runningSyncChannels() []syncChannel {
	var out []syncChannel
	for _, ch := range d.syncChannels {
		if ch.state == "running" {
			out = append(out, ch)
		}
	}
	return out
}

func (d *viewState) runningPairingChannels() []syncChannel {
	var out []syncChannel
	for _, ch := range d.syncChannels {
		if ch.pairing && ch.state == "running" {
			out = append(out, ch)
		}
	}
	return out
}

func (d *viewState) syncChannelByID(id string) *syncChannel {
	for i := range d.syncChannels {
		if d.syncChannels[i].id == id {
			return &d.syncChannels[i]
		}
	}
	return nil
}

func (d *viewState) unboundRunningPairingChannels(v device) []syncChannel {
	bound := map[string]bool{}
	for _, b := range v.bindings {
		bound[b.instanceID] = true
	}
	var out []syncChannel
	for _, ch := range d.runningPairingChannels() {
		if !bound[ch.id] {
			out = append(out, ch)
		}
	}
	return out
}

func (d *viewState) globalSyncEnabled() bool {
	if d.live != nil && d.live.loaded {
		return d.live.config.Config.Sync.Enabled
	}
	if on, ok := d.options["sync"]; ok {
		return on
	}
	return true
}

func (d *viewState) activePairingChannel() string {
	if d.live != nil {
		return d.live.pairingBackend
	}
	avail := d.runningPairingChannels()
	for _, ch := range avail {
		if ch.id == d.pairingChannelID {
			return ch.id
		}
	}
	if len(avail) > 0 {
		d.pairingChannelID = avail[0].id
		return d.pairingChannelID
	}
	return ""
}

func (d *viewState) setActivePairingChannel(id string, invitation bool) {
	if d.live != nil {
		if invitation {
			d.cancelLiveInvitation()
		}
		d.live.pairingBackend = id
		d.live.codeAttempted, d.live.ticketAttempted = false, false
		return
	}
	d.pairingChannelID = id
	if invitation {
		d.inviteCode = newPairCode()
		d.deviceNote = ""
	}
}

func (d *viewState) deviceByID(id string) *device {
	for i := range d.devices {
		if d.devices[i].id == id {
			return &d.devices[i]
		}
	}
	return nil
}

func (d *viewState) channelByID(id string) *externalChannel {
	for i := range d.channels {
		if d.channels[i].id == id {
			return &d.channels[i]
		}
	}
	return nil
}

func (d *viewState) isExistingPendingDevice(id string) bool {
	return id != "" && id != "local" && d.deviceByID(id) != nil
}

func (d *viewState) pendingForDevice(id string) *pendingDevice {
	for i := range d.pending {
		if d.pending[i].id == id {
			return &d.pending[i]
		}
	}
	return nil
}

func (d *viewState) pendingPathNames(p pendingDevice) string {
	if len(p.paths) == 0 {
		return ""
	}
	names := make([]string, 0, len(p.paths))
	for _, path := range p.paths {
		names = append(names, d.channelDisplayName(path.instanceID))
	}
	return strings.Join(names, "、")
}

func (d *viewState) openPairingSheet(forDeviceID, preferredChannel string) {
	if d.live != nil {
		if d.deviceTask == "" {
			known := make(map[string]int64, len(d.live.pending))
			for id, p := range d.live.pending {
				known[id] = p.Revision
			}
			d.live.taskKnownPending = known
		}
		avail := d.runningPairingChannels()
		if preferredChannel != "" && slices.ContainsFunc(avail, func(ch syncChannel) bool { return ch.id == preferredChannel }) {
			d.live.pairingBackend = preferredChannel
		} else if !slices.ContainsFunc(avail, func(ch syncChannel) bool { return ch.id == d.live.pairingBackend }) {
			if len(avail) > 0 {
				d.live.pairingBackend = avail[0].id
			} else {
				d.live.pairingBackend = ""
			}
		}
	} else {
		avail := d.runningPairingChannels()
		if preferredChannel != "" && slices.ContainsFunc(avail, func(ch syncChannel) bool { return ch.id == preferredChannel }) {
			d.pairingChannelID = preferredChannel
		} else if !slices.ContainsFunc(avail, func(ch syncChannel) bool { return ch.id == d.pairingChannelID }) {
			if len(avail) > 0 {
				d.pairingChannelID = avail[0].id
			} else {
				d.pairingChannelID = ""
			}
		}
	}
	d.deviceTaskForID = forDeviceID
	d.setDeviceTask("invite")
}

func (d *viewState) setDeviceTask(task string) {
	if d.live != nil {
		if d.deviceTask == "invite" && task != "invite" {
			d.cancelLiveInvitation()
		}
		if task == "" {
			d.deviceTaskForID = ""
			d.live.taskKnownPending = nil
			d.live.join = nil
		}
		d.deviceTask, d.fieldError, d.deviceNote = task, "", ""
		if task == "invite" {
			d.inviteMode = "code"
			d.live.codeAttempted, d.live.ticketAttempted = false, false
		}
		if task == "connect" {
			d.connectInput = ""
		}
		return
	}
	if task == "" {
		d.deviceTaskForID = ""
	}
	d.deviceTask, d.fieldError, d.deviceNote = task, "", ""
	switch task {
	case "invite":
		d.inviteCode, d.inviteMode = newPairCode(), "code"
	case "connect":
		d.connectInput = ""
	}
}

// matchingInlinePending returns a pending request that belongs to the active pairing
// sheet session, ignoring unrelated pre-existing pending requests captured before opening.
func (d *viewState) matchingInlinePending() *pendingDevice {
	if d.live == nil || (d.deviceTask != "invite" && d.deviceTask != "wait") {
		return nil
	}
	l := d.live
	if l.join != nil {
		for i := range d.pending {
			if d.pending[i].id == l.join.ID {
				return &d.pending[i]
			}
		}
		return nil
	}
	for i := range d.pending {
		p := &d.pending[i]
		if knownRev, existed := l.taskKnownPending[p.id]; existed && knownRev == p.revision {
			continue
		}
		if d.deviceTaskForID != "" && p.id != d.deviceTaskForID {
			continue
		}
		if p.byCode || l.pairingBackend == "" || len(p.paths) == 0 {
			return p
		}
		for _, path := range p.paths {
			if path.instanceID == l.pairingBackend {
				return p
			}
		}
	}
	return nil
}

func (d *viewState) ensureDeviceSelection() {
	id := d.deviceList.Value
	switch {
	case id == "local":
		if d.deviceByID("local") != nil {
			return
		}
	case strings.HasPrefix(id, "pending:"):
		pid := strings.TrimPrefix(id, "pending:")
		if slices.ContainsFunc(d.pending, func(p pendingDevice) bool { return p.id == pid && !d.isExistingPendingDevice(p.id) }) {
			return
		}
	case strings.HasPrefix(id, "ch:"):
		cid := strings.TrimPrefix(id, "ch:")
		if d.channelByID(cid) != nil {
			return
		}
	case strings.HasPrefix(id, "cand:"):
		cid := strings.TrimPrefix(id, "cand:")
		if slices.ContainsFunc(d.candidates, func(cd deviceCandidate) bool { return cd.id == cid }) {
			return
		}
	default:
		if d.deviceByID(id) != nil {
			return
		}
	}
	for _, p := range d.pending {
		if !d.isExistingPendingDevice(p.id) {
			d.deviceList.Value = "pending:" + p.id
			return
		}
	}
	for _, v := range d.devices {
		if v.id != "local" {
			d.deviceList.Value = v.id
			return
		}
	}
	if len(d.devices) > 0 {
		d.deviceList.Value = d.devices[0].id
	}
}

func (d *viewState) statusPillColors(k desktopkit.Kit, tone desktopkit.StatusTone) (bg, fg, dot ui.Color) {
	switch tone {
	case desktopkit.StatusPositive:
		fg = k.Color("state-success-text")
		if k.Dark {
			fg = k.Color("notification-success-foreground")
		}
		return k.Color("notification-success-background"), fg, k.Color("notification-success-foreground")
	case desktopkit.StatusWarning:
		bg = k.Color("status-yellow-background")
		if !k.Dark {
			bg = k.Color("background-primary-default").MixOklab(bg, 0.55)
		}
		return bg, k.Color("status-yellow-text"), k.Color("status-yellow-text")
	case desktopkit.StatusInfo:
		if k.Dark {
			return k.Color("accent-500").Alpha(0.20), k.Color("accent-300"), k.Color("accent-500")
		}
		return k.Color("badge-new-background"), k.Color("badge-new-text"), k.Color("accent-500")
	case desktopkit.StatusCritical:
		return k.Color("notification-error-background"), k.Color("notification-error-foreground"), k.Color("notification-error-foreground")
	default:
		return k.Color("background-tertiary-default"), k.Color("text-secondary"), k.Color("text-tertiary")
	}
}

func (d *viewState) statusPill(c *ui.Context, k desktopkit.Kit, text string, tone desktopkit.StatusTone) ui.Element {
	bg, fg, dot := d.statusPillColors(k, tone)
	return ui.Row(c).Height(22).Padding(0, 8).Radius(11).Gap(5).AlignItems(ui.Center).Shrink(0).Background(bg).Children(func() {
		ui.Box(c.Key("dot")).Size(6, 6).Radius(3).Background(dot).Shrink(0)
		k.Text(c.Key("lbl"), text, desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(fg)
	})
}

func (d *viewState) inlineBadge(c *ui.Context, k desktopkit.Kit, text string, tone desktopkit.StatusTone) ui.Element {
	bg, fg, _ := d.statusPillColors(k, tone)
	return ui.Row(c).Height(19).Padding(0, 6).Radius(5).AlignItems(ui.Center).Shrink(0).Background(bg).Children(func() {
		k.Text(c.Key("lbl"), text, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).TextColor(fg)
	})
}

func (d *viewState) deviceIconTile(c *ui.Context, k desktopkit.Kit, icon desktopkit.IconName, size float32, pending, inInspector bool) {
	iconSize := size * 0.5
	tile := ui.Row(c).Size(size, size).Radius(size * 0.26).Justify(ui.Center).AlignItems(ui.Center).Shrink(0)
	if pending {
		bg, _ := deviceAccentSurface(k)
		tile.Background(bg).Border(1, k.Color("accent-400"))
		tile.Children(func() { k.Icon(c, icon, iconSize).TextColor(k.Color("accent-500")) })
		return
	}
	tile.Background(deviceInsetBg(k, inInspector)).Border(1, deviceCardBorder(k))
	tile.Children(func() { k.Icon(c, icon, iconSize).TextColor(k.Color("text-primary")) })
}

func (d *viewState) fingerprintGrid(c *ui.Context, k desktopkit.Kit, fp string, inInspector bool) {
	groups := strings.Fields(fp)
	if len(groups) == 0 {
		return
	}
	cellBg := deviceInsetBg(k, inInspector)
	cellBorder := deviceCardBorder(k)
	ui.Column(c).Gap(6).Children(func() {
		for start := 0; start < len(groups); start += 4 {
			end := min(start+4, len(groups))
			ui.Row(c.Key(start)).Gap(6).Children(func() {
				for i := start; i < end; i++ {
					ui.Row(c.Key(i)).Grow(1).Basis(0).MinWidth(0).Height(30).Radius(7).
						Justify(ui.Center).AlignItems(ui.Center).
						Background(cellBg).Border(1, cellBorder).Children(func() {
						k.Text(c, groups[i], desktopkit.TextProps{Monospace: true, Emphasized: true, Role: desktopkit.RoleCaption})
					})
				}
			})
		}
	})
}

func (d *viewState) devicesBack(singlePane bool) {
	d.keyboardSequence = shortcutSequence{}
	if d.renaming {
		d.renaming = false
		return
	}
	if singlePane && d.deviceDetailOpen {
		d.deviceDetailOpen = false
		return
	}
	d.page, d.renaming, d.deviceDetailOpen = "history", false, false
}

// devicesPage renders the full-window Device Management overview with a
// refined 3-tier surface hierarchy, left overview canvas, and 380-DIP inspector.
func (d *viewState) devicesPage(c *ui.Context, k desktopkit.Kit) {
	d.ensureDeviceSelection()
	w, _ := c.Size()
	caption := c.TitleBar()
	integratedCaption := runtime.GOOS == "darwin" && caption.Left > 0
	headerHeight := float32(52)
	if integratedCaption {
		headerHeight = max(headerHeight, caption.Height)
	}
	singlePane := w < desktopkit.SinglePaneWidth

	onlinePeers := 0
	for _, v := range d.devices {
		if v.id != "local" {
			if _, _, on := d.deviceAggregateStatus(v); on {
				onlinePeers++
			}
		}
	}

	ui.Column(c.Key("devices-page")).Grow(1).MinHeight(0).AlignSelf(ui.Stretch).Background(deviceCanvasBg(k)).Children(func() {
		// Unified 52-DIP top band aligned with macOS traffic lights.
		trailing := d.trailingToolbar(c)
		tm := desktopkit.ToolbarLayout(c, zoomScale(d.zoomStep), 0, trailing != nil)
		padLeft := float32(16)
		padRight := float32(18)
		if trailing != nil {
			padRight = 8
		}
		if integratedCaption {
			padLeft = caption.Left + 10
		}
		barGap := float32(10)
		if tm.Compact {
			barGap = 8
		}
		bar := ui.Row(c.Key("devices-toolbar")).Height(headerHeight).Padding(0, padRight, 0, padLeft).Gap(barGap).
			AlignItems(ui.Center).Shrink(0).Role(ui.RoleToolbar).Label(d.tr("设备管理")).
			Background(deviceToolbarBg(k)).
			InsetShadow(0, -1, 0, 0, k.Color("separator-border")).DragWindow()
		bar.Children(func() {
			if singlePane && d.deviceDetailOpen {
				backLabel := d.tr("设备")
				back := ui.ButtonBase(c.Key("back-overview")).Label(backLabel).Tooltip(backLabel).KeyShortcuts(0, ui.KeyEscape).Height(28).Radius(7).
					AlignItems(ui.Center).Justify(ui.Center).Shrink(0).FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
				if tm.Compact {
					back.Width(28)
				} else {
					back.Padding(0, 8).Gap(4)
				}
				if back.Hovered() {
					back.Background(k.Color("background-secondary-hover"))
				}
				back.Children(func() {
					k.Icon(c, desktopkit.IconBack, 15).TextColor(k.Color("foreground-icon-secondary"))
					if !tm.Compact {
						k.Text(c, backLabel, desktopkit.TextProps{Role: desktopkit.RoleCallout})
					}
				})
				if back.Clicked() {
					d.deviceDetailOpen, d.renaming = false, false
				}
			} else {
				backLabel := d.tr("返回历史")
				back := ui.ButtonBase(c.Key("back-history")).Label(backLabel).Tooltip(backLabel).KeyShortcuts(0, ui.KeyEscape).Height(28).Radius(7).
					AlignItems(ui.Center).Justify(ui.Center).Shrink(0).FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
				if tm.Compact {
					back.Width(28)
				} else {
					back.Padding(0, 8).Gap(4)
				}
				if back.Hovered() {
					back.Background(k.Color("background-secondary-hover"))
				}
				back.Children(func() {
					k.Icon(c, desktopkit.IconBack, 15).TextColor(k.Color("foreground-icon-secondary"))
					if !tm.Compact {
						k.Text(c, backLabel, desktopkit.TextProps{Role: desktopkit.RoleCallout})
					}
				})
				if back.Clicked() {
					d.page, d.renaming, d.deviceDetailOpen = "history", false, false
				}
			}
			ui.Box(c.Key("sep")).Size(1, 16).Background(k.Color("separator-border")).Shrink(0)
			k.Text(c.Key("title"), d.tr("设备管理"), desktopkit.TextProps{Role: desktopkit.RoleHeadline, Emphasized: true, Truncate: true}).MinWidth(0).Shrink(1)
			if tm.AvailWidth >= 520 {
				switch {
				case !d.globalSyncEnabled():
					d.statusPill(c.Key("summary-pill"), k, d.tr("全局同步已关闭"), desktopkit.StatusWarning)
				case len(d.runningSyncChannels()) == 0:
					d.statusPill(c.Key("summary-pill"), k, d.tr("仅本地模式"), desktopkit.StatusNeutral)
				default:
					d.statusPill(c.Key("summary-pill"), k, fmt.Sprintf(d.tr("%d 台在线 · 共 %d 台"), onlinePeers, len(d.devices)), desktopkit.StatusPositive)
				}
			}
			ui.Spacer(c)
			refreshLabel := d.tr("刷新状态")
			refreshBusy := d.refreshingDevices && d.refreshingDeviceID == ""
			if refreshBusy {
				refreshLabel = d.tr("检测中…")
			}
			refreshDisabled := d.refreshingDevices || (d.live != nil && d.live.busy)
			if tm.Narrow {
				if k.ToolbarIconButton(c.Key("refresh-devices"), desktopkit.ToolbarIconButtonProps{
					Icon:     desktopkit.IconRetry,
					Label:    refreshLabel,
					Tooltip:  refreshLabel,
					Busy:     refreshBusy,
					Disabled: refreshDisabled,
				}).Clicked() {
					if d.live != nil {
						d.refreshLiveDevices("")
					}
				}
			} else if k.Button(c.Key("refresh-devices"), refreshLabel, desktopkit.ButtonProps{
				Variant:  desktopkit.Subtle,
				Size:     desktopkit.SizeSmall,
				Icon:     desktopkit.IconRetry,
				Disabled: refreshDisabled,
			}).Clicked() {
				if d.live != nil {
					d.refreshLiveDevices("")
				}
			}
			if k.Button(c.Key("add-device"), d.tr("添加设备"), desktopkit.ButtonProps{
				Variant: desktopkit.Primary,
				Size:    desktopkit.SizeSmall,
				Icon:    desktopkit.IconAdd,
			}).Clicked() {
				d.openPairingSheet("", "")
			}
			if trailing != nil {
				ui.Box(c.Key("host-sep")).Width(1).Height(16).Background(k.Color("separator-border")).Shrink(0).AccessHidden(true)
				trailing()
			}
		})

		// Body: left overview canvas + right 380-DIP inspector.
		ui.Row(c.Key("devices-body")).Grow(1).MinHeight(0).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
			if !singlePane || !d.deviceDetailOpen {
				d.deviceOverviewCanvas(c.Key("overview"), k, w)
			}
			if !singlePane || d.deviceDetailOpen {
				inspector := ui.Column(c.Key("inspector")).MinHeight(0).MinWidth(0).
					Background(deviceInspectorBg(k)).
					InsetShadow(1, 0, 0, 0, k.Color("separator-border")).
					Role(ui.RoleRegion).Label(d.tr("设备详情"))
				if singlePane {
					inspector.Grow(1)
				} else {
					inspector.Width(380).Shrink(0)
				}
				inspector.Children(func() {
					k.Scroll(c.Key("inspector-scroll")).Grow(1).MinHeight(0).Padding(18, 18, 28, 18).Children(func() {
						d.deviceDetail(c, k)
					})
				})
			}
		})
	})
}

func (d *viewState) syncChannelsStrip(c *ui.Context, k desktopkit.Kit) {
	running := d.runningSyncChannels()
	if len(running) == 0 {
		bg, edge, fg := deviceWarnBannerSurface(k)
		ui.Column(c.Key("channel-strip-zero")).Padding(10, 12).Radius(12).Gap(8).Shrink(0).
			Background(bg).Border(1, edge).Children(func() {
			ui.Row(c.Key("top")).Wrap().Gap(8).AlignItems(ui.Center).Children(func() {
				k.Icon(c.Key("ic"), desktopkit.IconInfo, 14).TextColor(fg).Shrink(0)
				k.Text(c.Key("lbl"), d.tr("同步渠道"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).TextColor(fg)
				k.Text(c.Key("desc"), d.tr("当前没有运行中的同步渠道（仅本地历史模式）"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(fg).Grow(1).MinWidth(0)
				if k.Button(c.Key("go-plugins"), d.tr("前往插件设置"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
					d.openPluginSettings("")
				}
			})
			if d.restartPending {
				ui.Box(c.Key("div")).Height(1).Background(edge)
				ui.Row(c.Key("restart")).Gap(8).AlignItems(ui.Center).Children(func() {
					k.Icon(c.Key("ric"), desktopkit.IconRetry, 13).TextColor(fg).Shrink(0)
					k.Text(c.Key("rtxt"), d.tr("同步渠道配置已更改，请退出并重新启动桌面端以生效。"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(fg).Grow(1).MinWidth(0)
					d.restartActionButton(c.Key("rbtn"), k)
				})
			}
		})
		return
	}

	ui.Column(c.Key("channel-strip")).Padding(10, 12).Radius(12).Gap(8).Shrink(0).
		Background(deviceCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
		ui.Row(c.Key("top")).Wrap().Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Row(c.Key("lbl-row")).Gap(5).AlignItems(ui.Center).Shrink(0).Children(func() {
				k.Icon(c.Key("ic"), desktopkit.IconPackage, 13).TextColor(k.Color("foreground-icon-secondary")).Shrink(0)
				k.Text(c.Key("lbl"), d.tr("同步渠道"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
			})
			ui.Row(c.Key("chips")).Wrap().Gap(6).AlignItems(ui.Center).Grow(1).MinWidth(0).Children(func() {
				for _, ch := range d.syncChannels {
					ch := ch
					dotColor := k.Color("text-tertiary")
					switch {
					case ch.state == "unavailable":
						dotColor = k.Color("notification-error-foreground")
					case ch.state == "restarting" || ch.restart:
						dotColor = k.Color("status-yellow-text")
					case ch.state == "running":
						dotColor = k.Color("notification-success-foreground")
					}
					meta := ""
					switch {
					case ch.externalChannels:
						meta = d.tr("第三方")
					case ch.state != "running":
						meta = d.channelStateLabel(ch.state)
					case ch.restart:
						meta = d.tr("需重启")
					}
					name := d.channelDisplayName(ch.id)
					chip := ui.ButtonBase(c.Key("ch-"+ch.id)).Label(name).Tooltip(ch.implementationID).
						Height(24).Padding(0, 9).Radius(12).Gap(6).AlignItems(ui.Center).Shrink(0).
						Background(deviceInsetBg(k, false)).Border(1, deviceCardBorder(k)).
						FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(1)
					if ch.state == "disabled" {
						chip.Opacity(0.70)
					}
					if chip.Hovered() {
						chip.Border(1, deviceCardBorderHover(k)).Background(k.Color("background-secondary-hover"))
					}
					chip.Children(func() {
						ui.Box(c.Key("dot")).Size(6, 6).Radius(3).Background(dotColor).Shrink(0)
						k.Text(c.Key("n"), name, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true})
						if meta != "" {
							k.Text(c.Key("m"), "· "+meta, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
						}
					})
					if chip.Clicked() {
						d.openPluginSettings(ch.id)
					}
				}
			})
			if k.Button(c.Key("manage-channels"), d.tr("管理同步渠道"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall}).Clicked() {
				d.openPluginSettings("")
			}
		})
		if d.restartPending {
			_, _, fg := deviceWarnBannerSurface(k)
			ui.Box(c.Key("div")).Height(1).Background(k.Color("separator-border"))
			ui.Row(c.Key("restart")).Gap(8).AlignItems(ui.Center).Children(func() {
				k.Icon(c.Key("ric"), desktopkit.IconRetry, 13).TextColor(fg).Shrink(0)
				k.Text(c.Key("rtxt"), d.tr("同步渠道设置已保存，请退出并重新启动桌面端以生效。"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(fg).Grow(1).MinWidth(0)
				d.restartActionButton(c.Key("rbtn"), k)
			})
		}
	})
}

func (d *viewState) deviceOverviewCanvas(c *ui.Context, k desktopkit.Kit, winWidth float32) {
	// Order: Local -> Has Pending Path -> Online -> Paused/Offline -> No Paths.
	var ordered []device
	var peers []device
	for _, v := range d.devices {
		if v.id == "local" {
			ordered = append(ordered, v)
		} else {
			peers = append(peers, v)
		}
	}
	deviceScore := func(v device) int {
		if len(pendingBindings(v)) > 0 {
			return 0
		}
		_, tone, online := d.deviceAggregateStatus(v)
		if online {
			return 1
		}
		if tone == desktopkit.StatusWarning {
			return 2
		}
		if len(authorizedBindings(v)) > 0 {
			return 3
		}
		return 4
	}
	slices.SortStableFunc(peers, func(a, b device) int {
		return deviceScore(a) - deviceScore(b)
	})
	ordered = append(ordered, peers...)

	twoCols := winWidth >= 960
	runningChannels := d.runningSyncChannels()
	runningPairing := d.runningPairingChannels()

	k.Scroll(c).Grow(1).MinWidth(0).MinHeight(0).Padding(18, 22, 28, 22).Gap(16).
		Background(deviceCanvasBg(k)).
		Role(ui.RoleRegion).Label(d.tr("设备")).Children(func() {
		// 1. Sync Channels Status Strip
		d.syncChannelsStrip(c.Key("sync-strip"), k)

		// 2. Global Sync Disabled Warning Banner (soft warm tint, never neon pill yellow)
		if !d.globalSyncEnabled() {
			bg, edge, fg := deviceWarnBannerSurface(k)
			ui.Row(c.Key("global-sync-off")).Padding(10, 12).Radius(11).Gap(10).AlignItems(ui.Center).Shrink(0).
				Background(bg).Border(1, edge).Children(func() {
				k.Icon(c.Key("ic"), desktopkit.IconInfo, 16).TextColor(fg).Shrink(0)
				ui.Column(c.Key("body")).Gap(2).Grow(1).MinWidth(0).Children(func() {
					k.Text(c.Key("t"), d.tr("全局同步已关闭"), desktopkit.TextProps{Role: desktopkit.RoleCallout, Emphasized: true}).TextColor(fg)
					k.Text(c.Key("s"), d.tr("所有同步渠道与设备路径当前不传输剪贴板内容，本机历史与配对管理不受影响。"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(fg)
				})
				if k.Button(c.Key("enable-sync"), d.tr("开启同步"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Disabled: d.live != nil && d.live.busy}).Clicked() {
					if d.live != nil {
						d.enableLiveGlobalSync()
					} else {
						d.options["sync"] = true
					}
				}
			})
		}

		// Live operation feedback notice (when sheet is not open).
		if d.live != nil && d.live.message != "" && d.deviceTask == "" {
			bg, edge, fg := deviceWarnBannerSurface(k)
			ui.Row(c.Key("live-msg")).Padding(10, 12).Radius(11).Gap(8).AlignItems(ui.Center).Shrink(0).
				Background(bg).Border(1, edge).Children(func() {
				k.Icon(c.Key("ic"), desktopkit.IconInfo, 15).TextColor(fg).Shrink(0)
				k.Text(c.Key("t"), d.tr(d.live.message), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).TextColor(fg).Grow(1)
			})
		}

		// 3. Pending approval banners
		if len(d.pending) > 0 {
			ui.Column(c.Key("pending-banners")).Gap(8).Shrink(0).Children(func() {
				ui.Row(c.Key("head")).AlignItems(ui.Center).Gap(8).Children(func() {
					k.Text(c, fmt.Sprintf(d.tr("待批准请求 (%d)"), len(d.pending)), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
					k.Text(c, d.tr("需核对双端指纹后方生效"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
				})
				for _, p := range d.pending {
					existing := d.isExistingPendingDevice(p.id)
					selID := "pending:" + p.id
					if existing {
						selID = p.id
					}
					selected := d.deviceList.Value == selID
					bg, edge := deviceAccentSurface(k)
					if selected {
						edge = k.Color("accent-500")
					}
					title := p.name + " " + d.tr("请求加入同步网络")
					if existing {
						title = p.name + " " + d.tr("请求新增同步路径")
					}
					pathNames := d.pendingPathNames(p)
					viaText := p.via
					if viaText == "" && p.byCode {
						viaText = d.tr("通过配对码")
					}
					card := ui.Row(c.Key("p-"+p.id)).Padding(11, 13).Radius(12).Gap(11).AlignItems(ui.Center).Shrink(0).
						Background(bg).Border(1.5, edge).Role(ui.RoleListItem).Label(p.name)
					card.AccessSelected(selected)
					card.Children(func() {
						d.deviceIconTile(c.Key("ic"), k, p.icon, 34, true, false)
						ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
							k.Text(c, title, desktopkit.TextProps{Emphasized: true, Truncate: true})
							ui.Row(c.Key("sub")).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
								if pathNames != "" {
									d.inlineBadge(c.Key("ch"), k, pathNames, desktopkit.StatusInfo)
								}
								if viaText != "" {
									k.Text(c.Key("via"), viaText, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
								}
							})
						})
						if k.Button(c.Key("verify"), d.tr("核对指纹"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
							d.deviceList.Value, d.deviceDetailOpen, d.renaming = selID, true, false
						}
					})
					if card.Clicked() {
						d.deviceList.Value, d.deviceDetailOpen, d.renaming = selID, true, false
					}
				}
			})
		}

		// 4. My Devices card grid
		ui.Column(c.Key("my-devices")).Gap(9).Shrink(0).Children(func() {
			ui.Row(c.Key("sec-head")).AlignItems(ui.Center).Gap(8).Children(func() {
				k.Text(c, d.tr("我的设备"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
				k.Text(c, d.tr("点击卡片在右侧管理同步路径与安全信息"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
			})
			renderCard := func(v device) {
				selected := d.deviceList.Value == v.id
				statusText, tone, online := d.deviceAggregateStatus(v)
				card := ui.Column(c.Key("dev-" + v.id)).MinWidth(0).Radius(13).Gap(11).
					Background(deviceCardBg(k)).Role(ui.RoleListItem).Label(v.displayName())
				if twoCols {
					card.Grow(1)
				}
				card.AccessSelected(selected)
				if selected {
					card.Padding(12.5, 13.5).Border(1.5, k.Color("accent-500"))
				} else if card.Hovered() {
					card.Padding(13, 14).Border(1, deviceCardBorderHover(k))
				} else {
					card.Padding(13, 14).Border(1, deviceCardBorder(k))
				}
				if !k.Dark {
					card.Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, .035))
				}
				if !online && v.id != "local" && !selected {
					card.Opacity(0.76)
				}
				card.Children(func() {
					ui.Row(c.Key("top")).Gap(10).AlignItems(ui.Center).Children(func() {
						d.deviceIconTile(c.Key("ic"), k, v.icon, 34, false, false)
						ui.Column(c.Key("info")).Gap(2).Grow(1).MinWidth(0).Children(func() {
							k.Text(c, v.displayName(), desktopkit.TextProps{Emphasized: true, Truncate: true})
							sub := d.tr("当前设备")
							if v.id != "local" {
								sub = d.deviceSubtitle(v)
							}
							k.Text(c, sub, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
						})
						d.statusPill(c.Key("st"), k, statusText, tone)
					})
					ui.Box(c.Key("div")).Height(1).Background(k.Color("separator-border"))
					ui.Row(c.Key("foot")).Gap(8).AlignItems(ui.Center).Children(func() {
						var footLeft, footRight string
						if v.id == "local" {
							if len(runningChannels) > 0 {
								names := make([]string, 0, min(2, len(runningChannels)))
								for i := 0; i < min(2, len(runningChannels)); i++ {
									names = append(names, d.channelDisplayName(runningChannels[i].id))
								}
								footLeft = fmt.Sprintf(d.tr("%d 个同步渠道运行中"), len(runningChannels)) + " · " + strings.Join(names, "、")
							} else {
								footLeft = d.tr("仅本地模式 · 未运行同步渠道")
							}
							footRight = d.tr("端到端加密")
						} else {
							footLeft = d.deviceFootPaths(v)
							footRight = d.displayRelativeTime(v.lastSeen)
							if footRight == "" {
								footRight = d.tr("尚无同步")
							}
						}
						k.Text(c, footLeft, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true}).Grow(1).MinWidth(0)
						k.Text(c, footRight, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).Shrink(0)
					})
				})
				if card.Clicked() {
					d.deviceList.Value, d.deviceDetailOpen, d.renaming, d.deviceNote = v.id, true, false, ""
				}
			}

			if twoCols {
				for i := 0; i < len(ordered); i += 2 {
					ui.Row(c.Key(i)).Gap(11).AlignItems(ui.Stretch).Children(func() {
						ui.Column(c.Key("slot-0")).Grow(1).Basis(0).MinWidth(0).Children(func() {
							renderCard(ordered[i])
						})
						if i+1 < len(ordered) {
							ui.Column(c.Key("slot-1")).Grow(1).Basis(0).MinWidth(0).Children(func() {
								renderCard(ordered[i+1])
							})
						} else {
							ui.Box(c.Key("spacer")).Grow(1).Basis(0).MinWidth(0)
						}
					})
				}
			} else {
				for _, v := range ordered {
					renderCard(v)
				}
			}

			if len(peers) == 0 {
				ui.Column(c.Key("empty-peers")).Padding(20, 18).Radius(12).Gap(8).AlignItems(ui.Center).
					Background(deviceCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
					k.Icon(c.Key("ic"), desktopkit.IconDevices, 24).TextColor(k.Color("foreground-icon-tertiary"))
					if len(runningPairing) > 0 {
						k.Text(c, d.tr("还没有关联设备"), desktopkit.TextProps{Emphasized: true})
						k.Text(c, d.tr("通过一次性配对码或完整票据关联另一台设备，即可跨设备同步剪贴板。"), footnote).TextAlign(ui.Center)
						if k.Button(c.Key("invite-empty"), d.tr("邀请设备"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
							d.openPairingSheet("", "")
						}
					} else {
						k.Text(c, d.tr("尚未启用可配对的同步渠道"), desktopkit.TextProps{Emphasized: true})
						k.Text(c, d.tr("请在「设置 → 插件」中启用 Tailcat 或外部同步渠道并重启后，再添加设备。"), footnote).TextAlign(ui.Center)
						if k.Button(c.Key("goto-plugins-empty"), d.tr("前往插件设置"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
							d.openPluginSettings("")
						}
					}
				})
			}
		})

		// 5. External Channels section (if any)
		if len(d.channels) > 0 {
			ui.Column(c.Key("ext-channels")).Gap(9).Shrink(0).Children(func() {
				ui.Row(c.Key("head")).AlignItems(ui.Center).Gap(8).Children(func() {
					k.Text(c, fmt.Sprintf(d.tr("外部渠道 (%d)"), len(d.channels)), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
					k.Text(c, d.tr("第三方对端 · 无设备指纹"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
				})
				ui.Column(c.Key("ch-box")).Radius(12).Background(deviceCardBg(k)).
					Border(1, deviceCardBorder(k)).Clip().Children(func() {
					for i, ch := range d.channels {
						if i > 0 {
							ui.Box(c.Key(fmt.Sprint("sep", i))).Height(1).Background(k.Color("separator-border"))
						}
						selID := "ch:" + ch.id
						selected := d.deviceList.Value == selID
						row := ui.Row(c.Key("ch-"+ch.id)).Padding(10, 13).Gap(10).AlignItems(ui.Center).
							Role(ui.RoleListItem).Label(ch.name)
						row.AccessSelected(selected)
						if selected {
							bg, _ := deviceAccentSurface(k)
							row.Background(bg)
						} else if row.Hovered() {
							row.Background(k.Color("background-secondary-hover"))
						}
						row.Children(func() {
							d.deviceIconTile(c.Key("ic"), k, desktopkit.IconChat, 34, false, false)
							ui.Column(c.Key("meta")).Gap(2).Grow(1).MinWidth(0).Children(func() {
								ui.Row(c.Key("trow")).Gap(6).AlignItems(ui.Center).Children(func() {
									k.Text(c.Key("n"), ch.name, desktopkit.TextProps{Emphasized: true, Truncate: true}).MinWidth(0)
									d.inlineBadge(c.Key("tag"), k, d.tr("外部渠道"), desktopkit.StatusNeutral)
								})
								modeText := d.tr("仅保存到历史")
								if ch.autoApply {
									modeText = d.tr("自动写入剪贴板")
								}
								k.Text(c.Key("sub"), ch.backendID+" · "+modeText, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
							})
							stText, stTone := d.tr("已停用"), desktopkit.StatusNeutral
							if ch.enabled {
								stText, stTone = d.tr("已启用"), desktopkit.StatusPositive
							}
							d.statusPill(c.Key("st"), k, stText, stTone)
						})
						if row.Clicked() {
							d.deviceList.Value, d.deviceDetailOpen, d.renaming, d.deviceNote = selID, true, false, ""
						}
					}
				})
			})
		}

		// 6. Candidates section (if any)
		if len(d.candidates) > 0 {
			ui.Column(c.Key("candidates")).Gap(9).Shrink(0).Children(func() {
				ui.Row(c.Key("head")).AlignItems(ui.Center).Gap(8).Children(func() {
					k.Text(c, fmt.Sprintf(d.tr("可能认识 (%d)"), len(d.candidates)), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
					k.Text(c, d.tr("由已信任设备介绍 · 仍需双方批准"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
				})
				ui.Column(c.Key("cand-box")).Radius(12).Background(deviceCardBg(k)).
					Border(1, deviceCardBorder(k)).Clip().Children(func() {
					for i, cd := range d.candidates {
						if i > 0 {
							ui.Box(c.Key(fmt.Sprint("sep", i))).Height(1).Background(k.Color("separator-border"))
						}
						cid := "cand:" + cd.id
						selected := d.deviceList.Value == cid
						row := ui.Row(c.Key("cd-"+cd.id)).Padding(10, 13).Gap(10).AlignItems(ui.Center).Role(ui.RoleListItem).Label(cd.name)
						row.AccessSelected(selected)
						if selected {
							bg, _ := deviceAccentSurface(k)
							row.Background(bg)
						} else if row.Hovered() {
							row.Background(k.Color("background-secondary-hover"))
						}
						row.Children(func() {
							d.deviceIconTile(c.Key("ic"), k, cd.icon, 32, true, false)
							ui.Column(c.Key("meta")).Gap(2).Grow(1).MinWidth(0).Children(func() {
								k.Text(c, cd.name, desktopkit.TextProps{Emphasized: true, Truncate: true})
								sub := fmt.Sprintf(d.tr("由 %s 介绍"), cd.introducer)
								if cd.backendID != "" {
									sub += " · " + d.channelDisplayName(cd.backendID)
								}
								k.Text(c, sub, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
							})
							if k.Button(c.Key("ignore"), d.tr("忽略"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Disabled: d.live != nil && d.live.busy}).Clicked() {
								if d.live != nil {
									client := d.live.client
									d.mutate(func(ctx context.Context) error { return client.IgnoreCandidate(ctx, cd.id, true) }, "已忽略")
								} else {
									d.candidates = slices.Delete(d.candidates, i, i+1)
									d.ensureDeviceSelection()
								}
							}
							if k.Button(c.Key("join"), d.tr("连接"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall, Disabled: d.live != nil && d.live.busy}).Clicked() {
								if d.live != nil {
									d.pairLive(cd.id, cd.backendID)
								} else {
									d.candidates = slices.Delete(d.candidates, i, i+1)
									d.ensureDeviceSelection()
									d.setDeviceTask("wait")
								}
							}
						})
						if row.Clicked() {
							d.deviceList.Value, d.deviceDetailOpen, d.renaming, d.deviceNote = cid, true, false, ""
						}
					}
				})
			})
		}
	})
}

// ---- Unified Right 380-DIP Inspector (Live + Demo) ----

func (d *viewState) deviceDetail(c *ui.Context, k desktopkit.Kit) {
	id := d.deviceList.Value
	switch {
	case strings.HasPrefix(id, "pending:"):
		d.pendingDetail(c, k, strings.TrimPrefix(id, "pending:"))
		return
	case strings.HasPrefix(id, "ch:"):
		d.externalChannelDetail(c, k, strings.TrimPrefix(id, "ch:"))
		return
	case strings.HasPrefix(id, "cand:"):
		d.candidateDetail(c, k, strings.TrimPrefix(id, "cand:"))
		return
	}
	v := d.deviceByID(id)
	if v == nil {
		k.ContentUnavailable(c, desktopkit.ContentUnavailableProps{Icon: desktopkit.IconDevices, Title: d.tr("还没有关联设备"), Description: d.tr("邀请或连接另一台设备，开始同步剪贴板。")}, func() {
			if k.Button(c, d.tr("邀请设备"), desktopkit.ButtonProps{Variant: desktopkit.Primary}).Clicked() {
				d.openPairingSheet("", "")
			}
		})
		return
	}
	if v.id == "local" {
		d.localDeviceDetail(c, k, v)
		return
	}
	d.trustedDeviceDetail(c, k, v)
}

func (d *viewState) localDeviceDetail(c *ui.Context, k desktopkit.Kit, v *device) {
	ui.Column(c.Key("local-detail")).Gap(14).Children(func() {
		// Hero Card
		ui.Row(c.Key("hero")).Padding(13, 14).Radius(12).Gap(12).AlignItems(ui.Center).
			Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
			d.deviceIconTile(c.Key("ic"), k, v.icon, 42, false, true)
			ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
				ui.Row(c.Key("name-row")).Gap(6).AlignItems(ui.Center).Children(func() {
					k.Text(c.Key("name"), v.displayName(), desktopkit.TextProps{Emphasized: true, Truncate: true}).MinWidth(0)
					rb := ui.ButtonBase(c.Key("rename")).Label(d.tr("重命名")).Tooltip(d.tr("重命名")).Size(24, 24).Radius(6).
						Justify(ui.Center).AlignItems(ui.Center).Shrink(0)
					if rb.Hovered() {
						rb.Background(k.Color("background-secondary-hover"))
					}
					rb.Children(func() { k.Icon(c, desktopkit.IconEdit, 13).TextColor(k.Color("foreground-icon-secondary")) })
					if rb.Clicked() {
						d.renaming, d.renameValue = true, v.displayName()
					}
				})
				k.Text(c.Key("sub"), d.tr("当前设备"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
				d.statusPill(c.Key("st"), k, d.tr("本机"), desktopkit.StatusPositive).AlignSelf(ui.Start)
			})
		})

		if d.renaming {
			d.inlineRenameBox(c.Key("rename-local"), k, d.tr("本机名称"), "", d.tr("其他设备在配对与列表中将看到此自报名。"), false, func(val string) {
				if val == "" {
					return
				}
				if d.live != nil {
					cfg := d.live.config.Config
					cfg.DeviceName = val
					d.saveLiveConfig(cfg)
				} else {
					v.name = val
					d.deviceName = val
				}
			})
		}

		if v.deviceID != "" {
			ui.Column(c.Key("local-id")).Gap(7).Children(func() {
				ui.Row(c.Key("head")).AlignItems(ui.Center).Children(func() {
					k.Text(c, d.tr("本机设备 ID"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
					if k.Button(c.Key("copy-id"), d.tr("复制"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy}).Clicked() {
						d.copyDeviceText(v.deviceID, "已复制本机设备 ID")
					}
				})
				ui.Box(c.Key("id-box")).Padding(8, 10).Radius(8).
					Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
					k.Text(c, v.deviceID, desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true})
				})
				if d.deviceNote != "" {
					k.Text(c, d.tr(d.deviceNote), footnote)
				}
				k.Text(c, d.tr("配对安全指纹由双方公钥共同派生，请在具体远端设备详情中核对。"), footnote)
			})
		}

		// Sync channels read-only overview
		ui.Column(c.Key("channels-overview")).Gap(8).Children(func() {
			ui.Row(c.Key("head")).AlignItems(ui.Center).Children(func() {
				k.Text(c, d.tr("同步渠道运行概览"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
				if k.Button(c.Key("goto-plugins"), d.tr("前往插件设置"), desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
					d.openPluginSettings("")
				}
			})
			ui.Column(c.Key("box")).Radius(12).Background(deviceInspectorCardBg(k)).
				Border(1, deviceCardBorder(k)).Clip().Children(func() {
				for i, ch := range d.syncChannels {
					ch := ch
					if i > 0 {
						ui.Box(c.Key(fmt.Sprint("sep", i))).Height(1).Background(k.Color("separator-border"))
					}
					row := ui.Row(c.Key("ch-"+ch.id)).Padding(10, 12).Gap(10).AlignItems(ui.Center)
					if row.Hovered() {
						row.Background(k.Color("background-secondary-hover"))
					}
					row.Children(func() {
						ui.Column(c.Key("meta")).Gap(2).Grow(1).MinWidth(0).Children(func() {
							k.Text(c, d.channelDisplayName(ch.id), desktopkit.TextProps{Emphasized: true, Truncate: true})
							sub := ch.implementationID
							if ch.policy != "" {
								sub += " · " + ch.policy
							}
							k.Text(c, sub, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
						})
						tone := desktopkit.StatusNeutral
						switch ch.state {
						case "running":
							tone = desktopkit.StatusPositive
						case "unavailable":
							tone = desktopkit.StatusCritical
						case "restarting":
							tone = desktopkit.StatusWarning
						}
						d.statusPill(c.Key("st"), k, d.channelStateLabel(ch.state), tone)
					})
					if row.Clicked() {
						d.openPluginSettings(ch.id)
					}
				}
			})
		})
	})
}

func (d *viewState) trustedDeviceDetail(c *ui.Context, k desktopkit.Kit, v *device) {
	statusText, tone, _ := d.deviceAggregateStatus(*v)
	auth := authorizedBindings(*v)
	pendEntry := d.pendingForDevice(v.id)
	unbound := d.unboundRunningPairingChannels(*v)
	allPaused := len(auth) > 0 && !slices.ContainsFunc(auth, func(b deviceBinding) bool { return b.syncEnabled })
	showBatchPause := len(auth) >= 2
	busy := d.live != nil && d.live.busy

	ui.Column(c.Key("trusted-detail")).Gap(14).Children(func() {
		// 1. Hero Card
		ui.Row(c.Key("hero")).Padding(13, 14).Radius(12).Gap(12).AlignItems(ui.Center).
			Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
			d.deviceIconTile(c.Key("ic"), k, v.icon, 42, false, true)
			ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
				ui.Row(c.Key("name-row")).Gap(6).AlignItems(ui.Center).Children(func() {
					k.Text(c.Key("name"), v.displayName(), desktopkit.TextProps{Emphasized: true, Truncate: true}).MinWidth(0)
					rb := ui.ButtonBase(c.Key("rename")).Label(d.tr("重命名")).Tooltip(d.tr("重命名")).Size(24, 24).Radius(6).
						Justify(ui.Center).AlignItems(ui.Center).Shrink(0)
					if rb.Hovered() {
						rb.Background(k.Color("background-secondary-hover"))
					}
					rb.Children(func() { k.Icon(c, desktopkit.IconEdit, 13).TextColor(k.Color("foreground-icon-secondary")) })
					if rb.Clicked() {
						d.renaming = true
						if v.userName != "" {
							d.renameValue = v.userName
						} else {
							d.renameValue = v.displayName()
						}
					}
				})
				if v.userName != "" && v.reportedName != "" && v.userName != v.reportedName {
					k.Text(c.Key("reported"), d.tr("对方自报：")+v.reportedName, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
				}
				ui.Row(c.Key("st-row")).Gap(8).AlignItems(ui.Center).Children(func() {
					d.statusPill(c.Key("st"), k, statusText, tone)
					if seen := d.displayRelativeTime(v.lastSeen); seen != "" {
						k.Text(c.Key("seen"), d.tr("最近活动 ")+seen, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
					}
				})
			})
		})

		// Inline Rename Box
		if d.renaming {
			hint := d.tr("只修改本机显示的名称；留空则恢复使用对方自报名。")
			if v.reportedName != "" {
				hint = fmt.Sprintf(d.tr("只修改本机显示的名称；留空则恢复使用对方自报名（%s）。"), v.reportedName)
			}
			d.inlineRenameBox(c.Key("rename-peer"), k, d.tr("设备备注名"), v.reportedName, hint, true, func(val string) {
				if d.live != nil {
					d.renameLivePeer(v.id, v.revision, val)
				} else {
					v.userName = val
					if val != "" {
						v.name = val
					} else if v.reportedName != "" {
						v.name = v.reportedName
					}
				}
			})
		}

		// 2. Inline Pending New-Path Approval Card (top priority inside inspector)
		if pendEntry != nil {
			bg, edge := deviceAccentSurface(k)
			ui.Column(c.Key("inline-approve")).Padding(12).Radius(12).Gap(10).
				Background(bg).Border(1.5, edge).Children(func() {
				ui.Row(c.Key("head")).Gap(8).AlignItems(ui.Start).Children(func() {
					k.Icon(c.Key("ic"), desktopkit.IconShield, 16).TextColor(k.Color("accent-500")).Shrink(0)
					ui.Column(c.Key("txt")).Gap(2).Grow(1).MinWidth(0).Children(func() {
						k.Text(c.Key("t"), d.tr("待批准新同步路径：")+d.pendingPathNames(*pendEntry), desktopkit.TextProps{Emphasized: true})
						k.Text(c.Key("s"), d.tr("此设备已受信任，但新增同步路径仍需核对指纹并显式批准。"), footnote)
					})
				})
				d.fingerprintGrid(c.Key("fp"), k, pendEntry.fingerprint, true)
				ui.Row(c.Key("acts")).Gap(8).Justify(ui.End).Children(func() {
					if k.Button(c.Key("deny-path"), d.tr("拒绝"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Size: desktopkit.SizeSmall, Disabled: busy}).Clicked() {
						if d.live != nil {
							d.rejectLivePendingPaths(pendEntry.id)
						} else {
							d.rejectDemoPending(pendEntry.id)
						}
					}
					if k.Button(c.Key("approve-path"), d.tr("接受"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall, Disabled: busy}).Clicked() {
						if d.live != nil {
							d.approveLive(pendEntry.id, true)
						} else {
							d.approveDemoPending(pendEntry.id)
						}
					}
				})
			})
		}

		// 3. Sync Paths (Bindings) Group
		ui.Column(c.Key("sync-paths")).Gap(8).Children(func() {
			ui.Row(c.Key("head")).AlignItems(ui.Center).Gap(8).Children(func() {
				k.Text(c, fmt.Sprintf(d.tr("同步路径 (%d)"), len(auth)), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
				if len(auth) > 0 {
					probeLabel := d.tr("检测连接")
					if d.refreshingDevices && (d.refreshingDeviceID == "" || d.refreshingDeviceID == v.id) {
						probeLabel = d.tr("检测中…")
					}
					if k.Button(c.Key("probe-device"), probeLabel, desktopkit.ButtonProps{
						Variant:  desktopkit.Subtle,
						Size:     desktopkit.SizeSmall,
						Icon:     desktopkit.IconRetry,
						Disabled: d.refreshingDevices || busy,
					}).Clicked() {
						if d.live != nil {
							d.refreshLiveDevices(v.id)
						}
					}
				}
				addTip := ""
				if len(unbound) == 0 {
					addTip = d.tr("所有运行中的可配对同步渠道均已关联")
				}
				if k.Button(c.Key("add-path"), d.tr("添加路径"), desktopkit.ButtonProps{
					Variant:  desktopkit.Subtle,
					Size:     desktopkit.SizeSmall,
					Icon:     desktopkit.IconAdd,
					Tooltip:  addTip,
					Disabled: len(unbound) == 0 || busy,
				}).Clicked() {
					d.openPairingSheet(v.id, unbound[0].id)
				}
			})

			if len(auth) == 0 {
				ui.Column(c.Key("empty-paths")).Padding(16, 14).Radius(12).Gap(8).AlignItems(ui.Center).
					Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
					k.Text(c, d.tr("暂无已授权同步路径"), desktopkit.TextProps{Emphasized: true})
					k.Text(c, d.tr("设备信任仍然保留，但目前不会同步任何内容。通过同步渠道重新关联即可恢复。"), footnote).TextAlign(ui.Center)
					if len(d.runningPairingChannels()) > 0 {
						if k.Button(c.Key("add-first-path"), d.tr("添加同步路径"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall, Icon: desktopkit.IconAdd, Disabled: busy}).Clicked() {
							pref := ""
							if len(unbound) > 0 {
								pref = unbound[0].id
							}
							d.openPairingSheet(v.id, pref)
						}
					} else {
						if k.Button(c.Key("go-plugins-path"), d.tr("前往插件设置"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
							d.openPluginSettings("")
						}
					}
				})
			} else {
				ui.Column(c.Key("paths-box")).Radius(12).Background(deviceInspectorCardBg(k)).
					Border(1, deviceCardBorder(k)).Clip().Children(func() {
					for i, b := range auth {
						b := b
						if i > 0 {
							ui.Box(c.Key(fmt.Sprint("sep", i))).Height(1).Background(k.Color("separator-border"))
						}
						chInfo := d.syncChannelByID(b.instanceID)
						beStopped := chInfo != nil && chInfo.state != "running"
						connText := d.tr("未连接")
						switch {
						case !b.syncEnabled:
							connText = d.tr("已暂停")
						case b.connection == "connected":
							parts := []string{d.tr("已连接")}
							if pLabel := d.bindingPathLabel(b.connectionPath); pLabel != "" {
								parts = append(parts, pLabel)
							}
							if b.latencyMs > 0 {
								parts = append(parts, fmt.Sprintf("%d ms", b.latencyMs))
							}
							connText = strings.Join(parts, " · ")
						case b.connection == "connecting":
							connText = d.tr("正在重连…")
						}
						sub := b.implementationID + " · " + connText
						var metaParts []string
						if checked := d.displayRelativeTime(b.lastChecked); checked != "" {
							metaParts = append(metaParts, d.tr("上次检测 ")+checked)
						}
						if act := d.displayRelativeTime(b.lastActive); act != "" {
							metaParts = append(metaParts, d.tr("最近活动 ")+act)
						}
						expanded := d.bindingExpanded(b, len(auth))
						customized := !isDefaultBindingPolicy(b.policy)
						ui.Column(c.Key("path-" + b.id)).Children(func() {
							ui.Row(c.Key("head")).Padding(11, 13).Gap(8).AlignItems(ui.Center).Children(func() {
								toggle := ui.ButtonBase(c.Key("toggle-expand")).Label(d.tr("展开或收起同步策略") + " " + d.channelDisplayName(b.instanceID)).
									Grow(1).MinWidth(0).Gap(6).AlignItems(ui.Start).
									FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(1)
								toggle.Children(func() {
									chev := desktopkit.IconChevronRight
									if expanded {
										chev = desktopkit.IconChevronDown
									}
									ui.Box(c.Key("chev")).Padding(3, 0, 0, 0).Shrink(0).Children(func() {
										k.Icon(c, chev, 12).TextColor(k.Color("foreground-icon-tertiary")).AccessHidden(true)
									})
									ui.Column(c.Key("info")).Gap(2).Grow(1).MinWidth(0).Children(func() {
										ui.Row(c.Key("trow")).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
											k.Text(c.Key("name"), d.channelDisplayName(b.instanceID), desktopkit.TextProps{Emphasized: true})
											if customized && b.syncEnabled {
												d.inlineBadge(c.Key("custom-badge"), k, d.tr("自定义"), desktopkit.StatusInfo)
											}
											if beStopped {
												d.inlineBadge(c.Key("stop-badge"), k, d.channelStateLabel(chInfo.state), desktopkit.StatusWarning)
											}
										})
										k.Text(c.Key("sub"), sub, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
										if !expanded {
											k.Text(c.Key("pol-sum"), d.bindingPolicySummary(b), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary, Truncate: true})
										}
										if len(metaParts) > 0 {
											k.Text(c.Key("meta-time"), strings.Join(metaParts, " · "), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary, Truncate: true})
										}
										if b.lastError != "" && b.syncEnabled && !beStopped {
											k.Text(c.Key("err"), d.bindingErrorLabel(b.lastError), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneCritical})
										}
										if beStopped {
											ui.Row(c.Key("warn-row")).Wrap().Gap(4).AlignItems(ui.Center).Children(func() {
												k.Text(c.Key("w"), d.tr("对应同步渠道未运行"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(k.Color("status-yellow-text"))
												if k.Button(c.Key("jump"), d.tr("前往插件设置"), desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
													d.openPluginSettings(b.instanceID)
												}
											})
										}
									})
								})
								if toggle.Clicked() {
									d.toggleBindingExpanded(b, len(auth))
								}
								ui.Row(c.Key("acts")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(func() {
									on := b.syncEnabled
									if k.Switch(c.Key("sw"), &on, desktopkit.SwitchProps{Label: d.tr("同步") + " " + d.channelDisplayName(b.instanceID), Disabled: busy}).Changed() {
										if d.live != nil {
											d.setLiveBindingSync(b.id, b.revision, on)
										} else {
											for j := range v.bindings {
												if v.bindings[j].id == b.id {
													v.bindings[j].syncEnabled = on
													break
												}
											}
										}
									}
									menuItems := []desktopkit.MenuEntry{}
									if customized {
										menuItems = append(menuItems, desktopkit.MenuEntry{
											ID:       "reset-policy",
											Label:    d.tr("恢复默认策略"),
											Icon:     desktopkit.IconRetry,
											Disabled: busy,
										})
									}
									menuItems = append(menuItems,
										desktopkit.MenuEntry{ID: "policy", Label: d.tr("查看同步渠道策略…"), Icon: desktopkit.IconSettings},
										desktopkit.MenuEntry{ID: "remove", Label: d.tr("移除此路径"), Icon: desktopkit.IconDelete, Destructive: true, Disabled: busy},
									)
									_, chosen := k.Menu(c.Key("more"), desktopkit.MenuProps{
										Label:    d.tr("更多操作"),
										Icon:     desktopkit.IconMore,
										IconOnly: true,
										Variant:  desktopkit.Subtle,
										Size:     desktopkit.SizeSmall,
										Items:    menuItems,
									})
									switch chosen {
									case "reset-policy":
										en := true
										var enPtr *bool
										if !b.syncEnabled {
											enPtr = &en
										}
										d.applyBindingPolicyUpdate(v, b, enPtr, engine.DefaultBindingSyncPolicy())
									case "policy":
										d.openPluginSettings(b.instanceID)
									case "remove":
										chName := d.channelDisplayName(b.instanceID)
										d.confirm = fmt.Sprintf(d.tr("移除 %s 路径？"), chName)
										d.confirmDesc = fmt.Sprintf(d.tr("仅移除与「%s」通过 %s 的同步路径。设备信任与其他同步路径将保留，日后可重新配对添加。"), v.displayName(), chName)
										d.confirmLabel = d.tr("移除路径")
										d.confirmID = "binding:" + v.id + ":" + b.id
									}
								})
							})
							if expanded {
								d.bindingPolicyPanel(c.Key("pol-panel"), k, v, b, chInfo, busy)
							}
						})
					}
				})
				k.Text(c, d.tr("单路径开关控制是否暂停该路径；下方策略仅控制本机自动同步与接收，手动发送不受限制，修改仅影响新开始的传输。"), footnote)
			}
		})

		// 4. Fingerprint Section
		if v.fingerprint != "" {
			ui.Column(c.Key("fp-sec")).Gap(7).Children(func() {
				ui.Row(c.Key("head")).AlignItems(ui.Center).Children(func() {
					k.Text(c, d.tr("安全核对（设备指纹）"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
					if k.Button(c.Key("copy-fp"), d.tr("复制"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy}).Clicked() {
						d.copyDeviceText(v.fingerprint, "已复制设备指纹")
					}
				})
				ui.Column(c.Key("fp-box")).Padding(11).Radius(12).Background(deviceInspectorCardBg(k)).
					Border(1, deviceCardBorder(k)).Children(func() {
					d.fingerprintGrid(c, k, v.fingerprint, true)
				})
				if d.deviceNote != "" {
					k.Text(c, d.tr(d.deviceNote), footnote)
				}
			})
		}

		// 5. Bottom Actions (Batch Pause ONLY when >= 2 authorized bindings!)
		ui.Row(c.Key("dev-actions")).Gap(8).Padding(4, 0, 0, 0).AlignItems(ui.Center).Children(func() {
			if showBatchPause {
				pauseLabel := d.tr("暂停全部路径")
				if allPaused {
					pauseLabel = d.tr("恢复全部路径")
				}
				if k.Button(c.Key("toggle-all-paths"), pauseLabel, desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Disabled: busy}).Grow(1).Clicked() {
					target := allPaused
					if d.live != nil {
						d.setLiveAllBindingsSync(auth, target)
					} else {
						for j := range v.bindings {
							if v.bindings[j].state == "authorized" {
								v.bindings[j].syncEnabled = target
							}
						}
					}
				}
			} else {
				ui.Spacer(c)
			}
			if k.Button(c.Key("remove-device"), d.tr("移除设备"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Size: desktopkit.SizeSmall, Icon: desktopkit.IconDelete, Disabled: busy}).Clicked() {
				d.confirm = fmt.Sprintf(d.tr("移除「%s」？"), v.displayName())
				d.confirmDesc = d.tr("将撤销此设备的信任并移除全部同步路径。对方需要重新配对并批准。已同步到对方的内容无法撤回。")
				d.confirmLabel = d.tr("移除设备")
				d.confirmID = "device:" + v.id
			}
		})
	})
}

func (d *viewState) bindingPolicyPanel(c *ui.Context, k desktopkit.Kit, v *device, b deviceBinding, chInfo *syncChannel, busy bool) {
	pol := b.policy.Normalized()
	globalOn := d.globalSyncEnabled()
	paused := !b.syncEnabled || !globalOn
	segTrackBg := k.Color("background-tertiary-default")
	segActiveBg := deviceCardBg(k)

	activePreset := engine.SyncDirection("")
	if b.syncEnabled && pol.Text == pol.Images && pol.Images == pol.Files {
		activePreset = pol.Text
	}

	ui.Box(c.Key("div")).Height(1).Background(k.Color("separator-border"))
	panel := ui.Column(c.Key("body")).Padding(9, 12, 10, 12).Gap(8).Background(deviceInsetBg(k, true))
	if paused {
		panel.Opacity(0.62)
	}
	panel.Children(func() {
		// Quick presets row
		ui.Row(c.Key("presets")).Gap(6).AlignItems(ui.Center).Children(func() {
			k.Text(c.Key("lbl"), d.tr("快捷模式"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Shrink(0)
			ui.Spacer(c)
			ui.Row(c.Key("track")).Padding(2).Radius(7).Gap(2).Background(segTrackBg).Shrink(0).Children(func() {
				presets := []struct {
					dir   engine.SyncDirection
					label string
				}{
					{engine.SyncDirectionBidirectional, d.tr("全部双向")},
					{engine.SyncDirectionSendOnly, d.tr("仅发送")},
					{engine.SyncDirectionReceiveOnly, d.tr("仅接收")},
					{engine.SyncDirectionDisabled, d.tr("不同步")},
				}
				for _, p := range presets {
					selected := activePreset == p.dir
					btn := ui.ButtonBase(c.Key("pre-"+string(p.dir))).Label(p.label).Height(22).Padding(0, 7).Radius(5).
						Justify(ui.Center).AlignItems(ui.Center).Shrink(0).Disabled(busy || !globalOn).
						FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(1)
					fg := k.Color("text-secondary")
					if selected {
						btn.Background(segActiveBg)
						fg = k.Color("text-primary")
						if p.dir == engine.SyncDirectionBidirectional {
							fg = k.Color("accent-500")
						}
					} else if btn.Hovered() && !busy && globalOn {
						fg = k.Color("text-primary")
					}
					btn.Children(func() {
						k.Text(c, p.label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: selected}).TextColor(fg)
					})
					if btn.Clicked() {
						next := engine.BindingSyncPolicy{Text: p.dir, Images: p.dir, Files: p.dir}
						var enPtr *bool
						if !b.syncEnabled && p.dir != engine.SyncDirectionDisabled {
							en := true
							enPtr = &en
						}
						d.applyBindingPolicyUpdate(v, b, enPtr, next)
					}
				}
			})
		})

		// Per-type direction rows (Plan A)
		type typeRow struct {
			kind  string
			label string
			icon  desktopkit.IconName
			cur   engine.SyncDirection
			set   func(p *engine.BindingSyncPolicy, dir engine.SyncDirection)
		}
		rows := []typeRow{
			{"text", d.tr("文本"), desktopkit.IconText, pol.Text, func(p *engine.BindingSyncPolicy, dir engine.SyncDirection) { p.Text = dir }},
			{"images", d.tr("图片"), desktopkit.IconImage, pol.Images, func(p *engine.BindingSyncPolicy, dir engine.SyncDirection) { p.Images = dir }},
			{"files", d.tr("文件"), desktopkit.IconFile, pol.Files, func(p *engine.BindingSyncPolicy, dir engine.SyncDirection) { p.Files = dir }},
		}
		dirs := []struct {
			dir   engine.SyncDirection
			label string
		}{
			{engine.SyncDirectionBidirectional, d.tr("双向")},
			{engine.SyncDirectionSendOnly, d.tr("仅发")},
			{engine.SyncDirectionReceiveOnly, d.tr("仅收")},
			{engine.SyncDirectionDisabled, d.tr("关闭")},
		}

		ui.Column(c.Key("matrix")).Gap(7).Children(func() {
			for _, tr := range rows {
				reason, locked := d.bindingTypeLockedReason(chInfo, tr.kind)
				ui.Row(c.Key("row-" + tr.kind)).Gap(8).AlignItems(ui.Center).Children(func() {
					ui.Row(c.Key("meta")).Gap(6).AlignItems(ui.Center).Grow(1).MinWidth(0).Children(func() {
						ui.Row(c.Key("ic")).Size(22, 22).Radius(6).Justify(ui.Center).AlignItems(ui.Center).Shrink(0).
							Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
							k.Icon(c, tr.icon, 12).TextColor(k.Color("foreground-icon-secondary")).AccessHidden(true)
						})
						k.Text(c.Key("lbl"), tr.label, desktopkit.TextProps{Role: desktopkit.RoleCallout, Emphasized: true, Truncate: true})
					})
					if locked {
						bg, edge, fg := deviceWarnBannerSurface(k)
						ui.Row(c.Key("locked")).Height(24).Padding(0, 7).Radius(7).Gap(5).AlignItems(ui.Center).Shrink(0).
							Background(bg).Border(1, edge).Children(func() {
							k.Text(c.Key("r"), reason, desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(fg)
							if reason == d.tr("渠道已关闭") {
								if k.Button(c.Key("adj"), d.tr("调整"), desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
									d.openPluginSettings(b.instanceID)
								}
							}
						})
					} else {
						ui.Row(c.Key("seg")).Padding(2).Radius(7).Gap(2).Background(segTrackBg).Shrink(0).Children(func() {
							for _, opt := range dirs {
								active := b.syncEnabled && tr.cur == opt.dir
								btn := ui.ButtonBase(c.Key("opt-"+string(opt.dir))).
									Label(tr.label+"："+opt.label).
									Height(22).Padding(0, 7).Radius(5).
									Justify(ui.Center).AlignItems(ui.Center).Shrink(0).
									Disabled(busy || paused).
									FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(1)
								fg := k.Color("text-secondary")
								if active {
									btn.Background(segActiveBg)
									switch opt.dir {
									case engine.SyncDirectionBidirectional:
										fg = k.Color("accent-500")
									case engine.SyncDirectionSendOnly:
										fg = k.Color("state-success-text")
										if k.Dark {
											fg = k.Color("notification-success-foreground")
										}
									case engine.SyncDirectionReceiveOnly:
										fg = k.Color("text-primary")
									default:
										fg = k.Color("text-secondary")
									}
								} else if btn.Hovered() && !busy && !paused {
									fg = k.Color("text-primary")
								}
								btn.Children(func() {
									k.Text(c, opt.label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: active}).TextColor(fg)
								})
								if btn.Clicked() && tr.cur != opt.dir {
									next := pol
									tr.set(&next, opt.dir)
									d.applyBindingPolicyUpdate(v, b, nil, next)
								}
							}
						})
					}
				})
			}
		})

		// Effective send/receive flow summary
		var sendLabels, recvLabels []string
		for _, tr := range rows {
			_, locked := d.bindingTypeLockedReason(chInfo, tr.kind)
			if locked || !b.syncEnabled {
				continue
			}
			if tr.cur.AllowsSend() {
				sendLabels = append(sendLabels, tr.label)
			}
			if tr.cur.AllowsReceive() {
				recvLabels = append(recvLabels, tr.label)
			}
		}
		formatKinds := func(items []string) string {
			if len(items) == 0 {
				return d.tr("无")
			}
			return strings.Join(items, " · ")
		}
		ui.Column(c.Key("flow")).Padding(7, 9).Radius(8).Gap(3).
			Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
			ui.Row(c.Key("out")).Gap(6).AlignItems(ui.Center).Children(func() {
				k.Text(c.Key("d"), d.tr("本机 → 对端"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).Grow(1)
				k.Text(c.Key("v"), formatKinds(sendLabels), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: len(sendLabels) > 0, Tone: desktopkit.TonePrimary})
			})
			ui.Row(c.Key("in")).Gap(6).AlignItems(ui.Center).Children(func() {
				k.Text(c.Key("d"), d.tr("对端 → 本机"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).Grow(1)
				k.Text(c.Key("v"), formatKinds(recvLabels), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: len(recvLabels) > 0, Tone: desktopkit.TonePrimary})
			})
		})
	})
}

func (d *viewState) externalChannelDetail(c *ui.Context, k desktopkit.Kit, id string) {
	ch := d.channelByID(id)
	if ch == nil {
		return
	}
	busy := d.live != nil && d.live.busy
	ui.Column(c.Key("ext-ch-detail")).Gap(14).Children(func() {
		// Hero Card
		ui.Row(c.Key("hero")).Padding(13, 14).Radius(12).Gap(12).AlignItems(ui.Center).
			Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
			d.deviceIconTile(c.Key("ic"), k, desktopkit.IconChat, 42, false, true)
			ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
				ui.Row(c.Key("name-row")).Gap(6).AlignItems(ui.Center).Children(func() {
					k.Text(c.Key("name"), ch.name, desktopkit.TextProps{Emphasized: true, Truncate: true}).MinWidth(0)
					rb := ui.ButtonBase(c.Key("rename")).Label(d.tr("重命名")).Tooltip(d.tr("重命名")).Size(24, 24).Radius(6).
						Justify(ui.Center).AlignItems(ui.Center).Shrink(0)
					if rb.Hovered() {
						rb.Background(k.Color("background-secondary-hover"))
					}
					rb.Children(func() { k.Icon(c, desktopkit.IconEdit, 13).TextColor(k.Color("foreground-icon-secondary")) })
					if rb.Clicked() {
						d.renaming, d.renameValue = true, ch.name
					}
				})
				k.Text(c.Key("sub"), d.tr("所属同步渠道：")+ch.backendID, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
				ui.Row(c.Key("badges")).Gap(6).AlignItems(ui.Center).Children(func() {
					d.inlineBadge(c.Key("ext"), k, d.tr("外部渠道"), desktopkit.StatusNeutral)
					stText, stTone := d.tr("已停用"), desktopkit.StatusNeutral
					if ch.enabled {
						stText, stTone = d.tr("已启用"), desktopkit.StatusPositive
					}
					d.statusPill(c.Key("st"), k, stText, stTone)
				})
			})
		})

		if d.renaming {
			d.inlineRenameBox(c.Key("rename-ch"), k, d.tr("渠道名称"), "", "", false, func(val string) {
				if val == "" {
					return
				}
				if d.live != nil {
					d.updateLiveChannel(ch.id, api.ChannelUpdate{Revision: ch.revision, Name: &val})
				} else {
					ch.name = val
				}
			})
		}

		// No-fingerprint disclosure
		bg, edge := deviceAccentSurface(k)
		ui.Row(c.Key("no-fp-notice")).Padding(10, 12).Radius(11).Gap(8).AlignItems(ui.Center).
			Background(bg).Border(1, edge).Children(func() {
			k.Text(c, d.tr("外部渠道无法提供 UniClipboard 设备身份证明，因此没有设备指纹，也不共享设备级信任。"), footnote)
		})

		if ch.lastError != "" {
			ui.Row(c.Key("ch-err")).Padding(10, 12).Radius(11).Gap(8).AlignItems(ui.Center).
				Background(k.Color("notification-error-background")).Border(1, k.Color("text-error-primary").Alpha(0.30)).Children(func() {
				k.Text(c, d.bindingErrorLabel(ch.lastError), desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(k.Color("notification-error-foreground"))
			})
		}

		// Controls
		ui.Column(c.Key("controls")).Gap(8).Children(func() {
			k.Text(c, d.tr("渠道控制"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
			ui.Column(c.Key("box")).Radius(12).Background(deviceInspectorCardBg(k)).
				Border(1, deviceCardBorder(k)).Clip().Children(func() {
				ui.Row(c.Key("row-en")).Padding(11, 13).Gap(10).AlignItems(ui.Center).Children(func() {
					ui.Column(c.Key("info")).Gap(2).Grow(1).MinWidth(0).Children(func() {
						k.Text(c, d.tr("启用此渠道同步"), desktopkit.TextProps{Emphasized: true})
						k.Text(c, d.tr("关闭后停止经此外部渠道收发内容"), footnote)
					})
					en := ch.enabled
					if k.Switch(c.Key("sw-en"), &en, desktopkit.SwitchProps{Label: d.tr("启用此渠道同步"), Disabled: busy}).Changed() {
						if d.live != nil {
							d.updateLiveChannel(ch.id, api.ChannelUpdate{Revision: ch.revision, Enabled: &en})
						} else {
							ch.enabled = en
						}
					}
				})
				ui.Box(c.Key("sep")).Height(1).Background(k.Color("separator-border"))
				ui.Row(c.Key("row-auto")).Padding(11, 13).Gap(10).AlignItems(ui.Center).Children(func() {
					ui.Column(c.Key("info")).Gap(2).Grow(1).MinWidth(0).Children(func() {
						k.Text(c, d.tr("收到时自动写入剪贴板"), desktopkit.TextProps{Emphasized: true})
						k.Text(c, d.tr("开启后来自此渠道的内容会自动应用到系统剪贴板"), footnote)
					})
					aa := ch.autoApply
					if k.Switch(c.Key("sw-auto"), &aa, desktopkit.SwitchProps{Label: d.tr("收到时自动写入剪贴板"), Disabled: busy || !ch.enabled}).Changed() {
						if d.live != nil {
							d.updateLiveChannel(ch.id, api.ChannelUpdate{Revision: ch.revision, AutoApply: &aa})
						} else {
							ch.autoApply = aa
						}
					}
				})
			})
		})

		ui.Row(c.Key("ch-actions")).Justify(ui.End).Padding(4, 0, 0, 0).Children(func() {
			if k.Button(c.Key("remove-ch"), d.tr("移除渠道"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Size: desktopkit.SizeSmall, Icon: desktopkit.IconDelete, Disabled: busy}).Clicked() {
				d.confirm = fmt.Sprintf(d.tr("移除外部渠道「%s」？"), ch.name)
				if ch.remoteRevocation {
					d.confirmDesc = d.tr("将停用并尝试退出远端群组，退出成功后清除本机记录；若退出失败将保留并标示异常。")
				} else {
					d.confirmDesc = d.tr("此操作仅从本机移除该外部渠道记录，不影响第三方远端群组状态。")
				}
				d.confirmLabel = d.tr("移除渠道")
				d.confirmID = "channel:" + ch.id
			}
		})
	})
}

func (d *viewState) inlineRenameBox(c *ui.Context, k desktopkit.Kit, label, placeholder, hint string, allowEmpty bool, onCommit func(string)) {
	busy := d.live != nil && d.live.busy
	ui.Column(c).Padding(10, 12).Radius(10).Gap(8).
		Background(deviceInspectorCardBg(k)).Border(1, k.Color("accent-400")).Children(func() {
		r := k.FormField(c.Key("field"), desktopkit.FormFieldProps{
			Label:       label,
			Value:       &d.renameValue,
			Placeholder: placeholder,
			Hint:        hint,
			AutoFocus:   true,
			Disabled:    busy,
		})
		trimmed := strings.TrimSpace(d.renameValue)
		canSave := !busy && (allowEmpty || trimmed != "")
		save := r.Submitted && canSave
		ui.Row(c.Key("acts")).Gap(8).Justify(ui.End).Children(func() {
			if k.Button(c.Key("cancel"), d.tr("取消"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() || r.Escaped {
				d.renaming = false
			}
			if k.Button(c.Key("save"), d.tr("保存"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall, Disabled: !canSave}).Clicked() {
				save = true
			}
		})
		if save {
			d.renaming = false
			onCommit(trimmed)
		}
	})
}

func (d *viewState) copyDeviceText(val, note string) {
	if d.live != nil {
		mygo.Clipboard.WriteText(val)
		d.deviceNote = note
		return
	}
	d.deviceNote = note + "（演示，未写入剪贴板）"
}

func (d *viewState) pendingDetail(c *ui.Context, k desktopkit.Kit, id string) {
	i := slices.IndexFunc(d.pending, func(p pendingDevice) bool { return p.id == id })
	if i < 0 {
		return
	}
	p := d.pending[i]
	busy := d.live != nil && d.live.busy
	ui.Column(c.Key("pending-detail")).Gap(14).Children(func() {
		bg, edge := deviceAccentSurface(k)
		ui.Row(c.Key("p-hero")).Padding(13, 14).Radius(12).Gap(12).AlignItems(ui.Center).Background(bg).Border(1, edge).Children(func() {
			d.deviceIconTile(c.Key("ic"), k, p.icon, 42, true, true)
			ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
				k.Text(c, p.name, desktopkit.TextProps{Emphasized: true, Truncate: true})
				viaText := p.via
				if viaText == "" && p.byCode {
					viaText = d.tr("通过配对码")
				}
				if viaText != "" {
					k.Text(c, viaText, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
				}
				d.statusPill(c.Key("st"), k, d.tr("新设备请求加入"), desktopkit.StatusInfo).AlignSelf(ui.Start)
			})
		})

		if len(p.paths) > 0 {
			ui.Column(c.Key("p-paths")).Gap(7).Children(func() {
				k.Text(c, d.tr("本次将授权的同步路径"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
				ui.Column(c.Key("box")).Radius(12).Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Clip().Children(func() {
					for idx, path := range p.paths {
						if idx > 0 {
							ui.Box(c.Key(fmt.Sprint("sep", idx))).Height(1).Background(k.Color("separator-border"))
						}
						ui.Row(c.Key("pp-"+path.instanceID)).Padding(10, 12).Gap(10).AlignItems(ui.Center).Children(func() {
							ui.Column(c.Key("info")).Gap(2).Grow(1).MinWidth(0).Children(func() {
								k.Text(c, d.channelDisplayName(path.instanceID), desktopkit.TextProps{Emphasized: true})
								k.Text(c, d.tr("批准后立即允许通过此同步渠道收发内容"), footnote)
							})
							d.statusPill(c.Key("st"), k, d.tr("待批准"), desktopkit.StatusInfo)
						})
					}
				})
			})
		}

		ui.Column(c.Key("p-fp")).Gap(7).Children(func() {
			k.Text(c, d.tr("安全核对（设备指纹）"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
			ui.Column(c.Key("fp-box")).Padding(11).Radius(12).Gap(8).Background(deviceInspectorCardBg(k)).
				Border(1, deviceCardBorder(k)).Children(func() {
				d.fingerprintGrid(c, k, p.fingerprint, true)
				k.Text(c, d.tr("仅当对方屏幕上显示完全相同的 8 组指纹时，才点击接受。"), footnote)
			})
		})

		ui.Row(c.Key("p-acts")).Gap(10).Children(func() {
			if k.Button(c.Key("accept"), d.tr("接受"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: busy}).Grow(1).Clicked() {
				if d.live != nil {
					d.approveLive(id, true)
					return
				}
				d.approveDemoPending(id)
			}
			if k.Button(c.Key("deny"), d.tr("拒绝"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Disabled: busy}).Clicked() {
				if d.live != nil {
					d.approveLive(id, false)
					return
				}
				d.rejectDemoPending(id)
			}
		})
		k.Text(c, d.tr("名称由对方自报，未经验证。拒绝后，对方再次请求会重新出现在这里。"), footnote)
	})
}

func (d *viewState) candidateDetail(c *ui.Context, k desktopkit.Kit, id string) {
	i := slices.IndexFunc(d.candidates, func(cd deviceCandidate) bool { return cd.id == id })
	if i < 0 {
		return
	}
	cd := d.candidates[i]
	busy := d.live != nil && d.live.busy
	ui.Column(c.Key("cand-detail")).Gap(14).Children(func() {
		ui.Row(c.Key("c-hero")).Padding(13, 14).Radius(12).Gap(12).AlignItems(ui.Center).
			Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Children(func() {
			d.deviceIconTile(c.Key("ic"), k, cd.icon, 42, true, true)
			ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
				k.Text(c, cd.name, desktopkit.TextProps{Emphasized: true, Truncate: true})
				sub := fmt.Sprintf(d.tr("由 %s 介绍"), cd.introducer)
				if cd.backendID != "" {
					sub += " · " + d.channelDisplayName(cd.backendID)
				}
				k.Text(c, sub, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
			})
		})
		k.Text(c, d.tr("名称未经验证。连接后仍需双方各自批准。"), footnote)
		ui.Row(c.Key("c-acts")).Gap(10).Children(func() {
			if k.Button(c.Key("join"), d.tr("连接"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: busy}).Grow(1).Clicked() {
				if d.live != nil {
					d.pairLive(cd.id, cd.backendID)
					return
				}
				d.candidates = slices.Delete(d.candidates, i, i+1)
				d.ensureDeviceSelection()
				d.setDeviceTask("wait")
			}
			if k.Button(c.Key("ignore"), d.tr("忽略"), desktopkit.ButtonProps{Disabled: busy}).Clicked() {
				if d.live != nil {
					client := d.live.client
					d.mutate(func(ctx context.Context) error { return client.IgnoreCandidate(ctx, cd.id, true) }, "已忽略")
					return
				}
				d.candidates = slices.Delete(d.candidates, i, i+1)
				d.ensureDeviceSelection()
			}
		})
	})
}

func (d *viewState) approveDemoPending(id string) {
	idx := slices.IndexFunc(d.pending, func(p pendingDevice) bool { return p.id == id })
	if idx < 0 {
		return
	}
	p := d.pending[idx]
	d.pending = slices.Delete(d.pending, idx, idx+1)
	if dev := d.deviceByID(p.id); dev != nil && p.id != "local" {
		for _, pp := range p.paths {
			found := false
			for j := range dev.bindings {
				if dev.bindings[j].id == pp.id || dev.bindings[j].instanceID == pp.instanceID {
					dev.bindings[j].state = "authorized"
					dev.bindings[j].connection = "connected"
					dev.bindings[j].lastActive = "刚刚"
					found = true
					break
				}
			}
			if !found {
				impl := pp.instanceID
				if ch := d.syncChannelByID(pp.instanceID); ch != nil && ch.implementationID != "" {
					impl = ch.implementationID
				}
				dev.bindings = append(dev.bindings, deviceBinding{
					id:               fmt.Sprintf("b-%d", len(dev.bindings)+1),
					instanceID:       pp.instanceID,
					implementationID: impl,
					state:            "authorized",
					syncEnabled:      true,
					connection:       "connected",
					lastActive:       "刚刚",
				})
			}
		}
		dev.online = true
		dev.lastSeen = "刚刚"
		d.deviceList.Value, d.deviceDetailOpen = dev.id, true
		return
	}
	d.addDemoPeer("a"+p.id, p.name, p.icon, p.fingerprint, p.paths)
}

func (d *viewState) rejectDemoPending(id string) {
	idx := slices.IndexFunc(d.pending, func(p pendingDevice) bool { return p.id == id })
	if idx < 0 {
		return
	}
	p := d.pending[idx]
	d.pending = slices.Delete(d.pending, idx, idx+1)
	if dev := d.deviceByID(p.id); dev != nil && p.id != "local" {
		dev.bindings = slices.DeleteFunc(dev.bindings, func(b deviceBinding) bool { return b.state == "pending" })
		return
	}
	d.ensureDeviceSelection()
	d.deviceDetailOpen = false
}

func (d *viewState) addDemoPeer(id, name string, icon desktopkit.IconName, fingerprint string, paths []pendingPath) {
	if len(paths) == 0 {
		chID := d.activePairingChannel()
		if chID == "" {
			chID = "builtin.tailcat"
		}
		paths = []pendingPath{{id: "b1", instanceID: chID, state: "pending"}}
	}
	bindings := make([]deviceBinding, 0, len(paths))
	for i, pp := range paths {
		impl := pp.instanceID
		if ch := d.syncChannelByID(pp.instanceID); ch != nil && ch.implementationID != "" {
			impl = ch.implementationID
		}
		bindings = append(bindings, deviceBinding{
			id:               fmt.Sprintf("b-%s-%d", id, i),
			instanceID:       pp.instanceID,
			implementationID: impl,
			state:            "authorized",
			syncEnabled:      true,
			connection:       "connected",
			lastActive:       "刚刚",
		})
	}
	d.devices = append(d.devices, device{
		id:           id,
		name:         name,
		reportedName: name,
		icon:         icon,
		online:       true,
		lastSeen:     "刚刚",
		fingerprint:  fingerprint,
		bindings:     bindings,
	})
	d.deviceList.Value, d.deviceDetailOpen = id, true
}

// ---- 440-DIP Pairing Sheet (Unified Live + Demo) ----

func (d *viewState) pairCodeBoxes(c *ui.Context, k desktopkit.Kit, code string) {
	digits := strings.ReplaceAll(strings.TrimSpace(code), "-", "")
	for len(digits) < 6 {
		digits += "·"
	}
	box := ui.Row(c.Key("code-boxes")).Padding(16, 10).Radius(14).Gap(6).
		Justify(ui.Center).AlignItems(ui.Center).
		Background(deviceInsetBg(k, false)).
		Border(1, deviceCardBorder(k))
	box.Children(func() {
		for i := 0; i < 6; i++ {
			if i == 3 {
				k.Text(c.Key("dash"), "-", desktopkit.TextProps{Tone: desktopkit.ToneTertiary, Emphasized: true}).Padding(0, 2)
			}
			tile := ui.Row(c.Key(i)).Size(44, 54).Radius(10).Justify(ui.Center).AlignItems(ui.Center).
				Background(deviceCardBg(k)).
				Border(1, deviceCardBorderHover(k))
			if !k.Dark {
				tile.Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, .05))
			}
			tile.Children(func() {
				k.Text(c, string(digits[i]), desktopkit.TextProps{Large: true, Monospace: true, Emphasized: true})
			})
		}
	})
}

func (d *viewState) deviceTaskSheet(c *ui.Context, k desktopkit.Kit) {
	open := d.deviceTask != ""
	if d.deviceTask == "phone" {
		d.setDeviceTask("invite")
	}
	closeModal := false
	header := func() {
		ui.Spacer(c)
		cb := ui.ButtonBase(c.Key("close-modal")).Label(d.tr("关闭")).Tooltip(d.tr("关闭")).
			Size(28, 28).Radius(8).Justify(ui.Center).AlignItems(ui.Center).
			FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
		ic := k.Color("foreground-icon-secondary")
		if cb.Hovered() {
			cb.Background(k.Color("background-secondary-hover"))
			ic = k.Color("foreground-icon-primary")
		}
		cb.Children(func() { k.Icon(c, desktopkit.IconDismiss, 15).TextColor(ic).AccessHidden(true) })
		if cb.Clicked() {
			closeModal = true
		}
	}
	done := k.Sheet(c, desktopkit.SheetProps{
		Variant: desktopkit.SheetCompact,
		Open:    &open,
		Title:   d.tr("添加设备"),
		Header:  header,
		OnClosed: func() {
			if d.deviceTask != "" {
				d.setDeviceTask("")
			}
		},
	}, func() {
		if d.deviceTask == "" {
			return
		}
		d.pairingSheetBody(c, k)
	})
	if done || closeModal || (!open && d.deviceTask != "") {
		d.setDeviceTask("")
	}
}

func (d *viewState) pairingSheetBody(c *ui.Context, k desktopkit.Kit) {
	avail := d.runningPairingChannels()
	busy := d.live != nil && d.live.busy
	var targetDev *device
	if d.deviceTaskForID != "" {
		targetDev = d.deviceByID(d.deviceTaskForID)
	}

	// Case 0: Zero running pairing channels
	if len(avail) == 0 {
		desc := d.tr("请先在「设置 → 插件」中启用支持设备配对的同步渠道并重启桌面端。")
		if d.restartPending {
			desc += " " + d.tr("当前存在待重启生效的插件变更。")
		}
		k.ContentUnavailable(c.Key("no-pairing-channel"), desktopkit.ContentUnavailableProps{
			Icon:        desktopkit.IconPackage,
			Title:       d.tr("没有正在运行的可配对同步渠道"),
			Description: desc,
		}, func() {
			if k.Button(c.Key("goto-plugins"), d.tr("前往插件设置"), desktopkit.ButtonProps{Variant: desktopkit.Primary}).Clicked() {
				d.openPluginSettings("")
			}
		})
		return
	}

	// Case 1: Precise inline approval (Live)
	if inline := d.matchingInlinePending(); inline != nil {
		d.approvalTask(c, k, *inline)
		return
	}

	activeCh := d.activePairingChannel()
	showSelector := len(avail) >= 2 || targetDev != nil

	ui.Column(c.Key("pairing-body")).Gap(14).Children(func() {
		if targetDev != nil {
			bg, edge := deviceAccentSurface(k)
			ui.Row(c.Key("add-path-ctx")).Padding(8, 10).Radius(9).Gap(8).AlignItems(ui.Center).
				Background(bg).Border(1, edge).Children(func() {
				k.Icon(c.Key("ic"), desktopkit.IconDevices, 14).TextColor(k.Color("accent-500")).Shrink(0)
				k.Text(c, fmt.Sprintf(d.tr("为「%s」添加同步路径"), targetDev.displayName()), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true})
			})
		}

		if d.live != nil && d.live.message != "" && d.deviceTask != "connect" {
			bg, edge, fg := deviceWarnBannerSurface(k)
			ui.Row(c.Key("sheet-msg")).Padding(9, 11).Radius(10).Gap(8).AlignItems(ui.Center).
				Background(bg).Border(1, edge).Children(func() {
				k.Icon(c.Key("ic"), desktopkit.IconInfo, 14).TextColor(fg).Shrink(0)
				k.Text(c, d.tr(d.live.message), desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(fg)
			})
		}

		if d.deviceTask == "invite" || d.deviceTask == "connect" {
			tab := d.deviceTask
			if k.SegmentedControl(c.Key("add-tab"), desktopkit.SegmentedControlProps{
				Label: d.tr("添加设备"),
				Value: &tab,
				Items: []desktopkit.SegmentedItem{
					{Value: "invite", Label: d.tr("出示配对码")},
					{Value: "connect", Label: d.tr("输入配对码")},
				},
			}).Changed {
				d.setDeviceTask(tab)
			}
		}

		if showSelector && (d.deviceTask == "invite" || d.deviceTask == "connect") {
			items := make([]desktopkit.SelectItem, 0, len(avail))
			for _, ch := range avail {
				items = append(items, desktopkit.SelectItem{Value: ch.id, Label: d.channelSelectLabel(ch)})
			}
			selected := activeCh
			ui.Column(c.Key("ch-select-wrap")).Gap(4).Children(func() {
				k.Text(c.Key("lbl"), d.tr("同步渠道"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
				if k.Select(c.Key("pairing-channel"), desktopkit.SelectProps{
					Label:    d.tr("同步渠道"),
					Value:    &selected,
					Items:    items,
					Disabled: busy,
				}).Changed {
					d.setActivePairingChannel(selected, d.deviceTask == "invite")
					activeCh = selected
				}
				if d.deviceTask == "connect" {
					k.Text(c.Key("hint"), d.tr("远端同步渠道名可以不同，但需使用相同的传输插件。"), footnote)
				}
			})
		}

		switch d.deviceTask {
		case "invite":
			ticket := d.inviteMode == "ticket"
			intro := d.tr("在另一台设备的 UniClipboard 中输入下方一次性配对码，随后核对指纹即可完成关联。")
			if targetDev != nil {
				intro = fmt.Sprintf(d.tr("在「%s」上输入下方配对码或票据，核对指纹后将为其新增此同步路径。"), targetDev.displayName())
			}
			k.Text(c, intro, secondary)

			if !ticket {
				d.pairCodeBoxes(c, k, d.inviteCode)
				ui.Row(c.Key("meta")).AlignItems(ui.Center).Gap(6).Children(func() {
					ui.Box(c.Key("dot")).Size(7, 7).Radius(3.5).Background(k.Color("accent-500")).Shrink(0)
					if d.live != nil {
						remaining := max(int(time.Until(time.UnixMilli(d.live.code.ExpiresAtMs)).Seconds()), 0)
						k.Text(c, fmt.Sprintf(d.tr("%d 秒内有效，仅可使用一次。"), remaining), footnote).Grow(1)
					} else {
						k.Text(c, d.tr("300 秒内有效 · 仅限一次"), footnote).Grow(1)
					}
					if k.Button(c.Key("copy-code"), d.tr("复制"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy, Disabled: d.inviteCode == ""}).Clicked() {
						d.copyDeviceText(d.inviteCode, "已复制配对码")
					}
					if k.Button(c.Key("regen"), d.tr("刷新"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconRetry, Disabled: busy}).Clicked() {
						if d.live != nil {
							d.newLiveCode()
						} else {
							d.inviteCode, d.deviceNote = newPairCode(), ""
						}
					}
				})
			} else {
				ticketText := "unic-pair:AQ7xK2m9pR4vN8qL…"
				if d.live != nil {
					ticketText = d.live.ticket
				}
				ui.Column(c.Key("ticket-box")).Padding(12, 14).Radius(12).Gap(10).
					Background(deviceInsetBg(k, false)).
					Border(1, deviceCardBorder(k)).Children(func() {
					if ticketText != "" {
						k.Text(c, ticketText, desktopkit.TextProps{Monospace: true, Truncate: true})
					}
					ui.Row(c.Key("ticket-act")).Justify(ui.End).Children(func() {
						if k.Button(c.Key("copy-ticket"), d.tr("复制票据"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy, Disabled: ticketText == ""}).Clicked() {
							d.copyDeviceText(ticketText, "已复制完整票据")
						}
					})
				})
			}

			ui.Row(c.Key("ticket-toggle")).Justify(ui.Center).Children(func() {
				toggleLabel := d.tr("跨网或命令行配对？改用完整票据 →")
				if ticket {
					toggleLabel = d.tr("← 返回 6 位配对码")
				}
				if k.Button(c.Key("toggle-mode"), toggleLabel, desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
					if ticket {
						d.inviteMode = "code"
					} else {
						d.inviteMode = "ticket"
					}
					d.deviceNote = ""
				}
			})

			if d.deviceNote != "" {
				k.Text(c, d.tr(d.deviceNote), footnote)
			}

			if !showSelector || d.live == nil {
				ui.Box(c.Key("sep")).Height(1).Background(k.Color("separator-border"))
				ui.Row(c.Key("foot")).AlignItems(ui.Center).Gap(8).Children(func() {
					footLabel := d.tr("配对全程端到端加密")
					if !showSelector && activeCh != "" {
						footLabel = fmt.Sprintf(d.tr("通过 %s · 端到端加密"), d.channelDisplayName(activeCh))
					}
					k.Text(c, footLabel, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).Grow(1)
					if d.live == nil {
						if k.Button(c.Key("sim"), d.tr("模拟对方加入"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() {
							chID := activeCh
							if chID == "" {
								chID = "builtin.tailcat"
							}
							if targetDev != nil {
								d.pending = append(d.pending, pendingDevice{
									id: targetDev.id, name: targetDev.displayName(), via: "配对码 " + d.inviteCode,
									byCode: true, icon: targetDev.icon, fingerprint: targetDev.fingerprint,
									paths: []pendingPath{{id: "bp-sim", instanceID: chID, state: "pending"}},
								})
								d.deviceList.Value, d.deviceDetailOpen = targetDev.id, true
							} else {
								id := fmt.Sprintf("p%d", len(d.pending)+len(d.devices)+1)
								d.pending = append(d.pending, pendingDevice{
									id: id, name: "新电脑", via: "配对码 " + d.inviteCode,
									byCode: true, icon: desktopkit.IconDesktop, fingerprint: "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D",
									paths: []pendingPath{{id: "bp-" + id, instanceID: chID, state: "pending"}},
								})
								d.deviceList.Value, d.deviceDetailOpen = "pending:"+id, true
							}
							d.setDeviceTask("")
						}
					}
				})
			}

		case "connect":
			kind := connectKind(d.connectInput)
			hint := d.tr("支持 6 位配对码（NNN-NNN）或以 unic-pair: 开头的完整票据。")
			if kind != "" {
				hint = d.tr("识别为：") + d.tr(kind)
			}
			k.Text(c, d.tr("输入对方提供的配对码或完整票据。"), secondary)
			r := k.FormField(c, desktopkit.FormFieldProps{
				Label:       d.tr("配对码或票据"),
				Value:       &d.connectInput,
				Placeholder: d.tr("482-913 或 unic-pair:…"),
				Hint:        hint,
				Error:       d.tr(d.fieldError),
				AutoFocus:   true,
				Disabled:    busy,
			})
			if r.Edited {
				d.fieldError = ""
			}
			submit := (r.Submitted && kind != "" && !busy) || k.Button(c.Key("go"), d.tr("连接设备"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: kind == "" || busy}).FillWidth().Clicked()
			if submit {
				if d.live != nil {
					d.pairLive(strings.TrimSpace(d.connectInput), "")
				} else {
					raw := strings.ReplaceAll(strings.TrimSpace(d.connectInput), "-", "")
					switch raw {
					case "000000":
						d.fieldError = "配对码或票据已过期，请让对方刷新后重试。"
					case "999999":
						d.fieldError = "当前选择的本地同步渠道与该票据不兼容，请切换为对应的同步渠道。"
					default:
						d.setDeviceTask("wait")
					}
				}
			}
			if !showSelector && activeCh != "" {
				k.Text(c, fmt.Sprintf(d.tr("通过 %s · 端到端加密"), d.channelDisplayName(activeCh)), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
			}

		case "wait":
			bg, edge := deviceAccentSurface(k)
			ui.Row(c.Key("banner")).Padding(12, 14).Radius(12).Gap(10).AlignItems(ui.Center).Background(bg).Border(1, edge).Children(func() {
				k.Icon(c, desktopkit.IconShield, 16).TextColor(k.Color("accent-500")).Shrink(0)
				ui.Column(c.Key("txt")).Gap(2).Grow(1).MinWidth(0).Children(func() {
					k.Text(c, d.tr("等待双方批准"), desktopkit.TextProps{Emphasized: true})
					k.Text(c, d.tr("在另一台设备核对指纹。批准本机收到的请求后，设备才会关联。"), footnote)
				})
			})
			fp := "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D"
			if d.live != nil {
				fp = ""
				if d.live.join != nil {
					fp = d.live.join.Fingerprint
				}
			}
			if fp != "" {
				d.fingerprintGrid(c, k, fp, false)
			}
			ui.Box(c.Key("sep")).Height(1).Background(k.Color("separator-border"))
			ui.Row(c.Key("wait-foot")).Gap(8).AlignItems(ui.Center).Children(func() {
				if k.Button(c.Key("back"), d.tr("返回"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconBack}).Clicked() {
					d.setDeviceTask("connect")
				}
				if d.live == nil {
					ui.Spacer(c)
					if k.Button(c.Key("reject"), d.tr("模拟对方拒绝"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() {
						d.setDeviceTask("connect")
						d.fieldError = "对方拒绝了请求，或网络不通。请检查后重试。"
					}
					if k.Button(c.Key("ok"), d.tr("模拟对方批准"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
						d.addDemoPeer(fmt.Sprintf("joined%d", len(d.devices)), "对方设备", desktopkit.IconDesktop, fp, nil)
						d.setDeviceTask("")
					}
				}
			})
		}
	})
}

// approvalTask shows the fingerprint approval inside the pairing task, so the user need not leave it.
func (d *viewState) approvalTask(c *ui.Context, k desktopkit.Kit, p pendingDevice) {
	busy := d.live != nil && d.live.busy
	existing := d.isExistingPendingDevice(p.id)
	ui.Column(c.Key("live-approval")).Gap(14).Children(func() {
		bg, edge := deviceAccentSurface(k)
		ui.Row(c.Key("banner")).Padding(12, 14).Radius(12).Gap(10).AlignItems(ui.Center).Background(bg).Border(1, edge).Children(func() {
			k.Icon(c, desktopkit.IconShield, 16).TextColor(k.Color("accent-500")).Shrink(0)
			ui.Column(c.Key("txt")).Gap(2).Grow(1).MinWidth(0).Children(func() {
				k.Text(c, d.tr("核对并批准"), desktopkit.TextProps{Emphasized: true})
				sub := p.name + " · " + d.tr("对方屏幕上显示相同的指纹时，才接受。")
				if existing {
					sub = p.name + " · " + d.tr("请求新增同步路径，核对指纹一致后才接受。")
				}
				k.Text(c, sub, footnote)
			})
		})
		if pathNames := d.pendingPathNames(p); pathNames != "" {
			ui.Row(c.Key("paths")).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
				k.Text(c.Key("lbl"), d.tr("同步渠道："), footnote)
				d.inlineBadge(c.Key("ch"), k, pathNames, desktopkit.StatusInfo)
			})
		}
		d.fingerprintGrid(c, k, p.fingerprint, false)
		k.Text(c, d.tr("名称由对方自报，未经验证。"), footnote)
		ui.Row(c.Key("acts")).Gap(8).Justify(ui.End).Children(func() {
			if k.Button(c.Key("deny"), d.tr("拒绝"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Size: desktopkit.SizeSmall, Disabled: busy}).Clicked() {
				if d.live != nil {
					if existing {
						d.rejectLivePendingPaths(p.id)
					} else {
						d.approveLive(p.id, false)
					}
				} else {
					d.rejectDemoPending(p.id)
				}
				d.setDeviceTask("")
				return
			}
			if k.Button(c.Key("accept"), d.tr("接受"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall, Disabled: busy}).Clicked() {
				if d.live != nil {
					d.approveLive(p.id, true)
				} else {
					d.approveDemoPending(p.id)
					d.setDeviceTask("")
				}
			}
		})
	})
}

var (
	secondary = desktopkit.TextProps{Tone: desktopkit.ToneSecondary}
	footnote  = desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}
)
