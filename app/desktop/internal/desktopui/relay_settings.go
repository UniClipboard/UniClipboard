package desktopui

import (
	"fmt"
	"slices"
	"strconv"
	"strings"
	"time"

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
	editingKey     bool
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
	d.ensureCloudSessionLoaded()
	syncPaused := !cfg.Sync.Enabled
	relayDim := syncPaused || !cfg.Sync.Relay.Enabled
	officialMode := strings.TrimSpace(cfg.Sync.Relay.URL) == ""
	hasFallback := hasRelayFallbackKey(*cfg)
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

	applyRelayURL := func(trimmed string) {
		prevURL := cfg.Sync.Relay.URL
		d.relay.urlErr = ""
		d.relay.urlDraft = trimmed
		d.relay.urlSynced = trimmed
		cfg.Sync.Relay.URL = trimmed
		if trimmed != prevURL {
			d.relay.editingKey = false
			d.relay.accessKeyDraft = ""
			d.relay.accessKeyErr = ""
			d.cloud.err = ""
			d.cloud.lastStatAt = time.Time{}
			cfg.Sync.Relay.AccessKeySet = false
			if trimmed == "" && !d.cloud.signedIn && !cfg.Sync.Relay.EnvAccessKeySet {
				cfg.Sync.Relay.Enabled = false
			}
		}
		*changed = true
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
			relaySwitch := cfg.Sync.Relay.Enabled
			if k.Switch(c, &relaySwitch, desktopkit.SwitchProps{
				Label:    d.tr("启用混合中转"),
				Disabled: l.showBusy() || syncPaused || d.cloud.showBusy(),
			}).Changed() && !l.busy && !syncPaused && !d.cloud.busy {
				if relaySwitch && !cfg.Sync.Relay.Enabled && officialMode && !d.cloud.session.HasActiveCredential(time.Now()) && !hasFallback {
					if d.cloud.signedIn {
						d.provisionCloudRelay(*cfg)
					} else {
						d.startCloudLogin(*cfg, true)
					}
				} else {
					cfg.Sync.Relay.Enabled = relaySwitch
					*changed = true
				}
			}
		})

		k.SettingsRow(c.Key("relay-url"), desktopkit.SettingsRowProps{
			Label:       d.tr("服务地址"),
			Description: d.tr("留空使用官方服务，也可填写自建地址。"),
			Dim:         syncPaused,
		}, nil, func() {
			ui.Row(c.Key("relay-url-controls")).Gap(8).AlignItems(ui.Center).Children(func() {
				field := k.TextField(c.Key("relay-url-field"), desktopkit.TextFieldProps{
					Label:       d.tr("服务地址"),
					Value:       &d.relay.urlDraft,
					Placeholder: relayclient.DefaultURL,
					Disabled:    l.showBusy() || syncPaused,
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
					applyRelayURL(trimmed)
				}
				if field.Submitted && !l.busy && !syncPaused {
					saveURL()
				}
				if k.Button(c.Key("relay-url-save"), d.tr("保存"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Disabled: l.showBusy() || syncPaused || strings.TrimSpace(d.relay.urlDraft) == cfg.Sync.Relay.URL,
				}).Clicked() && !l.busy && !syncPaused {
					saveURL()
				}
				if cfg.Sync.Relay.URL != "" {
					if k.Button(c.Key("relay-url-reset"), d.tr("恢复默认"), desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Disabled: l.showBusy() || syncPaused,
					}).Clicked() && !l.busy && !syncPaused {
						applyRelayURL("")
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
				Disabled: l.showBusy() || relayDim,
			}).Changed && !l.busy && !relayDim {
				if parsed, err := strconv.Atoi(selectedMB); err == nil && parsed > 0 {
					cfg.Sync.Relay.MaxBytes = config.Size(parsed) * (1 << 20)
					*changed = true
				}
			}
		})

		if officialMode {
			switch {
			case d.cloud.signedIn:
				k.SettingsRow(c.Key("relay-cloud-account"), desktopkit.SettingsRowProps{
					Label:       d.tr("云服务账号"),
					Description: fmt.Sprintf(d.tr("已登录 %s"), d.cloud.session.User.DisplayLabel()),
					Dim:         syncPaused,
				}, nil, func() {
					ui.Row(c.Key("relay-cloud-actions")).Gap(8).AlignItems(ui.Center).Children(func() {
						settingsPill(c.Key("relay-cloud-pill"), k, d.tr("已登录"), desktopkit.StatusPositive, true, false)
						if k.Button(c.Key("relay-cloud-logout"), d.tr("退出登录"), desktopkit.ButtonProps{
							Size:     desktopkit.SizeSmall,
							Variant:  desktopkit.Subtle,
							Disabled: l.showBusy() || syncPaused || d.cloud.showBusy(),
						}).Clicked() && !l.busy && !syncPaused && !d.cloud.busy {
							d.startCloudLogout(*cfg)
						}
					})
				})
			case d.cloud.reauthRequired:
				k.SettingsRow(c.Key("relay-cloud-account"), desktopkit.SettingsRowProps{
					Label:       d.tr("云服务账号"),
					Description: d.tr("登录已失效，请重新登录。"),
					Dim:         syncPaused,
				}, nil, func() {
					ui.Row(c.Key("relay-cloud-actions")).Gap(8).AlignItems(ui.Center).Children(func() {
						settingsPill(c.Key("relay-cloud-pill"), k, d.tr("需重新登录"), desktopkit.StatusWarning, true, false)
						if k.Button(c.Key("relay-cloud-reauth"), d.tr("重新登录"), desktopkit.ButtonProps{
							Size:     desktopkit.SizeSmall,
							Variant:  desktopkit.Primary,
							Disabled: l.showBusy() || syncPaused || d.cloud.showBusy(),
						}).Clicked() && !l.busy && !syncPaused && !d.cloud.busy {
							d.startCloudLogin(*cfg, true)
						}
					})
				})
			default:
				accountDesc := d.tr("官方服务需登录账号后使用。")
				if hasFallback {
					accountDesc = d.tr("当前使用手动配置的接入密钥，也可登录云账号。")
				}
				k.SettingsRow(c.Key("relay-cloud-account"), desktopkit.SettingsRowProps{
					Label:       d.tr("云服务账号"),
					Description: accountDesc,
					Dim:         syncPaused,
				}, nil, func() {
					if d.cloud.showBusy() {
						settingsPill(c.Key("relay-cloud-pill"), k, d.cloud.busyLabel, desktopkit.StatusWarning, true, false)
					} else if k.Button(c.Key("relay-cloud-login"), d.tr("登录"), desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Variant:  desktopkit.Primary,
						Disabled: l.showBusy() || syncPaused || d.cloud.showBusy(),
					}).Clicked() && !l.busy && !syncPaused && !d.cloud.busy {
						d.startCloudLogin(*cfg, true)
					}
				})
			}
			if d.cloud.err != "" {
				settingsNoteRow(c.Key("relay-cloud-err"), k, d.cloud.err)
			}
		} else {
			keyLabelExtra := func() {
				if cfg.Sync.Relay.AccessKeySet {
					settingsPill(c.Key("relay-key-pill"), k, d.tr("已配置"), desktopkit.StatusPositive, true, false)
				} else {
					settingsPill(c.Key("relay-key-pill"), k, d.tr("未配置"), desktopkit.StatusNeutral, false, false)
				}
			}
			keyDesc := d.tr("用于自建中转服务鉴权（仅写入不回显）。")
			if d.relay.editingKey {
				keyDesc = d.tr("输入后按回车或点击保存。")
			}
			k.SettingsRow(c.Key("relay-access-key"), desktopkit.SettingsRowProps{
				Label:       d.tr("接入密钥"),
				Description: keyDesc,
				Dim:         syncPaused,
			}, keyLabelExtra, func() {
				if d.relay.editingKey && !syncPaused {
					ui.Row(c.Key("relay-key-edit")).Gap(8).AlignItems(ui.Center).Children(func() {
						ui.Box(c.Key("relay-key-box")).Width(220).Children(func() {
							field := k.TextField(c.Key("relay-key-field"), desktopkit.TextFieldProps{
								Label:       d.tr("接入密钥"),
								Value:       &d.relay.accessKeyDraft,
								Placeholder: d.tr("输入接入密钥"),
								Password:    true,
								Disabled:    l.showBusy() || syncPaused,
								Error:       d.relay.accessKeyErr,
							})
							if field.Submitted && !l.busy && !syncPaused {
								d.submitRelayAccessKey(*cfg)
							}
						})
						if k.Button(c.Key("relay-key-save"), d.tr("保存"), desktopkit.ButtonProps{
							Size:     desktopkit.SizeSmall,
							Variant:  desktopkit.Primary,
							Disabled: l.showBusy() || syncPaused || strings.TrimSpace(d.relay.accessKeyDraft) == "",
						}).Clicked() && !l.busy && !syncPaused {
							d.submitRelayAccessKey(*cfg)
						}
						if k.Button(c.Key("relay-key-cancel"), d.tr("取消"), desktopkit.ButtonProps{
							Size:     desktopkit.SizeSmall,
							Variant:  desktopkit.Subtle,
							Disabled: l.showBusy(),
						}).Clicked() && !l.busy {
							d.relay.editingKey = false
							d.relay.accessKeyDraft = ""
							d.relay.accessKeyErr = ""
						}
					})
					return
				}

				ui.Row(c.Key("relay-key-actions")).Gap(8).AlignItems(ui.Center).Children(func() {
					editLabel := d.tr("填写密钥")
					if cfg.Sync.Relay.AccessKeySet {
						editLabel = d.tr("更换")
					}
					if k.Button(c.Key("relay-key-edit-btn"), editLabel, desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Disabled: l.showBusy() || syncPaused,
					}).Clicked() && !l.busy && !syncPaused {
						d.relay.editingKey = true
						d.relay.accessKeyDraft = ""
						d.relay.accessKeyErr = ""
					}
					if cfg.Sync.Relay.AccessKeySet {
						if k.Button(c.Key("relay-key-clear"), d.tr("清除"), desktopkit.ButtonProps{
							Size:     desktopkit.SizeSmall,
							Variant:  desktopkit.Subtle,
							Disabled: l.showBusy() || syncPaused,
						}).Clicked() && !l.busy && !syncPaused {
							d.relay.editingKey = false
							d.relay.accessKeyErr = ""
							d.relay.accessKeyDraft = ""
							d.clearLiveRelayAccessKey(*cfg, nil)
						}
					}
				})
			})
		}

		if !relayDim && totalDevices > 0 && readyDevices < totalDevices {
			settingsNoteRow(c.Key("relay-route-note"), k, d.tr("部分设备尚未同步配置，双方在线连接一次后自动生效。"))
		}
	})
}

func (d *viewState) submitRelayAccessKey(cfg config.Config) {
	key := strings.TrimSpace(d.relay.accessKeyDraft)
	if key == "" {
		d.relay.accessKeyErr = d.tr("请输入密钥")
		return
	}
	if len(key) > 512 || strings.ContainsAny(key, "\r\n\x00") {
		d.relay.accessKeyErr = d.tr("密钥格式无效")
		return
	}
	d.relay.accessKeyErr = ""
	d.relay.editingKey = false
	nextCfg := cfg
	nextCfg.Sync.Relay.AccessKey = key
	d.relay.accessKeyDraft = ""
	d.saveLiveConfig(nextCfg)
}
