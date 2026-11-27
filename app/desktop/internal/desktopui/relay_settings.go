package desktopui

import (
	"context"
	"fmt"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/cloudauth"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
)

type relaySettingsState struct {
	urlDraft         string
	urlSynced        string
	urlErr           string
	accessKeyDraft   string
	accessKeyErr     string
	cloudLoginBusy   bool
	cloudLoginErr    string
	cloudLoginCancel context.CancelFunc
	cloudLoginFn     func(context.Context, cloudauth.LoginOptions) (cloudauth.RelayCredential, error)
}

func (d *viewState) relayReadyCounts() (ready, total int) {
	for _, dev := range d.devices {
		hasTailcat := false
		isReady := false
		for _, b := range dev.bindings {
			if b.instanceID == string(engine.BackendTailcat) && b.state == "authorized" {
				hasTailcat = true
				if b.relayRouteReady {
					isReady = true
				}
			}
		}
		if hasTailcat {
			total++
			if isReady {
				ready++
			}
		}
	}
	return ready, total
}

func (d *viewState) relaySettingsGroup(c *ui.Context, k desktopkit.Kit, cfg *config.Config, changed *bool) {
	l := d.live
	syncPaused := !cfg.Sync.Enabled
	relayDim := syncPaused || !cfg.Sync.Relay.Enabled
	readyDevices, totalDevices := d.relayReadyCounts()

	if cfg.Sync.Relay.URL != d.relay.urlSynced && d.relay.urlDraft == d.relay.urlSynced {
		d.relay.urlDraft = cfg.Sync.Relay.URL
		d.relay.urlSynced = cfg.Sync.Relay.URL
	}

	trailing := func() {
		switch {
		case syncPaused:
			settingsPill(c.Key("relay-status-pill"), k, d.tr("同步已暂停"), desktopkit.StatusNeutral, false, false)
		case !cfg.Sync.Relay.Enabled:
			settingsPill(c.Key("relay-status-pill"), k, d.tr("已关闭"), desktopkit.StatusNeutral, false, false)
		case totalDevices > 0:
			tone := desktopkit.StatusPositive
			if readyDevices == 0 {
				tone = desktopkit.StatusWarning
			}
			settingsPill(c.Key("relay-status-pill"), k, fmt.Sprintf(d.tr("%d/%d 台设备中转就绪"), readyDevices, totalDevices), tone, true, false)
		default:
			settingsPill(c.Key("relay-status-pill"), k, d.tr("已开启"), desktopkit.StatusPositive, true, false)
		}
	}

	k.SettingsGroup(c.Key("relay"), desktopkit.SettingsGroupProps{
		Title:    d.tr("混合中转"),
		Trailing: trailing,
	}, func() {
		k.SettingsRow(c.Key("relay-enabled"), desktopkit.SettingsRowProps{
			Label:       d.tr("启用混合中转"),
			Description: d.tr("P2P 在线直连优先；对端离线时 0 等待走零知识端到端加密临时 HTTP 中转。与 Tailcat P2P/DERP 节点相互独立。"),
			Dim:         syncPaused,
		}, nil, func() {
			if k.Switch(c, &cfg.Sync.Relay.Enabled, desktopkit.SwitchProps{
				Label:    d.tr("启用混合中转"),
				Disabled: l.busy || syncPaused,
			}).Changed() {
				*changed = true
			}
		})

		k.SettingsRow(c.Key("relay-url"), desktopkit.SettingsRowProps{
			Label:       d.tr("中转服务地址"),
			Description: d.tr("留空使用官方默认地址；填入自部署 HTTPS 地址并保存即可切换。"),
			Dim:         relayDim,
		}, nil, func() {
			ui.Row(c.Key("relay-url-controls")).Gap(8).AlignItems(ui.Center).Children(func() {
				field := k.TextField(c.Key("relay-url-field"), desktopkit.TextFieldProps{
					Label:       d.tr("中转服务地址"),
					Value:       &d.relay.urlDraft,
					Placeholder: relayclient.DefaultURL,
					Disabled:    l.busy || relayDim,
					Error:       d.relay.urlErr,
				})
				saveURL := func() {
					trimmed := strings.TrimSpace(d.relay.urlDraft)
					if trimmed != "" {
						norm, err := relaycrypto.NormalizeRelayOrigin(trimmed)
						if err != nil {
							d.relay.urlErr = d.tr("请输入有效的 http(s) 中转地址")
							return
						}
						trimmed = norm
					}
					d.relay.urlErr = ""
					d.relay.urlDraft = trimmed
					d.relay.urlSynced = trimmed
					cfg.Sync.Relay.URL = trimmed
					*changed = true
				}
				if field.Submitted && !l.busy && !relayDim {
					saveURL()
				}
				if k.Button(c.Key("relay-url-save"), d.tr("保存"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Disabled: l.busy || relayDim || strings.TrimSpace(d.relay.urlDraft) == cfg.Sync.Relay.URL,
				}).Clicked() {
					saveURL()
				}
				if cfg.Sync.Relay.URL != "" {
					if k.Button(c.Key("relay-url-reset"), d.tr("恢复默认"), desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Disabled: l.busy || relayDim,
					}).Clicked() {
						d.relay.urlErr = ""
						d.relay.urlDraft = ""
						d.relay.urlSynced = ""
						cfg.Sync.Relay.URL = ""
						*changed = true
					}
				}
			})
		})

		maxMB := int(cfg.Sync.Relay.MaxBytes / (1 << 20))
		if maxMB <= 0 {
			maxMB = 25
		}
		values := []int{10, 25, 50, 100, 250}
		if !slices.Contains(values, maxMB) {
			values = append(values, maxMB)
			slices.Sort(values)
		}
		opts := make([]desktopkit.SelectItem, 0, len(values))
		for _, v := range values {
			label := fmt.Sprintf("%d MB", v)
			if v == 25 {
				label = d.tr("25 MB（默认）")
			}
			opts = append(opts, desktopkit.SelectItem{Value: strconv.Itoa(v), Label: label})
		}
		selectedMB := strconv.Itoa(maxMB)
		k.SettingsRow(c.Key("relay-max-bytes"), desktopkit.SettingsRowProps{
			Label:       d.tr("单次中转大小上限"),
			Description: d.tr("超出上限的内容仅在 P2P 在线时直连传输。"),
			Dim:         relayDim,
		}, nil, func() {
			if k.Select(c.Key("relay-max-select"), desktopkit.SelectProps{
				Label:    d.tr("单次中转大小上限"),
				Value:    &selectedMB,
				Items:    opts,
				Disabled: l.busy || relayDim,
			}).Changed {
				if parsed, err := strconv.Atoi(selectedMB); err == nil && parsed > 0 {
					cfg.Sync.Relay.MaxBytes = config.Size(parsed) * (1 << 20)
					*changed = true
				}
			}
		})

		k.SettingsRow(c.Key("relay-cloud-login"), desktopkit.SettingsRowProps{
			Label:       d.tr("UniClipboard Cloud 授权"),
			Description: d.tr("跳转浏览器登录 UniClipboard Cloud，授权后自动填入官方中转 API Key 并开启混合中转。"),
			Dim:         syncPaused,
		}, nil, func() {
			ui.Row(c.Key("relay-cloud-controls")).Gap(8).AlignItems(ui.Center).Children(func() {
				if d.relay.cloudLoginBusy {
					settingsPill(c.Key("relay-cloud-pill"), k, d.tr("等待浏览器授权…"), desktopkit.StatusWarning, true, false)
					if k.Button(c.Key("relay-cloud-cancel"), d.tr("取消"), desktopkit.ButtonProps{
						Size: desktopkit.SizeSmall,
					}).Clicked() {
						if d.relay.cloudLoginCancel != nil {
							d.relay.cloudLoginCancel()
							d.relay.cloudLoginCancel = nil
						}
						d.relay.cloudLoginBusy = false
						d.relay.cloudLoginErr = ""
					}
				} else {
					btnLabel := d.tr("登录 Cloud 获取 API Key")
					if cfg.Sync.Relay.AccessKeySet {
						btnLabel = d.tr("重新登录 Cloud 获取 API Key")
					}
					if k.Button(c.Key("relay-cloud-btn"), btnLabel, desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Variant:  desktopkit.Primary,
						Disabled: l.busy || syncPaused,
					}).Clicked() {
						d.startCloudRelayLogin(*cfg)
					}
				}
			})
		})
		if d.relay.cloudLoginErr != "" {
			settingsNoteRow(c.Key("relay-cloud-err"), k, d.relay.cloudLoginErr)
		}

		k.SettingsRow(c.Key("relay-access-key"), desktopkit.SettingsRowProps{
			Label:       d.tr("中转 API Key / 接入密钥"),
			Description: d.tr("官方中转请使用 Cloud API Key；自部署可填入对应 X-Unic-Relay-Key。仅写入不回显，不入日志或备份。"),
			Dim:         relayDim,
		}, nil, func() {
			ui.Row(c.Key("relay-key-controls")).Gap(8).AlignItems(ui.Center).Children(func() {
				if cfg.Sync.Relay.AccessKeySet {
					settingsPill(c.Key("relay-key-pill"), k, d.tr("已配置"), desktopkit.StatusPositive, true, false)
				} else {
					settingsPill(c.Key("relay-key-pill"), k, d.tr("未配置"), desktopkit.StatusNeutral, false, false)
				}
				field := k.TextField(c.Key("relay-key-field"), desktopkit.TextFieldProps{
					Label:       d.tr("中转 API Key / 接入密钥"),
					Value:       &d.relay.accessKeyDraft,
					Placeholder: d.tr("输入 Cloud API Key 或自部署密钥（只写不回显）"),
					Password:    true,
					Disabled:    l.busy || relayDim,
					Error:       d.relay.accessKeyErr,
				})
				saveKey := func() {
					key := strings.TrimSpace(d.relay.accessKeyDraft)
					if key == "" {
						d.relay.accessKeyErr = d.tr("请输入 API Key 或接入密钥，或点击清除密钥")
						return
					}
					if len(key) > 512 || strings.ContainsAny(key, "\r\n\x00") {
						d.relay.accessKeyErr = d.tr("接入密钥格式无效")
						return
					}
					d.relay.accessKeyErr = ""
					nextCfg := *cfg
					nextCfg.Sync.Relay.AccessKey = key
					d.relay.accessKeyDraft = ""
					d.saveLiveConfig(nextCfg)
				}
				if field.Submitted && !l.busy && !relayDim {
					saveKey()
				}
				if k.Button(c.Key("relay-key-save"), d.tr("保存密钥"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Disabled: l.busy || relayDim || strings.TrimSpace(d.relay.accessKeyDraft) == "",
				}).Clicked() {
					saveKey()
				}
				if cfg.Sync.Relay.AccessKeySet {
					if k.Button(c.Key("relay-key-clear"), d.tr("清除密钥"), desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Disabled: l.busy || relayDim,
					}).Clicked() {
						d.relay.accessKeyErr = ""
						d.relay.accessKeyDraft = ""
						d.clearLiveRelayAccessKey(*cfg, nil)
					}
				}
			})
		})

		if !relayDim && totalDevices > 0 && readyDevices < totalDevices {
			settingsNoteRow(c.Key("relay-route-note"), k, d.tr("部分已配对设备尚未交换中转路由，请确保两端配置了相同的中转服务地址并至少完成一次 P2P 连接。"))
		}
	})
}

func (d *viewState) startCloudRelayLogin(curCfg config.Config) {
	if d.live == nil || d.relay.cloudLoginBusy {
		return
	}
	baseCtx := d.live.ctx
	if baseCtx == nil {
		baseCtx = context.Background()
	}
	ctx, cancel := context.WithTimeout(baseCtx, 5*time.Minute)
	d.relay.cloudLoginBusy = true
	d.relay.cloudLoginErr = ""
	d.relay.cloudLoginCancel = cancel

	loginFn := d.relay.cloudLoginFn
	if loginFn == nil {
		loginFn = cloudauth.LoginForRelayKey
	}
	deviceName := strings.TrimSpace(curCfg.DeviceName)
	live := d.live
	results := live.results

	go func() {
		defer cancel()
		cred, err := loginFn(ctx, cloudauth.LoginOptions{
			DeviceName:  deviceName,
			OpenBrowser: d.openExternalURL,
		})
		post := func() {
			d.relay.cloudLoginBusy = false
			d.relay.cloudLoginCancel = nil
			if err != nil {
				if ctx.Err() == context.Canceled {
					return
				}
				d.relay.cloudLoginErr = fmt.Sprintf(d.tr("Cloud 授权失败：%v"), err)
				return
			}
			d.relay.cloudLoginErr = ""
			nextCfg := d.live.config.Config
			nextCfg.Sync.Relay.Enabled = true
			if cred.RelayURL == "" || strings.EqualFold(strings.TrimRight(cred.RelayURL, "/"), relayclient.DefaultURL) {
				nextCfg.Sync.Relay.URL = ""
			} else if norm, nerr := relaycrypto.NormalizeRelayOrigin(cred.RelayURL); nerr == nil {
				nextCfg.Sync.Relay.URL = norm
			}
			nextCfg.Sync.Relay.AccessKey = cred.APIKey
			d.relay.urlDraft = nextCfg.Sync.Relay.URL
			d.relay.urlSynced = nextCfg.Sync.Relay.URL
			d.saveLiveConfig(nextCfg)
		}
		select {
		case results <- post:
			live.wake()
		case <-baseCtx.Done():
		}
	}()
}
