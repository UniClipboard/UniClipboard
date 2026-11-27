package desktopui

import (
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/engine"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
)

type relaySettingsState struct {
	urlDraft       string
	urlSynced      string
	urlErr         string
	accessKeyDraft string
	accessKeyErr   string
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
			settingsPill(c.Key("relay-status-pill"), k, fmt.Sprintf(d.tr("%d/%d 台已就绪"), readyDevices, totalDevices), tone, true, false)
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
			Description: d.tr("设备在线时优先直连，离线时自动加密暂存转发。"),
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
			Label:       d.tr("服务地址"),
			Description: d.tr("留空使用官方服务，也可填写自建地址。"),
			Dim:         relayDim,
		}, nil, func() {
			ui.Row(c.Key("relay-url-controls")).Gap(8).AlignItems(ui.Center).Children(func() {
				field := k.TextField(c.Key("relay-url-field"), desktopkit.TextFieldProps{
					Label:       d.tr("服务地址"),
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
							d.relay.urlErr = d.tr("请输入有效的 http(s) 地址")
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
			Label:       d.tr("单次大小上限"),
			Description: d.tr("超出后仅在设备在线时直连发送。"),
			Dim:         relayDim,
		}, nil, func() {
			if k.Select(c.Key("relay-max-select"), desktopkit.SelectProps{
				Label:    d.tr("单次大小上限"),
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

		d.ensureCloudSessionLoaded()
		if strings.TrimSpace(cfg.Sync.Relay.URL) == "" {
			if d.cloud.signedIn {
				k.SettingsRow(c.Key("relay-cloud-account"), desktopkit.SettingsRowProps{
					Label:       d.tr("云服务账号"),
					Description: fmt.Sprintf(d.tr("已登录 %s"), d.cloud.session.User.DisplayLabel()),
					Dim:         syncPaused,
				}, nil, func() {
					btnLabel := d.tr("重新获取密钥")
					var variant desktopkit.ButtonVariant
					if !cfg.Sync.Relay.AccessKeySet {
						btnLabel = d.tr("一键开启")
						variant = desktopkit.Primary
					}
					if k.Button(c.Key("relay-cloud-provision"), btnLabel, desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Variant:  variant,
						Disabled: l.busy || syncPaused || d.cloud.busy,
					}).Clicked() {
						d.provisionCloudRelay(*cfg)
					}
				})
			} else {
				k.SettingsRow(c.Key("relay-cloud-account"), desktopkit.SettingsRowProps{
					Label:       d.tr("云服务账号"),
					Description: d.tr("官方服务需登录账号后使用。"),
					Dim:         syncPaused,
				}, nil, func() {
					if d.cloud.busy {
						settingsPill(c.Key("relay-cloud-pill"), k, d.cloud.busyLabel, desktopkit.StatusWarning, true, false)
					} else if k.Button(c.Key("relay-cloud-login"), d.tr("登录"), desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Variant:  desktopkit.Primary,
						Disabled: l.busy || syncPaused,
					}).Clicked() {
						d.startCloudLogin(*cfg, true)
					}
				})
			}
			if d.cloud.err != "" {
				settingsNoteRow(c.Key("relay-cloud-err"), k, d.cloud.err)
			}
		}

		k.SettingsRow(c.Key("relay-access-key"), desktopkit.SettingsRowProps{
			Label:       d.tr("接入密钥"),
			Description: d.tr("登录账号后自动填入，自建服务请手动输入。"),
			Dim:         relayDim,
		}, nil, func() {
			ui.Row(c.Key("relay-key-controls")).Gap(8).AlignItems(ui.Center).Children(func() {
				if cfg.Sync.Relay.AccessKeySet {
					settingsPill(c.Key("relay-key-pill"), k, d.tr("已配置"), desktopkit.StatusPositive, true, false)
				} else {
					settingsPill(c.Key("relay-key-pill"), k, d.tr("未配置"), desktopkit.StatusNeutral, false, false)
				}
				field := k.TextField(c.Key("relay-key-field"), desktopkit.TextFieldProps{
					Label:       d.tr("接入密钥"),
					Value:       &d.relay.accessKeyDraft,
					Placeholder: d.tr("输入接入密钥"),
					Password:    true,
					Disabled:    l.busy || relayDim,
					Error:       d.relay.accessKeyErr,
				})
				saveKey := func() {
					key := strings.TrimSpace(d.relay.accessKeyDraft)
					if key == "" {
						d.relay.accessKeyErr = d.tr("请输入密钥，或点击清除")
						return
					}
					if len(key) > 512 || strings.ContainsAny(key, "\r\n\x00") {
						d.relay.accessKeyErr = d.tr("密钥格式无效")
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
				if k.Button(c.Key("relay-key-save"), d.tr("保存"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Disabled: l.busy || relayDim || strings.TrimSpace(d.relay.accessKeyDraft) == "",
				}).Clicked() {
					saveKey()
				}
				if cfg.Sync.Relay.AccessKeySet {
					if k.Button(c.Key("relay-key-clear"), d.tr("清除"), desktopkit.ButtonProps{
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
			settingsNoteRow(c.Key("relay-route-note"), k, d.tr("部分设备尚未同步配置，双方在线连接一次后自动生效。"))
		}
	})
}
