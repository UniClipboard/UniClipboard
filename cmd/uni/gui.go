package main

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"uni/internal/api"
	"uni/internal/engine"
)

// gui is a client of the daemon: all state it shows comes over the socket.
type gui struct {
	c   *api.Client
	win *mygo.Window

	mu       sync.Mutex // guards the fields below; the view and goroutines both touch them
	status   api.Status
	daemonUp bool
	log      []string
	pairErr  string
	pairing  bool
	copied   bool

	input string // owned by the main thread
}

func runGUI() error {
	c, err := client()
	if err != nil {
		return err
	}
	g := &gui{c: c}

	mygo.App.SetName("uni")
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
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) { cancel() })
	mygo.App.WhenReady(func() {
		g.win = mygo.NewWindow(mygo.WindowOptions{
			Title: "uni", Width: 460, Height: 560, Content: ui.View(g.view),
		})
		go g.follow(ctx)
	})
	return mygo.App.Run()
}

// follow keeps the status fresh: it refreshes on every daemon event and
// reconnects when the daemon restarts.
func (g *gui) follow(ctx context.Context) {
	for ctx.Err() == nil {
		g.refresh(ctx)
		_ = g.c.Events(ctx, func(ev engine.Event) {
			if ev.Kind != "peers" {
				g.addLog(ev)
			}
			g.refresh(ctx)
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

func (g *gui) refresh(ctx context.Context) {
	cctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	st, err := g.c.Status(cctx)
	g.mu.Lock()
	g.daemonUp = err == nil
	if err == nil {
		g.status = st
	}
	g.mu.Unlock()
	g.win.Invalidate()
}

func (g *gui) addLog(ev engine.Event) {
	t := time.Unix(ev.Time, 0).Format("15:04:05")
	var line string
	switch ev.Kind {
	case "peer-up":
		line = fmt.Sprintf("%s  %s connected", t, ev.Peer)
	case "peer-down":
		line = fmt.Sprintf("%s  %s disconnected", t, ev.Peer)
	case "sent":
		line = fmt.Sprintf("%s  sent %s", t, ev.Info)
	case "received":
		line = fmt.Sprintf("%s  received %s from %s", t, ev.Info, ev.Peer)
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

func (g *gui) pair(addr string) {
	g.mu.Lock()
	g.pairing, g.pairErr = true, ""
	g.mu.Unlock()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
		defer cancel()
		_, err := g.c.Pair(ctx, addr)
		g.mu.Lock()
		g.pairing = false
		if err != nil {
			g.pairErr = err.Error()
		}
		g.mu.Unlock()
		if err == nil {
			g.refresh(ctx)
		}
		g.win.Invalidate()
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

func (g *gui) view(c *ui.Context) {
	c.OnShortcut(ui.Cmd, ui.KeyW, g.win.Close)
	c.OnShortcut(ui.Cmd, ui.KeyQ, mygo.App.Quit)
	t := c.Theme()

	g.mu.Lock()
	st, up, logs, pairErr, pairing := g.status, g.daemonUp, append([]string(nil), g.log...), g.pairErr, g.pairing
	g.mu.Unlock()

	ui.Column(c).Fill().Padding(20).Gap(14).Children(func() {
		ui.Row(c).Gap(10).Children(func() {
			ui.Text(c, "uni").FontSize(24).Bold()
			ui.Spacer(c)
			if up {
				ui.Badge(c, "daemon running")
			} else {
				ui.Badge(c, "daemon not running")
			}
		})

		if !up {
			ui.Text(c, "Start the background node with `uni daemon`; this window reconnects by itself.").
				TextColor(t.TextMuted)
			return
		}

		// This device.
		ui.Column(c).Gap(4).Children(func() {
			ui.Text(c, "This device").FontSize(12).TextColor(t.TextMuted)
			ui.Text(c, st.Name).Bold()
			ui.Row(c).Gap(8).Children(func() {
				addr := st.Addr
				if addr == "" {
					addr = "starting…"
				}
				ui.Text(c, trunc(addr, 28)+"…").Grow(1).FontSize(12).TextColor(t.TextMuted)
				if ui.Button(c.Key("copy-addr"), "Copy address").Disabled(st.Addr == "").Clicked() {
					mygo.Clipboard.WriteText(st.Addr)
				}
			})
			ui.Text(c, "The address works like a password: share it only with your own devices.").
				FontSize(11).TextColor(t.TextMuted)
		})

		ui.Divider(c)

		// Peers.
		ui.Text(c, "Paired devices").FontSize(12).TextColor(t.TextMuted)
		if len(st.Peers) == 0 {
			ui.Text(c, "None yet. Paste the address of another device below.").TextColor(t.TextMuted)
		}
		for _, p := range st.Peers {
			ui.Row(c.Key("peer-"+p.ID)).Gap(8).Children(func() {
				ui.Text(c, p.Name).Grow(1)
				if p.Connected {
					ui.Badge(c, "online")
				} else {
					ui.Badge(c, "offline")
				}
				if ui.Button(c.Key("rm-"+p.ID), "Remove").Clicked() {
					g.unpair(p.ID)
				}
			})
		}

		// Pairing.
		ui.Row(c).Gap(8).Children(func() {
			ui.TextInput(c.Key("pair-input"), &g.input).Grow(1).Placeholder("Address of the other device")
			addr := strings.TrimSpace(g.input)
			if ui.PrimaryButton(c.Key("pair"), "Pair").Disabled(addr == "" || pairing).Clicked() {
				g.input = ""
				g.pair(addr)
			}
		})
		if pairing {
			ui.Text(c, "Connecting…").FontSize(12).TextColor(t.TextMuted)
		}
		if pairErr != "" {
			ui.Text(c, pairErr).FontSize(12).TextColor(t.Danger)
		}

		ui.Divider(c)
		ui.Text(c, "Activity").FontSize(12).TextColor(t.TextMuted)
		ui.List(c, nil, len(logs), func(i int) {
			ui.Text(c, logs[i]).FontSize(12).Padding(2, 0)
		}).Grow(1)
	})
}
