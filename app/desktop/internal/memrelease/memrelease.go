package memrelease

import (
	"runtime/debug"
	"time"

	"github.com/egoist/mygo"
)

// hiddenReleaseDelay is how long the window stays hidden before the Go heap
// goes back to the system. It lets the hide settle and skips quick hide/show.
const hiddenReleaseDelay = 3 * time.Second

// WhenHidden returns free Go heap pages to the system once the
// window has been hidden for hiddenReleaseDelay. Go's own scavenger is too
// slow for a tray application that stays hidden for hours.
func WhenHidden(win *mygo.Window) {
	var timer *time.Timer
	schedule := func() {
		if timer != nil {
			timer.Stop()
		}
		timer = time.AfterFunc(hiddenReleaseDelay, func() {
			visible := false
			mygo.RunOnMain(func() { visible = win.IsVisible() })
			if !visible {
				debug.FreeOSMemory()
			}
		})
	}
	win.OnHide(schedule)
	win.OnShow(func() {
		if timer != nil {
			timer.Stop()
		}
	})
	if !win.IsVisible() {
		schedule()
	}
}
