package desktopdemo

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

func (d *demo) entryPage(c *ui.Context, k uikit.Kit) {
	p := uikit.EntryPageProps{AppIcon: appIcon(), Title: "欢迎使用 UniClipboard", Description: "在你的设备之间安全地同步剪贴板。", BackLabel: "返回历史"}
	switch d.page {
	case "create":
		p.Title, p.Description, p.BackLabel = "创建空间", "设置设备名称和空间口令。", "返回"
	case "join":
		p.Title, p.Description, p.BackLabel = "加入空间", "输入另一台设备提供的邀请信息。", "返回"
	case "import":
		p.Title, p.Description, p.BackLabel = "导入设备备份", "从已有备份恢复设备。", "返回"
	case "ready":
		p.Title, p.Description, p.Status = "空间已就绪", "现在可以邀请其他设备。", uikit.EntryPageStatus{Text: "设置完成", Tone: uikit.EntryPositive, Icon: uikit.IconShield}
	case "pending":
		p.Title, p.Description, p.Status = "等待对方批准", "在另一台设备上核对指纹。", uikit.EntryPageStatus{Text: "加入请求已发送", Busy: true}
	case "unlock":
		p.Title, p.Description, p.Status = "解锁 UniClipboard", "输入空间口令以访问本机历史。", uikit.EntryPageStatus{Text: "空间已锁定", Icon: uikit.IconLock}
	case "startup":
		p.Title, p.Description, p.Status = "正在启动 UniClipboard", "正在准备本机历史和同步服务。", uikit.EntryPageStatus{Text: "正在启动", Busy: true}
	case "recovery":
		p.Title, p.Description, p.Status = "恢复本机数据", "本机数据需要检查。选择恢复方式。", uikit.EntryPageStatus{Text: "需要处理", Tone: uikit.EntryCritical, Icon: uikit.IconShield}
	}
	back := k.EntryPage(c.Key(d.page), p, func() {
		switch d.page {
		case "setup":
			r := k.ChoiceList(c.Key("choices"), uikit.ChoiceListProps{Label: "设置设备", Items: []uikit.ChoiceListItem{
				{ID: "create", Title: "创建新空间", Description: "这是我的第一台设备", Icon: uikit.IconShield},
				{ID: "join", Title: "加入已有空间", Description: "我已经在其他设备上使用 UniClipboard", Icon: uikit.IconPhone},
			}})
			if r.Selected != "" {
				d.page = r.Selected
			}
			r = k.ChoiceList(c.Key("import"), uikit.ChoiceListProps{Title: "迁移与恢复", Items: []uikit.ChoiceListItem{{ID: "import", Title: "导入设备备份", Description: "恢复已有设备的数据", Icon: uikit.IconPackage}}})
			if r.Selected != "" {
				d.page = "import"
			}
			k.Text(c, "数据仅保存在你的设备上。", uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
		case "create":
			k.FormField(c.Key("name"), uikit.FormFieldProps{Label: "设备名称", Value: &d.deviceName})
			d.passwordField(c.Key("password"), k, "空间口令", &d.passphrase)
			d.passwordField(c.Key("confirmation"), k, "确认空间口令", &d.confirmation)
		case "join":
			k.FormField(c, uikit.FormFieldProps{Label: "邀请信息", Value: &d.invitation, Placeholder: "DEMO-123-456"})
		case "import":
			k.ContentUnavailable(c, uikit.ContentUnavailableProps{Icon: uikit.IconPackage, Title: "设备备份.zip", Description: "演示备份 · 7 条历史 · 13.1 MB"}, nil)
		case "ready":
			k.FactList(c, uikit.FactListProps{Label: "空间信息", Facts: []uikit.Fact{{ID: "name", Label: "设备", Value: d.deviceName}, {ID: "code", Label: "演示邀请码", Value: "DEMO-123-456"}}})
		case "pending":
			k.Text(c, "DEMO 4A8C 2E91 7B30", uikit.TextProps{Role: uikit.RoleHeadline, Emphasized: true}).Font("monospace")
			k.Text(c, "只有双方指纹一致时才能批准。", uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
		case "unlock":
			d.passwordField(c, k, "空间口令", &d.passphrase)
		case "startup":
			progress := 68.0
			k.ProgressBar(c, uikit.ProgressBarProps{Label: "启动进度", Value: &progress}).FillWidth()
			k.StepList(c, uikit.StepListProps{Label: "启动步骤", Steps: []uikit.StepListStep{{ID: "data", Title: "打开本机历史", Done: true}, {ID: "plugins", Title: "加载插件", Done: true}, {ID: "sync", Title: "连接同步服务", Status: &uikit.StepStatus{Text: "正在连接", Active: true}}}})
		case "recovery":
			r := k.ChoiceList(c, uikit.ChoiceListProps{Label: "恢复方式", Items: []uikit.ChoiceListItem{{ID: "retry", Title: "重试启动", Description: "重新检查本机数据", Icon: uikit.IconRetry}, {ID: "import", Title: "从备份恢复", Description: "使用已有设备备份", Icon: uikit.IconPackage}}})
			if r.Selected == "retry" {
				d.page = "startup"
			}
			if r.Selected == "import" {
				d.page = "import"
			}
		}
		if d.fieldError != "" {
			k.Text(c, d.fieldError, uikit.TextProps{Tone: uikit.ToneCritical}).Role(ui.RoleAlert)
		}
	}, func() {
		label := ""
		switch d.page {
		case "create":
			label = "创建空间"
		case "join":
			label = "继续"
		case "import":
			label = "模拟导入"
		case "ready":
			label = "开始使用"
		case "pending":
			label = "模拟对方批准"
		case "unlock":
			label = "解锁"
		case "startup":
			label = "模拟启动完成"
		}
		if label != "" && k.Button(c, label, uikit.ButtonProps{Variant: uikit.Primary, Size: uikit.SizeLarge}).Clicked() {
			d.fieldError = ""
			switch d.page {
			case "create":
				switch {
				case strings.TrimSpace(d.deviceName) == "":
					d.fieldError = "请输入设备名称。"
				case d.passphrase == "":
					d.fieldError = "请输入演示口令。"
				case d.passphrase != d.confirmation:
					d.fieldError = "两次输入的口令不一致。"
				default:
					d.page = "ready"
					d.devices[0].name = d.deviceName
					d.passphrase, d.confirmation = "", ""
				}
			case "join":
				if strings.TrimSpace(d.invitation) == "" {
					d.fieldError = "请输入演示邀请信息。"
				} else {
					d.page = "pending"
				}
			case "unlock":
				if d.passphrase == "" {
					d.fieldError = "请输入演示口令。"
				} else {
					d.page, d.passphrase = "history", ""
				}
			case "import", "pending":
				d.page = "ready"
			default:
				d.page = "history"
			}
		}
	})
	if back.BackClicked {
		if d.page == "create" || d.page == "join" || d.page == "import" {
			d.page = "setup"
		} else {
			d.page = "history"
		}
		d.fieldError, d.passphrase, d.confirmation = "", "", ""
	}
}

func (d *demo) passwordField(c *ui.Context, k uikit.Kit, label string, value *string) {
	k.FormField(c, uikit.FormFieldProps{Label: label, Value: value, Password: true, RevealShow: "显示口令", RevealHide: "隐藏口令", Hint: "此处只演示界面。请勿输入真实口令。"})
}

func (d *demo) quickPanel(c *ui.Context, _ uikit.Kit) {
	if d.quick == nil {
		d.quick = newQuickDemo(d.theme, "history")
	}
	d.quick.panel(c)
}
