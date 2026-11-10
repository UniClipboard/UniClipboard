package desktopdemo

import (
	"fmt"
	"math/rand/v2"
	"regexp"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
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

// latencyTone grades a round-trip time.
func latencyTone(ms int) uikit.StatusTone {
	if ms < 50 {
		return uikit.StatusPositive
	}
	return uikit.StatusWarning
}

func deviceStatus(v device) (string, uikit.StatusTone) {
	switch {
	case v.id == "local":
		return "本机", uikit.StatusPositive
	case v.direction == "pause":
		return "已暂停", uikit.StatusNeutral
	case v.online:
		return fmt.Sprintf("%d ms · %s", v.latency, v.link), latencyTone(v.latency)
	}
	return "离线", uikit.StatusNeutral
}

// policySummary states the result of the direction and content-type choices in one sentence.
func policySummary(v device) string {
	if v.direction == "pause" {
		return "同步已暂停。信任和路径保留，不传输任何内容。"
	}
	var kinds []string
	for _, k := range []struct {
		on   bool
		name string
	}{{v.text, "文字"}, {v.image, "图片"}, {v.file, "文件"}} {
		if k.on {
			kinds = append(kinds, k.name)
		}
	}
	if len(kinds) == 0 {
		return "未选择任何内容类型，不会同步。"
	}
	way := map[string]string{"send": "仅发送到此设备", "recv": "仅从此设备接收"}[v.direction]
	if way == "" {
		way = "双向同步"
	}
	return strings.Join(kinds, "、") + "：" + way + "。"
}

func (d *demo) deviceByID(id string) *device {
	for i := range d.devices {
		if d.devices[i].id == id {
			return &d.devices[i]
		}
	}
	return nil
}

func (d *demo) setDeviceTask(task string) {
	d.deviceTask, d.fieldError, d.deviceNote, d.approveChecked = task, "", "", false
	switch task {
	case "invite":
		d.inviteCode, d.inviteMode = newPairCode(), "code"
	case "connect":
		d.connectInput = ""
	}
}

func (d *demo) deviceSheet(c *ui.Context, k uikit.Kit) {
	variant := uikit.SheetManagement
	if d.deviceTask != "" {
		variant = uikit.SheetTask
	}
	selected := d.deviceList.Value
	props := uikit.SheetProps{Open: &d.devicesOpen, Variant: variant, Title: "管理设备", Done: "完成", OnClosed: func() {
		d.setDeviceTask("")
		d.renaming = false
	}}
	if d.deviceTask == "" {
		props.Footer = func() {
			_, action := k.Menu(c.Key("add"), uikit.MenuProps{Label: "添加设备", Icon: uikit.IconAdd, IconOnly: true, Items: []uikit.MenuEntry{
				{ID: "invite", Label: "邀请设备（配对码）", Icon: uikit.IconAdd},
				{ID: "connect", Label: "连接设备（配对码或地址）", Icon: uikit.IconGlobe},
				{ID: "phone", Label: "连接手机", Icon: uikit.IconPhone},
			}})
			if action != "" {
				d.setDeviceTask(action)
			}
			removable := selected != "local" && d.deviceByID(selected) != nil
			if k.Button(c.Key("remove"), "", uikit.ButtonProps{Icon: uikit.IconRemove, Label: "移除设备", Disabled: !removable}).Clicked() {
				d.confirm, d.confirmID = "移除"+d.sourceName(selected)+"？", "device:"+selected
			}
		}
	}
	done := k.Sheet(c, props, func() {
		if d.deviceTask != "" {
			d.deviceTaskView(c, k)
			return
		}
		k.MasterDetail(c, uikit.MasterDetailProps{DetailKey: selected, DetailLabel: "设备详情"}, func() { d.deviceSidebar(c, k) }, func() { d.deviceDetail(c, k) })
	})
	if done {
		d.devicesOpen = false
	}
}

func (d *demo) deviceSidebar(c *ui.Context, k uikit.Kit) {
	var sections []uikit.SourceListSection
	if len(d.pending) > 0 {
		var items []uikit.SourceListItem
		for _, p := range d.pending {
			items = append(items, uikit.SourceListItem{ID: "pending:" + p.id, Title: p.name, Subtitle: "请求加入 · " + p.via, Icon: p.icon, Pending: true})
		}
		sections = append(sections, uikit.SourceListSection{ID: "pending", Title: fmt.Sprintf("待批准 · %d", len(items)), Items: items})
	}
	var local, peers []uikit.SourceListItem
	for _, v := range d.devices {
		text, tone := deviceStatus(v)
		item := uikit.SourceListItem{ID: v.id, Title: v.name, Subtitle: text, Icon: v.icon, Status: &tone, Dimmed: !v.online}
		if v.id == "local" {
			local = append(local, item)
		} else {
			peers = append(peers, item)
		}
	}
	// Online devices first; keep the fixture order within each group.
	slices.SortStableFunc(peers, func(a, b uikit.SourceListItem) int {
		return btoi(b.Dimmed) - btoi(a.Dimmed)
	})
	sections = append(sections, uikit.SourceListSection{ID: "local", Title: "本机", Items: local})
	sections = append(sections, uikit.SourceListSection{ID: "peers", Title: "已关联设备", Items: peers, Placeholder: "邀请设备"})
	if len(d.candidates) > 0 {
		var items []uikit.SourceListItem
		for _, cd := range d.candidates {
			items = append(items, uikit.SourceListItem{ID: "cand:" + cd.id, Title: cd.name, Subtitle: cd.introducer, Icon: cd.icon, Pending: true})
		}
		sections = append(sections, uikit.SourceListSection{ID: "candidates", Title: "可能认识", Items: items})
	}
	k.SourceList(c, &d.deviceList, uikit.SourceListProps{Label: "设备", Sections: sections, Footer: "演示空间 · 端到端加密"}).Grow(1)
	if kind, _ := d.deviceList.Command(); kind == uikit.SourcePlaceholder {
		d.setDeviceTask("invite")
	}
}

func btoi(b bool) int {
	if b {
		return 1
	}
	return 0
}

func (d *demo) deviceDetail(c *ui.Context, k uikit.Kit) {
	id := d.deviceList.Value
	switch {
	case strings.HasPrefix(id, "pending:"):
		d.pendingDetail(c, k, strings.TrimPrefix(id, "pending:"))
		return
	case strings.HasPrefix(id, "cand:"):
		d.candidateDetail(c, k, strings.TrimPrefix(id, "cand:"))
		return
	}
	v := d.deviceByID(id)
	if v == nil {
		k.ContentUnavailable(c, uikit.ContentUnavailableProps{Icon: uikit.IconDevices, Title: "还没有关联设备", Description: "邀请或连接另一台设备，开始同步剪贴板。"}, func() {
			if k.Button(c, "邀请设备", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				d.setDeviceTask("invite")
			}
		})
		return
	}
	ui.Column(c).Gap(20).Children(func() {
		state, tone := "本机", uikit.StatusPositive
		if v.id != "local" {
			state = "离线"
			tone = uikit.StatusNeutral
			if v.online {
				state, tone = "在线", latencyTone(v.latency)
			}
			if v.direction == "pause" {
				state, tone = "同步已暂停", uikit.StatusNeutral
			}
		}
		k.EntityHeader(c, uikit.EntityHeaderProps{Icon: v.icon, Title: v.name, Centered: true, Status: uikit.EntityStatus{Text: state, Tone: tone}}, nil)
		if v.id == "local" {
			k.SettingsGroup(c, uikit.SettingsGroupProps{}, func() {
				k.SettingsRow(c.Key("os"), uikit.SettingsRowProps{Label: "系统"}, nil, func() { k.Text(c, v.platform, secondary) })
			})
			k.Text(c, "同步策略在其他设备上设置，用来控制它与本机之间的同步。", footnote)
			return
		}
		if !d.options["sync"] {
			k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: "全局同步已关闭", Text: "设备策略暂不生效。"})
		}
		if d.renaming {
			r := k.FormField(c, uikit.FormFieldProps{Label: "设备名称", Value: &d.renameValue, AutoFocus: true, Hint: "只改本机显示的名称。对方自报的名称单独保存。"})
			if r.Submitted && strings.TrimSpace(d.renameValue) != "" {
				v.name, d.renaming = strings.TrimSpace(d.renameValue), false
			}
			if r.Escaped {
				d.renaming = false
			}
		}
		k.SettingsGroup(c, uikit.SettingsGroupProps{}, func() {
			k.SettingsRow(c.Key("name"), uikit.SettingsRowProps{Label: "名称"}, nil, func() {
				ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
					k.Text(c, v.name, secondary)
					if k.Button(c.Key("rename"), "", uikit.ButtonProps{Variant: uikit.Subtle, Icon: uikit.IconEdit, Label: "重命名"}).Clicked() {
						d.renaming, d.renameValue = true, v.name
					}
				})
			})
			k.SettingsRow(c.Key("os"), uikit.SettingsRowProps{Label: "系统"}, nil, func() { k.Text(c, v.platform, secondary) })
			conn := "未连接"
			if v.online {
				conn = fmt.Sprintf("%s · %d ms", v.link, v.latency)
			}
			k.SettingsRow(c.Key("conn"), uikit.SettingsRowProps{Label: "连接"}, nil, func() { k.Text(c, conn, secondary) })
			k.SettingsRow(c.Key("seen"), uikit.SettingsRowProps{Label: "最近活动"}, nil, func() { k.Text(c, v.lastSeen, secondary) })
		})
		d.policyGroup(c, k, v)
		k.SettingsGroup(c, uikit.SettingsGroupProps{Title: "连接路径"}, func() {
			for i, p := range v.paths {
				pt := secondary
				if p.failed {
					pt.Tone = uikit.ToneCritical
				}
				k.SettingsRow(c.Key(fmt.Sprint("path", i)), uikit.SettingsRowProps{Label: p.name, Description: "最近同步 " + p.lastSync}, nil, func() { k.Text(c, p.state, pt) })
			}
		})
		k.SettingsGroup(c, uikit.SettingsGroupProps{Title: "安全"}, func() {
			k.SettingsRow(c.Key("fp"), uikit.SettingsRowProps{Label: "设备指纹", Description: "与对方屏幕上的指纹比对"}, nil, func() {
				if k.Button(c, "复制", uikit.ButtonProps{Variant: uikit.Subtle, Icon: uikit.IconCopy}).Clicked() {
					d.deviceNote = "指纹已复制（演示，未写入剪贴板）"
				}
			})
		})
		k.Text(c, v.fingerprint, uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary, Monospace: true})
		if d.deviceNote != "" {
			k.Text(c, d.deviceNote, footnote)
		}
	})
}

var (
	secondary = uikit.TextProps{Tone: uikit.ToneSecondary}
	footnote  = uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary}
)

func (d *demo) policyGroup(c *ui.Context, k uikit.Kit, v *device) {
	k.SettingsGroup(c, uikit.SettingsGroupProps{Title: "同步"}, func() {
		k.SettingsRow(c.Key("direction"), uikit.SettingsRowProps{Label: "同步方式"}, nil, func() {
			k.Select(c, uikit.SelectProps{Label: "同步方式", Value: &v.direction, Items: []uikit.SelectItem{
				{Value: "both", Label: "双向同步"}, {Value: "send", Label: "仅发送到此设备"}, {Value: "recv", Label: "仅从此设备接收"}, {Value: "pause", Label: "暂停同步"},
			}})
		})
		off := v.direction == "pause"
		k.SettingsRow(c.Key("text"), uikit.SettingsRowProps{Label: "文字"}, nil, func() {
			k.Switch(c, &v.text, uikit.SwitchProps{Label: "同步文字", LabelHidden: true, Disabled: off})
		})
		k.SettingsRow(c.Key("image"), uikit.SettingsRowProps{Label: "图片"}, nil, func() {
			k.Switch(c, &v.image, uikit.SwitchProps{Label: "同步图片", LabelHidden: true, Disabled: off})
		})
		k.SettingsRow(c.Key("file"), uikit.SettingsRowProps{Label: "文件", Description: "受全局 100 MB 上限限制"}, nil, func() {
			k.Switch(c, &v.file, uikit.SwitchProps{Label: "同步文件", LabelHidden: true, Disabled: off})
		})
	})
	k.Text(c, policySummary(*v), footnote)
}

func (d *demo) pendingDetail(c *ui.Context, k uikit.Kit, id string) {
	i := slices.IndexFunc(d.pending, func(p pendingDevice) bool { return p.id == id })
	if i < 0 {
		return
	}
	p := d.pending[i]
	ui.Column(c).Gap(20).Children(func() {
		k.EntityHeader(c, uikit.EntityHeaderProps{Icon: p.icon, Pending: true, Centered: true, Title: p.name, Status: uikit.EntityStatus{Text: "等待你批准", Tone: uikit.StatusInfo}}, func() {
			if k.Button(c, "核对指纹…", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				d.setDeviceTask("approve")
				d.approveID = p.id
			}
		})
		k.SettingsGroup(c, uikit.SettingsGroupProps{}, func() {
			k.SettingsRow(c.Key("os"), uikit.SettingsRowProps{Label: "系统"}, nil, func() { k.Text(c, p.platform, secondary) })
			k.SettingsRow(c.Key("via"), uikit.SettingsRowProps{Label: "来路"}, nil, func() { k.Text(c, p.via, secondary) })
		})
		k.Text(c, "名称由对方自报，未经验证。批准前必须核对指纹。", footnote)
	})
}

func (d *demo) candidateDetail(c *ui.Context, k uikit.Kit, id string) {
	i := slices.IndexFunc(d.candidates, func(cd deviceCandidate) bool { return cd.id == id })
	if i < 0 {
		return
	}
	cd := d.candidates[i]
	ui.Column(c).Gap(20).Children(func() {
		k.EntityHeader(c, uikit.EntityHeaderProps{Icon: cd.icon, Pending: true, Centered: true, Title: cd.name, Description: cd.introducer}, func() {
			if k.Button(c.Key("join"), "连接", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				d.candidates = slices.Delete(d.candidates, i, i+1)
				d.deviceList.Value = "local"
				d.setDeviceTask("wait")
			}
			if k.Button(c.Key("ignore"), "忽略", uikit.ButtonProps{}).Clicked() {
				d.candidates = slices.Delete(d.candidates, i, i+1)
				d.deviceList.Value = "local"
			}
		})
		k.Text(c, "名称未经验证。连接后仍需双方各自批准。", footnote)
	})
}

func (d *demo) addPeer(id, name, platform string, icon uikit.IconName, fingerprint string) {
	d.devices = append(d.devices, device{id: id, name: name, platform: platform, icon: icon, online: true, latency: 22, link: "局域网", lastSeen: "刚刚",
		direction: "both", text: true, image: true, file: true, fingerprint: fingerprint,
		paths: []devicePath{{name: "tailcat · 默认", state: "已连接", lastSync: "—"}}})
	d.deviceList.Value = id
}

func (d *demo) deviceTaskView(c *ui.Context, k uikit.Kit) {
	const back = "返回设备列表"
	switch d.deviceTask {
	case "invite":
		ticket := d.inviteMode == "ticket"
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: "邀请设备", Description: "在另一台设备上输入配对码。对方加入后，你仍需核对指纹并批准。", BackLabel: back}, func() {
			mode := d.inviteMode
			if k.SegmentedControl(c, uikit.SegmentedControlProps{Label: "邀请方式", Value: &mode, Items: []uikit.SegmentedItem{{Value: "code", Label: "配对码"}, {Value: "ticket", Label: "完整票据"}}}).Changed {
				d.inviteMode = mode
			}
			if ticket {
				k.Text(c, "unic-pair:AQ7xK2…", uikit.TextProps{Monospace: true, Truncate: true})
				if k.Button(c, "复制票据", uikit.ButtonProps{Icon: uikit.IconCopy}).Clicked() {
					d.deviceNote = "票据已复制（演示，未写入剪贴板）"
				}
			} else {
				k.Text(c, d.inviteCode, uikit.TextProps{Large: true, Monospace: true, Emphasized: true})
			}
			k.Text(c, "300 秒内有效，仅可使用一次。重新生成会让旧码立即失效。", uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
			if d.deviceNote != "" {
				k.Text(c, d.deviceNote, uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
			}
		}, func() {
			if k.Button(c.Key("regen"), "重新生成", uikit.ButtonProps{Icon: uikit.IconRetry}).Clicked() {
				d.inviteCode, d.deviceNote = newPairCode(), ""
			}
			if k.Button(c.Key("sim"), "模拟对方加入", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				id := fmt.Sprintf("p%d", len(d.pending)+len(d.devices)+1)
				d.pending = append(d.pending, pendingDevice{id: id, name: "新电脑", platform: "Windows 11", via: "配对码 " + d.inviteCode, icon: uikit.IconDesktop, fingerprint: "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D"})
				d.deviceList.Value = "pending:" + id
				d.setDeviceTask("")
			}
		}) {
			d.setDeviceTask("")
		}
	case "connect":
		kind := connectKind(d.connectInput)
		hint := "支持 6 位配对码（NNN-NNN）或以 unic-pair: 开头的完整票据。"
		if kind != "" {
			hint = "识别为：" + kind
		}
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: "连接设备", Description: "输入对方提供的配对码或完整票据。", BackLabel: back}, func() {
			r := k.FormField(c, uikit.FormFieldProps{Label: "配对码或票据", Value: &d.connectInput, Placeholder: "482-913 或 unic-pair:…", Hint: hint, Error: d.fieldError, AutoFocus: true})
			if r.Edited {
				d.fieldError = ""
			}
		}, func() {
			if k.Button(c.Key("go"), "连接", uikit.ButtonProps{Variant: uikit.Primary, Disabled: kind == ""}).Clicked() {
				if strings.ReplaceAll(strings.TrimSpace(d.connectInput), "-", "") == "000000" {
					d.fieldError = "配对码无效或已过期。"
				} else {
					d.setDeviceTask("wait")
				}
			}
		}) {
			d.setDeviceTask("")
		}
	case "wait":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: "等待对方批准", Description: "在对方设备上核对下方指纹，然后批准。", BackLabel: back}, func() {
			k.Text(c, "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D", uikit.TextProps{Monospace: true, Emphasized: true})
		}, func() {
			if k.Button(c.Key("reject"), "模拟对方拒绝", uikit.ButtonProps{}).Clicked() {
				d.setDeviceTask("connect")
				d.fieldError = "对方拒绝了请求，或网络不通。请检查后重试。"
			}
			if k.Button(c.Key("ok"), "模拟对方批准", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				d.addPeer(fmt.Sprintf("joined%d", len(d.devices)), "对方设备", "macOS 15", uikit.IconDesktop, "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D")
				d.setDeviceTask("")
			}
		}) {
			d.setDeviceTask("")
		}
	case "approve":
		i := slices.IndexFunc(d.pending, func(p pendingDevice) bool { return p.id == d.approveID })
		if i < 0 {
			d.setDeviceTask("")
			return
		}
		p := d.pending[i]
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: p.name + " 请求加入", Description: p.platform + " · 名称由对方自报，未经验证 · " + p.via, BackLabel: back}, func() {
			k.Text(c, p.fingerprint, uikit.TextProps{Monospace: true, Emphasized: true})
			k.Checkbox(c, &d.approveChecked, uikit.CheckboxProps{Label: "我已在对方设备上核对，指纹完全一致"})
			k.Text(c, "拒绝等同于移除：对方再次拨入会重新出现在待批准里。", uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
		}, func() {
			if k.Button(c.Key("deny"), "拒绝", uikit.ButtonProps{Variant: uikit.Destructive}).Clicked() {
				d.pending = slices.Delete(d.pending, i, i+1)
				d.deviceList.Value = "local"
				d.setDeviceTask("")
				return
			}
			if k.Button(c.Key("approve"), "批准", uikit.ButtonProps{Variant: uikit.Primary, Disabled: !d.approveChecked}).Clicked() {
				d.pending = slices.Delete(d.pending, i, i+1)
				d.addPeer("a"+p.id, p.name, p.platform, p.icon, p.fingerprint)
				d.setDeviceTask("")
			}
		}) {
			d.setDeviceTask("")
		}
	case "phone":
		if k.TaskPanel(c, uikit.TaskPanelProps{Title: "连接手机", Description: "在手机上完成以下步骤。", BackLabel: back}, func() {
			r := k.StepList(c, uikit.StepListProps{Label: "连接手机步骤", Steps: []uikit.StepListStep{
				{ID: "install", Title: "安装 UniClipboard", Description: "打开手机端应用。", Done: true},
				{ID: "network", Title: "连接同一局域网", Description: "手机与电脑连接同一网络。", Done: true},
				{ID: "code", Title: "在手机上输入配对码", Description: "先在这里生成配对码。", Action: &uikit.StepAction{Label: "获取配对码", Primary: true}},
			}})
			if r.Clicked == "code" {
				d.setDeviceTask("invite")
			}
		}, nil) {
			d.setDeviceTask("")
		}
	}
}
