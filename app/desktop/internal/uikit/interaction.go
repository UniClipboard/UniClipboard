package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// pressFeedback keeps press and release motion consistent across macOS controls.
// It follows the physical press, independently of a toggle's selected state.
func (k Kit) pressFeedback(c *ui.Context, e ui.Element, depth float32) {
	if k.Skin != MacOS || c.Preferences().ReduceMotion {
		return
	}
	target := float32(1)
	motion := ui.Animation{Spring: ui.SpringOptions{Duration: 350 * time.Millisecond, Bounce: .12}}
	if !e.IsDisabled() && e.Pressed() {
		target = depth
		motion = ui.Animation{Duration: 80 * time.Millisecond, Ease: ui.EaseOut}
	}
	scale := e.AnimateTo("uikit.press", target, motion)
	e.Transform(ui.Scaling(scale, scale))
}
