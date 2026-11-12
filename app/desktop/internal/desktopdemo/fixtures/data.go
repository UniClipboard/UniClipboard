// Package fixtures supplies synthetic data for previews and view acceptance only.
package fixtures

import (
	"fmt"
	"slices"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui/preview"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	"github.com/uniclipboard/unic/plugin"
)

const Reference = "8dd3796f8bbb6b1e1e07ee352a4e65e4341f56e7"
const QuickReference = "885546c7cf6f6f0d2076104efd1d90d9f73d9744"

func Desktop(page, theme string) preview.Desktop {
	d := preview.Desktop{
		Page: page, Theme: theme, Layout: "list", Selected: "note",

		DeviceName: "MacBook Pro",
		Options:    map[string]bool{"startup": true, "tray": true, "sound": true, "sync": true, "autoSync": true, "text": true, "image": true, "file": true, "relay": true, "quickPanel": true, "encryption": true},
		Choices:    map[string]string{"appearance": theme, "font": "标准", "fileLimit": "50 MB", "retention": "30 天", "proxy": "系统代理", "shortcut": "⌘⇧V"},
	}
	d.Entries = []preview.Entry{
		{ID: "note", Title: "周一产品讨论", Text: "周一产品讨论\n\n1. 确认桌面端布局与导航。\n2. 核对设备连接状态。\n3. 整理本周设计评审记录。\n\n下次评审：周三 14:00。", Source: "local", Date: "今天 10:42", Size: "96 个字符", Kind: uikit.KindText, Tags: []string{"工作"}, Pinned: true},
		{ID: "code", Title: "HTTP 服务示例", Text: "func main() {\n    http.HandleFunc(\"/health\", handler)\n    http.ListenAndServe(\":8080\", nil)\n}", Source: "linux", Date: "今天 10:35", Size: "Go · 4 行", Kind: uikit.KindCode, Tags: []string{"开发"}},
		{ID: "link", Title: "UniClipboard", Text: "https://github.com/UniClipboard/UniClipboard", Source: "windows", Date: "今天 10:28", Size: "链接", Kind: uikit.KindLink, Tags: []string{"工作"}},
		{ID: "image", Title: "桌面布局.png", Text: "桌面布局", Source: "phone", Date: "今天 10:16", Size: "1600 × 1000 · PNG · 286 KB", Kind: uikit.KindImage, Tags: []string{"设计"}},
		{ID: "files", Title: "设计交付 · 3 个文件", Text: "界面规范.pdf\n组件清单.csv\n桌面布局.png", Source: "windows", Date: "今天 09:50", Size: "3 个文件 · 12.8 MB", Kind: uikit.KindFile, Tags: []string{"设计", "工作"}},
		{ID: "text", Title: "会议室预订", Text: "已预订 3 楼会议室。\n时间：14:00—15:00。\n参会人数：6 人。", Source: "local", Date: "昨天 17:20", Size: "42 个字符", Kind: uikit.KindText},
		{ID: "offline", Title: "发布检查清单", Text: "确认版本号\n确认安装包\n核对更新说明", Source: "linux", Date: "昨天 16:05", Size: "21 个字符", Kind: uikit.KindText, Tags: []string{"开发"}},
	}
	// Match the reference's eight current-day rows and fourteen older rows.
	// Use fabricated content rather than the user's clipboard data.
	d.Entries = slices.Insert(d.Entries, 5,
		preview.Entry{ID: "schedule", Title: "下午设计评审：确认三栏布局、字体和控件间距", Text: "下午 14:00 开始设计评审。请准备界面规范与组件清单。", Source: "local", Date: "今天 09:30", Kind: uikit.KindText},
		preview.Entry{ID: "notes", Title: "本周迭代安排", Text: "完成历史窗口、设备管理和设置页面的布局核对。", Source: "local", Date: "今天 09:15", Kind: uikit.KindText},
		preview.Entry{ID: "thanks", Title: "收到，评审记录已经整理完成。", Text: "收到，评审记录已经整理完成。", Source: "windows", Date: "今天 08:50", Kind: uikit.KindText},
	)
	older := []string{"确认设备名称与连接状态的显示方式", "整理桌面端快捷键说明", "历史内容预览与图片缩放", "组件清单与界面规范", "发送文件后的进度显示", "调整列表选中状态与悬停状态", "检查窄窗口中的返回操作", "核对深色模式下的文本对比度", "新增设备的指纹核对步骤", "统一设置页面中的分组间距", "更新设计评审结论", "明天继续核对安装包"}
	for i, title := range older {
		d.Entries = append(d.Entries, preview.Entry{ID: fmt.Sprintf("older-%d", i), Title: title, Text: title, Source: "local", Date: fmt.Sprintf("昨天 %02d:20", 15-i), Kind: uikit.KindText})
	}
	d.Devices = []preview.Device{
		{ID: "local", Name: "MacBook Pro", Platform: "macOS 15.6", Icon: uikit.IconDesktop, Online: true},
		{ID: "windows", Name: "工作电脑", Platform: "Windows 11", Icon: uikit.IconDesktop, Online: true, Latency: 15, Link: "局域网", LastSeen: "刚刚", Direction: "both", Text: true, Image: true, File: true,
			Fingerprint: "8A42 7C91 2F60 B3D8 41E5 9A0C 7D12 5F3E", Paths: []preview.DevicePath{{Name: "tailcat · 默认", State: "已连接", LastSync: "1 分钟前"}}},
		{ID: "linux", Name: "Linux 工作站", Platform: "Fedora 43", Icon: uikit.IconDesktop, LastSeen: "昨天 18:02", Direction: "both", Text: true,
			Fingerprint: "E310 4B6A 92CF 7D05 18A3 CC42 6F9B 20D7", Paths: []preview.DevicePath{{Name: "tailcat · 默认", State: "不可达", LastSync: "昨天 18:02", Failed: true}}},
		{ID: "phone", Name: "我的手机", Platform: "Android 16", Icon: uikit.IconPhone, Online: true, Latency: 180, Link: "加密中继", LastSeen: "刚刚", Direction: "recv", Text: true, Image: true,
			Fingerprint: "5D08 AA13 7E42 9B6C F1D4 30E7 82A5 C619", Paths: []preview.DevicePath{{Name: "tailcat · 默认", State: "已连接（经中继）", LastSync: "12 分钟前"}, {Name: "tailcat · 工作仓库", State: "对方未批准", LastSync: "—", Failed: true}}},
	}
	d.Pending = []preview.PendingDevice{{ID: "p1", Name: "新电脑", Platform: "Windows 11", Via: "配对码 482-913", Icon: uikit.IconDesktop, Fingerprint: "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D"}}
	d.Candidates = []preview.DeviceCandidate{{ID: "c1", Name: "Alice 的平板", Introducer: "工作电脑 介绍", Icon: uikit.IconPhone}}
	d.InviteCode, d.InviteMode = "482-913", "code"
	classify(&d)
	d.Image = Image()
	d.Reference = Reference
	d.Quick = Quick(theme, "history")

	return d
}

func Quick(theme, mode string) preview.Quick {
	p := preview.Quick{Theme: theme, Mode: mode, Image: Image()}
	p.Entries = []preview.QuickEntry{
		{ID: "note", Title: "周一产品讨论", Text: "周一产品讨论\n\n1. 确认桌面端布局与导航。\n2. 核对设备连接状态。\n3. 整理本周设计评审记录。\n\n下次评审：周三 14:00。", Kind: "文本", Source: "本机", Age: "刚刚", Tags: []string{"工作"}},
		{ID: "code", Title: "HTTP 服务示例", Text: "func main() {\n    http.HandleFunc(\"/health\", handler)\n    http.ListenAndServe(\":8080\", nil)\n}", Kind: "代码", Source: "Linux 工作站", Age: "2分钟前", Tags: []string{"代码", "开发"}},
		{ID: "link", Title: "github.com/UniClipboard/UniClipboard", Text: "https://github.com/UniClipboard/UniClipboard", Kind: "链接", Source: "工作电脑", Age: "5分钟前", Tags: []string{"链接", "工作"}},
		{ID: "rich", Title: "设计评审记录", Text: "设计评审记录\n\n确认列表高度与键盘操作。\n保留清晰的选中状态。", Kind: "富文本", Source: "本机", Age: "8分钟前", Tags: []string{"工作"}},
		{ID: "files", Title: "界面规范.pdf", Text: "界面规范.pdf\n组件清单.csv\n桌面布局.png", Kind: "文件", Source: "工作电脑", Age: "12分钟前", Tags: []string{"设计", "工作"}},
		{ID: "image", Title: "桌面布局.png", Text: "桌面布局.png", Kind: "图片", Source: "我的手机", Age: "16分钟前", Tags: []string{"设计"}},
		{ID: "schedule", Title: "下午设计评审：确认三栏布局、字体和控件间距", Text: "下午 14:00 开始设计评审。请准备界面规范与组件清单。", Kind: "文本", Source: "本机", Age: "20分钟前"},
		{ID: "meeting", Title: "会议室预订", Text: "已预订 3 楼会议室。\n时间：14:00—15:00。\n参会人数：6 人。", Kind: "文本", Source: "本机", Age: "25分钟前", Favorite: true},
		{ID: "older", Title: "发布检查清单", Text: "确认版本号\n确认安装包\n核对更新说明", Kind: "文本", Source: "本机", Age: "昨天", Tags: []string{"开发"}},
		{ID: "lost", Title: "丢失的文件.zip", Text: "丢失的文件.zip", Kind: "文件", Source: "工作电脑", Age: "1小时前", Lost: true},
	}
	for i := 1; i <= 14; i++ {
		p.Entries = append(p.Entries, preview.QuickEntry{ID: fmt.Sprintf("image-%02d", i), Title: fmt.Sprintf("界面方案-%02d.png", i), Text: fmt.Sprintf("界面方案-%02d.png", i), Kind: "图片", Source: "我的手机", Age: "昨天", Tags: []string{"设计"}})
	}

	return p
}

func classify(d *preview.Desktop) {
	d.Tags = []preview.Tag{
		{ID: "工作", Name: "工作", Color: "blue", Sidebar: true},
		{ID: "设计", Name: "设计", Color: "purple", Sidebar: true},
		{ID: "开发", Name: "开发", Color: "green", Sidebar: true},
		{ID: "custom:0", Name: "自定义标签", Color: "orange"},
	}
	for i := range d.Entries {
		e := &d.Entries[i]
		tag := plugin.TagText
		switch e.Kind {
		case uikit.KindImage:
			tag = plugin.TagImage
		case uikit.KindFile:
			tag = plugin.TagFiles
		case uikit.KindLink:
			e.Tags = append(e.Tags, plugin.TagLink)
		}
		e.Tags = append(e.Tags, tag)
	}
}
