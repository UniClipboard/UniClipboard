package desktopui

import (
	"context"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type weTypePairing struct {
	code        string
	message     string
	busy        bool
	optionsOpen bool
}

func isWeType(item api.PluginInfo) bool {
	return item.ExternalChannels && item.ImplementationID == "unic.wxime"
}

func (d *viewState) channelForPlugin(id string) *externalChannel {
	for i := range d.channels {
		if d.channels[i].backendID == id {
			return &d.channels[i]
		}
	}
	return nil
}

func (d *viewState) weTypePairingContent(c *ui.Context, k desktopkit.Kit, item api.PluginInfo, status string, canPair bool) {
	l := d.live
	p := &d.pluginSettings.channelPair
	k.SettingsGroup(c, desktopkit.SettingsGroupProps{Title: d.tr("微信输入法共享空间")}, func() {
		if ch := d.channelForPlugin(item.ID); ch != nil {
			label := d.tr("已加入共享空间")
			if !ch.enabled {
				label = d.tr("已暂停")
			}
			k.SettingsRow(c.Key("connected"), desktopkit.SettingsRowProps{Label: label, Description: ch.name}, nil, nil)
			return
		}
		if !canPair {
			var description string
			switch {
			case !item.Configured:
				description = d.tr("启用插件并重启后，即可输入配对码。")
			case item.Restart:
				description = d.tr("重启后即可输入配对码。")
			}
			k.SettingsRow(c.Key("not-ready"), desktopkit.SettingsRowProps{
				Label:       status,
				Description: description,
			}, nil, nil)
			return
		}
		ui.Column(c.Key("pair-form")).Padding(14, 16).Gap(10).Children(func() {
			k.Text(c.Key("instructions"), d.tr("在微信输入法的跨设备设置中获取配对码。如有提示，请在微信输入法中确认配对。"), secondary)
			field := k.CodeField(c.Key("code"), desktopkit.CodeFieldProps{
				Label: d.tr("6 位配对码"), Length: 6, Value: &p.code, Disabled: l.busy,
			})
			label := "加入共享空间"
			if p.busy {
				label = "正在连接…"
			}
			ready := !l.busy && len(p.code) == 6
			button := k.Button(c.Key("join"), d.tr(label), desktopkit.ButtonProps{
				Variant: desktopkit.Primary, Disabled: !ready, Busy: p.busy,
			})
			if ready && (button.Clicked() || field.Submitted) {
				d.pairWeType(item.ID, false)
			}
		})
	})
}

func (d *viewState) weTypeConnectionOptions(c *ui.Context, k desktopkit.Kit, item api.PluginInfo, canPair bool) {
	l := d.live
	p := &d.pluginSettings.channelPair
	k.Disclosure(c, d.tr("连接选项"), &p.optionsOpen, func() {
		if canPair && d.channelForPlugin(item.ID) == nil {
			if k.Button(c.Key("resume"), d.tr("恢复已有连接"), desktopkit.ButtonProps{Disabled: l.busy}).Clicked() {
				d.pairWeType(item.ID, true)
			}
		}
		d.createPluginInstance(c.Key("create-instance"), k, item)
	})
}

func (d *viewState) pairWeType(id string, resume bool) {
	l := d.live
	p := &d.pluginSettings.channelPair
	req := api.ChannelPairRequest{BackendID: id, Resume: resume}
	if !resume {
		req.Code = p.code
	}
	p.code, p.message = "", ""
	d.toastDismiss(toastIDWeTypePair)
	p.busy, l.busy = true, true
	if !l.enqueueWithin(func(ctx context.Context) func() {
		_, err := l.client.PairChannel(ctx, req)
		return func() {
			l.busy = false
			l.nextRefresh = time.Time{}
			if d.pluginSettings.id != id {
				return
			}
			p.busy = false
			switch api.ReasonOf(err) {
			case "invalid_argument":
				p.message = "配对码无效或已过期。"
			case "conflict":
				p.message = "此插件已有配对。请在连接选项中恢复已有连接。"
			case "invalid_state":
				p.message = "没有可恢复的连接。请使用新的配对码。"
			default:
				if err != nil {
					p.message = "未能加入共享空间。请检查微信输入法并重试。"
				}
			}
			if p.message != "" {
				d.toastError(d.tr(p.message), desktopkit.ToastOptions{ID: toastIDWeTypePair})
			}
		}
	}, 95*time.Second) {
		p.busy, l.busy = false, false
	}
}
