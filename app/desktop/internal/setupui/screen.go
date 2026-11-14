package setupui

import (
	"context"
	"errors"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
	"github.com/uniclipboard/unic/internal/v1migration"
)

type completion struct {
	report v1migration.Report
	err    error
}

// Screen owns view-thread state. Workers publish bounded updates; they never
// mutate a view model or run application lifecycle actions.
type Screen struct {
	flow       *setupflow.Flow
	model      Model
	fields     Fields
	enter      func()
	cancel     context.CancelFunc
	results    chan completion
	progress   chan v1migration.Progress
	detections chan detection
	closeAfter func()
}
type detection struct {
	source string
	found  bool
	err    error
}

func New(flow *setupflow.Flow, language string, migrationOnly bool, enter func()) *Screen {
	s := &Screen{flow: flow, model: Model{Stage: "welcome", Language: language}, enter: enter, results: make(chan completion, 1), progress: make(chan v1migration.Progress, 1), detections: make(chan detection, 1)}
	if source := flow.State().Source; source != "" {
		s.model.Source, s.model.Stage = source, "confirm"
	} else if migrationOnly {
		s.model.Stage = "confirm"
	}
	s.detect(s.model.Source)
	return s
}
func (s *Screen) detect(source string) {
	if s.model.Detecting {
		return
	}
	s.model.Detecting, s.model.Error = true, ""
	go func() {
		var err error
		if source == "" {
			source, err = defaultSourceDirectory()
		}
		found := false
		if err == nil {
			found, err = setupflow.Detect(source)
		}
		s.detections <- detection{source, found, err}
	}()
}
func (s *Screen) begin() {
	if s.model.Entering {
		return
	}
	if s.model.Detecting {
		s.model.PendingStart = true
		return
	}
	if s.model.Found {
		s.model.Stage = "confirm"
		if err := s.flow.SelectSource(s.model.Source); err != nil {
			s.model.Stage, s.model.Error = "invalid", "无法保存引导状态。请检查数据目录权限，然后重试。"
		}
		return
	}
	s.enterApp()
}
func (s *Screen) enterApp() {
	if s.cancel != nil || s.model.Entering {
		return
	}
	s.fields.Passphrase = ""
	s.model.Entering, s.model.Error = true, ""
	s.enter()
}
func (s *Screen) SetLanguage(language string) { s.model.Language = language }
func (s *Screen) EnterFailed(message string)  { s.model.Entering, s.model.Error = false, message }
func (s *Screen) pick() {
	if s.cancel != nil || s.model.Detecting || s.model.Entering {
		return
	}
	selected, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Title: i18n.Translate(s.model.Language, "选择 v1 数据目录"), Directory: true, DefaultPath: s.model.Source})
	if err != nil {
		s.model.Error = "无法打开目录选择窗口。请重试。"
		return
	}
	if len(selected) > 0 {
		s.model.Stage = "confirm"
		s.detect(selected[0])
	}
}
func (s *Screen) start(password []byte) {
	if s.cancel != nil || s.model.Entering {
		clear(password)
		return
	}
	s.fields.Passphrase = ""
	s.model.Error = ""
	if err := s.flow.SelectSource(s.model.Source); err != nil {
		clear(password)
		s.model.Stage, s.model.Error = "invalid", "这个目录不是受支持的 v1 数据目录。请选择其他目录。"
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.cancel, s.model.Stage = cancel, "credentials"
	s.model.Report, s.model.Progress = v1migration.Report{}, v1migration.Progress{}
	go func() {
		defer clear(password)
		defer cancel()
		report, err := s.flow.Run(ctx, password, readSourceSecret, func(p v1migration.Progress) {
			select {
			case <-s.progress:
			default:
			}
			s.progress <- p
		})
		if ctx.Err() != nil {
			report.Status, err = "cancelled", ctx.Err()
		}
		s.results <- completion{report, err}
	}()
}
func (s *Screen) stop() {
	if s.cancel != nil {
		s.model.Stage = "stopping"
		s.cancel()
	}
}

// RequestClose preserves pending setup and waits for migration to release all
// storage resources before the host exits. The host keeps the window alive.
func (s *Screen) RequestClose(done func()) bool {
	s.fields.Passphrase = ""
	if s.cancel == nil {
		return true
	}
	s.closeAfter = done
	s.stop()
	return false
}
func (s *Screen) View(c *ui.Context) {
	s.poll()
	c.After(50 * time.Millisecond)
	Render(c, s.model, &s.fields, Actions{
		Begin: s.begin, Redetect: func() { s.detect("") }, Pick: s.pick,
		Start: func() { s.start(nil) }, Unlock: func() { s.start([]byte(s.fields.Passphrase)) },
		Stop: s.stop, Enter: s.enterApp,
	})
}
func (s *Screen) poll() {
	select {
	case d := <-s.detections:
		s.model.Detecting, s.model.Source, s.model.Found = false, d.source, d.found
		if d.err != nil {
			s.model.Error = "无法检查 v1 数据。请重试或手动选择目录。"
			if s.model.Stage != "welcome" {
				s.model.Stage = "invalid"
			}
		} else if s.model.Stage != "welcome" && !d.found {
			s.model.Stage, s.model.Error = "invalid", "未发现 v1 数据。请选择其他目录。"
		}
		if s.model.PendingStart {
			s.model.PendingStart = false
			s.begin()
		}
	default:
	}
	select {
	case p := <-s.progress:
		s.model.Progress = p
		if s.cancel != nil && s.model.Stage != "stopping" {
			s.model.Stage = p.Stage
		}
	default:
	}
	select {
	case r := <-s.results:
		s.cancel = nil
		s.model.Report, s.model.Stage = r.report, r.report.Status
		if errors.Is(r.err, v1migration.ErrUnlock) {
			s.model.Stage, s.model.Error = "password", "无法解锁旧数据。请核对 v1 口令后重试。"
		} else if errors.Is(r.err, setupflow.ErrTargetBusy) {
			s.model.Stage = "conflict"
		} else if r.err != nil && !errors.Is(r.err, context.Canceled) {
			s.model.Stage, s.model.Error = "failed", "迁移未完成。请检查数据目录、可用磁盘空间和访问权限，然后重试。"
		}
		if s.closeAfter != nil {
			done := s.closeAfter
			s.closeAfter = nil
			done()
		}
	default:
	}
}
