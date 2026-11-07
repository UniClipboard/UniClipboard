package main

import (
	"bytes"
	"context"
	"fmt"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"log"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"time"

	"github.com/egoist/mygo"

	"unic/internal/api"
	"unic/internal/board"
	"unic/internal/config"
	"unic/internal/desktop"
	"unic/internal/paths"
)

// runDaemon runs the node as a windowless mygo app: mygo owns the main thread
// the system clipboard must be touched from, and gives us the tray icon.
func runDaemon() error {
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	sock, err := paths.Socket()
	if err != nil {
		return err
	}
	cfgPath, err := paths.Config()
	if err != nil {
		return err
	}
	settings, err := config.Open(cfgPath, log.Printf)
	if err != nil {
		return fmt.Errorf("%s: %w", cfgPath, err)
	}
	inbox := settings.Current().Inbox // changing it needs a restart
	if inbox == "" {
		if inbox, err = paths.Inbox(); err != nil {
			return err
		}
	}

	// Files of clips received by versions before the engine owned its inbox are
	// no longer on the clipboard. The inbox may be a directory of the user's,
	// so remove only what those versions made.
	if old, _ := filepath.Glob(filepath.Join(inbox, "clip-*")); len(old) > 0 {
		for _, d := range old {
			_ = os.RemoveAll(d)
		}
	}

	node, err := desktop.Open(context.Background(), desktop.Options{
		Dir: dir, Settings: settings, Board: board.MyGo{}, Logf: log.Printf,
	})
	if err != nil {
		return err
	}
	settings.OnChange = func(_, _ config.Config, err error) { node.ConfigChanged(err) }

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	go func() {
		if err := settings.Watch(ctx); err != nil {
			log.Printf("config: not watching %s, edits need a restart: %v", cfgPath, err)
		}
	}()

	mygo.App.SetName("unicd")
	mygo.App.SetActivationPolicy(mygo.ActivationPolicyAccessory) // no Dock icon
	mygo.App.OnWindowAllClosed(func() {})                        // the daemon has no windows to keep it alive

	var ln interface{ Close() error }
	mygo.App.WhenReady(func() {
		l, err := api.Listen(sock)
		if err != nil {
			log.Print(err)
			mygo.App.Exit(1)
			return
		}
		ln = l
		go func() {
			// Start reaches out to a DERP relay, which can take a moment.
			if err := node.Start(ctx); err != nil {
				log.Printf("start: %v", err)
				mygo.App.Exit(1)
				return
			}
			log.Printf("unicd up; socket %s", sock)
			_ = http.Serve(l, api.Handler(node, settings))
		}()
		setupTray()
		go notifyPending(ctx, sock)
	})
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) {
		cancel()
		if ln != nil {
			_ = ln.Close()
		}
		_ = os.Remove(sock)
		closeCtx, done := context.WithTimeout(context.Background(), 10*time.Second)
		defer done()
		if err := node.Close(closeCtx); err != nil {
			log.Printf("close: %v", err)
		}
	})
	return mygo.App.Run()
}

func setupTray() {
	tray, err := mygo.NewTray(mygo.TrayOptions{
		ID:             "unicd",
		Icon:           trayIcon(),
		IconIsTemplate: true,
		ToolTip:        "unic — clipboard sync",
		Menu: mygo.NewMenu([]*mygo.MenuItem{
			{Label: "Open unic…", Click: func(*mygo.MenuItem, *mygo.Window) { openGUI() }},
			mygo.Separator(),
			{Role: mygo.RoleQuit},
		}),
	})
	if err != nil {
		// A headless Linux box has no tray; the daemon works without one.
		log.Printf("no tray icon: %v", err)
		return
	}
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) { tray.Destroy() })
}

// notifyPending raises a system notification when a device asks to join, so
// that the request is not missed while the window is closed. It is best
// effort: some systems refuse notifications from unsigned programs.
func notifyPending(ctx context.Context, sock string) {
	if !mygo.NotificationsSupported() {
		return
	}
	c := api.NewClient(sock)
	for ctx.Err() == nil {
		_ = c.Events(ctx, func(ev api.Event) {
			if ev.Kind != "pending" {
				return
			}
			body := ev.Peer + " asks to sync clipboards with this device. Open unic to approve or reject."
			if ev.Info == "code" {
				body = ev.Peer + " entered this device's pairing code. Open unic, compare the fingerprints and approve."
			} else if ev.Info != "" {
				body = ev.Peer + ", introduced by " + ev.Info + ", wants to join. Open unic to approve or reject."
			}
			n := mygo.NewNotification(mygo.NotificationOptions{Title: "New device", Body: body})
			if err := n.Show(); err != nil {
				log.Printf("notification: %v", err)
			}
		})
		select {
		case <-ctx.Done():
		case <-time.After(2 * time.Second):
		}
	}
}

// openGUI starts the GUI as its own process: like the docker CLI, it is just
// another client of the daemon's socket. A second launch is a no-op because
// the GUI holds a single-instance lock.
func openGUI() {
	cmd := exec.Command(unicPath(), "gui")
	if err := cmd.Start(); err != nil {
		log.Printf("open gui: %v", err)
		return
	}
	go func() { _ = cmd.Wait() }()
}

// unicPath finds the unic program: next to unicd if installed together, else
// on PATH.
func unicPath() string {
	name := "unic"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	if exe, err := os.Executable(); err == nil {
		if p := filepath.Join(filepath.Dir(exe), name); fileExists(p) {
			return p
		}
	}
	return name
}

func fileExists(p string) bool {
	st, err := os.Stat(p)
	return err == nil && !st.IsDir()
}

// trayIcon draws two overlapping sheets, black on transparent so macOS tints
// it for the menu bar; elsewhere it is blue to show on any taskbar.
func trayIcon() []byte {
	img := image.NewNRGBA(image.Rect(0, 0, 32, 32))
	ink := color.NRGBA{R: 79, G: 124, B: 255, A: 255}
	if runtime.GOOS == "darwin" {
		ink = color.NRGBA{A: 255}
	}
	for _, r := range []image.Rectangle{
		// back sheet
		image.Rect(4, 3, 20, 5), image.Rect(4, 5, 6, 21), image.Rect(18, 3, 20, 9),
		// front sheet
		image.Rect(11, 11, 28, 13), image.Rect(11, 28, 28, 30),
		image.Rect(11, 13, 13, 28), image.Rect(26, 13, 28, 28),
		image.Rect(15, 17, 24, 19), image.Rect(15, 22, 22, 24),
	} {
		draw.Draw(img, r, image.NewUniform(ink), image.Point{}, draw.Src)
	}
	var b bytes.Buffer
	_ = png.Encode(&b, img)
	return b.Bytes()
}
