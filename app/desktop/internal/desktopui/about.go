package desktopui

import (
	"context"
	"fmt"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/internal/releaseinfo"
)

// UpdateStatus describes the updater capability and preferences owned by the tray process.
type UpdateStatus struct {
	Supported          bool
	Owner              string
	AutoCheckUpdate    bool
	AutoDownloadUpdate bool
	AvailableVersion   string
}

type aboutState struct {
	version              string
	buildNumber          string
	updateRead           func() UpdateStatus
	updateCheck          func()
	updateSetPrefs       func(autoCheck, autoDownload *bool)
	copyText             func(string) error
	fetchSponsors        func(context.Context) ([]Sponsor, error)
	fetchAvatar          func(context.Context, string) ([]byte, error)
	alphaWarningOpen     bool
	pendingChannel       string
	manualUpdateOpen     bool
	updateNotice         string
	copiedCommand        bool
	sponsorsStarted      bool
	sponsorsLoaded       bool
	sponsors             []Sponsor
	sponsorAvatars       map[string]ui.ImageSource
	sponsorAvatarStarted map[string]bool
}

// SetUpdater connects the tray-owned native updater status and actions to the About view.
// All callbacks are invoked on the view thread.
func (s *Desktop) SetUpdater(read func() UpdateStatus, check func(), setPrefs func(autoCheck, autoDownload *bool)) {
	s.d.about.updateRead = read
	s.d.about.updateCheck = check
	s.d.about.updateSetPrefs = setPrefs
}

// SetOpenExternal overrides how external links are opened.
func (s *Desktop) SetOpenExternal(open func(string) error) {
	s.d.openURL = open
}

func (d *viewState) copyAboutText(text string) {
	if text == "" {
		return
	}
	if d.about.copyText != nil {
		_ = d.about.copyText(text)
		return
	}
	mygo.Clipboard.WriteText(text)
}

func (d *viewState) aboutVersionParts() (version, build string) {
	version = strings.TrimSpace(d.about.version)
	build = strings.TrimSpace(d.about.buildNumber)
	if version == "" {
		version = strings.TrimSpace(releaseinfo.Version)
	}
	if build == "" {
		build = strings.TrimSpace(releaseinfo.BuildNumber)
	}
	return version, build
}

func (d *viewState) aboutDisplayVersion() string {
	ver, build := d.aboutVersionParts()
	if ver == "" {
		return "..."
	}
	if ver != "dev" && build != "" && build != "0" && !strings.Contains(ver, "(") {
		return fmt.Sprintf("%s (%s)", ver, build)
	}
	return ver
}

func parseAboutChannel(version, configured string) string {
	lower := strings.ToLower(version)
	switch {
	case strings.Contains(lower, "-alpha"):
		return "alpha"
	case strings.Contains(lower, "-beta"):
		return "beta"
	case strings.Contains(lower, "-rc"):
		return "rc"
	case lower == "dev":
		if configured == "alpha" {
			return "alpha"
		}
		return "dev"
	case configured == "alpha":
		return "alpha"
	default:
		return "stable"
	}
}

func aboutChannelLabel(channel string) string {
	switch channel {
	case "alpha":
		return "Alpha"
	case "beta":
		return "Beta"
	case "rc":
		return "RC"
	case "dev":
		return "Dev"
	default:
		return "Stable"
	}
}

func (d *viewState) currentUpdateStatus() UpdateStatus {
	if d.live == nil {
		autoCheck := true
		if v, ok := d.options["autoCheckUpdate"]; ok {
			autoCheck = v
		}
		return UpdateStatus{
			Supported:          true,
			Owner:              "direct",
			AutoCheckUpdate:    autoCheck,
			AutoDownloadUpdate: d.options["autoDownloadUpdate"],
		}
	}
	if d.about.updateRead != nil {
		st := d.about.updateRead()
		if st.Owner == "" {
			st.Owner = "direct"
		}
		return st
	}
	return UpdateStatus{Owner: "direct"}
}

func (d *viewState) currentUpdateChannel() string {
	if d.live != nil {
		if ch := d.live.config.Config.Desktop.UpdateChannel; ch == "alpha" || ch == "stable" {
			return ch
		}
		return "stable"
	}
	if ch := d.choices["updateChannel"]; ch == "alpha" || ch == "stable" {
		return ch
	}
	return "stable"
}

func (d *viewState) ownerDisplayName(owner string) string {
	switch owner {
	case "portable archive":
		return d.tr("便携版")
	case "system package manager":
		return d.tr("系统包管理器")
	default:
		return owner
	}
}

func (d *viewState) manualUpdateDetails(owner string) (title, hint, command string) {
	switch owner {
	case "portable archive":
		return d.tr("请下载新的便携版"),
			d.tr("你正在运行便携版，它不会原地自更新——请从发布页下载新的便携版压缩包并替换可执行文件（你的数据目录不受影响）。"),
			""
	case "AppImage":
		return d.tr("请下载新的 AppImage"),
			d.tr("当前版本不支持 AppImage 原地自更新——请从发布页下载新的 AppImage 文件（你的数据目录不受影响）。"),
			""
	case "Homebrew":
		return d.tr("请通过系统包管理器升级"),
			d.tr("你当前通过 Homebrew 安装了 UniClipboard，请在终端执行以下命令升级："),
			"brew upgrade --cask uniclipboard"
	case "Snap":
		return d.tr("请通过系统包管理器升级"),
			d.tr("你当前通过 Snap 安装了 UniClipboard，请在终端执行以下命令升级："),
			"sudo snap refresh uniclipboard"
	case "Flatpak":
		return d.tr("请通过系统包管理器升级"),
			d.tr("你当前通过 Flatpak 安装了 UniClipboard，请在终端执行以下命令升级："),
			"flatpak update"
	case "Scoop":
		return d.tr("请通过系统包管理器升级"),
			d.tr("你当前通过 Scoop 安装了 UniClipboard，请在终端执行以下命令升级："),
			"scoop update uniclipboard"
	case "winget":
		return d.tr("请通过系统包管理器升级"),
			d.tr("你当前通过 winget 安装了 UniClipboard，请在终端执行以下命令升级："),
			"winget upgrade UniClipboard.UniClipboard"
	case "Chocolatey":
		return d.tr("请通过系统包管理器升级"),
			d.tr("你当前通过 Chocolatey 安装了 UniClipboard，请在终端执行以下命令升级："),
			"choco upgrade uniclipboard"
	default:
		return d.tr("请通过系统包管理器升级"),
			d.tr("你当前通过系统包管理器安装了 UniClipboard，请使用安装时对应的包管理器进行升级。"),
			""
	}
}

func (d *viewState) commitUpdateChannel(channel string, checkAfter bool) {
	if channel != "stable" && channel != "alpha" {
		return
	}
	if d.live == nil {
		d.choices["updateChannel"] = channel
		if checkAfter {
			d.about.updateNotice = "当前已是最新版本。"
		}
		return
	}
	cfg := d.live.config.Config
	cfg.Desktop.UpdateChannel = channel
	var after func()
	if checkAfter {
		st := d.currentUpdateStatus()
		if st.Supported && d.about.updateCheck != nil {
			after = d.about.updateCheck
		}
	}
	d.saveLiveConfigThen(cfg, after)
}

func (d *viewState) aboutContent(c *ui.Context, k desktopkit.Kit) {
	if d.live != nil {
		d.ensureSponsorsLoaded()
	}
	status := d.currentUpdateStatus()
	channel := d.currentUpdateChannel()
	ver, _ := d.aboutVersionParts()
	badgeChannel := parseAboutChannel(ver, channel)
	busy := d.live != nil && d.live.busy
	isExternalOwner := status.Owner != "" && status.Owner != "direct"

	// 1. App header: icon + title + channel badge + version + check-for-updates button.
	ui.Row(c.Key("about-header")).Wrap().Gap(16).AlignItems(ui.Center).Padding(0, 4).MinWidth(0).Children(func() {
		ui.Box(c.Key("icon-wrap")).Size(48, 48).Radius(10).Clip().Shrink(0).Children(func() {
			ui.Image(c.Key("app-icon"), appIcon()).Size(48, 48).Fit(ui.Cover)
		})
		ui.Column(c.Key("identity")).Grow(1).Basis(200).MinWidth(0).Gap(4).Children(func() {
			ui.Row(c.Key("title-row")).Wrap().Gap(8).AlignItems(ui.Center).MinWidth(0).Children(func() {
				k.Text(c.Key("app-name"), "UniClipboard", desktopkit.TextProps{Role: desktopkit.RoleTitle, Emphasized: true}).Level(2)
				badge := ui.Row(c.Key("channel-badge")).Height(22).Padding(0, 9).Radius(11).AlignItems(ui.Center).Shrink(0)
				if badgeChannel == "stable" {
					badge.Background(k.Color("background-tertiary-default"))
				} else {
					badge.Border(1, k.Color("border-button-default"))
				}
				badge.Children(func() {
					k.Text(c.Key("label"), aboutChannelLabel(badgeChannel), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
				})
			})
			k.Text(c.Key("version"), fmt.Sprintf(d.tr("版本 %s"), d.aboutDisplayVersion()), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
		})
		if k.Button(c.Key("check-update"), d.tr("检查更新"), desktopkit.ButtonProps{Variant: desktopkit.Standard, Size: desktopkit.SizeSmall, Icon: desktopkit.IconUpdate, Disabled: busy}).Clicked() {
			if d.live == nil {
				d.about.updateNotice = "当前已是最新版本。"
			} else if isExternalOwner {
				d.about.copiedCommand = false
				d.about.manualUpdateOpen = true
			} else if !status.Supported || d.about.updateCheck == nil {
				d.about.updateNotice = "当前开发构建未启用应用内更新。"
			} else {
				d.about.updateNotice = ""
				d.about.updateCheck()
			}
		}
	})

	if d.about.updateNotice != "" {
		k.InlineNotice(c.Key("update-notice"), desktopkit.NoticeProps{Tone: desktopkit.ToneInfo, Title: d.tr(d.about.updateNotice)})
	}

	// 2. Update preferences group.
	k.SettingsGroup(c.Key("updates"), desktopkit.SettingsGroupProps{Title: d.tr("更新")}, func() {
		prefsDisabled := busy || (d.live != nil && (!status.Supported || d.about.updateSetPrefs == nil))
		autoCheck := status.AutoCheckUpdate
		k.SettingsRow(c.Key("auto-check"), desktopkit.SettingsRowProps{
			Label:       d.tr("自动检查更新"),
			Description: d.tr("应用启动时自动检查是否有新版本"),
		}, nil, func() {
			if k.Switch(c, &autoCheck, desktopkit.SwitchProps{Label: d.tr("自动检查更新"), Disabled: prefsDisabled}).Changed() {
				if d.live == nil {
					d.options["autoCheckUpdate"] = autoCheck
				} else if d.about.updateSetPrefs != nil {
					next := autoCheck
					d.about.updateSetPrefs(&next, nil)
				}
			}
		})

		downloadDesc := d.tr("检测到新版本时静默下载安装包，点击安装时无需等待。")
		if !autoCheck {
			downloadDesc = d.tr("请先开启「自动检查更新」")
		}
		autoDownload := status.AutoDownloadUpdate && autoCheck
		k.SettingsRow(c.Key("auto-download"), desktopkit.SettingsRowProps{
			Label:       d.tr("后台下载更新"),
			Description: downloadDesc,
		}, nil, func() {
			if k.Switch(c, &autoDownload, desktopkit.SwitchProps{Label: d.tr("后台下载更新"), Disabled: prefsDisabled || !autoCheck}).Changed() {
				if d.live == nil {
					d.options["autoDownloadUpdate"] = autoDownload
				} else if d.about.updateSetPrefs != nil {
					next := autoDownload
					d.about.updateSetPrefs(nil, &next)
				}
			}
		})

		selectedChannel := channel
		k.SettingsRow(c.Key("update-channel"), desktopkit.SettingsRowProps{
			Label:       d.tr("更新渠道"),
			Description: d.tr("选择接收更新的发布渠道"),
		}, nil, func() {
			res := k.Select(c, desktopkit.SelectProps{
				Label:    d.tr("更新渠道"),
				Value:    &selectedChannel,
				Disabled: busy,
				Items: []desktopkit.SelectItem{
					{Value: "stable", Label: d.tr("稳定版")},
					{Value: "alpha", Label: d.tr("内测版")},
				},
			})
			if res.Changed && selectedChannel != channel {
				if selectedChannel == "alpha" {
					d.about.pendingChannel = "alpha"
					d.about.alphaWarningOpen = true
				} else {
					d.commitUpdateChannel(selectedChannel, false)
				}
			}
		})
	})

	if d.live != nil && isExternalOwner {
		k.InlineNotice(c.Key("external-owner"), desktopkit.NoticeProps{
			Tone:  desktopkit.ToneInfo,
			Title: fmt.Sprintf(d.tr("此安装由 %s 管理更新，应用内自动更新已关闭。"), d.ownerDisplayName(status.Owner)),
		})
	}

	// 3. Sponsors section.
	d.aboutSponsorsSection(c.Key("sponsors"), k)

	// 4. Footer: links + copyright.
	ui.Column(c.Key("about-footer")).Gap(8).AlignItems(ui.Center).Padding(4, 0, 8).MinWidth(0).Children(func() {
		ui.Row(c.Key("links")).Wrap().Justify(ui.Center).AlignItems(ui.Center).GapX(20).GapY(6).Children(func() {
			if k.Button(c.Key("privacy"), d.tr("隐私政策"), desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
				d.openExternalURL(privacyPolicyURL)
			}
			if k.Button(c.Key("terms"), d.tr("使用条款"), desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
				d.openExternalURL(termsOfServiceURL)
			}
			if k.Button(c.Key("maintainers"), d.tr("关于与维护者"), desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
				d.openExternalURL(aboutMaintainersURL)
			}
		})
		k.Text(c.Key("copyright"), d.tr("© 2025 UniClipboard. All rights reserved."), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
	})
}

func (d *viewState) aboutSponsorsSection(c *ui.Context, k desktopkit.Kit) {
	// Loaded and empty (or fetch failed) → hide the entire section, matching v1.
	if d.about.sponsorsLoaded && len(d.about.sponsors) == 0 {
		return
	}
	ui.Column(c).Role(ui.RoleRegion).Label(d.tr("赞助者")).Gap(12).MinWidth(0).Children(func() {
		ui.Box(c.Key("title")).Padding(0, 2, 0).Children(func() {
			k.Text(c, d.tr("赞助者"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Level(3)
		})
		k.Text(c.Key("thanks"), d.tr("感谢以下朋友的赞助支持，让 UniClipboard 持续向前。"), desktopkit.TextProps{Role: desktopkit.RoleCallout, Tone: desktopkit.ToneSecondary})

		if !d.about.sponsorsLoaded {
			ui.Row(c.Key("skeletons")).Wrap().Gap(10).MinWidth(0).Children(func() {
				for i := 0; i < 3; i++ {
					ui.Column(c.Key(fmt.Sprintf("sk-%d", i))).Basis(180).Grow(1).MinWidth(150).MaxWidth(260).
						Padding(12).Gap(8).AlignItems(ui.Center).Radius(10).
						Background(k.CardSurface()).Border(1, k.Color("border-button-default")).Children(func() {
						ui.Box(c.Key("av")).Size(40, 40).Radius(20).Background(k.Color("background-tertiary-default"))
						ui.Box(c.Key("n")).Size(72, 12).Radius(4).Background(k.Color("background-tertiary-default"))
						ui.Box(c.Key("s")).Size(48, 10).Radius(4).Background(k.Color("background-secondary-default"))
					})
				}
			})
			return
		}

		ui.Row(c.Key("grid")).Wrap().Gap(10).AlignItems(ui.Stretch).MinWidth(0).Children(func() {
			for _, sp := range d.about.sponsors {
				d.aboutSponsorCard(c.Key("sp-"+sp.ID), k, sp)
			}
		})

		ui.Row(c.Key("actions")).Wrap().Justify(ui.Center).Gap(8).Padding(2, 0, 0).Children(func() {
			if k.Button(c.Key("become-sponsor"), d.tr("成为赞助者 →"), desktopkit.ButtonProps{Variant: desktopkit.Standard, Size: desktopkit.SizeSmall}).Clicked() {
				d.openExternalURL(sponsorChannelURL)
			}
			if k.Button(c.Key("github-star"), d.tr("去 GitHub 点 Star →"), desktopkit.ButtonProps{Variant: desktopkit.Standard, Size: desktopkit.SizeSmall}).Clicked() {
				d.openExternalURL(githubRepositoryURL)
			}
		})
	})
}

func (d *viewState) aboutSponsorCard(c *ui.Context, k desktopkit.Kit, sp Sponsor) {
	isGold := sp.Tier == "gold"
	clickable := allowedExternalURL(sp.URL)

	var card ui.Element
	if clickable {
		card = ui.ButtonBase(c).Label(sp.Name)
	} else {
		card = ui.Box(c)
	}
	card.Basis(180).Grow(1).MinWidth(150).MaxWidth(260).
		Padding(12).Radius(10).Clip()

	if isGold {
		bg := k.Color("status-yellow-background").Alpha(0.22)
		if clickable && card.Hovered() {
			bg = k.Color("status-yellow-background").Alpha(0.36)
		}
		card.Background(bg).Border(1, k.Color("status-yellow-text").Alpha(0.35))
	} else {
		bg := k.CardSurface()
		if clickable && card.Hovered() {
			bg = k.Color("background-primary-hover")
		}
		card.Background(bg).Border(1, k.Color("border-button-default"))
	}

	card.Children(func() {
		ui.Column(c.Key("body")).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			if img := d.about.sponsorAvatars[sp.ID]; img != nil {
				ui.Box(c.Key("avatar-img")).Size(40, 40).Radius(20).Clip().Shrink(0).Children(func() {
					ui.Image(c.Key("img"), img).Size(40, 40).Fit(ui.Cover)
				})
			} else {
				avatarBg := k.Color("background-tertiary-default")
				if k.Dark {
					avatarBg = k.Color("background-secondary-default")
				}
				ui.Row(c.Key("avatar-fallback")).Size(40, 40).Radius(20).Center().Shrink(0).
					Background(avatarBg).Border(1, k.Color("border-button-default")).AccessHidden(true).Children(func() {
					k.Text(c.Key("initials"), sponsorInitials(sp.Name), desktopkit.TextProps{Role: desktopkit.RoleCallout, Tone: desktopkit.ToneSecondary, Emphasized: true})
				})
			}

			ui.Column(c.Key("info")).FillWidth().MinWidth(0).Gap(4).AlignItems(ui.Center).Children(func() {
				k.Text(c.Key("name"), sp.Name, desktopkit.TextProps{Role: desktopkit.RoleCallout, Emphasized: true, Truncate: true}).Tooltip(sp.Name)
				if isGold {
					ui.Row(c.Key("gold-badge")).Height(20).Padding(0, 8).Radius(10).AlignItems(ui.Center).
						Background(k.Color("status-yellow-background")).Children(func() {
						k.Text(c.Key("gold"), d.tr("金牌"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).
							TextColor(k.Color("status-yellow-text"))
					})
				} else if sp.Note != "" {
					ui.Row(c.Key("note-wrap")).MinHeight(20).AlignItems(ui.Center).Children(func() {
						k.Text(c.Key("note"), sp.Note, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true}).Tooltip(sp.Note)
					})
				}
			})
		})
	})

	if clickable && card.Clicked() {
		d.openExternalURL(sp.URL)
	}
}

func (d *viewState) aboutDialogs(c *ui.Context, k desktopkit.Kit) {
	if d.about.alphaWarningOpen {
		r := k.AlertDialog(c.Key("alpha-warning"), desktopkit.AlertDialogProps{
			Open:         &d.about.alphaWarningOpen,
			Title:        d.tr("切换到内测版？"),
			Description:  d.tr("内测版可能包含未充分验证的功能和回归问题。切换后会立即检查内测版更新。"),
			ConfirmLabel: d.tr("继续切换"),
			CancelLabel:  d.tr("取消"),
		}, nil)
		if r.Confirmed && d.about.pendingChannel == "alpha" {
			d.about.pendingChannel = ""
			d.commitUpdateChannel("alpha", true)
		} else if !d.about.alphaWarningOpen {
			d.about.pendingChannel = ""
		}
	}

	if d.about.manualUpdateOpen {
		status := d.currentUpdateStatus()
		title, hint, command := d.manualUpdateDetails(status.Owner)
		r := k.AlertDialog(c.Key("manual-update"), desktopkit.AlertDialogProps{
			Open:         &d.about.manualUpdateOpen,
			Title:        title,
			Description:  hint,
			ConfirmLabel: d.tr("知道了"),
			CancelLabel:  d.tr("打开发布页"),
		}, func() {
			ui.Row(c.Key("current-ver")).FillWidth().Justify(ui.SpaceBetween).AlignItems(ui.Center).Children(func() {
				k.Text(c.Key("l"), d.tr("当前版本"), desktopkit.TextProps{Role: desktopkit.RoleCallout, Tone: desktopkit.ToneSecondary})
				k.Text(c.Key("v"), d.aboutDisplayVersion(), desktopkit.TextProps{Role: desktopkit.RoleCallout, Emphasized: true})
			})
			if command != "" {
				ui.Row(c.Key("cmd-box")).FillWidth().Padding(8, 10).Gap(8).AlignItems(ui.Center).Radius(8).
					Background(k.SubtleCardSurface()).Border(1, k.Color("border-button-default")).Children(func() {
					k.Text(c.Key("cmd"), command, desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true}).Grow(1).MinWidth(0)
					copyLabel := d.tr("复制命令")
					if d.about.copiedCommand {
						copyLabel = d.tr("已复制")
					}
					if k.Button(c.Key("copy-cmd"), copyLabel, desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy}).Clicked() {
						d.copyAboutText(command)
						d.about.copiedCommand = true
					}
				})
			}
		})
		if r.Cancelled {
			d.openExternalURL(releaseDownloadURL)
		}
		if !d.about.manualUpdateOpen {
			d.about.copiedCommand = false
		}
	}
}
