// Package quickpanel owns Desktop-only target sessions and paste ordering.
// All Controller and Shortcut methods run on the host event thread.
package quickpanel

import (
	"context"
	"errors"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

var (
	ErrTarget      = errors.New("quick panel: target unavailable")
	ErrPermission  = errors.New("quick panel: permission required")
	ErrProtected   = errors.New("quick panel: secure input enabled")
	ErrPlain       = errors.New("quick panel: no plain-text representation")
	ErrFocus       = errors.New("quick panel: target focus changed")
	ErrInput       = errors.New("quick panel: input unavailable")
	ErrClipboard   = errors.New("quick panel: clipboard changed")
	ErrShortcut    = errors.New("quick panel: shortcut unavailable")
	ErrBusy        = errors.New("quick panel: operation in progress")
	ErrUnsupported = errors.New("quick panel: unsupported session")
)

// Target is an opaque process/window instance. Never log or persist it.
type Target struct {
	ID   uintptr
	PID  int
	Born uint64
	Name string
}

// Platform methods must not block waiting for focus or held keys.
type Platform interface {
	Capture() (Target, error)
	Check(Target) error
	Activate(Target) error
	Ready(Target) (bool, error)
	Paste(Target) error
	OwnsForeground() bool
}
type Options struct {
	Context   context.Context
	Client    *api.Client
	Clipboard *Clipboard
	Platform  Platform
	Dispatch  func(func())
	Visible   func(bool)
	Changed   func()
}
type Controller struct {
	captureErr           error
	options              Options
	target               Target
	generation           uint64
	cancel               context.CancelFunc
	Busy, CanPaste, Sent bool
	TargetName, Message  string
}

func New(o Options) *Controller { return &Controller{options: o} }
func (c *Controller) changed() {
	if c.options.Changed != nil {
		c.options.Changed()
	}
}
func (c *Controller) visible(v bool) {
	if c.options.Visible != nil {
		c.options.Visible(v)
	}
}
func (c *Controller) invalidate() {
	c.generation++
	if c.cancel != nil {
		c.cancel()
		c.cancel = nil
	}
	c.Busy = false
	c.Sent = false
}
func (c *Controller) Open() {
	previousFailure := c.Message
	previousSent := c.Sent
	c.invalidate()
	c.target = Target{}
	c.TargetName = ""
	c.Message = ""
	if !previousSent && previousFailure != "已复制" {
		c.Message = previousFailure
	}
	c.CanPaste = false
	t, err := c.options.Platform.Capture()
	c.captureErr = err
	if err == nil && t.ID == 0 {
		c.captureErr = ErrTarget
	}
	if err == nil && t.ID != 0 {
		c.target = t
		c.TargetName = t.Name
		c.CanPaste = true
	} else {
		c.Message = message(err)
	}
	c.changed()
}
func (c *Controller) Dismiss(restore bool) {
	c.invalidate()
	if restore && c.CanPaste && c.options.Platform.OwnsForeground() {
		_ = c.options.Platform.Activate(c.target)
	}
	c.visible(false)
	c.changed()
}
func (c *Controller) Start(id string, plain, paste, keep bool) error {
	if c.Busy {
		return ErrBusy
	}
	if id == "" {
		return ErrTarget
	}
	c.Sent = false
	c.Message = ""
	if paste {
		if !c.CanPaste {
			err := c.captureErr
			if err == nil {
				err = ErrTarget
			}
			c.Message = message(err)
			c.changed()
			return err
		}
		if err := c.options.Platform.Check(c.target); err != nil {
			c.Message = message(err)
			c.changed()
			return err
		}
	}
	c.generation++
	generation := c.generation
	ctx, cancel := context.WithTimeout(c.options.Context, 10*time.Second)
	c.cancel = cancel
	c.Busy = true
	c.changed()
	go func() {
		mark, err := c.options.Clipboard.Apply(ctx, c.options.Client, id, plain)
		c.options.Dispatch(func() {
			if generation != c.generation || ctx.Err() != nil {
				if generation == c.generation {
					c.finish(ctx.Err(), false)
				}
				return
			}
			if err != nil {
				c.finish(err, false)
				return
			}
			if !paste {
				c.finish(nil, false)
				if !keep {
					c.Dismiss(true)
				}
				return
			}
			if !c.options.Platform.OwnsForeground() {
				c.finish(ErrFocus, false)
				return
			}
			// Hide both panel and its satellite before restoring the external target.
			c.visible(false)
			if err = c.options.Platform.Activate(c.target); err != nil {
				c.failHidden(err)
				return
			}
			c.await(generation, mark, time.Now().Add(time.Second), keep)
		})
	}()
	return nil
}
func (c *Controller) await(generation uint64, mark string, deadline time.Time, keep bool) {
	if generation != c.generation {
		return
	}
	if c.options.Context.Err() != nil {
		c.finish(c.options.Context.Err(), false)
		return
	}
	ready, err := c.options.Platform.Ready(c.target)
	if err != nil {
		c.failHidden(err)
		return
	}
	if ready {
		if !c.options.Clipboard.Matches(mark) {
			c.failHidden(ErrClipboard)
			return
		}
		if err = c.options.Platform.Paste(c.target); err != nil {
			c.failHidden(err)
			return
		}
		if keep {
			// SendInput acknowledges queue insertion, not target consumption. Leave a
			// short dispatch interval before reclaiming focus, then respect user switches.
			time.AfterFunc(150*time.Millisecond, func() {
				c.options.Dispatch(func() {
					if generation != c.generation || c.options.Context.Err() != nil {
						return
					}
					ready, err := c.options.Platform.Ready(c.target)
					c.finish(nil, true)
					if err == nil && ready {
						c.visible(true)
					}
				})
			})
		} else {
			c.finish(nil, true)
		}
		return
	}
	if !time.Now().Before(deadline) {
		c.failHidden(ErrFocus)
		return
	}
	time.AfterFunc(20*time.Millisecond, func() { c.options.Dispatch(func() { c.await(generation, mark, deadline, keep) }) })
}
func (c *Controller) failHidden(err error) {
	// Do not steal focus back after a user switch. Reopening shows the error.
	c.finish(err, false)
}
func (c *Controller) finish(err error, sent bool) {
	if c.cancel != nil {
		c.cancel()
		c.cancel = nil
	}
	c.Busy = false
	c.Sent = sent
	if err != nil {
		c.Message = message(err)
	} else if sent {
		c.Message = "已发送粘贴指令"
	} else {
		c.Message = "已复制"
	}
	c.changed()
}
func message(err error) string {
	switch {
	case errors.Is(err, ErrPlain):
		return "此记录没有纯文本表示。请使用普通粘贴或复制。"
	case errors.Is(err, ErrProtected):
		return "原应用启用了安全输入。请使用复制。"
	case errors.Is(err, ErrPermission):
		return "自动粘贴需要系统辅助功能权限。请在系统设置中授权后重试。"
	case errors.Is(err, ErrUnsupported):
		return "当前桌面会话不支持自动粘贴。请使用复制。"
	case errors.Is(err, ErrFocus):
		return "原应用未取得焦点，或按键尚未松开。未发送粘贴指令。"
	case errors.Is(err, ErrClipboard):
		return "剪贴板已变化。未发送粘贴指令。"
	case errors.Is(err, ErrInput):
		return "系统未确认发送粘贴指令。请检查目标应用后重试。"
	case errors.Is(err, ErrTarget), err == nil:
		return "未找到原应用。请从其他应用使用全局快捷键打开面板。"
	default:
		return "操作未能确认完成。请检查剪贴板后重试。"
	}
}
