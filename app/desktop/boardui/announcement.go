// Derived from official BoardUI components/base/announcement/announcement.tsx
// (commit ec5c5eb3e29c2441be63b9422cd54f411ef23466). MIT; see assets/BOARDUI-MIT.txt.
// Default icon RiShieldStarFill is from Remix Icon v4.9.0; see assets/REMIX-LICENSE.txt.
package boardui

import (
	"fmt"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
)

// Exact SVG from Remix Icon v4.9.0 (icons/System/shield-star-fill.svg / RiShieldStarFill).
const shieldStarFillSVG = `<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path d="M3.78307 2.82598L12 1L20.2169 2.82598C20.6745 2.92766 21 3.33347 21 3.80217V13.7889C21 15.795 19.9974 17.6684 18.3282 18.7812L12 23L5.6718 18.7812C4.00261 17.6684 3 15.795 3 13.7889V3.80217C3 3.33347 3.32553 2.92766 3.78307 2.82598ZM12 13.5L14.9389 15.0451L14.3776 11.7725L16.7553 9.45492L13.4695 8.97746L12 6L10.5305 8.97746L7.24472 9.45492L9.62236 11.7725L9.06107 15.0451L12 13.5Z"/></svg>`

var (
	shieldStarOnce sync.Once
	shieldStarIcon *ui.SVG
)

func defaultAnnouncementIcon() *ui.SVG {
	shieldStarOnce.Do(func() {
		svg, err := ui.ParseSVG([]byte(shieldStarFillSVG))
		if err != nil {
			panic(fmt.Sprintf("boardui: announcement default icon: %v", err))
		}
		shieldStarIcon = svg
	})
	return shieldStarIcon
}

// AnnouncementProps configures the compact BoardUI announcement / onboarding card.
//
// When Open is non-nil, the caller owns visibility; clicking the close button
// sets *Open = false and runs the 250ms blur/scale/fade exit animation before
// unmounting. When Open is nil, the card stays open until dismissed locally.
type AnnouncementProps struct {
	Open        *bool
	Title       string
	Description string
	// Icon selects a named boardui glyph; empty defaults to shield-star-fill.
	Icon string
	// Leading overrides the 20x20 leading icon slot with a caller-supplied glyph.
	Leading func(size float32)
	// ActionLabel renders a full-width secondary small CTA button when non-empty.
	ActionLabel string
	// Dismissible renders a top-right xs CloseButton when true.
	Dismissible bool
	// CloseLabel sets the accessible name of the dismiss button (default "Dismiss").
	CloseLabel string
	// IntroDelay opts into the blur/fade/rise/scale-up entrance after the delay.
	// Zero skips the entrance animation (card appears immediately).
	IntroDelay time.Duration
}

// AnnouncementResult reports the card element and frame interactions.
type AnnouncementResult struct {
	ui.Element
	// Present is true while the card is mounted (including during its exit animation).
	Present bool
	// ActionClicked is true on the frame the CTA button was activated.
	ActionClicked bool
	// Dismissed is true on the frame the close button was activated.
	Dismissed bool
	// Closed is true on the frame the exit animation finished and the card unmounted.
	Closed bool
}

type announcementState struct {
	initialized bool
	present     bool
	entering    bool
	dismissed   bool
}

const announcementMotionDuration = 250 * time.Millisecond

// Announcement renders a BoardUI Event/Announcement card sized for a sidebar footer.
func (k Kit) Announcement(c *ui.Context, p AnnouncementProps) AnnouncementResult {
	var res AnnouncementResult
	owner := ui.Box(c.Key("owner")).Absolute().Size(0, 0).PassThrough().AccessHidden(true)
	st := ui.Local(owner, "boardui.announcement", func() announcementState { return announcementState{} })

	open := !st.dismissed
	if p.Open != nil {
		open = *p.Open
	}

	reduceMotion := c.Preferences().ReduceMotion
	hasIntro := p.IntroDelay > 0 && !reduceMotion

	if !st.initialized {
		st.initialized = true
		if open {
			st.present = true
			if hasIntro {
				st.entering = true
				owner.Jump("ann-p", 0)
			} else {
				owner.Jump("ann-p", 1)
			}
		} else {
			st.present = false
			owner.Jump("ann-p", 0)
		}
	}

	if open && !st.present {
		st.present = true
		st.dismissed = false
		if hasIntro {
			st.entering = true
			owner.Jump("ann-p", 0)
		} else {
			st.entering = false
			owner.Jump("ann-p", 1)
		}
	}

	target := float32(0)
	if open {
		target = 1
	} else {
		st.entering = false
	}

	var anim ui.Animation
	if reduceMotion {
		anim = ui.Animation{Duration: 0}
		owner.Jump("ann-p", target)
	} else if open {
		anim = ui.Animation{
			Duration: announcementMotionDuration,
			Ease:     ui.EaseOut,
			Delay:    p.IntroDelay,
		}
	} else {
		anim = ui.Animation{
			Duration: announcementMotionDuration,
			Ease:     ui.EaseInOut,
		}
	}

	progress := owner.AnimateTo("ann-p", target, anim)
	if open && progress >= 0.999 {
		st.entering = false
	}
	if st.present && !open && progress <= 0.001 {
		st.present = false
		res.Closed = true
		c.Invalidate()
		return res
	}
	if !st.present {
		return res
	}

	card := ui.Column(c.Key("card")).FillWidth().AlignItems(ui.Start).Gap(12).Padding(12).
		Radius(12).Border(1, k.Color("border-button-default")).
		Background(k.Color("background-primary-default")).
		Role(ui.RoleRegion).Label(p.Title)

	if progress < 0.999 {
		scale := float32(0.85) + 0.15*progress
		card.Opacity(progress).Scale(scale, scale).Blur(6 * (1 - progress))
		if st.entering {
			card.Translate(0, 15*(1-progress))
		}
	}
	if !open {
		card.PassThrough().AccessHidden(true)
	}

	card.Children(func() {
		ui.Column(c.Key("content")).FillWidth().AlignItems(ui.Start).Gap(4).Children(func() {
			switch {
			case p.Leading != nil:
				p.Leading(20)
			case p.Icon != "":
				ui.Icon(c.Key("icon"), IconSVG(p.Icon)).Size(20, 20).Shrink(0).
					TextColor(k.Color("foreground-icon-secondary")).AccessHidden(true)
			default:
				ui.Icon(c.Key("icon"), defaultAnnouncementIcon()).Size(20, 20).Shrink(0).
					TextColor(k.Color("foreground-icon-secondary")).AccessHidden(true)
			}

			ui.Column(c.Key("text")).FillWidth().AlignItems(ui.Start).Gap(2).Children(func() {
				k.Text(c.Key("title"), p.Title, "body-medium").
					TextColor(k.Color("text-primary")).FillWidth()
				if p.Description != "" {
					k.Text(c.Key("description"), p.Description, "body-2-medium").
						TextColor(k.Color("text-secondary")).FillWidth()
				}
			})
		})

		if p.ActionLabel != "" {
			btn := k.Button(c.Key("action"), p.ActionLabel, ButtonProps{
				Variant: "secondary",
				Size:    "small",
			}).FillWidth().Justify(ui.Center)
			if open && btn.Clicked() {
				res.ActionClicked = true
			}
		}

		if p.Dismissible {
			closeLabel := p.CloseLabel
			if closeLabel == "" {
				closeLabel = "Dismiss"
			}
			closeBtn := k.CloseButton(c.Key("close"), CloseButtonProps{
				Size:  "xs",
				Label: closeLabel,
			}).Absolute().Right(12).Top(12)
			if open && closeBtn.Clicked() {
				res.Dismissed = true
				if p.Open != nil {
					*p.Open = false
				} else {
					st.dismissed = true
				}
				c.Invalidate()
			}
		}
	})

	res.Element = card
	res.Present = true
	return res
}
