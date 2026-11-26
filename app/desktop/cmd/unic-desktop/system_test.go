package main

import (
	"context"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/notifications"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type fakeTrayHost struct {
	initOpts     mygo.TrayOptions
	menu         *mygo.Menu
	poppedMenus  []*mygo.Menu
	events       []string
	tooltip      string
	onClick      func()
	onRightClick func()
	destroyed    bool
}

func (f *fakeTrayHost) SetIcon([]byte, bool) error { return nil }
func (f *fakeTrayHost) SetToolTip(tip string)      { f.tooltip = tip }
func (f *fakeTrayHost) SetMenu(m *mygo.Menu) {
	f.menu = m
	if m == nil {
		f.events = append(f.events, "set-menu:nil")
	} else {
		f.events = append(f.events, "set-menu:present")
	}
}
func (f *fakeTrayHost) PopUpMenu() {
	f.events = append(f.events, "popup")
	f.poppedMenus = append(f.poppedMenus, f.menu)
}
func (f *fakeTrayHost) OnClick(fn func()) func() {
	f.onClick = fn
	return func() { f.onClick = nil }
}
func (f *fakeTrayHost) OnRightClick(fn func()) func() {
	f.onRightClick = fn
	return func() { f.onRightClick = nil }
}
func (f *fakeTrayHost) Destroy() { f.destroyed = true }

func menuLabels(m *mygo.Menu) []string {
	if m == nil {
		return nil
	}
	var out []string
	for _, item := range m.Items() {
		if item.Label != "" {
			out = append(out, item.Label)
		}
	}
	return out
}

func testTrayHooks() (trayHooks, *func()) {
	var quit func()
	return trayHooks{
		onThemeUpdated: func(func()) func() { return func() {} },
		watchTheme:     func(context.Context, func()) {},
		onWillQuit:     func(fn func()) { quit = fn },
		runOnMain:      func(fn func()) { fn() },
	}, &quit
}

func TestSetupTrayLeftClickOpensMainWindowOnDarwin(t *testing.T) {
	prevShow := showDesktop
	defer func() { showDesktop = prevShow }()
	opened := 0
	showDesktop = func() { opened++ }

	var fake *fakeTrayHost
	hooks, quit := testTrayHooks()
	ctrl := setupTrayWith(context.Background(), config.Desktop{Language: "zh-CN", TrayIcon: "glyph"}, "darwin", func(opts mygo.TrayOptions) (trayHost, error) {
		fake = &fakeTrayHost{initOpts: opts, menu: opts.Menu}
		return fake, nil
	}, hooks)

	if fake == nil {
		t.Fatal("tray was not created")
	}
	if fake.initOpts.Menu != nil {
		t.Fatal("darwin tray must not attach a persistent menu on creation")
	}
	if fake.onClick == nil {
		t.Fatal("OnClick listener not registered")
	}
	if fake.onRightClick == nil {
		t.Fatal("OnRightClick listener not registered on darwin")
	}

	// Primary click opens main window without touching menu.
	fake.onClick()
	if opened != 1 {
		t.Fatalf("opened = %d, want 1", opened)
	}
	if fake.menu != nil || len(fake.events) != 0 {
		t.Fatalf("primary click unexpectedly modified menu: menu=%v events=%v", fake.menu, fake.events)
	}

	// Secondary click pops up menu then clears it so subsequent left-clicks still work.
	fake.onRightClick()
	wantEvents := []string{"set-menu:present", "popup", "set-menu:nil"}
	if !slices.Equal(fake.events, wantEvents) {
		t.Fatalf("events = %v, want %v", fake.events, wantEvents)
	}
	if fake.menu != nil {
		t.Fatal("darwin tray menu was not cleared after PopUpMenu")
	}
	if len(fake.poppedMenus) != 1 || !slices.Contains(menuLabels(fake.poppedMenus[0]), "打开主窗口") || !slices.Contains(menuLabels(fake.poppedMenus[0]), "退出") {
		t.Fatalf("popped menu labels = %v, want 打开主窗口 and 退出", menuLabels(fake.poppedMenus[0]))
	}

	// Language and pending updates must NOT attach a persistent menu on darwin,
	// and the next right-click must pop up the updated menu.
	fake.events = nil
	ctrl.updateConfig(config.Desktop{Language: "en-US", TrayIcon: "glyph"})
	ctrl.updatePending(notifications.PendingSummary{Count: 2, FirstID: "req-1"})
	if fake.menu != nil || len(fake.events) != 0 {
		t.Fatalf("updates attached persistent menu on darwin: menu=%v events=%v", fake.menu, fake.events)
	}
	if !strings.Contains(fake.tooltip, "(2)") {
		t.Fatalf("tooltip = %q, want pending count", fake.tooltip)
	}

	fake.onRightClick()
	if !slices.Equal(fake.events, wantEvents) {
		t.Fatalf("events after update = %v, want %v", fake.events, wantEvents)
	}
	if fake.menu != nil {
		t.Fatal("darwin tray menu was not cleared after second PopUpMenu")
	}
	labels := menuLabels(fake.poppedMenus[1])
	if len(labels) == 0 || !strings.Contains(labels[0], "(2)") {
		t.Fatalf("updated menu labels = %v, want pending request entry at top", labels)
	}

	// Primary click still works after right-click and config/pending updates.
	fake.onClick()
	if opened != 2 {
		t.Fatalf("opened = %d, want 2", opened)
	}

	(*quit)()
	if !fake.destroyed {
		t.Fatal("tray was not destroyed on quit")
	}
}

func TestSetupTrayLeftClickOpensMainWindowOnWindowsAndLinux(t *testing.T) {
	for _, goos := range []string{"windows", "linux"} {
		t.Run(goos, func(t *testing.T) {
			prevShow := showDesktop
			defer func() { showDesktop = prevShow }()
			opened := 0
			showDesktop = func() { opened++ }

			var fake *fakeTrayHost
			hooks, quit := testTrayHooks()
			ctrl := setupTrayWith(context.Background(), config.Desktop{Language: "zh-CN", TrayIcon: "glyph"}, goos, func(opts mygo.TrayOptions) (trayHost, error) {
				fake = &fakeTrayHost{initOpts: opts, menu: opts.Menu}
				return fake, nil
			}, hooks)

			if fake == nil {
				t.Fatal("tray was not created")
			}
			if fake.initOpts.Menu == nil {
				t.Fatalf("%s tray must attach persistent menu on creation", goos)
			}
			if fake.onClick == nil {
				t.Fatalf("%s tray did not register OnClick", goos)
			}
			if fake.onRightClick != nil {
				t.Fatalf("%s tray should not override native right-click menu handling", goos)
			}

			fake.onClick()
			if opened != 1 {
				t.Fatalf("opened = %d, want 1", opened)
			}

			// Icon style change alone must not rebuild tray menu.
			ctrl.updateConfig(config.Desktop{Language: "zh-CN", TrayIcon: "cat"})
			if len(fake.events) != 0 {
				t.Fatalf("style-only update unexpectedly rebuilt menu: %v", fake.events)
			}

			// Language and pending updates refresh the persistent menu.
			ctrl.updateConfig(config.Desktop{Language: "en-US", TrayIcon: "cat"})
			ctrl.updatePending(notifications.PendingSummary{Count: 1, FirstID: "dev-1"})
			if len(fake.events) != 2 || fake.menu == nil {
				t.Fatalf("expected 2 menu refreshes, got events=%v menu=%v", fake.events, fake.menu)
			}
			labels := menuLabels(fake.menu)
			if len(labels) == 0 || !strings.Contains(labels[0], "(1)") {
				t.Fatalf("updated menu labels = %v, want pending request entry", labels)
			}

			(*quit)()
			if !fake.destroyed {
				t.Fatal("tray was not destroyed on quit")
			}
		})
	}
}
