package desktopdemo

import (
	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

func (d *demo) deviceSheet(c *ui.Context, k uikit.Kit) {
	variant := uikit.SheetManagement
	if d.deviceTask != "" {
		variant = uikit.SheetTask
	}
	done := k.Sheet(c, uikit.SheetProps{Open: &d.devicesOpen, Variant: variant, Title: "管理设备", Done: "完成", OnClosed: func() { d.deviceTask, d.fieldError = "", "" }, Footer: func() {
		if d.deviceTask != "" {
			if k.Button(c, "返回设备列表", uikit.ButtonProps{Icon: uikit.IconBack}).Clicked() {
				d.deviceTask, d.fieldError = "", ""
			}
			return
		}
		_, action := k.Menu(c.Key("add"), uikit.MenuProps{Label: "添加设备", Icon: uikit.IconAdd, IconOnly: true, Items: []uikit.MenuEntry{{ID: "invite", Label: "邀请设备", Icon: uikit.IconAdd}, {ID: "phone", Label: "连接手机", Icon: uikit.IconPhone}, {ID: "join", Label: "加入其他空间", Icon: uikit.IconDevices}}})
		if action != "" {
			d.deviceTask = action
		}
		if k.Button(c.Key("remove"), "", uikit.ButtonProps{Icon: uikit.IconRemove, Label: "移除设备", Disabled: d.deviceList.Value == "local"}).Clicked() {
			d.confirm, d.confirmID = "移除"+d.sourceName(d.deviceList.Value)+"？", "device:"+d.deviceList.Value
		}
	}}, func() {
		if d.deviceTask != "" {
			d.deviceTaskView(c, k)
			return
		}
		k.MasterDetail(c, uikit.MasterDetailProps{DetailKey: d.deviceList.Value, DetailLabel: "设备详情"}, func() {
			local := []uikit.SourceListItem{}
			peers := []uikit.SourceListItem{}
			for _, v := range d.devices {
				tone := uikit.StatusNeutral
				if v.online {
					tone = uikit.StatusPositive
				}
				item := uikit.SourceListItem{ID: v.id, Title: v.name, Subtitle: v.status, Icon: v.icon, Status: &tone, Dimmed: !v.online}
				if v.id == "local" {
					local = append(local, item)
				} else {
					peers = append(peers, item)
				}
			}
			k.SourceList(c, &d.deviceList, uikit.SourceListProps{Label: "设备", Sections: []uikit.SourceListSection{{ID: "local", Title: "本机", Items: local}, {ID: "peers", Title: "已关联设备", Items: peers}}, Footer: "演示空间 · 端到端加密"}).Grow(1)
		}, func() {
			for i := range d.devices {
				v := &d.devices[i]
				if v.id != d.deviceList.Value {
					continue
				}
				badge := ""
				if v.id == "local" {
					badge = "本机"
				}
				tone := uikit.StatusNeutral
				if v.online {
					tone = uikit.StatusPositive
				}
				k.EntityHeader(c, uikit.EntityHeaderProps{Icon: v.icon, Title: v.name, Badge: badge, Status: uikit.EntityStatus{Text: v.status, Tone: tone}, Description: v.platform}, nil)
				k.FactList(c, uikit.FactListProps{Label: "设备信息", Facts: []uikit.Fact{{ID: "system", Label: "操作系统", Value: v.platform}, {ID: "seen", Label: "最近活动", Value: "今天 10:42"}}})
				k.SettingsGroup(c, uikit.SettingsGroupProps{Title: "同步偏好"}, func() {
					k.SettingsRow(c.Key("send"), uikit.SettingsRowProps{Label: "向此设备发送", Description: "自动发送新复制的内容"}, nil, func() { k.Switch(c, &v.send, uikit.SwitchProps{Label: "向此设备发送"}) })
					k.SettingsRow(c.Key("receive"), uikit.SettingsRowProps{Label: "从此设备接收", Description: "接收后自动写入剪贴板"}, nil, func() { k.Switch(c, &v.receive, uikit.SwitchProps{Label: "从此设备接收"}) })
				})
				k.SettingsGroup(c, uikit.SettingsGroupProps{Title: "安全"}, func() {
					k.SettingsRow(c, uikit.SettingsRowProps{Label: "设备指纹", Description: "DEMO 8A42 7C91 2F60"}, nil, func() { k.Icon(c, uikit.IconShield, 18) })
				})
			}
		})
	})
	if done {
		d.devicesOpen = false
	}
}

func (d *demo) deviceTaskView(c *ui.Context, k uikit.Kit) {
	ui.Column(c).Padding(16).Gap(16).Children(func() {
		switch d.deviceTask {
		case "invite":
			k.EntityHeader(c, uikit.EntityHeaderProps{Icon: uikit.IconAdd, Pending: true, Title: "邀请设备", Description: "在另一台设备上输入邀请信息。此处仅展示假数据。"}, nil)
			k.Text(c, "DEMO-123-456", uikit.TextProps{Role: uikit.RoleTitle, Emphasized: true}).Font("monospace")
			k.Text(c, "剩余 04:32 · 仅可使用一次", uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
			if k.Button(c, "模拟设备请求", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				d.deviceTask = "approve"
			}
		case "approve":
			k.EntityHeader(c, uikit.EntityHeaderProps{Icon: uikit.IconDesktop, Pending: true, Title: "新电脑请求加入", Description: "核对另一台设备上的完整指纹。"}, nil)
			k.Text(c, "DEMO 4A8C 2E91 7B30", uikit.TextProps{Emphasized: true}).Font("monospace")
			if k.Button(c, "指纹一致，批准", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				found := false
				for _, v := range d.devices {
					if v.id == "new" {
						found = true
					}
				}
				if !found {
					d.devices = append(d.devices, device{id: "new", name: "新电脑", platform: "Windows 11", status: "已连接 · 局域网", icon: uikit.IconDesktop, online: true, send: true, receive: true})
				}
				d.deviceTask, d.deviceList.Value = "", "new"
			}
			if k.Button(c, "拒绝", uikit.ButtonProps{}).Clicked() {
				d.deviceTask = ""
			}
		case "phone":
			k.EntityHeader(c, uikit.EntityHeaderProps{Icon: uikit.IconPhone, Title: "连接手机", Description: "在手机上完成以下步骤。"}, nil)
			r := k.StepList(c, uikit.StepListProps{Label: "连接手机步骤", Steps: []uikit.StepListStep{
				{ID: "install", Title: "安装 UniClipboard", Description: "打开手机端应用。", Done: true},
				{ID: "network", Title: "连接同一局域网", Description: "手机与电脑连接同一网络。", Done: true},
				{ID: "register", Title: "注册手机", Description: "演示地址：192.0.2.10:7834", Action: &uikit.StepAction{Label: "模拟完成连接", Primary: true}},
			}})
			if r.Clicked == "register" {
				d.deviceTask = "phone-done"
			}
		case "phone-done":
			k.ContentUnavailable(c, uikit.ContentUnavailableProps{Icon: uikit.IconPhone, Title: "手机已连接", Description: "演示手机已出现在设备列表中。"}, nil)
		case "join":
			k.EntityHeader(c, uikit.EntityHeaderProps{Icon: uikit.IconDevices, Title: "加入其他空间", Description: "输入另一台设备提供的邀请信息。"}, nil)
			k.FormField(c, uikit.FormFieldProps{Label: "邀请信息", Value: &d.invitation})
			if d.fieldError != "" {
				k.Text(c, d.fieldError, uikit.TextProps{Tone: uikit.ToneCritical})
			}
			if k.Button(c, "继续", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				if d.invitation == "" {
					d.fieldError = "请输入演示邀请信息。"
				} else {
					d.deviceTask, d.fieldError = "join-wait", ""
				}
			}
		case "join-wait":
			k.EntityHeader(c, uikit.EntityHeaderProps{Icon: uikit.IconShield, Title: "等待对方批准", Description: "在另一台设备上核对指纹后批准。"}, nil)
			k.Text(c, "DEMO 4A8C 2E91 7B30", uikit.TextProps{Emphasized: true}).Font("monospace")
			if k.Button(c, "模拟对方批准", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				d.deviceTask = ""
			}
		}
	})
}
