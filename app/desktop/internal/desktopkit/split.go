package desktopkit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

type layTokens struct {
	Window     ui.Color // window background behind panes
	View       ui.Color // list/content background
	Card       ui.Color // raised group / card fill
	CardStroke ui.Color // hairline around cards
	Border     ui.Color // dividers
	Shade      ui.Color // quiet fill (tiles, badges, hover)
	Hover      ui.Color
	Fg         ui.Color
	Fg2        ui.Color
	Fg3        ui.Color
	Accent     ui.Color
	Code       ui.Color // dark code card
	Header     ui.Color // toolbar band
	Detail     ui.Color // detail pane background
}

func (k Kit) layTok() layTokens {
	{
	}

	return layTokens{
		Window: k.Color("background-full"), View: k.Color("background-primary-default"), Card: k.Color("background-primary-default"),
		CardStroke: k.Color("separator-border"), Border: k.Color("separator-border"), Shade: k.Color("background-tertiary-default"),
		Hover: k.Color("background-secondary-hover"), Fg: k.Color("text-primary"), Fg2: k.Color("text-secondary"), Fg3: k.Color("text-tertiary"),
		Accent: k.Color("accent-500"), Code: k.Color("accent-950"), Header: k.Color("background-secondary-default"), Detail: k.Color("background-full"),
	}
}

// layStatusColor is the colour of a status dot / status text.
func (k Kit) layStatusColor(t StatusTone) ui.Color {

	switch t {
	case StatusPositive:
		return k.Color("notification-success-foreground")
	case StatusInfo:
		return k.Color("accent-500")
	case StatusWarning:

		return k.Color("status-yellow-text")
	case StatusCritical:
		return k.Color("text-error-primary")
	}
	return k.layTok().Fg3
}

// layRadius is the corner radius of cards and groups.
func (k Kit) layRadius() float32 {

	return 12
}

// SinglePaneWidth is the window width below which a SplitView with DetailOpen shows the list
// and the detail one at a time (the list, or the detail with a back button).
const SinglePaneWidth = 760

type SplitViewProps struct {
	// Title of the content at the leading end of the toolbar band; Subtitle is secondary
	// text after it, e.g. the item count.
	Title, Subtitle string
	// TitleLeading holds navigation controls immediately before the heading.
	TitleLeading func()
	// Sidebar is drawn on the sidebar window region. Leave nil for a window without one.
	Sidebar func()
	// SidebarHidden is the hidden state of the sidebar; the toggle flips it. Nil keeps the
	// sidebar always shown and draws no toggle.
	SidebarHidden *bool
	// SidebarOverlay shows the expanded sidebar over the content, without taking column width.
	SidebarOverlay bool
	// SidebarToggleLabel names the action the toggle performs now ("Hide sidebar" ...);
	// SidebarToggleHint is a display-only shortcut hint appended to its tooltip.
	SidebarToggleLabel, SidebarToggleHint string
	Search                                func()
	// ToolbarLeading holds controls just before the search field, e.g. the list / grid switch.
	ToolbarLeading func()
	// Toolbar holds controls at the trailing end of the toolbar band, after the search field.
	Toolbar func()
	// DetailOpen is the explicit detail navigation: below SinglePaneWidth the list and the
	// detail share one pane and a back button (labelled BackLabel) closes the detail. Pass the
	// same pointer to the Panes. Nil disables the adaptive behaviour.
	DetailOpen *bool
	BackLabel  string
}

// SplitViewResult is the element plus the toolbar interactions of the frame.
type SplitViewResult struct {
	ui.Element
	// SidebarToggled is true on the frame the toggle was activated (the pointer is already flipped).
	SidebarToggled bool
}

// SplitView is the adaptive window layout: a sidebar beside the content whose toolbar band
func (k Kit) SplitView(c *ui.Context, p SplitViewProps, body func()) SplitViewResult {
	t := k.layTok()
	hidden := p.SidebarHidden != nil && *p.SidebarHidden
	overlay := p.SidebarOverlay && p.Sidebar != nil
	w, _ := c.Size()
	single := p.DetailOpen != nil && w < SinglePaneWidth
	detail := single && *p.DetailOpen

	sidebarW := float32(250)
	{
	}

	if p.Sidebar == nil {
		sidebarW = 0
	}
	target := sidebarW
	if hidden || overlay { // an overlay sidebar floats over the content and takes no column
		target = 0
	}

	root := ui.Column(c).Grow(1).MinHeight(0).AlignSelf(ui.Stretch).Clip().Background(t.Window).AlignItems(ui.Stretch)
	motion := ui.Animation{Duration: 280 * time.Millisecond, Ease: ui.EaseOut}

	curW := root.AnimateTo("sidebar", target, motion)
	var res SplitViewResult
	res.Element = root

	toggle := func() {
		if p.SidebarHidden == nil || p.SidebarToggleLabel == "" {
			return
		}
		tip := p.SidebarToggleLabel
		if p.SidebarToggleHint != "" {
			tip += " (" + p.SidebarToggleHint + ")"
		}
		b := k.Button(c.Key("split-toggle"), "", ButtonProps{
			Variant: Subtle, Icon: IconSidebar, Label: p.SidebarToggleLabel, Tooltip: tip,
		})
		if b.Clicked() {
			*p.SidebarHidden = !*p.SidebarHidden
			res.SidebarToggled = true
		}
	}

	sidebar := func(width float32, overlayPanel bool) {
		if p.Sidebar == nil {
			return
		}
		s := k.Surface(c.Key("split-sidebar"), RegionSidebar).Width(width).AlignItems(ui.Stretch).Shrink(0).MinHeight(0)
		s.Clip()
		if overlayPanel {
			s.Role(ui.RoleDialog).Label("Sidebar")
		}
		{

			s.InsetShadow(-1, 0, 0, 0, t.Border)
		}

		if overlayPanel {
			s.Absolute().Left(0).Bottom(0).ZIndex(3)

			{
				s.Top(0)
			}
			s.Background(k.Color("background-secondary-default"))
		}
		s.Children(func() {
			// Only the outer viewport changes width. Padding belongs to this
			// fixed-width inner body: padding on the viewport would impose a
			body := ui.Column(c.Key("split-sidebar-body")).Width(sidebarW).MinWidth(sidebarW).Grow(1).MinHeight(0).AccessHidden(hidden)
			{

				body.Padding(8)
			}

			body.Children(p.Sidebar)
		})
	}

	toolbar := func() {
		h := float32(52)
		{
		}

		bar := ui.Row(c.Key("split-toolbar")).Height(h).Gap(6).AlignItems(ui.Center).Role(ui.RoleToolbar).
			Label(p.Title)
		{

			bar.Padding(0, 8).Background(t.Header).InsetShadow(0, -1, 0, 0, t.Border)
		}

		bar.DragWindow()
		bar.Children(func() {
			if detail {
				back := k.Button(c.Key("split-back"), p.BackLabel, ButtonProps{Variant: Subtle, Icon: IconBack})
				if back.Clicked() {
					*p.DetailOpen = false
				}
			}
			{
				toggle()
			}
			if p.TitleLeading != nil {
				p.TitleLeading()
			}
			heading := func() {
				ui.Row(c.Key("split-heading")).Gap(8).AlignItems(ui.Baseline).MinWidth(0).Children(func() {
					k.Text(c.Key("t"), p.Title, TextProps{Role: RoleHeadline, Emphasized: true, Truncate: true})

					if p.Subtitle != "" {
						k.Text(c.Key("s"), p.Subtitle, TextProps{Role: RoleCaption, Tone: ToneSecondary, Truncate: true})
					}
				})
			}
			{

				// Centred heading between the leading controls and the trailing ones.
				if p.ToolbarLeading != nil {
					p.ToolbarLeading()
				}
				ui.Row(c.Key("split-center")).Grow(1).MinWidth(0).Justify(ui.Center).Children(heading)
			}

			if p.Search != nil {
				ui.Row(c.Key("split-search")).Shrink(1).MinWidth(32).Children(p.Search)
			}

			if p.Toolbar != nil {
				ui.Row(c.Key("split-commands")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(p.Toolbar)
			}
		})
	}

	panes := func() {
		content := k.Surface(c.Key("split-content"), RegionContent).Grow(1).MinWidth(0).MinHeight(0).Clip().AlignItems(ui.Stretch)

		// only the leading top corner is round

		content.Children(func() {
			{
				toolbar()
			}
			ui.Row(c.Key("split-panes")).Grow(1).MinHeight(0).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
				if body != nil {
					body()
				}
			})
		})
	}

	root.Children(func() {

		ui.Row(c.Key("split-body")).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
			if !overlay && p.Sidebar != nil {
				// Keep the viewport mounted at zero width, preserving navigation
				// state and avoiding a structural jump at the end of the motion.
				sidebar(max(0, curW), false)
			}
			panes()
		})

		if overlay && !hidden {
			// Light dismiss: the scrim and Escape ask the sidebar to hide again.
			scrim := ui.ButtonBase(c.Key("split-scrim")).Absolute().Left(0).Top(0).Right(0).Bottom(0).
				Background(k.Color("foreground-full")).ZIndex(2).Label(p.SidebarToggleLabel).TabStop(false)
			if scrim.Clicked() && p.SidebarHidden != nil {
				*p.SidebarHidden = true
				res.SidebarToggled = true
			}
			sidebar(sidebarW, true)
			c.OnWindowShortcut(0, ui.KeyEscape, func() {
				if p.SidebarHidden != nil {
					*p.SidebarHidden = true
					res.SidebarToggled = true
				}
			})
		}
	})
	return res
}
