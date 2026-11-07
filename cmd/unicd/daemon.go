package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
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

	"github.com/egoist/mygo"

	"unic/internal/api"
	"unic/internal/board"
	"unic/internal/engine"
	"unic/internal/paths"
	"unic/internal/store"
	"unic/internal/tcnet"
)

// runDaemon runs the node as a windowless mygo app: mygo owns the main thread
// the system clipboard must be touched from, and gives us the tray icon.
func runDaemon() error {
	dbPath, err := paths.Database()
	if err != nil {
		return err
	}
	sock, err := paths.Socket()
	if err != nil {
		return err
	}
	inbox, err := paths.Inbox()
	if err != nil {
		return err
	}
	st, err := store.Open(dbPath)
	if err != nil {
		return err
	}
	defer st.Close()

	id, err := deviceID(st)
	if err != nil {
		return err
	}
	name, _ := os.Hostname()
	if name == "" {
		name = "device-" + id[:6]
	}
	tr, err := tcnet.New(st, log.Printf)
	if err != nil {
		return err
	}
	eng := engine.New(engine.Config{
		ID: id, Name: name, Board: board.MyGo{}, Store: st, Transport: tr, Inbox: inbox,
	})

	// Files of clips received in an earlier run are no longer on the clipboard.
	_ = os.RemoveAll(inbox)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

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
			if err := tr.Start(); err != nil {
				log.Printf("tailcat: %v", err)
				mygo.App.Exit(1)
				return
			}
			go func() {
				if err := eng.Run(ctx); err != nil && ctx.Err() == nil {
					log.Printf("engine: %v", err)
					mygo.App.Exit(1)
				}
			}()
			log.Printf("unicd %q up; socket %s", name, sock)
			_ = http.Serve(l, api.Handler(eng))
		}()
		setupTray()
	})
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) {
		cancel()
		if ln != nil {
			ln.Close()
		}
		_ = os.Remove(sock)
		_ = tr.Close()
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

// openGUI starts the GUI as its own process: like the docker CLI, it is just
// another client of the daemon's socket. A second launch is a no-op because
// the GUI holds a single-instance lock.
func openGUI() {
	cmd := exec.Command(unicPath(), "gui")
	if err := cmd.Start(); err != nil {
		log.Printf("open gui: %v", err)
		return
	}
	go cmd.Wait()
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

func deviceID(st *store.Store) (string, error) {
	if b, err := st.Meta("device.id"); err != nil {
		return "", err
	} else if b != nil {
		return string(b), nil
	}
	var r [8]byte
	_, _ = rand.Read(r[:])
	id := hex.EncodeToString(r[:])
	return id, st.SetMeta("device.id", []byte(id))
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
