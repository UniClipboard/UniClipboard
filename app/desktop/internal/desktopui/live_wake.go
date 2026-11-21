package desktopui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// Bind only the window that drains this binding. Services is weak and its
// invalidation method is safe on background workers and after window closure.
func (l *liveState) bindWake(c *ui.Context) {
	l.wakeMu.Lock()
	if l.ctx.Err() == nil {
		l.wakeServices = c.Services()
	}
	l.wakeMu.Unlock()
}

func (l *liveState) wake() {
	l.wakeMu.Lock()
	services := l.wakeServices
	l.wakeMu.Unlock()
	if l.ctx.Err() == nil {
		services.Invalidate()
	}
}

func (l *liveState) stopWake() {
	l.wakeMu.Lock()
	defer l.wakeMu.Unlock()
	if l.wakeTimer != nil {
		l.wakeTimer.Stop()
	}
	l.wakeServices = ui.Services{}
}

// Recovery, failed-page retry and invitation expiry are model deadlines, not
// frame heartbeats. Completions and events wake the window independently.
func (l *liveState) scheduleWake(d *viewState) {
	var due time.Time
	if !l.loading && !l.busy {
		due = l.nextRefresh
		if l.pageRetry.After(due) {
			due = l.pageRetry
		}
	}
	if d.page == "devices" && d.deviceTask == "invite" && d.inviteMode != "ticket" && !l.busy && l.code.ExpiresAtMs > 0 {
		expires := time.UnixMilli(l.code.ExpiresAtMs)
		if due.IsZero() || expires.Before(due) {
			due = expires
		}
	}
	l.wakeMu.Lock()
	defer l.wakeMu.Unlock()
	if l.ctx.Err() != nil || due.Equal(l.wakeDue) {
		return
	}
	if l.wakeTimer != nil {
		l.wakeTimer.Stop()
	}
	l.wakeDue = due
	if !due.IsZero() {
		l.wakeTimer = time.AfterFunc(max(time.Millisecond, time.Until(due)), l.wake)
	}
}
