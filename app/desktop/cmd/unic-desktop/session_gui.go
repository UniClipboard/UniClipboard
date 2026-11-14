package main

import (
	"context"
	"sync"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/lifecycle"
	"github.com/uniclipboard/unic/app/desktop/internal/quickpanel"
	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
	"github.com/uniclipboard/unic/app/desktop/internal/setupui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type desktopRuntime struct {
	screen      *desktopui.Desktop
	service     *lifecycle.Process
	cancel      context.CancelFunc
	adapterDone chan struct{}
}

// desktopSession serializes setup and ordinary runtime ownership on the view
// thread. Only the startup/shutdown worker touches a service in transition.
type desktopSession struct {
	g                 *gui
	ctx               context.Context
	settings          *config.Manager
	flow              *setupflow.Flow
	dir, executable   string
	setup             *setupui.Screen
	runtime           *desktopRuntime
	starting, closing bool
	message           string
	workers           sync.WaitGroup
}

func (s *desktopSession) showSetup(migrationOnly bool) {
	s.setup = setupui.New(s.flow, s.settings.Current().Desktop.Language, migrationOnly, s.start)
	s.g.win.SetMinimumSize(600, 480)
	s.g.win.SetContentSize(640, 560)
	s.g.win.Show()
	s.g.win.Focus()
	s.g.win.Invalidate()
}
func (s *desktopSession) view(c *ui.Context) {
	c.OnWindowShortcut(ui.Cmd, ui.KeyQ, mygo.App.Quit)
	if s.setup != nil {
		s.setup.View(c)
		return
	}
	if s.runtime != nil {
		s.runtime.screen.View(c)
		return
	}
	k := uikit.For(c.Theme().Dark)
	k.UiRoot(c, func() {
		ui.Column(c).Fill().Padding(40).Gap(12).Center().Children(func() {
			if s.message == "" {
				ui.Spinner(c)
				k.Text(c, i18n.Translate(s.settings.Current().Desktop.Language, "正在准备应用…"), uikit.TextProps{})
			} else {
				k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneError, Title: i18n.Translate(s.settings.Current().Desktop.Language, s.message)})
				if k.Button(c, i18n.Translate(s.settings.Current().Desktop.Language, "重试"), uikit.ButtonProps{Variant: uikit.Primary, Disabled: s.starting}).Clicked() {
					s.start()
				}
			}
		})
	})
}
func (s *desktopSession) start() {
	if s.starting || s.runtime != nil {
		return
	}
	s.starting, s.message = true, ""
	s.workers.Add(1)
	go func() {
		defer s.workers.Done()
		service, err := lifecycle.Start(s.ctx, s.dir, s.g.c.Path, s.executable, true)
		mygo.RunOnMain(func() {
			if s.closing || s.ctx.Err() != nil {
				if service == nil {
					s.starting = false
					mygo.App.Quit()
					return
				}
				s.retireCandidate(service)
				return
			}
			// Persist completion only after readiness and before entering history.
			// A close during startup must leave first-install intent pending.
			if err == nil {
				err = s.flow.Complete()
			}
			if err != nil {
				if service != nil {
					s.retireCandidate(service)
					return
				}
				s.starting = false
				s.startFailed()
				return
			}
			s.starting = false
			s.setup = nil
			ctx, cancel := context.WithCancel(s.ctx)
			clipboard := quickpanel.NewClipboard(newDesktopBoard())
			screen := desktopui.NewDesktop(ctx, s.g.c)
			r := &desktopRuntime{screen: screen, service: service, cancel: cancel, adapterDone: make(chan struct{})}
			s.runtime = r
			screen.SetLoginStatus(func() (bool, string) { s.g.mu.Lock(); defer s.g.mu.Unlock(); return s.g.loginEnabled, s.g.loginError }, func() { s.g.applyLoginSettings(s.settings.Current().Desktop.LaunchAtLogin, true) })
			screen.SetMigrationOpener(s.migrate)
			screen.AttachWindow(s.g.win, clipboard)
			showQuickPanel = screen.OpenQuickPanel
			screen.SetQuickPanelShortcut(s.settings.Current().Desktop.QuickPanelShortcut)
			s.g.applyLoginSettings(s.settings.Current().Desktop.LaunchAtLogin, false)
			go func() { defer close(r.adapterDone); _ = clipboardintegration.RunRemote(ctx, s.g.c, clipboard, nil) }()
			go notifyPending(ctx, s.g.c.Path, func() string { return s.settings.Current().Desktop.Language })
			go func() {
				select {
				case <-ctx.Done():
					return
				case <-service.Done():
				}
				if ctx.Err() == nil {
					mygo.App.Quit()
				}
			}()
			s.g.win.SetMinimumSize(600, 480)
			s.g.win.SetContentSize(1100, 720)
			s.g.win.Invalidate()
		})
	}()
}
func (s *desktopSession) retireCandidate(service *lifecycle.Process) {
	s.workers.Add(1)
	go func() {
		defer s.workers.Done()
		_ = service.Stop()
		mygo.RunOnMain(func() {
			s.starting = false
			if s.closing || s.ctx.Err() != nil {
				mygo.App.Quit()
				return
			}
			s.startFailed()
		})
	}()
}
func (s *desktopSession) startFailed() {
	s.message = "无法启动应用。请检查数据目录和后台服务，然后重试。"
	if s.setup != nil {
		s.setup.EnterFailed(s.message)
	}
	s.g.win.Invalidate()
}
func (s *desktopSession) migrate() {
	if s.starting || s.runtime == nil {
		return
	}
	r := s.runtime
	s.runtime = nil
	s.starting = true
	s.message = ""
	showQuickPanel = func() {}
	r.screen.Close()
	r.cancel()
	s.workers.Add(1)
	go func() {
		defer s.workers.Done()
		<-r.adapterDone
		err := r.service.Stop()
		mygo.RunOnMain(func() {
			s.starting = false
			if s.closing {
				mygo.App.Quit()
				return
			}
			if err != nil {
				s.message = "无法停止当前后台服务。请退出应用后重试迁移。"
				s.g.win.Invalidate()
				return
			}
			s.showSetup(true)
		})
	}()
	s.g.win.Invalidate()
}

// requestQuit is used by both window close during setup and application quit.
func (s *desktopSession) requestQuit(e *mygo.QuitEvent) {
	if s.starting {
		e.PreventDefault()
		s.closing = true
		return
	}
	if s.setup != nil && !s.setup.RequestClose(func() { s.closing = false; mygo.App.Quit() }) {
		e.PreventDefault()
		s.closing = true
	}

}
func (s *desktopSession) close() {
	s.workers.Wait()
	if r := s.runtime; r != nil {
		r.cancel()
		<-r.adapterDone
		_ = r.service.Stop()
	}
}
