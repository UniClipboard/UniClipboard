package desktopkit

import (
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// AnnouncementProps configures an application Announcement card backed by boardui.Announcement.
type AnnouncementProps struct {
	Open        *bool
	Title       string
	Description string
	// Icon overrides the leading 20x20 glyph; empty keeps BoardUI's default shield-star-fill.
	Icon        IconName
	ActionLabel string
	Dismissible bool
	CloseLabel  string
	IntroDelay  time.Duration
}

// AnnouncementResult forwards boardui.AnnouncementResult to application callers.
type AnnouncementResult = boardui.AnnouncementResult

// Announcement delegates directly to the public BoardUI Announcement control.
func (k Kit) Announcement(c *ui.Context, p AnnouncementProps) AnnouncementResult {
	bp := boardui.AnnouncementProps{
		Open:        p.Open,
		Title:       p.Title,
		Description: p.Description,
		ActionLabel: p.ActionLabel,
		Dismissible: p.Dismissible,
		CloseLabel:  p.CloseLabel,
		IntroDelay:  p.IntroDelay,
	}
	if p.Icon != "" {
		icon := p.Icon
		bp.Leading = func(size float32) {
			k.Icon(c.Key("icon"), icon, size).TextColor(k.Color("foreground-icon-secondary")).AccessHidden(true)
		}
	}
	return k.native().Announcement(c, bp)
}
