package desktopui

import (
	"context"
	"fmt"
	"math/rand/v2"
	"regexp"
	"runtime"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
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
func latencyTone(ms int) desktopkit.StatusTone {
	if ms < 50 {
		return desktopkit.StatusPositive
	}
	return desktopkit.StatusWarning
}

func deviceStatus(v device) (string, desktopkit.StatusTone) {
	switch {
	case v.id == "local":
		return "本机", desktopkit.StatusPositive
	case v.direction == "pause":
		return "已暂停", desktopkit.StatusNeutral
	case v.online && v.latency < 0:
		return "在线", desktopkit.StatusPositive
	case v.online:
		return fmt.Sprintf("%d ms · %s", v.latency, v.link), latencyTone(v.latency)
	}
	return "离线", desktopkit.StatusNeutral
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

func (d *viewState) deviceByID(id string) *device {
	for i := range d.devices {
		if d.devices[i].id == id {
			return &d.devices[i]
		}
	}
	return nil
}

func (d *viewState) setDeviceTask(task string) {
	if d.live != nil {
		if d.deviceTask == "invite" && task != "invite" {
			d.cancelLiveInvitation()
		}
		d.deviceTask, d.fieldError, d.deviceNote = task, "", ""
		if task == "invite" {
			d.inviteMode = "code"
			d.live.codeAttempted, d.live.ticketAttempted = false, false
		}
		if task == "connect" {
			d.connectInput = ""
		}
		return
	}
	d.deviceTask, d.fieldError, d.deviceNote = task, "", ""
	switch task {
	case "invite":
		d.inviteCode, d.inviteMode = newPairCode(), "code"
	case "connect":
		d.connectInput = ""
	}
}

func (d *viewState) ensureDeviceSelection() {
	id := d.deviceList.Value
	switch {
	case strings.HasPrefix(id, "pending:"):
		pid := strings.TrimPrefix(id, "pending:")
		if slices.ContainsFunc(d.pending, func(p pendingDevice) bool { return p.id == pid }) {
			return
		}
	case strings.HasPrefix(id, "cand:"):
		cid := strings.TrimPrefix(id, "cand:")
		if slices.ContainsFunc(d.candidates, func(cd deviceCandidate) bool { return cd.id == cid }) {
			return
		}
	default:
		if d.deviceByID(id) != nil {
			return
		}
	}
	if len(d.pending) > 0 {
		d.deviceList.Value = "pending:" + d.pending[0].id
		return
	}
	for _, v := range d.devices {
		if v.id != "local" {
			d.deviceList.Value = v.id
			return
		}
	}
	if len(d.devices) > 0 {
		d.deviceList.Value = d.devices[0].id
	}
}

func (d *viewState) statusPillColors(k desktopkit.Kit, tone desktopkit.StatusTone) (bg, fg, dot ui.Color) {
	switch tone {
	case desktopkit.StatusPositive:
		return k.Color("notification-success-background"), k.Color("state-success-text"), k.Color("notification-success-foreground")
	case desktopkit.StatusWarning:
		return k.Color("status-yellow-background"), k.Color("status-yellow-text"), k.Color("status-yellow-text")
	case desktopkit.StatusInfo:
		return k.Color("badge-new-background"), k.Color("badge-new-text"), k.Color("accent-500")
	default:
		return k.Color("background-tertiary-default"), k.Color("text-secondary"), k.Color("text-tertiary")
	}
}

func (d *viewState) statusPill(c *ui.Context, k desktopkit.Kit, text string, tone desktopkit.StatusTone) ui.Element {
	bg, fg, dot := d.statusPillColors(k, tone)
	if k.Dark && tone == desktopkit.StatusPositive {
		fg = k.Color("notification-success-foreground")
	}
	return ui.Row(c).Height(22).Padding(0, 8).Radius(11).Gap(5).AlignItems(ui.Center).Shrink(0).Background(bg).Children(func() {
		ui.Box(c.Key("dot")).Size(6, 6).Radius(3).Background(dot).Shrink(0)
		k.Text(c.Key("lbl"), text, desktopkit.TextProps{Role: desktopkit.RoleCaption}).TextColor(fg)
	})
}

func (d *viewState) deviceIconTile(c *ui.Context, k desktopkit.Kit, icon desktopkit.IconName, size float32, pending bool) {
	iconSize := size * 0.5
	tile := ui.Row(c).Size(size, size).Radius(size * 0.26).Justify(ui.Center).AlignItems(ui.Center).Shrink(0)
	if pending {
		bg := k.Color("accent-50")
		if k.Dark {
			bg = k.Color("accent-950")
		}
		tile.Background(bg).Border(1, k.Color("accent-400"))
		tile.Children(func() { k.Icon(c, icon, iconSize).TextColor(k.Color("accent-500")) })
		return
	}
	tile.Background(k.Color("background-secondary-default")).Border(1, k.Color("separator-border"))
	tile.Children(func() { k.Icon(c, icon, iconSize).TextColor(k.Color("text-primary")) })
}

func (d *viewState) fingerprintGrid(c *ui.Context, k desktopkit.Kit, fp string) {
	groups := strings.Fields(fp)
	if len(groups) == 0 {
		return
	}
	ui.Column(c).Gap(6).Children(func() {
		for start := 0; start < len(groups); start += 4 {
			end := min(start+4, len(groups))
			ui.Row(c.Key(start)).Gap(6).Children(func() {
				for i := start; i < end; i++ {
					ui.Row(c.Key(i)).Grow(1).MinWidth(0).Height(30).Radius(7).
						Justify(ui.Center).AlignItems(ui.Center).
						Background(k.Color("background-full")).
						Border(1, k.Color("separator-border")).Children(func() {
						k.Text(c, groups[i], desktopkit.TextProps{Monospace: true, Emphasized: true, Role: desktopkit.RoleCaption})
					})
				}
			})
		}
	})
}

// devicesPage renders the full-window Option B device management overview with
// a 2-column device card canvas and a dedicated right-hand inspector.
func (d *viewState) devicesPage(c *ui.Context, k desktopkit.Kit) {
	d.ensureDeviceSelection()
	w, _ := c.Size()
	caption := c.TitleBar()
	integratedCaption := runtime.GOOS == "darwin" && caption.Left > 0
	headerHeight := float32(52)
	if integratedCaption {
		headerHeight = max(headerHeight, caption.Height)
	}
	singlePane := w < desktopkit.SinglePaneWidth

	onlinePeers := 0
	for _, v := range d.devices {
		if v.id != "local" && v.online {
			onlinePeers++
		}
	}

	ui.Column(c.Key("devices-page")).Grow(1).MinHeight(0).AlignSelf(ui.Stretch).Background(k.WorkspaceSurface()).Children(func() {
		// Unified 52-DIP top band aligned with macOS traffic lights.
		padLeft := float32(16)
		if integratedCaption {
			padLeft = caption.Left + 10
		}
		bar := ui.Row(c.Key("devices-toolbar")).Height(headerHeight).Padding(0, 18, 0, padLeft).Gap(10).
			AlignItems(ui.Center).Shrink(0).Role(ui.RoleToolbar).Label(d.tr("设备管理")).
			Background(k.WorkspaceSurface()).
			InsetShadow(0, -1, 0, 0, k.Color("separator-border")).DragWindow()
		bar.Children(func() {
			if singlePane && d.deviceDetailOpen {
				back := ui.ButtonBase(c.Key("back-overview")).Label(d.tr("设备")).Height(28).Padding(0, 8).Radius(7).Gap(4).
					AlignItems(ui.Center).FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
				if back.Hovered() {
					back.Background(k.Color("background-secondary-hover"))
				}
				back.Children(func() {
					k.Icon(c, desktopkit.IconBack, 15).TextColor(k.Color("foreground-icon-secondary"))
					k.Text(c, d.tr("设备"), desktopkit.TextProps{Role: desktopkit.RoleCallout})
				})
				if back.Clicked() {
					d.deviceDetailOpen = false
				}
			} else {
				back := ui.ButtonBase(c.Key("back-history")).Label(d.tr("返回历史")).Height(28).Padding(0, 8).Radius(7).Gap(4).
					AlignItems(ui.Center).FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
				if back.Hovered() {
					back.Background(k.Color("background-secondary-hover"))
				}
				back.Children(func() {
					k.Icon(c, desktopkit.IconBack, 15).TextColor(k.Color("foreground-icon-secondary"))
					k.Text(c, d.tr("返回历史"), desktopkit.TextProps{Role: desktopkit.RoleCallout})
				})
				if back.Clicked() {
					d.page, d.renaming, d.deviceDetailOpen = "history", false, false
				}
			}
			ui.Box(c.Key("sep")).Size(1, 16).Background(k.Color("separator-border")).Shrink(0)
			k.Text(c.Key("title"), d.tr("设备管理"), desktopkit.TextProps{Role: desktopkit.RoleHeadline, Emphasized: true})
			d.statusPill(c.Key("summary-pill"), k, fmt.Sprintf(d.tr("%d 台在线 · 共 %d 台"), onlinePeers, len(d.devices)), desktopkit.StatusPositive)
			ui.Spacer(c)
			if k.Button(c.Key("add-device"), d.tr("添加设备"), desktopkit.ButtonProps{
				Variant: desktopkit.Primary,
				Size:    desktopkit.SizeSmall,
				Icon:    desktopkit.IconAdd,
			}).Clicked() {
				d.setDeviceTask("invite")
			}
		})

		// Body: device card overview canvas + right-hand inspector.
		ui.Row(c.Key("devices-body")).Grow(1).MinHeight(0).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
			if !singlePane || !d.deviceDetailOpen {
				d.deviceOverviewCanvas(c.Key("overview"), k, w)
			}
			if !singlePane || d.deviceDetailOpen {
				inspector := ui.Column(c.Key("inspector")).MinHeight(0).MinWidth(0).
					Background(k.Color("background-secondary-default")).
					InsetShadow(1, 0, 0, 0, k.Color("separator-border")).
					Role(ui.RoleRegion).Label(d.tr("设备详情"))
				if singlePane {
					inspector.Grow(1)
				} else {
					inspector.Width(380).Shrink(0)
				}
				inspector.Children(func() {
					k.Scroll(c.Key("inspector-scroll")).Grow(1).MinHeight(0).Padding(18, 20, 28, 20).Children(func() {
						d.deviceDetail(c, k)
					})
				})
			}
		})
	})
}

func (d *viewState) deviceOverviewCanvas(c *ui.Context, k desktopkit.Kit, winWidth float32) {
	// Order: local device first, then online peers, then offline peers.
	var ordered []device
	var peers []device
	for _, v := range d.devices {
		if v.id == "local" {
			ordered = append(ordered, v)
		} else {
			peers = append(peers, v)
		}
	}
	slices.SortStableFunc(peers, func(a, b device) int {
		return btoi(!a.online) - btoi(!b.online)
	})
	ordered = append(ordered, peers...)

	twoCols := winWidth >= 960

	k.Scroll(c).Grow(1).MinWidth(0).MinHeight(0).Padding(20, 24, 28, 24).Gap(18).
		Role(ui.RoleRegion).Label(d.tr("设备")).Children(func() {
		// Pending approval banners at top priority.
		if len(d.pending) > 0 {
			ui.Column(c.Key("pending-banners")).Gap(8).Shrink(0).Children(func() {
				for _, p := range d.pending {
					id := "pending:" + p.id
					selected := d.deviceList.Value == id
					bg := k.Color("accent-50")
					edge := k.Color("accent-200")
					if k.Dark {
						bg = k.Color("accent-950")
						edge = k.Color("accent-800")
					}
					if selected {
						edge = k.Color("accent-500")
					}
					card := ui.Row(c.Key("p-"+p.id)).Padding(12, 14).Radius(12).Gap(12).AlignItems(ui.Center).Shrink(0).
						Background(bg).Border(1.5, edge).Role(ui.RoleListItem).Label(p.name)
					card.AccessSelected(selected)
					card.Children(func() {
						d.deviceIconTile(c.Key("ic"), k, p.icon, 36, true)
						ui.Column(c.Key("meta")).Gap(2).Grow(1).MinWidth(0).Children(func() {
							k.Text(c, p.name+" "+d.tr("请求加入同步网络"), desktopkit.TextProps{Emphasized: true, Truncate: true})
							k.Text(c, p.platform+" · "+p.via, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
						})
						if k.Button(c.Key("verify"), d.tr("核对指纹"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
							d.deviceList.Value, d.deviceDetailOpen, d.renaming = id, true, false
						}
					})
					if card.Clicked() {
						d.deviceList.Value, d.deviceDetailOpen, d.renaming = id, true, false
					}
				}
			})
		}

		// My Devices card grid.
		ui.Column(c.Key("my-devices")).Gap(10).Shrink(0).Children(func() {
			ui.Row(c.Key("sec-head")).AlignItems(ui.Center).Children(func() {
				k.Text(c, d.tr("我的设备"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
				k.Text(c, d.tr("点击任意卡片在右侧调整策略"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
			})
			renderCard := func(v device) {
				selected := d.deviceList.Value == v.id
				statusText, tone := deviceStatus(v)
				statusText = d.tr(statusText)
				card := ui.Column(c.Key("dev-"+v.id)).Grow(1).MinWidth(0).Padding(14, 15).Radius(13).Gap(12).
					Background(k.Color("background-primary-default")).Role(ui.RoleListItem).Label(v.name)
				card.AccessSelected(selected)
				if selected {
					card.Border(1.5, k.Color("accent-500"))
				} else if card.Hovered() {
					card.Border(1, k.Color("border-button-hover"))
				} else {
					card.Border(1, k.Color("separator-border"))
				}
				if !v.online && v.id != "local" {
					card.Opacity(0.72)
				}
				card.Children(func() {
					ui.Row(c.Key("top")).Gap(10).AlignItems(ui.Center).Children(func() {
						d.deviceIconTile(c.Key("ic"), k, v.icon, 34, false)
						ui.Column(c.Key("info")).Gap(2).Grow(1).MinWidth(0).Children(func() {
							k.Text(c, v.name, desktopkit.TextProps{Emphasized: true, Truncate: true})
							k.Text(c, v.platform, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
						})
						d.statusPill(c.Key("st"), k, statusText, tone)
					})
					ui.Box(c.Key("div")).Height(1).Background(k.Color("separator-border"))
					ui.Row(c.Key("foot")).Gap(8).AlignItems(ui.Center).Children(func() {
						summary := d.tr("当前本机 · 端到端加密")
						if v.id != "local" {
							summary = strings.TrimSuffix(d.policyText(v), "。")
						}
						k.Text(c, summary, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true}).Grow(1).MinWidth(0)
						k.Text(c, d.tr(v.lastSeen), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).Shrink(0)
					})
				})
				if card.Clicked() {
					d.deviceList.Value, d.deviceDetailOpen, d.renaming = v.id, true, false
				}
			}

			if twoCols {
				for i := 0; i < len(ordered); i += 2 {
					ui.Row(c.Key(i)).Gap(12).AlignItems(ui.Stretch).Children(func() {
						renderCard(ordered[i])
						if i+1 < len(ordered) {
							renderCard(ordered[i+1])
						} else {
							ui.Box(c.Key("spacer")).Grow(1).MinWidth(0)
						}
					})
				}
			} else {
				for _, v := range ordered {
					renderCard(v)
				}
			}

			if len(peers) == 0 {
				ui.Column(c.Key("empty-peers")).Padding(20).Radius(12).Gap(10).AlignItems(ui.Center).
					Background(k.Color("background-secondary-default")).Border(1, k.Color("separator-border")).Children(func() {
					k.Text(c, d.tr("还没有关联设备"), desktopkit.TextProps{Emphasized: true})
					k.Text(c, d.tr("邀请或连接另一台设备，开始同步剪贴板。"), footnote)
					if k.Button(c.Key("invite-empty"), d.tr("邀请设备"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
						d.setDeviceTask("invite")
					}
				})
			}
		})

		// Candidates section (if any).
		if len(d.candidates) > 0 {
			ui.Column(c.Key("candidates")).Gap(8).Shrink(0).Children(func() {
				k.Text(c, d.tr("可能认识"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
				ui.Column(c.Key("cand-box")).Radius(12).Background(k.Color("background-primary-default")).
					Border(1, k.Color("separator-border")).Children(func() {
					for i, cd := range d.candidates {
						if i > 0 {
							ui.Box(c.Key(fmt.Sprint("sep", i))).Height(1).Background(k.Color("separator-border"))
						}
						cid := "cand:" + cd.id
						row := ui.Row(c.Key("cd-"+cd.id)).Padding(11, 14).Gap(10).AlignItems(ui.Center).Role(ui.RoleListItem).Label(cd.name)
						row.AccessSelected(d.deviceList.Value == cid)
						if row.Hovered() {
							row.Background(k.Color("background-secondary-hover"))
						}
						row.Children(func() {
							d.deviceIconTile(c.Key("ic"), k, cd.icon, 32, true)
							ui.Column(c.Key("meta")).Gap(2).Grow(1).MinWidth(0).Children(func() {
								k.Text(c, cd.name, desktopkit.TextProps{Emphasized: true, Truncate: true})
								k.Text(c, cd.introducer, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
							})
							if k.Button(c.Key("ignore"), d.tr("忽略"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Disabled: d.live != nil && d.live.busy}).Clicked() {
								if d.live != nil {
									client := d.live.client
									d.mutate(func(ctx context.Context) error { return client.IgnoreCandidate(ctx, cd.id, true) }, "已忽略")
								} else {
									d.candidates = slices.Delete(d.candidates, i, i+1)
									d.ensureDeviceSelection()
								}
							}
							if k.Button(c.Key("join"), d.tr("连接"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall, Disabled: d.live != nil && d.live.busy}).Clicked() {
								if d.live != nil {
									d.pairLive(cd.id, cd.backendID)
								} else {
									d.candidates = slices.Delete(d.candidates, i, i+1)
									d.ensureDeviceSelection()
									d.setDeviceTask("wait")
								}
							}
						})
						if row.Clicked() {
							d.deviceList.Value, d.deviceDetailOpen, d.renaming = cid, true, false
						}
					}
				})
			})
		}
	})
}

// pairCodeBoxes renders a 6-digit pairing code as six crisp individual tiles (NNN - NNN).
func (d *viewState) pairCodeBoxes(c *ui.Context, k desktopkit.Kit, code string) {
	digits := strings.ReplaceAll(strings.TrimSpace(code), "-", "")
	for len(digits) < 6 {
		digits += "·"
	}
	box := ui.Row(c.Key("code-boxes")).Padding(16, 10).Radius(14).Gap(6).
		Justify(ui.Center).AlignItems(ui.Center).
		Background(k.Color("background-secondary-default")).
		Border(1, k.Color("separator-border"))
	box.Children(func() {
		for i := 0; i < 6; i++ {
			if i == 3 {
				k.Text(c.Key("dash"), "-", desktopkit.TextProps{Tone: desktopkit.ToneTertiary, Emphasized: true}).Padding(0, 2)
			}
			tile := ui.Row(c.Key(i)).Size(44, 54).Radius(10).Justify(ui.Center).AlignItems(ui.Center).
				Background(k.Color("background-primary-default")).
				Border(1, k.Color("border-button-hover"))
			if !k.Dark {
				tile.Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, .05))
			}
			tile.Children(func() {
				k.Text(c, string(digits[i]), desktopkit.TextProps{Large: true, Monospace: true, Emphasized: true})
			})
		}
	})
}

// deviceTaskSheet hosts the compact Option A Add Device modal (440 DIP):
// Show Code / Enter Code with 6-digit boxes, quiet ticket toggle, and inline
// fingerprint approval without nested headings or phone-onboarding clutter.
func (d *viewState) deviceTaskSheet(c *ui.Context, k desktopkit.Kit) {
	open := d.deviceTask != ""
	if d.deviceTask == "phone" {
		d.setDeviceTask("invite")
	}
	inLiveApproval := d.live != nil && (d.deviceTask == "invite" || d.deviceTask == "wait") && len(d.pending) > 0
	closeModal := false
	header := func() {
		ui.Spacer(c)
		cb := ui.ButtonBase(c.Key("close-modal")).Label(d.tr("关闭")).Tooltip(d.tr("关闭")).
			Size(28, 28).Radius(8).Justify(ui.Center).AlignItems(ui.Center).
			FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
		ic := k.Color("foreground-icon-secondary")
		if cb.Hovered() {
			cb.Background(k.Color("background-secondary-hover"))
			ic = k.Color("foreground-icon-primary")
		}
		cb.Children(func() { k.Icon(c, desktopkit.IconDismiss, 15).TextColor(ic).AccessHidden(true) })
		if cb.Clicked() {
			closeModal = true
		}
	}
	done := k.Sheet(c, desktopkit.SheetProps{
		Variant: desktopkit.SheetCompact,
		Open:    &open,
		Title:   d.tr("添加设备"),
		Header:  header,
		OnClosed: func() {
			if d.deviceTask != "" {
				d.setDeviceTask("")
			}
		},
	}, func() {
		if d.deviceTask == "" {
			return
		}
		if d.live != nil {
			d.liveDeviceTaskView(c, k)
			return
		}
		if !inLiveApproval && (d.deviceTask == "invite" || d.deviceTask == "connect") {
			ui.Column(c.Key("add-body")).Gap(14).Children(func() {
				tab := d.deviceTask
				if k.SegmentedControl(c.Key("add-tab"), desktopkit.SegmentedControlProps{
					Label: d.tr("添加设备"),
					Value: &tab,
					Items: []desktopkit.SegmentedItem{
						{Value: "invite", Label: d.tr("出示配对码")},
						{Value: "connect", Label: d.tr("输入配对码")},
					},
				}).Changed {
					d.setDeviceTask(tab)
				}
				d.deviceTaskView(c, k)
			})
			return
		}
		d.deviceTaskView(c, k)
	})
	if done || closeModal || (!open && d.deviceTask != "") {
		d.setDeviceTask("")
	}
}

func btoi(b bool) int {
	if b {
		return 1
	}
	return 0
}

func (d *viewState) deviceDetail(c *ui.Context, k desktopkit.Kit) {
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
		k.ContentUnavailable(c, desktopkit.ContentUnavailableProps{Icon: desktopkit.IconDevices, Title: d.tr("还没有关联设备"), Description: d.tr("邀请或连接另一台设备，开始同步剪贴板。")}, func() {
			if k.Button(c, d.tr("邀请设备"), desktopkit.ButtonProps{Variant: desktopkit.Primary}).Clicked() {
				d.setDeviceTask("invite")
			}
		})
		return
	}
	if d.live != nil {
		d.liveDeviceDetail(c, k, v)
		return
	}
	ui.Column(c).Gap(16).Children(func() {
		state, tone := deviceStatus(*v)
		state = d.tr(state)
		// Hero card
		ui.Row(c.Key("hero")).Padding(14).Radius(12).Gap(12).AlignItems(ui.Center).
			Background(k.Color("background-primary-default")).Border(1, k.Color("separator-border")).Children(func() {
			d.deviceIconTile(c.Key("ic"), k, v.icon, 42, false)
			ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
				ui.Row(c.Key("name-row")).Gap(6).AlignItems(ui.Center).Children(func() {
					k.Text(c, v.name, desktopkit.TextProps{Emphasized: true, Truncate: true}).MinWidth(0)
					if v.id != "local" {
						rb := ui.ButtonBase(c.Key("rename")).Label(d.tr("重命名")).Tooltip(d.tr("重命名")).Size(22, 22).Radius(6).
							Justify(ui.Center).AlignItems(ui.Center)
						if rb.Hovered() {
							rb.Background(k.Color("background-secondary-hover"))
						}
						rb.Children(func() { k.Icon(c, desktopkit.IconEdit, 13).TextColor(k.Color("foreground-icon-secondary")) })
						if rb.Clicked() {
							d.renaming, d.renameValue = true, v.name
						}
					}
				})
				k.Text(c, v.platform+" · "+d.tr(v.lastSeen), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
				d.statusPill(c.Key("st"), k, state, tone).AlignSelf(ui.Start)
			})
		})

		if v.id == "local" {
			if v.fingerprint != "" {
				ui.Column(c.Key("local-fp")).Gap(8).Children(func() {
					ui.Row(c.Key("fp-head")).AlignItems(ui.Center).Children(func() {
						k.Text(c, d.tr("设备指纹"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
						if k.Button(c.Key("copy-fp"), d.tr("复制"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy}).Clicked() {
							d.deviceNote = "指纹已复制（演示，未写入剪贴板）"
						}
					})
					ui.Column(c.Key("fp-box")).Padding(12).Radius(12).Background(k.Color("background-primary-default")).
						Border(1, k.Color("separator-border")).Children(func() {
						d.fingerprintGrid(c, k, v.fingerprint)
					})
				})
			}
			k.Text(c, d.tr("同步策略在其他设备上设置，用来控制它与本机之间的同步。"), footnote)
			return
		}

		if !d.options["sync"] {
			k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneWarning, Title: d.tr("全局同步已关闭"), Text: d.tr("设备策略暂不生效。")})
		}
		if d.renaming {
			r := k.FormField(c, desktopkit.FormFieldProps{Label: d.tr("设备名称"), Value: &d.renameValue, AutoFocus: true, Hint: d.tr("只改本机显示的名称。对方自报的名称单独保存。")})
			if r.Submitted && strings.TrimSpace(d.renameValue) != "" {
				v.name, d.renaming = strings.TrimSpace(d.renameValue), false
			}
			if r.Escaped {
				d.renaming = false
			}
		}

		d.policyGroup(c, k, v)

		if len(v.paths) > 0 {
			k.SettingsGroup(c, desktopkit.SettingsGroupProps{Title: d.tr("连接路径")}, func() {
				for i, p := range v.paths {
					pt := secondary
					if p.failed {
						pt.Tone = desktopkit.ToneCritical
					}
					k.SettingsRow(c.Key(fmt.Sprint("path", i)), desktopkit.SettingsRowProps{Label: p.name, Description: d.tr("最近同步 ") + p.lastSync}, nil, func() { k.Text(c, d.tr(p.state), pt) })
				}
			})
		}

		ui.Column(c.Key("fp-sec")).Gap(8).Children(func() {
			ui.Row(c.Key("fp-head")).AlignItems(ui.Center).Children(func() {
				k.Text(c, d.tr("设备指纹"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
				if k.Button(c.Key("copy-fp"), d.tr("复制"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy}).Clicked() {
					d.deviceNote = "指纹已复制（演示，未写入剪贴板）"
				}
			})
			ui.Column(c.Key("fp-box")).Padding(12).Radius(12).Background(k.Color("background-primary-default")).
				Border(1, k.Color("separator-border")).Children(func() {
				d.fingerprintGrid(c, k, v.fingerprint)
			})
			if d.deviceNote != "" {
				k.Text(c, d.tr(d.deviceNote), footnote)
			}
		})

		// Quick Pause/Resume & Remove actions.
		ui.Row(c.Key("dev-actions")).Gap(8).Padding(4, 0, 0, 0).Children(func() {
			pauseLabel := d.tr("暂停同步")
			if v.direction == "pause" {
				pauseLabel = d.tr("恢复同步")
			}
			if k.Button(c.Key("toggle-pause"), pauseLabel, desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Grow(1).Clicked() {
				if v.direction == "pause" {
					v.direction = "both"
				} else {
					v.direction = "pause"
				}
			}
			if k.Button(c.Key("remove"), d.tr("移除设备"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Size: desktopkit.SizeSmall, Icon: desktopkit.IconDelete}).Clicked() {
				d.confirm, d.confirmID = fmt.Sprintf(d.tr("移除%s？"), d.sourceName(v.id)), "device:"+v.id
			}
		})
	})
}

var (
	secondary = desktopkit.TextProps{Tone: desktopkit.ToneSecondary}
	footnote  = desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}
)

func (d *viewState) policyGroup(c *ui.Context, k desktopkit.Kit, v *device) {
	k.SettingsGroup(c, desktopkit.SettingsGroupProps{Title: d.tr("同步")}, func() {
		k.SettingsRow(c.Key("direction"), desktopkit.SettingsRowProps{Label: d.tr("同步方式")}, nil, func() {
			k.SegmentedControl(c, desktopkit.SegmentedControlProps{
				Label: d.tr("同步方式"),
				Value: &v.direction,
				Items: []desktopkit.SegmentedItem{
					{Value: "both", Label: d.tr("双向")},
					{Value: "send", Label: d.tr("仅发")},
					{Value: "recv", Label: d.tr("仅收")},
					{Value: "pause", Label: d.tr("暂停")},
				},
			})
		})
		off := v.direction == "pause"
		k.SettingsRow(c.Key("text"), desktopkit.SettingsRowProps{Label: d.tr("文字")}, nil, func() {
			k.Switch(c, &v.text, desktopkit.SwitchProps{Label: d.tr("同步文字"), Disabled: off})
		})
		k.SettingsRow(c.Key("image"), desktopkit.SettingsRowProps{Label: d.tr("图片")}, nil, func() {
			k.Switch(c, &v.image, desktopkit.SwitchProps{Label: d.tr("同步图片"), Disabled: off})
		})
		k.SettingsRow(c.Key("file"), desktopkit.SettingsRowProps{Label: d.tr("文件"), Description: d.tr("受全局 100 MB 上限限制")}, nil, func() {
			k.Switch(c, &v.file, desktopkit.SwitchProps{Label: d.tr("同步文件"), Disabled: off})
		})
	})
	k.Text(c, d.policyText(*v), footnote)
}

func (d *viewState) pendingDetail(c *ui.Context, k desktopkit.Kit, id string) {
	i := slices.IndexFunc(d.pending, func(p pendingDevice) bool { return p.id == id })
	if i < 0 {
		return
	}
	p := d.pending[i]
	ui.Column(c).Gap(16).Children(func() {
		bg := k.Color("accent-50")
		edge := k.Color("accent-200")
		if k.Dark {
			bg = k.Color("accent-950")
			edge = k.Color("accent-800")
		}
		ui.Row(c.Key("p-hero")).Padding(14).Radius(12).Gap(12).AlignItems(ui.Center).Background(bg).Border(1, edge).Children(func() {
			d.deviceIconTile(c.Key("ic"), k, p.icon, 42, true)
			ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
				k.Text(c, p.name, desktopkit.TextProps{Emphasized: true, Truncate: true})
				k.Text(c, p.platform+" · "+p.via, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
				d.statusPill(c.Key("st"), k, d.tr("请求加入"), desktopkit.StatusInfo).AlignSelf(ui.Start)
			})
		})
		ui.Column(c.Key("p-fp")).Gap(8).Children(func() {
			k.Text(c, d.tr("设备指纹"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
			ui.Column(c.Key("fp-box")).Padding(12).Radius(12).Gap(8).Background(k.Color("background-primary-default")).
				Border(1, k.Color("separator-border")).Children(func() {
				d.fingerprintGrid(c, k, p.fingerprint)
				k.Text(c, d.tr("对方屏幕上显示相同的指纹时，才接受。"), footnote)
			})
		})
		ui.Row(c.Key("p-acts")).Gap(10).Children(func() {
			if k.Button(c.Key("accept"), d.tr("接受"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: d.live != nil && d.live.busy}).Grow(1).Clicked() {
				if d.live != nil {
					d.approveLive(id, true)
					return
				}
				d.pending = slices.Delete(d.pending, i, i+1)
				d.addPeer("a"+p.id, p.name, p.platform, p.icon, p.fingerprint)
			}
			if k.Button(c.Key("deny"), d.tr("拒绝"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Disabled: d.live != nil && d.live.busy}).Clicked() {
				if d.live != nil {
					d.approveLive(id, false)
					return
				}
				d.pending = slices.Delete(d.pending, i, i+1)
				d.ensureDeviceSelection()
				d.deviceDetailOpen = false
			}
		})
		k.Text(c, d.tr("名称由对方自报，未经验证。拒绝后，对方再次请求会重新出现在这里。"), footnote)
	})
}

func (d *viewState) candidateDetail(c *ui.Context, k desktopkit.Kit, id string) {
	i := slices.IndexFunc(d.candidates, func(cd deviceCandidate) bool { return cd.id == id })
	if i < 0 {
		return
	}
	cd := d.candidates[i]
	ui.Column(c).Gap(16).Children(func() {
		ui.Row(c.Key("c-hero")).Padding(14).Radius(12).Gap(12).AlignItems(ui.Center).
			Background(k.Color("background-primary-default")).Border(1, k.Color("separator-border")).Children(func() {
			d.deviceIconTile(c.Key("ic"), k, cd.icon, 42, true)
			ui.Column(c.Key("meta")).Gap(3).Grow(1).MinWidth(0).Children(func() {
				k.Text(c, cd.name, desktopkit.TextProps{Emphasized: true, Truncate: true})
				k.Text(c, cd.introducer, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
			})
		})
		k.Text(c, d.tr("名称未经验证。连接后仍需双方各自批准。"), footnote)
		ui.Row(c.Key("c-acts")).Gap(10).Children(func() {
			if k.Button(c.Key("join"), d.tr("连接"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: d.live != nil && d.live.busy}).Grow(1).Clicked() {
				if d.live != nil {
					d.pairLive(cd.id, cd.backendID)
					return
				}
				d.candidates = slices.Delete(d.candidates, i, i+1)
				d.ensureDeviceSelection()
				d.setDeviceTask("wait")
			}
			if k.Button(c.Key("ignore"), d.tr("忽略"), desktopkit.ButtonProps{Disabled: d.live != nil && d.live.busy}).Clicked() {
				if d.live != nil {
					client := d.live.client
					d.mutate(func(ctx context.Context) error { return client.IgnoreCandidate(ctx, cd.id, true) }, "已忽略")
					return
				}
				d.candidates = slices.Delete(d.candidates, i, i+1)
				d.ensureDeviceSelection()
			}
		})
	})
}

func (d *viewState) addPeer(id, name, platform string, icon desktopkit.IconName, fingerprint string) {
	d.devices = append(d.devices, device{id: id, name: name, platform: platform, icon: icon, online: true, latency: 22, link: "局域网", lastSeen: "刚刚",
		direction: "both", text: true, image: true, file: true, fingerprint: fingerprint,
		paths: []devicePath{{name: "tailcat · 默认", state: "已连接", lastSync: "—"}}})
	d.deviceList.Value, d.deviceDetailOpen = id, true
}

func (d *viewState) deviceTaskView(c *ui.Context, k desktopkit.Kit) {
	switch d.deviceTask {
	case "invite":
		ticket := d.inviteMode == "ticket"
		k.Text(c, d.tr("在另一台设备的 UniClipboard 中输入下方一次性配对码，随后核对指纹即可完成关联。"), secondary)
		if !ticket {
			d.pairCodeBoxes(c, k, d.inviteCode)
			ui.Row(c.Key("meta")).AlignItems(ui.Center).Gap(6).Children(func() {
				ui.Box(c.Key("dot")).Size(7, 7).Radius(3.5).Background(k.Color("accent-500")).Shrink(0)
				k.Text(c, d.tr("300 秒内有效 · 仅限一次"), footnote).Grow(1)
				if k.Button(c.Key("copy-code"), d.tr("复制"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy}).Clicked() {
					d.deviceNote = "配对码已复制（演示，未写入剪贴板）"
				}
				if k.Button(c.Key("regen"), d.tr("刷新"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconRetry}).Clicked() {
					d.inviteCode, d.deviceNote = newPairCode(), ""
				}
			})
		} else {
			ui.Column(c.Key("ticket-box")).Padding(12, 14).Radius(12).Gap(10).
				Background(k.Color("background-secondary-default")).
				Border(1, k.Color("separator-border")).Children(func() {
				k.Text(c, "unic-pair:AQ7xK2m9pR4vN8qL…", desktopkit.TextProps{Monospace: true, Truncate: true})
				ui.Row(c.Key("ticket-act")).Justify(ui.End).Children(func() {
					if k.Button(c.Key("copy-ticket"), d.tr("复制票据"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall, Icon: desktopkit.IconCopy}).Clicked() {
						d.deviceNote = "票据已复制（演示，未写入剪贴板）"
					}
				})
			})
		}
		ui.Row(c.Key("ticket-toggle")).Justify(ui.Center).Children(func() {
			toggleLabel := d.tr("跨网或命令行配对？改用完整票据 →")
			if ticket {
				toggleLabel = d.tr("← 返回 6 位配对码")
			}
			if k.Button(c.Key("toggle-mode"), toggleLabel, desktopkit.ButtonProps{Variant: desktopkit.Link, Size: desktopkit.SizeSmall}).Clicked() {
				if ticket {
					d.inviteMode = "code"
				} else {
					d.inviteMode = "ticket"
				}
				d.deviceNote = ""
			}
		})
		if d.deviceNote != "" {
			k.Text(c, d.tr(d.deviceNote), footnote)
		}
		ui.Box(c.Key("sep")).Height(1).Background(k.Color("separator-border"))
		ui.Row(c.Key("foot")).AlignItems(ui.Center).Gap(8).Children(func() {
			k.Text(c, d.tr("配对全程端到端加密"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).Grow(1)
			if k.Button(c.Key("sim"), d.tr("模拟对方加入"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() {
				id := fmt.Sprintf("p%d", len(d.pending)+len(d.devices)+1)
				d.pending = append(d.pending, pendingDevice{id: id, name: "新电脑", platform: "Windows 11", via: "配对码 " + d.inviteCode, icon: desktopkit.IconDesktop, fingerprint: "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D"})
				d.deviceList.Value, d.deviceDetailOpen = "pending:"+id, true
				d.setDeviceTask("")
			}
		})
	case "connect":
		kind := connectKind(d.connectInput)
		hint := d.tr("支持 6 位配对码（NNN-NNN）或以 unic-pair: 开头的完整票据。")
		if kind != "" {
			hint = d.tr("识别为：") + kind
		}
		k.Text(c, d.tr("输入对方提供的配对码或完整票据。"), secondary)
		r := k.FormField(c, desktopkit.FormFieldProps{Label: d.tr("配对码或票据"), Value: &d.connectInput, Placeholder: d.tr("482-913 或 unic-pair:…"), Hint: hint, Error: d.tr(d.fieldError), AutoFocus: true})
		if r.Edited {
			d.fieldError = ""
		}
		submit := (r.Submitted && kind != "") || k.Button(c.Key("go"), d.tr("连接设备"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: kind == ""}).FillWidth().Clicked()
		if submit {
			if strings.ReplaceAll(strings.TrimSpace(d.connectInput), "-", "") == "000000" {
				d.fieldError = "配对码无效或已过期。"
			} else {
				d.setDeviceTask("wait")
			}
		}
	case "wait":
		ui.Column(c.Key("wait-body")).Gap(14).Children(func() {
			bg := k.Color("accent-50")
			edge := k.Color("accent-200")
			if k.Dark {
				bg = k.Color("accent-950")
				edge = k.Color("accent-800")
			}
			ui.Row(c.Key("banner")).Padding(12, 14).Radius(12).Gap(10).AlignItems(ui.Center).Background(bg).Border(1, edge).Children(func() {
				k.Icon(c, desktopkit.IconShield, 16).TextColor(k.Color("accent-500")).Shrink(0)
				ui.Column(c.Key("txt")).Gap(2).Grow(1).MinWidth(0).Children(func() {
					k.Text(c, d.tr("等待对方批准"), desktopkit.TextProps{Emphasized: true})
					k.Text(c, d.tr("在对方设备上核对下方指纹，然后批准。"), footnote)
				})
			})
			d.fingerprintGrid(c, k, "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D")
			ui.Box(c.Key("sep")).Height(1).Background(k.Color("separator-border"))
			ui.Row(c.Key("wait-foot")).Gap(8).AlignItems(ui.Center).Children(func() {
				if k.Button(c.Key("back"), d.tr("返回"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Icon: desktopkit.IconBack}).Clicked() {
					d.setDeviceTask("connect")
				}
				ui.Spacer(c)
				if k.Button(c.Key("reject"), d.tr("模拟对方拒绝"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() {
					d.setDeviceTask("connect")
					d.fieldError = "对方拒绝了请求，或网络不通。请检查后重试。"
				}
				if k.Button(c.Key("ok"), d.tr("模拟对方批准"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
					d.addPeer(fmt.Sprintf("joined%d", len(d.devices)), "对方设备", "macOS 15", desktopkit.IconDesktop, "4A8C 2E91 7B30 D6F5 08B1 E49A 53C2 7F6D")
					d.setDeviceTask("")
				}
			})
		})
	}
}
