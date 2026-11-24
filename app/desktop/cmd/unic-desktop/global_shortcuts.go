package main

import (
	"strings"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/internal/desktopkeys"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type globalCommand struct {
	first, second string
	run           func()
}

// The tray owns the OS registrations. All methods run on the main thread.
type globalShortcuts struct {
	commands []globalCommand
	keys     map[string]bool
	leader   string
	deadline time.Time
}

func (g *globalShortcuts) apply(desktop config.Desktop, paused bool, handlers map[string]func()) error {
	var commands []globalCommand
	keys := map[string]bool{}
	if !paused {
		for _, action := range desktopkeys.ActionsForPlatform() {
			if action.Scope != desktopkeys.Global {
				continue
			}
			values := desktopkeys.Bindings(desktop.Shortcuts, action)
			if _, ok := desktop.Shortcuts[action.ID]; !ok && action.ID == desktopkeys.TogglePanel && desktop.QuickPanelShortcut != "" {
				values = []string{desktop.QuickPanelShortcut}
			}
			for _, value := range values {
				normalized, err := desktopkeys.Normalize(value)
				if err != nil {
					return err
				}
				first, second, _ := strings.Cut(desktopkeys.Physical(normalized), " ")
				commands = append(commands, globalCommand{first, second, handlers[action.ID]})
				keys[first] = true
				if second != "" {
					keys[second] = true
				}
			}
		}
	}
	var added []string
	for key := range keys {
		if !g.keys[key] {
			if err := mygo.GlobalShortcut.Register(desktopkeys.Accelerator(key), func() { g.press(key) }); err != nil {
				for _, registered := range added {
					mygo.GlobalShortcut.Unregister(desktopkeys.Accelerator(registered))
				}
				return err
			}
			added = append(added, key)
		}
	}
	for key := range g.keys {
		if !keys[key] {
			mygo.GlobalShortcut.Unregister(desktopkeys.Accelerator(key))
		}
	}
	g.keys, g.commands, g.leader = keys, commands, ""
	return nil
}

func (g *globalShortcuts) press(key string) {
	now := time.Now()
	leader := g.leader
	g.leader = ""
	if now.Before(g.deadline) {
		for _, cmd := range g.commands {
			if cmd.first == leader && cmd.second == key {
				cmd.run()
				return
			}
		}
	}
	for _, cmd := range g.commands {
		if cmd.first != key {
			continue
		}
		if cmd.second == "" {
			cmd.run()
		} else {
			g.leader, g.deadline = key, now.Add(time.Second)
		}
		return
	}
}
