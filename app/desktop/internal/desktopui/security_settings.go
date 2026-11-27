package desktopui

import (
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/plugins/builtinsecurity"
)

type securitySettingsState struct {
	revision       string
	expireDraft    string
	expireError    string
	tagDraft       string
	tagError       string
	scanDraft      string
	scanError      string
	blockedApps    []string
	allowedApps    []string
	customPatterns []string
	customKeywords []string
	rulesDirty     bool
	rulesError     string
}

func (st *securitySettingsState) syncFrom(rev string, s builtinsecurity.Settings) {
	st.revision = rev
	st.expireDraft = strconv.Itoa(s.EffectiveExpireSeconds())
	st.expireError = ""
	st.tagDraft = s.EffectiveTag()
	st.tagError = ""
	st.scanDraft = strconv.Itoa(s.EffectiveMaxScanBytes())
	st.scanError = ""
	st.blockedApps = slices.Clone(s.BlockedApps)
	st.allowedApps = slices.Clone(s.AllowedApps)
	st.customPatterns = slices.Clone(s.CustomPatterns)
	st.customKeywords = slices.Clone(s.CustomKeywords)
	st.rulesDirty = false
	st.rulesError = ""
}

func (d *viewState) securityGuardSettings(c *ui.Context, k desktopkit.Kit, cfg config.Config, instance config.Instance) {
	l := d.live
	ss := &d.pluginSettings.security
	var s builtinsecurity.Settings
	if instance.Security != nil {
		s = *instance.Security
	}
	if ss.revision != l.config.Revision {
		ss.syncFrom(l.config.Revision, s)
	}

	busy := l.showBusy()
	logicBusy := l.busy
	enabled := cfg.PluginEnabled(builtinsecurity.ID)
	mode := s.EffectiveMode()

	save := func(nextEnabled bool, nextOnError string, next builtinsecurity.Settings) {
		en := nextEnabled
		instance.Enabled = &en
		instance.OnError = nextOnError
		copySettings := next
		instance.Security = &copySettings
		cfg.Plugins.Instances[builtinsecurity.ID] = instance
		d.saveLiveConfig(cfg)
	}

	// 1. Enablement & error policy card.
	k.SettingsGroup(c.Key("plugin-detail-builtin.security"), desktopkit.SettingsGroupProps{
		Title:  d.tr("敏感内容拦截"),
		Footer: d.tr("此插件运行在前置拦截阶段，始终先于常规处理器与所有同步后端执行。"),
	}, func() {
		k.SettingsRow(c.Key("enabled"), desktopkit.SettingsRowProps{
			Label:       d.tr("启用敏感内容拦截"),
			Description: d.tr("退出并重新启动桌面端后生效。"),
		}, nil, func() {
			if k.Switch(c, &enabled, desktopkit.SwitchProps{
				Label:    d.tr("启用敏感内容拦截"),
				Disabled: busy,
			}).Changed() && !logicBusy {
				save(enabled, instance.OnError, s)
			}
		})

		global := d.tr("继续处理")
		if cfg.Plugins.OnError == "block_sync" {
			global = d.tr("禁止同步此条目")
		}
		policy := instance.OnError
		if policy == "" {
			policy = "inherit"
		}
		k.SettingsRow(c.Key("failure"), desktopkit.SettingsRowProps{
			Label:       d.tr("处理失败时"),
			Description: d.tr("当此插件运行出错或超时时，决定是否允许该条剪贴板继续同步。重启后生效。"),
		}, nil, func() {
			items := []desktopkit.SelectItem{
				{Value: "inherit", Label: fmt.Sprintf(d.tr("跟随全局（%s）"), global)},
				{Value: "continue", Label: d.tr("继续处理")},
				{Value: "block_sync", Label: d.tr("禁止同步此条目")},
			}
			ui.Box(c.Key("on-error-box")).Width(220).Children(func() {
				if k.Select(c, desktopkit.SelectProps{
					Label:    d.tr("处理失败时"),
					Value:    &policy,
					Items:    items,
					Disabled: busy,
				}).Changed && !logicBusy {
					nextOnError := policy
					if policy == "inherit" {
						nextOnError = ""
					}
					save(enabled, nextOnError, s)
				}
			})
		})
	})

	// 2. Interception policy card.
	k.SettingsGroup(c.Key("security-interception"), desktopkit.SettingsGroupProps{
		Title:  d.tr("拦截策略"),
		Footer: d.tr("100% 本地检测，零网络访问；绝不在日志、错误或通知中记录剪贴板原文、密钥值或文件名。修改后重启生效。"),
	}, func() {
		k.SettingsRow(c.Key("mode"), desktopkit.SettingsRowProps{
			Label:       d.tr("拦截模式"),
			Description: d.tr("识别到敏感内容时的处理方式。"),
			Dim:         !enabled,
		}, nil, func() {
			selected := mode
			items := []desktopkit.SelectItem{
				{Value: builtinsecurity.ModeBlock, Label: d.tr("拦截不入库且不同步")},
				{Value: builtinsecurity.ModeBlockSync, Label: d.tr("仅本机保留（禁止同步）")},
				{Value: builtinsecurity.ModeExpire, Label: d.tr("本机短暂保留后自动过期（禁止同步）")},
			}
			ui.Box(c.Key("mode-box")).Width(260).Children(func() {
				if k.Select(c, desktopkit.SelectProps{
					Label:    d.tr("拦截模式"),
					Value:    &selected,
					Items:    items,
					Disabled: busy || !enabled,
				}).Changed && !logicBusy {
					s.Mode = selected
					save(enabled, instance.OnError, s)
				}
			})
		})

		if mode == builtinsecurity.ModeExpire {
			k.SettingsRow(c.Key("expire-seconds"), desktopkit.SettingsRowProps{
				Label:       d.tr("自动过期时长（秒）"),
				Description: d.tr("敏感条目在本地历史中保留的秒数（10–86400），回车保存。"),
				Dim:         !enabled,
			}, nil, func() {
				ui.Box(c.Key("exp-box")).Width(180).Children(func() {
					field := k.TextField(c, desktopkit.TextFieldProps{
						Label:    d.tr("自动过期时长（秒）"),
						Value:    &ss.expireDraft,
						Disabled: busy || !enabled,
						Error:    d.tr(ss.expireError),
					})
					if field.Submitted && !logicBusy {
						val, err := strconv.Atoi(strings.TrimSpace(ss.expireDraft))
						if err != nil || val < builtinsecurity.MinExpireSeconds || val > builtinsecurity.MaxExpireSeconds {
							ss.expireError = "请输入 10 到 86400 之间的整数。"
						} else {
							ss.expireError = ""
							s.ExpireSeconds = val
							save(enabled, instance.OnError, s)
						}
					}
				})
			})
		}

		if mode != builtinsecurity.ModeBlock {
			k.SettingsRow(c.Key("tag"), desktopkit.SettingsRowProps{
				Label:       d.tr("敏感标签"),
				Description: d.tr("在本地保留模式下为敏感条目添加的标签，回车保存。"),
				Dim:         !enabled,
			}, nil, func() {
				ui.Box(c.Key("tag-box")).Width(200).Children(func() {
					field := k.TextField(c, desktopkit.TextFieldProps{
						Label:    d.tr("敏感标签"),
						Value:    &ss.tagDraft,
						Disabled: busy || !enabled,
						Error:    d.tr(ss.tagError),
					})
					if field.Submitted && !logicBusy {
						candidate := s
						candidate.Tag = strings.TrimSpace(ss.tagDraft)
						if candidate.Tag == "" || candidate.Validate() != nil {
							ss.tagError = "请输入 1 到 64 字节的有效标签名。"
						} else {
							ss.tagError = ""
							s.Tag = candidate.Tag
							save(enabled, instance.OnError, s)
						}
					}
				})
			})
		}

		interceptRecv := s.InterceptReceivedEnabled()
		k.SettingsRow(c.Key("intercept-received"), desktopkit.SettingsRowProps{
			Label:       d.tr("同时拦截远端接收内容"),
			Description: d.tr("对从其他设备或外部渠道同步接收的敏感内容同样执行拦截。"),
			Dim:         !enabled,
		}, nil, func() {
			if k.Switch(c, &interceptRecv, desktopkit.SwitchProps{
				Label:    d.tr("同时拦截远端接收内容"),
				Disabled: busy || !enabled,
			}).Changed() && !logicBusy {
				v := interceptRecv
				s.InterceptReceived = &v
				save(enabled, instance.OnError, s)
			}
		})

		notify := s.NotifyEnabled()
		k.SettingsRow(c.Key("notify"), desktopkit.SettingsRowProps{
			Label:       d.tr("拦截时显示通知"),
			Description: d.tr("拦截时展示仅包含规则类别的简短提示，绝不包含剪贴板原文。"),
			Dim:         !enabled,
		}, nil, func() {
			if k.Switch(c, &notify, desktopkit.SwitchProps{
				Label:    d.tr("拦截时显示通知"),
				Disabled: busy || !enabled,
			}).Changed() && !logicBusy {
				v := notify
				s.Notify = &v
				save(enabled, instance.OnError, s)
			}
		})
	})

	// 3. Built-in detection rules card.
	k.SettingsGroup(c.Key("security-builtin-rules"), desktopkit.SettingsGroupProps{
		Title: d.tr("内置检测规则"),
	}, func() {
		ruleToggle := func(key, label, desc string, examples []string, current bool, set func(bool)) {
			on := current
			k.SettingsRow(c.Key(key), desktopkit.SettingsRowProps{
				Label:       d.tr(label),
				Description: d.tr(desc),
				Dim:         !enabled,
			}, func() {
				d.securityRuleExampleHint(c.Key("example"), k, label, examples)
			}, func() {
				if k.Switch(c, &on, desktopkit.SwitchProps{
					Label:    d.tr(label),
					Disabled: busy || !enabled,
				}).Changed() && !logicBusy {
					set(on)
					save(enabled, instance.OnError, s)
				}
			})
		}
		ruleToggle("private-keys", "私钥文本", "识别 PEM、OpenSSH、PGP 与 PuTTY 私钥块。", []string{
			"-----BEGIN OPENSSH PRIVATE KEY-----",
			"-----BEGIN RSA PRIVATE KEY-----",
			"-----BEGIN PGP PRIVATE KEY BLOCK-----",
			"PuTTY-User-Key-File-3: ssh-rsa",
		}, s.DetectPrivateKeysEnabled(), func(v bool) { s.DetectPrivateKeys = &v })
		ruleToggle("tokens-jwt", "API 密钥、令牌与 JWT", "识别常见云服务与 AI 平台的 API 密钥、Authorization 凭据头以及 JWT。", []string{
			"AKIAIOSFODNN7EXAMPLE",
			"ghp_1234567890abcdef1234567890abcdef1234",
			"sk-proj-xxxxxxxxxxxxxxxxxxxxxxxx",
			"Authorization: Bearer eyJhbGciOiJIUzI1NiJ9...",
		}, s.DetectTokensAndJWTEnabled(), func(v bool) { s.DetectTokensAndJWT = &v })
		ruleToggle("conn-strings", "含密码的连接串", "识别包含明文密码的数据库或服务连接 URI。", []string{
			"postgres://admin:s3cr3t@db.example.com:5432/app",
			"redis://:my_redis_pass@10.0.0.5:6379/0",
			"mongodb+srv://user:p4ssw0rd@cluster.example.net",
		}, s.DetectConnectionStringsEnabled(), func(v bool) { s.DetectConnectionStrings = &v })
		ruleToggle("credentials", "密码与密钥键值对", "识别 .env、YAML、JSON 等格式中的 password、secret、api_key 赋值（自动忽略常见占位符）。", []string{
			"DB_PASSWORD=myStrongPass123",
			`api_key: "live_9f8a7b6c5d4e"`,
			`client_secret="xK9mP2vQ8nL4"`,
		}, s.DetectCredentialsEnabled(), func(v bool) { s.DetectCredentials = &v })
		ruleToggle("password-managers", "密码管理器来源应用", "自动拦截来自 1Password、Bitwarden、KeePassXC、Apple 密码/钥匙串等密码管理器的剪贴板内容。", []string{
			"1Password · Bitwarden · KeePassXC",
			"Apple Passwords · Keychain Access",
			"Proton Pass · LastPass · Dashlane · Enpass",
		}, s.DetectPasswordManagersEnabled(), func(v bool) { s.DetectPasswordManagers = &v })
		ruleToggle("sensitive-files", "敏感文件名", "拦截 .env、id_rsa、*.pem、*.key、*.kdbx 等敏感文件。", []string{
			".env · .env.production · .npmrc",
			"id_rsa · id_ed25519 · server.pem · private.key",
			"vault.kdbx · credentials.json · kubeconfig",
		}, s.DetectSensitiveFilesEnabled(), func(v bool) { s.DetectSensitiveFiles = &v })

		k.SettingsRow(c.Key("max-scan-bytes"), desktopkit.SettingsRowProps{
			Label:       d.tr("最大扫描文本字节数"),
			Description: d.tr("单条文本内容参与敏感检测的最大前缀字节数（1024–1048576），回车保存。"),
			Dim:         !enabled,
		}, nil, func() {
			ui.Box(c.Key("scan-box")).Width(180).Children(func() {
				field := k.TextField(c, desktopkit.TextFieldProps{
					Label:    d.tr("最大扫描文本字节数"),
					Value:    &ss.scanDraft,
					Disabled: busy || !enabled,
					Error:    d.tr(ss.scanError),
				})
				if field.Submitted && !logicBusy {
					val, err := strconv.Atoi(strings.TrimSpace(ss.scanDraft))
					if err != nil || val < builtinsecurity.MinMaxScanBytes || val > builtinsecurity.MaxMaxScanBytes {
						ss.scanError = "请输入 1024 到 1048576 之间的整数。"
					} else {
						ss.scanError = ""
						s.MaxScanBytes = val
						save(enabled, instance.OnError, s)
					}
				}
			})
		})
	})

	// 4. Apps & custom rules card.
	k.SettingsGroup(c.Key("security-custom-rules"), desktopkit.SettingsGroupProps{
		Title: d.tr("应用与自定义规则"),
	}, func() {
		renderList := func(key, label, desc, placeholder string, examples []string, maxItems int, list *[]string) {
			ui.Column(c.Key(key)).Padding(12, 16).Gap(8).MinWidth(0).Children(func() {
				ui.Row(c.Key("head")).Justify(ui.SpaceBetween).AlignItems(ui.Center).Gap(12).MinWidth(0).Children(func() {
					ui.Column(c.Key("text")).Grow(1).MinWidth(0).Gap(2).Children(func() {
						ui.Row(c.Key("label")).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
							k.Text(c.Key("l"), d.tr(label), desktopkit.TextProps{Emphasized: true})
							d.securityRuleExampleHint(c.Key("example"), k, label, examples)
							if len(*list) > 0 {
								k.Tag(c.Key("count"), fmt.Sprintf(d.tr("%d 项"), len(*list)), desktopkit.TagProps{Color: "gray"})
							}
						})
						k.Text(c.Key("d"), d.tr(desc), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
					})
					if k.Button(c.Key("add"), d.tr("添加"), desktopkit.ButtonProps{
						Icon:     desktopkit.IconAdd,
						Size:     desktopkit.SizeSmall,
						Disabled: busy || !enabled || len(*list) >= maxItems,
					}).Clicked() {
						*list = append(*list, "")
						ss.rulesDirty = true
						ss.rulesError = ""
					}
				})
				if len(*list) == 0 {
					k.Text(c.Key("empty"), d.tr("暂无配置项"), footnote)
					return
				}
				ui.Column(c.Key("items")).Gap(6).MinWidth(0).Children(func() {
					for i := range *list {
						idx := i
						remove := false
						ui.Row(c.Key(strconv.Itoa(idx))).Gap(8).AlignItems(ui.Center).FillWidth().MinWidth(0).Children(func() {
							ui.Box(c.Key("ctrl")).Grow(1).MinWidth(0).Children(func() {
								val := (*list)[idx]
								if k.TextField(c.Key("in"), desktopkit.TextFieldProps{
									Label:       d.tr(label),
									Value:       &val,
									Placeholder: placeholder,
									Disabled:    busy || !enabled,
								}).Edited {
									(*list)[idx] = val
									ss.rulesDirty = true
									ss.rulesError = ""
								}
							})
							remove = k.Button(c.Key("rm"), "", desktopkit.ButtonProps{
								Variant:  desktopkit.Subtle,
								Icon:     desktopkit.IconDelete,
								Size:     desktopkit.SizeSmall,
								Label:    d.tr("删除"),
								Tooltip:  d.tr("删除"),
								Disabled: busy || !enabled,
							}).Clicked()
						})
						if remove {
							*list = slices.Delete(*list, idx, idx+1)
							ss.rulesDirty = true
							ss.rulesError = ""
							break
						}
					}
				})
			})
		}

		renderList("blocked-apps", "额外拦截应用列表", "填写应用标识符（Bundle ID / 进程名）或应用名称，来自这些应用的内容将一律被拦截。", "com.agilebits.onepassword7", []string{
			"com.agilebits.onepassword7",
			"org.keepassxc.keepassxc",
			"1Password",
		}, builtinsecurity.MaxAppEntries, &ss.blockedApps)
		renderList("allowed-apps", "白名单应用列表", "来自这些应用的内容将跳过所有敏感检测规则。", "com.apple.Terminal", []string{
			"com.apple.Terminal",
			"com.microsoft.VSCode",
			"iTerm2",
		}, builtinsecurity.MaxAppEntries, &ss.allowedApps)
		renderList("custom-patterns", "自定义正则表达式", "使用 Go RE2 语法添加额外的敏感文本匹配规则。", `(?i)\bCORP-SECRET-[0-9]{6}\b`, []string{
			`(?i)\bCORP-SECRET-[0-9]{6}\b`,
			`\bINTERNAL_[A-Z0-9]{16}\b`,
		}, builtinsecurity.MaxCustomPatterns, &ss.customPatterns)
		renderList("custom-keywords", "自定义关键词", "不区分大小写的字面量关键词，命中即视为敏感内容。", "CONFIDENTIAL", []string{
			"CONFIDENTIAL",
			"DO NOT SHARE",
			"TOP-SECRET",
		}, builtinsecurity.MaxCustomKeywords, &ss.customKeywords)
	})

	if ss.rulesDirty || ss.rulesError != "" {
		ui.Row(c.Key("security-rules-actions")).AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(12).MinWidth(0).Children(func() {
			ui.Box(c.Key("status")).Grow(1).MinWidth(0).Children(func() {
				if ss.rulesError != "" {
					k.Text(c.Key("err"), d.tr(ss.rulesError), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneCritical})
				} else {
					k.Text(c.Key("dirty"), d.tr("有未保存的更改。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
				}
			})
			ui.Row(c.Key("btns")).Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
				if k.Button(c.Key("reset-rules"), d.tr("重置"), desktopkit.ButtonProps{Disabled: busy}).Clicked() {
					ss.syncFrom(l.config.Revision, s)
				}
				if k.Button(c.Key("save-rules"), d.tr("保存规则"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: busy || !enabled}).Clicked() && !logicBusy {
					clean := func(in []string) []string {
						var out []string
						for _, v := range in {
							if t := strings.TrimSpace(v); t != "" {
								out = append(out, t)
							}
						}
						return out
					}
					candidate := s
					candidate.BlockedApps = clean(ss.blockedApps)
					candidate.AllowedApps = clean(ss.allowedApps)
					candidate.CustomPatterns = clean(ss.customPatterns)
					candidate.CustomKeywords = clean(ss.customKeywords)
					if err := candidate.Validate(); err != nil {
						ss.rulesError = "规则格式无效，请检查正则表达式语法或条目长度。"
					} else {
						ss.rulesError = ""
						ss.rulesDirty = false
						save(enabled, instance.OnError, candidate)
					}
				}
			})
		})
	}
}

// securityRuleExampleHint renders a subtle circled '!' badge next to a rule
// title that reveals concrete matching examples on pointer hover.
func (d *viewState) securityRuleExampleHint(c *ui.Context, k desktopkit.Kit, ruleLabel string, examples []string) {
	if len(examples) == 0 {
		return
	}
	hintLabel := d.tr("查看示例") + "：" + d.tr(ruleLabel)
	anchor := ui.Box(c).Size(20, 20).Radius(10).Shrink(0).Center().Label(hintLabel)
	fg := k.Color("text-tertiary")
	if anchor.Hovered() {
		anchor.Background(k.Color("background-tertiary-hover"))
		fg = k.Color("text-primary")
	}
	anchor.Children(func() {
		k.Icon(c.Key("ic"), desktopkit.IconWarning, 14).TextColor(fg)
	})
	k.WithRichTooltip(c.Key("tip"), anchor, strings.Join(examples, "\n"), func() {
		ui.Column(c.Key("body")).Gap(4).MinWidth(0).Children(func() {
			k.Text(c.Key("title"), d.tr("匹配示例"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
			for i, ex := range examples {
				ui.Text(c.Key(strconv.Itoa(i)), ex).Font("monospace").FontSize(12).TextColor(k.Color("text-primary"))
			}
		})
	})
}
