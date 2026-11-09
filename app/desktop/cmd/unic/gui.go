package main

import (
	"context"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
)

// Tabs and panes; the layout follows prototype/gui.html.
const (
	tabHistory = iota
	tabDevices
	tabActivity
	tabAdd
	tabSettings
)

const (
	addShow = iota // 让设备加入本机: show a pairing code
	addJoin        // 加入其他设备: enter one
)

const (
	joinCode = iota
	joinAddr
)

// gui is a client of the daemon: all state it shows comes over the socket.
type gui struct {
	c   *api.Client
	win *mygo.Window

	mu       sync.Mutex // guards the fields below; the view and goroutines both touch them
	status   api.Status
	daemonUp bool
	clip     string // summary of the clipboard
	log      []string

	// The code this device shows.
	showing  bool // the pane showing it is on screen; set by the view
	code     api.Code
	hasCode  bool
	codeBusy bool
	codeErr  string // reason, see api.ReasonOf; "" when fine

	// Joining another device.
	joinBusy bool
	joinMsg  string // what joinBusy is doing
	joinErr  string
	joinWait *api.PairResult // found the device; waiting for it to approve this one
	joinDone string          // name of the device just joined

	// Owned by the main thread.
	tab, addMode, joinMode      int
	codeInput, addrInput        string
	revealAddr, addrOpen        bool
	pairingTicket               string
	ticketBusy                  bool
	confirmRemove               string
	history                     []api.HistoryItem
	historyPreviews             map[string]string
	historyCursor, historyError string
	historyBusy, historyLoaded  bool
	historyTag                  string
	cleanupUI                   cleanupUIState
}

func runGUI() error {
	c, err := client()
	if err != nil {
		return err
	}
	g := &gui{c: c}

	mygo.App.SetName("unic")
	if !mygo.App.RequestSingleInstanceLock() {
		return nil // the running window comes forward
	}
	mygo.App.OnSecondInstance(func([]string, string) {
		if g.win != nil {
			g.win.Show()
			g.win.Focus()
		}
	})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) {
		// A code nobody is looking at should not stay usable.
		qctx, qcancel := context.WithTimeout(context.Background(), 2*time.Second)
		_ = g.c.CancelCode(qctx)
		qcancel()
		cancel()
	})
	mygo.App.WhenReady(func() {
		g.win = mygo.NewWindow(mygo.WindowOptions{
			Title: "unic", Width: 480, Height: 640, Content: ui.View(g.view),
		})
		go g.follow(ctx)
		go g.tick(ctx)
	})
	return mygo.App.Run()
}

// follow keeps the status fresh: it refreshes on every daemon event and
// reconnects when the daemon restarts.
func (g *gui) follow(ctx context.Context) {
	for ctx.Err() == nil {
		g.refresh(ctx)
		_ = g.c.Events(ctx, func(ev api.Event) {
			g.addLog(ev)
			if ev.Kind == "code" { // our code was used: show a fresh one
				g.mu.Lock()
				g.hasCode = false
				g.mu.Unlock()
			}
			g.refresh(ctx)
			if ev.Kind == "history" {
				g.loadHistory("", "")
			}
		})
		g.mu.Lock()
		g.daemonUp = false
		g.mu.Unlock()
		g.win.Invalidate()
		select {
		case <-ctx.Done():
		case <-time.After(2 * time.Second):
		}
	}
}

// tick redraws the countdown and keeps a valid code on screen while the
// pane that shows it is open. Codes are only made while someone looks.
func (g *gui) tick(ctx context.Context) {
	t := time.NewTicker(time.Second)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
		}
		if showing, need := g.codeState(); need {
			g.fetchCode(false)
		} else if showing {
			g.win.Invalidate() // the countdown
		}
	}
}

// codeState reports whether the code pane is on screen, and whether it needs
// a code it does not have.
func (g *gui) codeState() (showing, need bool) {
	g.mu.Lock()
	defer g.mu.Unlock()
	stale := !g.hasCode || !time.Now().Before(g.code.Expires())
	return g.showing, g.showing && stale && !g.codeBusy && g.codeErr == ""
}

func (g *gui) refresh(ctx context.Context) {
	cctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	st, err := g.c.Status(cctx)
	var clip string
	if err == nil {
		if v, cerr := g.c.Clipboard(cctx); cerr == nil {
			clip = v.Summary
		}
	}
	g.mu.Lock()
	g.daemonUp = err == nil
	if err == nil {
		g.status = st
		g.clip = clip
		if w := g.joinWait; w != nil {
			for _, p := range st.Peers {
				if p.ID == w.ID && p.Connected {
					g.joinWait, g.joinDone = nil, p.Name
				}
			}
		}
	}
	g.mu.Unlock()
	g.win.Invalidate()
	if err == nil {
		g.mu.Lock()
		need := !g.historyLoaded
		g.mu.Unlock()
		if need {
			g.loadHistory("", "")
		}
	}
}

func (g *gui) addLog(ev api.Event) {
	t := time.Unix(ev.Time, 0).Format("15:04:05")
	var line string
	switch ev.Kind {
	case "peer-up":
		line = fmt.Sprintf("%s  %s 已连接", t, ev.Peer)
	case "peer-down":
		line = fmt.Sprintf("%s  %s 已断开", t, ev.Peer)
	case "sent":
		line = fmt.Sprintf("%s  发送 %s", t, ev.Info)
	case "received":
		line = fmt.Sprintf("%s  收到 %s,来自 %s", t, ev.Info, ev.Peer)
	case "pending":
		switch ev.Info {
		case "":
			line = fmt.Sprintf("%s  %s 请求加入", t, ev.Peer)
		case "code":
			line = fmt.Sprintf("%s  %s 通过配对码请求加入", t, ev.Peer)
		default:
			line = fmt.Sprintf("%s  %s 请求加入(由 %s 介绍)", t, ev.Peer, ev.Info)
		}
	default:
		return
	}
	g.mu.Lock()
	g.log = append([]string{line}, g.log...)
	if len(g.log) > 100 {
		g.log = g.log[:100]
	}
	g.mu.Unlock()
}

// ---- actions ----

// fetchCode shows the live code, or registers a new one if there is none or
// fresh is set (换一个).
func (g *gui) fetchCode(fresh bool) {
	g.mu.Lock()
	if g.codeBusy {
		g.mu.Unlock()
		return
	}
	g.codeBusy, g.codeErr = true, ""
	g.mu.Unlock()
	g.win.Invalidate()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		var (
			code api.Code
			err  = errors.New("no live code")
		)
		if !fresh {
			code, err = g.c.Code(ctx)
		}
		if err != nil || !time.Now().Before(code.Expires()) {
			code, err = g.c.NewCode(ctx)
		}
		g.mu.Lock()
		g.codeBusy = false
		if err != nil {
			g.hasCode = false
			g.codeErr = api.ReasonOf(err)
			if g.codeErr == "" {
				g.codeErr = "unavailable"
			}
		} else {
			g.code, g.hasCode = code, true
		}
		g.mu.Unlock()
		g.win.Invalidate()
	}()
}

func (g *gui) startJoin(msg string, do func(ctx context.Context) (api.PairResult, error)) {
	g.mu.Lock()
	g.joinBusy, g.joinMsg, g.joinErr, g.joinWait, g.joinDone = true, msg, "", nil, ""
	g.mu.Unlock()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
		defer cancel()
		r, err := do(ctx)
		g.mu.Lock()
		g.joinBusy = false
		switch {
		case err != nil:
			g.joinErr = joinError(err)
		case r.Awaiting:
			g.joinWait = &r
		default:
			g.joinDone = r.Name
		}
		g.mu.Unlock()
		g.refresh(ctx)
	}()
}

func joinError(err error) string {
	switch api.ReasonOf(err) {
	case "incomplete":
		return "请输入完整的 6 位数字。"
	case "expired":
		return "这个配对码已过期或已被使用,请让对方换一个。"
	case "invalid":
		return "这不是 unic 的配对码,请核对数字后重试。"
	case "rate-limited":
		return "尝试次数过多,请稍等一分钟再试。"
	case "unavailable":
		return "暂时无法核对配对码,请改用完整地址。"
	}
	if errors.Is(err, api.ErrNoDaemon) {
		return "后台节点未运行。"
	}
	return "无法连接:" + err.Error()
}

func (g *gui) fetchTicket() {
	g.mu.Lock()
	if g.ticketBusy {
		g.mu.Unlock()
		return
	}
	g.ticketBusy = true
	g.mu.Unlock()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		ticket, err := g.c.ExportTicket(ctx)
		g.mu.Lock()
		g.ticketBusy = false
		if err == nil {
			g.pairingTicket = ticket
		} else {
			g.pairingTicket = ""
		}
		g.mu.Unlock()
		g.win.Invalidate()
	}()
}

func (g *gui) decide(p api.PendingPeer, approve bool) {
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if approve {
			_, _ = g.c.Approve(ctx, p.ID, api.Approval{Fingerprint: p.Fingerprint, Revision: p.Revision, Bindings: p.Bindings})
		} else {
			_ = g.c.Reject(ctx, p.ID)
		}
		g.refresh(ctx)
	}()
}

func (g *gui) unpair(id string) {
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_ = g.c.Unpair(ctx, id)
		g.refresh(ctx)
	}()
}

// ---- view ----

// snapshot is what one frame shows, copied under the lock.
type snapshot struct {
	pairingTicket               string
	ticketBusy                  bool
	st                          api.Status
	up                          bool
	clip                        string
	logs                        []string
	code                        api.Code
	hasCode, codeBusy           bool
	codeErr                     string
	joinBusy                    bool
	joinMsg, joinErr, joinDone  string
	joinWait                    *api.PairResult
	history                     []api.HistoryItem
	historyPreviews             map[string]string
	historyCursor, historyError string
	historyBusy                 bool
}

func (g *gui) view(c *ui.Context) {
	k := uikit.For(c.Theme().Dark)
	k.Apply(c)
	k.UiRoot(c, func() { g.content(c, k) })
}

func (g *gui) content(c *ui.Context, k uikit.Kit) {
	if g.win != nil {
		c.OnShortcut(ui.Cmd, ui.KeyW, g.win.Close)
	}
	c.OnShortcut(ui.Cmd, ui.KeyQ, mygo.App.Quit)
	t := c.Theme()

	g.mu.Lock()
	s := snapshot{
		pairingTicket: g.pairingTicket, ticketBusy: g.ticketBusy, st: g.status, up: g.daemonUp, clip: g.clip, logs: append([]string(nil), g.log...),
		code: g.code, hasCode: g.hasCode, codeBusy: g.codeBusy, codeErr: g.codeErr,
		joinBusy: g.joinBusy, joinMsg: g.joinMsg, joinErr: g.joinErr, joinDone: g.joinDone, joinWait: g.joinWait,
		history: g.history, historyPreviews: g.historyPreviews, historyCursor: g.historyCursor, historyError: g.historyError, historyBusy: g.historyBusy,
	}
	g.mu.Unlock()

	if !s.up {
		g.mu.Lock()
		g.showing = false
		g.mu.Unlock()
		ui.Column(c).Fill().Padding(40).Gap(10).Center().Children(func() {
			k.ContentUnavailable(c, uikit.ContentUnavailableProps{
				Icon: uikit.IconDevices, Title: "后台节点未运行",
				Description: "unic 的同步由 unicd 负责。启动 unicd 后,此窗口会自动重新连接。",
			}, func() {
				ui.Row(c).Gap(8).Center().Children(func() {
					ui.Spinner(c)
					k.Text(c, "每 2 秒重试连接", uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
				})
			})
		})
		return
	}

	online := 0
	for _, p := range s.st.Peers {
		if p.Connected {
			online++
		}
	}

	ui.Column(c).Fill().Children(func() {
		ui.Column(c).Grow(1).MinHeight(0).Padding(16, 0, 12).Gap(14).Children(func() {
			// This device.
			k.Surface(c, uikit.RegionSidebar).MarginX(20).Padding(14).Radius(16).Children(func() {
				ui.Column(c).Gap(2).Grow(1).Children(func() {
					k.Text(c, s.st.Name, uikit.TextProps{}).FontSize(15).Bold()
					sub := "同步中 · 尚未配对设备"
					if len(s.st.Peers) > 0 {
						sub = fmt.Sprintf("同步中 · %d/%d 台设备在线", online, len(s.st.Peers))
					}
					k.Text(c, sub, uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
				})
			})

			devices := "设备"
			if n := len(s.st.Pending); n > 0 {
				devices = fmt.Sprintf("设备 (%d)", n)
			}
			ui.Box(c).PaddingX(20).Children(func() {
				kitSegments(k, c.Key("tabs"), &g.tab, "历史", devices, "动态", "添加设备").FillWidth()
			})

			if g.tab == tabHistory {
				// The history toolbar stays outside the scrolling list: its focus
				// ring is never clipped, and the empty state gets the remaining height.
				ui.Column(c).Grow(1).MinHeight(0).Padding(4, 20, 8).Gap(10).Children(func() {
					g.historyTab(c, s)
				})
			} else if g.tab == tabSettings {
				g.cleanupSettingsTab(c, k)
			} else {
				ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
					// Keep focus rings inside the scroll viewport's clip, and let short
					// pages retain a gutter on every side.
					ui.Column(c).Padding(4, 20, 8).Gap(10).Children(func() {
						switch g.tab {
						case tabDevices:
							g.devicesTab(c, s)
						case tabActivity:
							g.activityTab(c, s)
						default:
							g.addTab(c, s)
						}
					})
				})
			}
		})
		// Tell the ticker whether to keep a code on screen; this frame's
		// clicks have already moved the tabs.
		g.mu.Lock()
		g.showing = g.tab == tabAdd && g.addMode == addShow
		g.mu.Unlock()
		if _, need := g.codeState(); need {
			go g.fetchCode(false)
		}
		ui.Divider(c)
		ui.Row(c).Padding(8, 16).Gap(8).Children(func() {
			k.Text(c, "daemon 运行中", uikit.TextProps{}).FontSize(11).TextColor(t.TextMuted)
			ui.Spacer(c)
			if k.Button(c.Key("settings"), "设置", uikit.ButtonProps{}).Clicked() {
				g.tab = tabSettings
				g.loadCleanupSettings()
			}
			k.Text(c, "ID "+s.st.ID, uikit.TextProps{}).FontSize(11).TextColor(t.TextMuted)
		})
	})
	g.removeDialog(c, k, s)
}

// kitSegments bridges the GUI's tab indexes to the Kit's stable string values.
func kitSegments(k uikit.Kit, c *ui.Context, selected *int, labels ...string) ui.Element {
	value := strconv.Itoa(*selected)
	items := make([]uikit.SegmentedItem, len(labels))
	for i, label := range labels {
		items[i] = uikit.SegmentedItem{Value: strconv.Itoa(i), Label: label}
	}
	r := k.SegmentedControl(c, uikit.SegmentedControlProps{Items: items, Value: &value})
	if r.Changed {
		*selected, _ = strconv.Atoi(value)
	}
	return r.Element
}

func (g *gui) removeDialog(c *ui.Context, k uikit.Kit, s snapshot) {
	if g.confirmRemove == "" {
		return
	}
	for _, p := range s.st.Peers {
		if p.ID != g.confirmRemove {
			continue
		}
		open := true
		r := k.AlertDialog(c.Key("remove-device"), uikit.AlertDialogProps{
			Open: &open, Title: "移除 " + p.Name + "?",
			Description:  "之后不再同步,需要重新配对。",
			ConfirmLabel: "移除", CancelLabel: "取消", Destructive: true,
		}, nil)
		if !open {
			g.confirmRemove = ""
		}
		if r.Confirmed {
			g.unpair(p.ID)
		}
		return
	}
	g.confirmRemove = ""
}

func label(c *ui.Context, s string) {
	k := uikit.For(c.Theme().Dark)
	k.Text(c, s, uikit.TextProps{}).FontSize(12).Bold().TextColor(c.Theme().TextMuted)
}

func (g *gui) devicesTab(c *ui.Context, s snapshot) {
	k := uikit.For(c.Theme().Dark)
	t := c.Theme()
	// Devices waiting for a decision come first: they need the user.
	if len(s.st.Pending) > 0 {
		label(c, "等待你批准 · 批准后才会连接与同步")
		for _, p := range s.st.Pending {
			g.pendingCard(c, p)
		}
	}

	label(c, "当前剪贴板")
	clip := s.clip
	if clip == "" {
		clip = "还没有内容。复制文本、图片或文件,会自动发送到已配对设备。"
	}
	k.Text(c, clip, uikit.TextProps{}).Padding(10, 12).Radius(10).Border(1, t.Border).FillWidth()

	k.SettingsGroup(c, uikit.SettingsGroupProps{Title: "已配对设备"}, func() {
		if len(s.st.Peers) == 0 {
			k.ContentUnavailable(c, uikit.ContentUnavailableProps{
				Icon: uikit.IconDevices, Title: "还没有配对的设备",
				Description: "在另一台电脑上运行 unic,用 6 位配对码或完整地址配对。",
			}, func() {
				if k.Button(c.Key("go-add"), "添加设备", uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
					g.tab = tabAdd
				}
			})
			return
		}
		for _, p := range s.st.Peers {
			state := "离线"
			if p.Connected {
				state = "在线"
			} else if p.LastSeen > 0 {
				state = "离线 · " + ago(time.Unix(p.LastSeen, 0)) + "在线"
			}
			k.SettingsRow(c.Key("peer-"+p.ID), uikit.SettingsRowProps{
				Label: p.Name, Description: state,
			}, nil, func() {
				if k.Button(c.Key("rm-"+p.ID), "移除…", uikit.ButtonProps{}).Clicked() {
					g.confirmRemove = p.ID
				}
			})
		}
	})
}

func (g *gui) pendingCard(c *ui.Context, p api.PendingPeer) {
	k := uikit.For(c.Theme().Dark)
	t := c.Theme()
	color := t.Warning
	if p.ByCode {
		color = t.Accent
	}
	ui.Column(c.Key("pend-"+p.ID)).Gap(8).Padding(12).Radius(10).
		Background(color.Alpha(0.1)).Border(1, color.Alpha(0.45)).Children(func() {
		from := "直接连接到本机"
		switch {
		case p.ByCode:
			from = "通过配对码"
		case p.Via != "":
			from = "由 " + p.Via + " 介绍"
		}
		k.Text(c, p.Name+" 请求加入", uikit.TextProps{}).Bold()
		k.Text(c, from+" · "+ago(time.Unix(p.Time, 0)), uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
		k.Text(c, "确认对方屏幕上显示相同的指纹:", uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
		k.Text(c, p.Fingerprint, uikit.TextProps{}).Font("monospace").FontSize(15).Bold().LetterSpacing(1.5).
			Padding(2, 8).Radius(5).Background(t.Background).AlignSelf(ui.Start)
		ui.Row(c).Gap(8).Children(func() {
			approve := "批准"
			if p.ByCode {
				approve = "指纹一致,批准"
			}
			if k.Button(c.Key("ok-"+p.ID), approve, uikit.ButtonProps{Variant: uikit.Primary}).Clicked() {
				g.decide(p, true)
			}
			if k.Button(c.Key("no-"+p.ID), "拒绝", uikit.ButtonProps{}).Clicked() {
				g.decide(p, false)
			}
			if p.Via == "" && !p.ByCode {
				k.Text(c, "不认识的设备请拒绝", uikit.TextProps{}).FontSize(11).TextColor(t.Warning)
			}
		})
	})
}

func (g *gui) activityTab(c *ui.Context, s snapshot) {
	k := uikit.For(c.Theme().Dark)
	t := c.Theme()
	if len(s.logs) == 0 {
		k.Text(c, "暂无动态。设备连接、剪贴板收发都会记录在这里(仅本次打开期间)。", uikit.TextProps{}).
			TextColor(t.TextMuted).Padding(20)
		return
	}
	for i, l := range s.logs {
		k.Text(c.Key(i), l, uikit.TextProps{}).FontSize(12).FontFeatures("tnum").Padding(3, 0)
	}
}

func (g *gui) addTab(c *ui.Context, s snapshot) {
	k := uikit.For(c.Theme().Dark)
	kitSegments(k, c.Key("add-mode"), &g.addMode, "让设备加入本机", "加入其他设备").FillWidth()
	if g.addMode == addShow {
		g.showPane(c, s)
	} else {
		g.joinPane(c, s)
	}
}

func (g *gui) showPane(c *ui.Context, s snapshot) {
	k := uikit.For(c.Theme().Dark)
	t := c.Theme()
	ui.Column(c).Gap(10).Padding(20, 22).Radius(12).Border(1, t.Border).AlignItems(ui.Center).Children(func() {
		k.Text(c, "在另一台电脑的 unic 里输入这个配对码", uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
		mono := func(s string) ui.Element {
			return k.Text(c, s, uikit.TextProps{}).Font("monospace").FontSize(44).Bold().LetterSpacing(3).FontFeatures("tnum")
		}
		switch {
		case s.codeErr != "":
			mono("––– –––").TextColor(t.TextMuted)
			msg := "暂时无法获取配对码"
			if s.codeErr == "rate-limited" {
				msg = "获取配对码太频繁,请稍等一分钟"
			}
			k.Text(c, msg, uikit.TextProps{}).FontSize(12).TextColor(t.Danger)
			if k.Button(c.Key("code-retry"), "重试", uikit.ButtonProps{}).Clicked() {
				g.fetchCode(true)
			}
		case !s.hasCode || s.codeBusy:
			mono("––– –––").TextColor(t.TextMuted)
			ui.Row(c).Gap(6).Children(func() {
				ui.Spinner(c)
				k.Text(c, "正在获取配对码…", uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
			})
		default:
			left := time.Until(s.code.Expires())
			mono(s.code.Display())
			progress := max(0, left.Seconds()/3)
			k.ProgressBar(c, uikit.ProgressBarProps{Label: "配对码有效期", Value: &progress}).FillWidth()
			ui.Row(c).FillWidth().Children(func() {
				k.Text(c, countdown(left)+" 后过期", uikit.TextProps{}).FontSize(11).FontFeatures("tnum").TextColor(t.TextMuted)
				ui.Spacer(c)
				k.Text(c, "仅可使用一次", uikit.TextProps{}).FontSize(11).TextColor(t.TextMuted)
			})
			ui.Row(c).Gap(8).Children(func() {
				if k.Button(c.Key("code-copy"), "复制", uikit.ButtonProps{}).Clicked() {
					mygo.Clipboard.WriteText(strings.ReplaceAll(s.code.Code, "-", ""))
				}
				if k.Button(c.Key("code-new"), "换一个", uikit.ButtonProps{}).Clicked() {
					g.fetchCode(true)
				}
			})
		}
	})
	note := "配对码过期或被使用后会自动换新。对方输入后,你需要在这里核对指纹并批准。"
	if s.codeErr != "" {
		note = "使用下方的完整地址仍然可以配对,不需要配对码服务。"
	}
	k.Text(c, note, uikit.TextProps{}).FontSize(11).TextColor(t.TextMuted)

	for _, p := range s.st.Pending {
		if p.ByCode {
			g.pendingCard(c, p)
		}
	}

	k.Disclosure(c.Key("full-addr"), "完整地址 · 用于命令行或无法使用配对码时", &g.addrOpen, func() {
		ui.Column(c).Gap(8).PaddingY(6).Children(func() {
			addr := s.pairingTicket
			shown := "点击生成配对票据"
			if addr != "" {
				shown = trunc(addr, 10) + strings.Repeat("•", 24) + addr[max(0, len(addr)-4):]
				if g.revealAddr {
					shown = addr
				}
			}
			k.Text(c, shown, uikit.TextProps{}).Font("monospace").FontSize(11).TextColor(t.TextMuted).
				Padding(6, 8).Radius(6).Background(t.Surface).FillWidth()
			ui.Row(c).Gap(8).Children(func() {
				if k.Button(c.Key("addr-generate"), "生成票据", uikit.ButtonProps{Disabled: s.ticketBusy}).Clicked() {
					g.fetchTicket()
				}
				show := "显示"
				if g.revealAddr {
					show = "隐藏"
				}
				if k.Button(c.Key("addr-reveal"), show, uikit.ButtonProps{Disabled: addr == ""}).Clicked() {
					g.revealAddr = !g.revealAddr
				}
				if k.Button(c.Key("addr-copy"), "复制", uikit.ButtonProps{Disabled: addr == ""}).Clicked() {
					mygo.Clipboard.WriteText(addr)
				}
			})
			k.Text(c, "票据等同密码,5 分钟有效且仅能使用一次。只分享给你自己的设备。", uikit.TextProps{}).
				FontSize(11).TextColor(t.Warning)
		})
	})
}

func (g *gui) joinPane(c *ui.Context, s snapshot) {
	k := uikit.For(c.Theme().Dark)
	t := c.Theme()
	kitSegments(k, c.Key("join-mode"), &g.joinMode, "6 位数字", "完整地址")

	if g.joinMode == joinCode {
		k.Text(c, "输入另一台设备「添加设备」页上显示的 6 位配对码。", uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
		ui.Row(c).Gap(8).Children(func() {
			g.codeInput = strings.ReplaceAll(formatCode(g.codeInput), " ", "")
			submit := func() {
				if !s.joinBusy && len(g.codeInput) == 6 {
					code := g.codeInput
					g.startJoin("正在核对配对码…", func(ctx context.Context) (api.PairResult, error) {
						return g.c.Join(ctx, code)
					})
				}
			}
			field := k.CodeField(c.Key("code-input"), uikit.CodeFieldProps{
				Label: "6 位配对码", Length: 6, Value: &g.codeInput, Disabled: s.joinBusy,
			})
			field.Element.Grow(1)
			if field.Submitted {
				submit()
			}
			ready := len(g.codeInput) == 6
			if k.Button(c.Key("join"), "配对", uikit.ButtonProps{Variant: uikit.Primary, Disabled: !ready || s.joinBusy}).Clicked() {
				submit()
			}
		})
	} else {
		k.Text(c, "粘贴另一台设备的完整地址(unic addr 的输出)。", uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
		field := k.TextField(c.Key("addr-input"), uikit.TextFieldProps{
			Label: "完整地址", Value: &g.addrInput, Placeholder: "tc…", Disabled: s.joinBusy,
		})
		field.Element.FillWidth()
		field.Input.Font("monospace")
		ui.Row(c).Gap(8).Children(func() {
			ui.Spacer(c)
			addr := strings.TrimSpace(g.addrInput)
			if k.Button(c.Key("pair"), "配对", uikit.ButtonProps{Variant: uikit.Primary, Disabled: addr == "" || s.joinBusy}).Clicked() {
				g.startJoin("正在连接…最长约 45 秒", func(ctx context.Context) (api.PairResult, error) {
					return g.c.Pair(ctx, addr)
				})
			}
		})
	}

	switch {
	case s.joinBusy:
		ui.Row(c).Gap(6).Children(func() {
			ui.Spinner(c)
			k.Text(c, s.joinMsg, uikit.TextProps{}).FontSize(12).TextColor(t.TextMuted)
		})
	case s.joinErr != "":
		k.Text(c, s.joinErr, uikit.TextProps{}).FontSize(12).TextColor(t.Danger)
	case s.joinWait != nil:
		ui.Column(c).Gap(6).Padding(12).Radius(10).
			Background(t.Accent.Alpha(0.1)).Border(1, t.Accent.Alpha(0.45)).Children(func() {
			k.Text(c, "等待 "+s.joinWait.Name+" 批准", uikit.TextProps{}).Bold()
			k.Text(c, "在 "+s.joinWait.Name+" 上核对下面的指纹,一致后批准。批准后自动开始同步。", uikit.TextProps{}).
				FontSize(12).TextColor(t.TextMuted)
			k.Text(c, s.joinWait.Fingerprint, uikit.TextProps{}).Font("monospace").FontSize(15).Bold().LetterSpacing(1.5)
		})
	case s.joinDone != "":
		k.Text(c, "✓ 已连接 "+s.joinDone+",开始同步", uikit.TextProps{}).FontSize(12).TextColor(t.Success)
	}
}

// formatCode keeps digits only and groups them "NNN NNN" while typing.
func formatCode(in string) string {
	var d []rune
	for _, r := range in {
		if r >= '０' && r <= '９' {
			r = r - '０' + '0'
		}
		if r >= '0' && r <= '9' && len(d) < 6 {
			d = append(d, r)
		}
	}
	if len(d) > 3 {
		return string(d[:3]) + " " + string(d[3:])
	}
	return string(d)
}

func countdown(d time.Duration) string {
	s := int(max(0, d.Seconds()+0.999))
	return fmt.Sprintf("%d:%02d", s/60, s%60)
}

func ago(t time.Time) string {
	d := time.Since(t)
	switch {
	case d < time.Minute:
		return "刚刚"
	case d < time.Hour:
		return fmt.Sprintf("%d 分钟前", int(d.Minutes()))
	case d < 24*time.Hour:
		return fmt.Sprintf("%d 小时前", int(d.Hours()))
	default:
		return fmt.Sprintf("%d 天前", int(d.Hours()/24))
	}
}
