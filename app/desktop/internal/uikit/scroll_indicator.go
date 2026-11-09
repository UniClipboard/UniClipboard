package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

type scrollIndicatorState struct {
	position ui.ScrollState
	x, y     float32
	until    time.Time
}

// Scroll is the Kit's shared vertical scroll container. Desktop pages use
// this constructor, so visibility policy is owned here rather than by pages.
// Layout and native wheel, thumb, keyboard and accessibility behavior remain mygo's.
func (k Kit) Scroll(c *ui.Context) ui.Element {
	return scrollingIndicator(c, ui.Scroll(c))
}

// scrollingIndicator keeps mygo's native scroll geometry and thumb input.
// Hover alone never shows it. Attach before Children or List.Rows so normal
// containers, nested previews and virtual lists share the same behavior.
func scrollingIndicator(c *ui.Context, container ui.Element) ui.Element {
	st := ui.Local(container, "uikit.scroll-indicator", func() scrollIndicatorState { return scrollIndicatorState{} })
	container.TrackScroll(&st.position)
	now := c.Now()
	if st.position.X != st.x || st.position.Y != st.y {
		st.x, st.y = st.position.X, st.position.Y
		st.until = now.Add(600 * time.Millisecond)
	}
	visible := now.Before(st.until)
	container.Scrollbars(visible).KeepScrollbars(visible)
	if visible {
		c.After(st.until.Sub(now))
	}
	return container
}
