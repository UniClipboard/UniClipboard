// Command tray_probe reproduces the gui-go tray menu refresh pattern with the pinned Wails version: a root menu
// holding a dynamic submenu that is rebuilt on a timer, like tray_devices.go. -refresh selects how the rebuilt
// menu is published: "update" (Menu.Update, the pre-17c14 behaviour) or "settray" (SystemTray.SetMenu).
package main

import (
	"flag"
	"fmt"
	"os"
	"sync"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
)

func main() {
	icon := flag.String("icon", "", "tray icon PNG")
	mode := flag.String("refresh", "update", "update|settray")
	every := flag.Duration("every", 2*time.Second, "submenu rebuild period")
	flag.Parse()
	data, err := os.ReadFile(*icon)
	if err != nil {
		fmt.Fprintln(os.Stderr, "icon:", err)
		os.Exit(2)
	}
	app := application.New(application.Options{Name: "tray-probe"})
	root := app.NewMenu()
	root.Add("Probe action").OnClick(func(*application.Context) { fmt.Println("PROBE action-clicked"); os.Stdout.Sync() })
	sub := root.AddSubmenu("Probe devices")
	sub.Add("placeholder").SetEnabled(false)
	root.AddSeparator()
	root.Add("Probe quit").OnClick(func(*application.Context) { fmt.Println("PROBE quit-clicked"); app.Quit() })
	tray := app.SystemTray.New()
	tray.SetIcon(data)
	tray.SetTooltip("tray-probe")
	tray.SetMenu(root)

	var mu sync.Mutex
	gen := 0
	go func() {
		for range time.Tick(*every) {
			mu.Lock()
			gen++
			sub.Clear()
			sub.AddCheckbox(fmt.Sprintf("device-gen-%d", gen), gen%2 == 0)
			mu.Unlock()
			switch *mode {
			case "update":
				root.Update()
			case "settray":
				tray.SetMenu(root)
			}
			fmt.Println("PROBE rebuilt", gen)
		}
	}()
	if err := app.Run(); err != nil {
		fmt.Fprintln(os.Stderr, "run:", err)
		os.Exit(1)
	}
	fmt.Println("PROBE exited-clean")
}
