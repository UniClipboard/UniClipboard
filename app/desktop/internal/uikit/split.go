package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// layTokens are the skin colours shared by the layout components (split view, panes,
// master-detail, headers, preview, settings, transfer ...), resolved once per call so the
// components do not repeat the per-skin token names.
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
	switch k.Skin {
	case MacOS:
		return layTokens{
			Window: k.Color("window"), View: k.Color("content"), Card: k.Color("fill-4"),
			CardStroke: k.Color("separator"), Border: k.Color("separator"), Shade: k.Color("fill-3"),
			Hover: k.Color("fill-2"), Fg: k.Color("label"), Fg2: k.Color("label-2"), Fg3: k.Color("label-3"),
			Accent: k.Color("accent"), Code: k.Color("code-bg"), Header: k.Color("content"), Detail: k.Color("content"),
		}
	case Windows:
		return layTokens{
			Window: k.Color("w11-base"), View: k.Color("colorNeutralBackground1"), Card: k.Color("w11-card"),
			CardStroke: k.Color("w11-card-stroke"), Border: k.Color("w11-divider"), Shade: k.Color("w11-selected"),
			Hover: k.Color("colorSubtleBackgroundHover"), Fg: k.Color("colorNeutralForeground1"),
			Fg2: k.Color("colorNeutralForeground2"), Fg3: k.Color("colorNeutralForeground3"),
			Accent: k.Color("colorBrandBackground"), Code: k.Color("w11-code-bg"), Header: k.Color("w11-base"),
			Detail: k.Color("w11-detail"),
		}
	}
	return layTokens{
		Window: k.Color("window"), View: k.Color("view"), Card: k.Color("card"),
		CardStroke: k.Color("border"), Border: k.Color("border"), Shade: k.Color("shade"),
		Hover: k.Color("hover"), Fg: k.Color("fg"), Fg2: k.Color("fg2"), Fg3: k.Color("fg3"),
		Accent: k.Color("accent"), Code: k.Color("code"), Header: k.Color("header"), Detail: k.Color("window"),
	}
}

// layStatusColor is the colour of a status dot / status text.
func (k Kit) layStatusColor(t StatusTone) ui.Color {
	if k.Skin == Windows {
		switch t {
		case StatusPositive:
			return k.Color("w11-status-positive")
		case StatusInfo:
			return k.Color("w11-status-info")
		case StatusWarning:
			return k.Color("w11-status-warning")
		case StatusCritical:
			return k.Color("w11-status-critical")
		}
		return k.Color("w11-status-neutral")
	}
	switch t {
	case StatusPositive:
		return k.Color("green")
	case StatusInfo:
		return k.Color("accent")
	case StatusWarning:
		if k.Skin == MacOS {
			return k.Color("orange")
		}
		return k.Color("warning")
	case StatusCritical:
		return k.Color("red")
	}
	return k.layTok().Fg3
}

// layRadius is the corner radius of cards and groups.
func (k Kit) layRadius() float32 {
	if k.Skin == Windows {
		return k.Px("borderRadiusLarge")
	}
	return 12
}

// SinglePaneWidth is the window width below which a SplitView with DetailOpen shows the list
// and the detail one at a time (the list, or the detail with a back button).
const SinglePaneWidth = 760

// SplitViewProps mirrors contract.ts SplitViewProps. Slots are builder funcs; the sidebar
// toggle and the detail navigation use state pointers instead of callbacks.
type SplitViewProps struct {
	// Title of the content at the leading end of the toolbar band; Subtitle is secondary
	// text after it, e.g. the item count.
	Title, Subtitle string
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
	// Search is the window's search field (macOS: first of the trailing group; Windows: centred).
	Search func()
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

const layMacToolbar = 52

// SplitView is the adaptive window layout: a sidebar beside the content whose toolbar band
// (title, search, commands) runs across the panes built by body. Linux/macOS put the sidebar
// full height; Windows puts a 48 px title bar across the whole window (toggle at the far
// left, search centred); the 48 px icon rail of a hidden sidebar is not drawn.
func (k Kit) SplitView(c *ui.Context, p SplitViewProps, body func()) SplitViewResult {
	t := k.layTok()
	hidden := p.SidebarHidden != nil && *p.SidebarHidden
	overlay := p.SidebarOverlay && p.Sidebar != nil
	w, _ := c.Size()
	single := p.DetailOpen != nil && w < SinglePaneWidth
	detail := single && *p.DetailOpen

	sidebarW := float32(250)
	switch k.Skin {
	case MacOS:
		sidebarW = 236
	case Windows:
		sidebarW = 260
	}
	if p.Sidebar == nil {
		sidebarW = 0
	}
	target := sidebarW
	if hidden || overlay { // an overlay sidebar floats over the content and takes no column
		target = 0
	}

	root := ui.Column(c).Grow(1).MinHeight(0).AlignSelf(ui.Stretch).Clip().Background(t.Window).AlignItems(ui.Stretch)
	motion := ui.Smooth
	if k.Skin == MacOS {
		motion = ui.Animation{Duration: 280 * time.Millisecond, Ease: ui.CubicBezier(.2, 0, 0, 1)}
	}
	curW := root.AnimateTo("sidebar", target, motion)
	// Derive all macOS chrome positions from the same animated width.
	// Reading hidden directly would jump the controls to their final positions.
	reveal := float32(1)
	if !overlay && sidebarW > 0 {
		reveal = min(1, max(0, curW/sidebarW))
	} else if hidden {
		reveal = 0
	}
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
		switch k.Skin {
		case Windows:
			if overlayPanel {
				s.Shadow(0, 8, 28, 0, ui.RGBA(0, 0, 0, .28)).Radius(0, k.Px("borderRadiusXLarge"), 0, 0)
			}
		case Linux:
			s.InsetShadow(-1, 0, 0, 0, t.Border)
		}
		if overlayPanel {
			s.Absolute().Left(0).Bottom(0).ZIndex(3)
			if k.Skin == Windows {
				s.Top(48)
			} else {
				s.Top(0)
			}
			s.Background(k.Sem("sidebar"))
		}
		s.Children(func() {
			// Only the outer viewport changes width. Padding belongs to this
			// fixed-width inner body: padding on the viewport would impose a
			// 20 px minimum on macOS, followed by a jump when it unmounts.
			body := ui.Column(c.Key("split-sidebar-body")).Width(sidebarW).MinWidth(sidebarW).Grow(1).MinHeight(0).AccessHidden(hidden)
			switch k.Skin {
			case MacOS:
				body.Padding(layMacToolbar, 10, 12)
			case Windows:
				body.Padding(4, 6, 10)
			default:
				body.Padding(8)
			}
			if k.Skin == MacOS && !overlay {
				body.Transform(ui.Translation((reveal-1)*sidebarW, 0))
			}
			body.Children(p.Sidebar)
		})
	}

	toolbar := func() {
		h := float32(46)
		switch k.Skin {
		case MacOS:
			h = layMacToolbar
		case Windows:
			h = 48
		}
		bar := ui.Row(c.Key("split-toolbar")).Height(h).Gap(6).AlignItems(ui.Center).Role(ui.RoleToolbar).
			Label(p.Title)
		switch k.Skin {
		case MacOS:
			inset := float32(16)
			if p.Sidebar != nil {
				inset += 108 * (1 - reveal)
			}
			bar.Padding(8, 12, 0, inset).Gap(10)
		case Windows:
			bar.Padding(0, 8, 0, 4)
		default:
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
			if k.Skin != MacOS {
				toggle()
			}
			heading := func() {
				ui.Row(c.Key("split-heading")).Gap(8).AlignItems(ui.Baseline).MinWidth(0).Children(func() {
					title := k.Text(c.Key("t"), p.Title, TextProps{Role: RoleHeadline, Emphasized: true, Truncate: true})
					if k.Skin == MacOS {
						title.FontSize(15)
					}
					if p.Subtitle != "" {
						k.Text(c.Key("s"), p.Subtitle, TextProps{Role: RoleCaption, Tone: ToneSecondary, Truncate: true})
					}
				})
			}
			switch k.Skin {
			case Linux:
				// Centred heading between the leading controls and the trailing ones.
				if p.ToolbarLeading != nil {
					p.ToolbarLeading()
				}
				ui.Row(c.Key("split-center")).Grow(1).MinWidth(0).Justify(ui.Center).Children(heading)
			case Windows:
				heading()
				ui.Row(c.Key("split-center")).Grow(1).MinWidth(0).Justify(ui.Center).Children(func() {
					if p.Search != nil {
						ui.Row(c.Key("split-search")).Grow(1).MaxWidth(460).Children(p.Search)
					}
				})
				if p.ToolbarLeading != nil {
					p.ToolbarLeading()
				}
			default:
				heading()
				ui.Spacer(c)
				if p.ToolbarLeading != nil {
					p.ToolbarLeading()
				}
			}
			if k.Skin != Windows && p.Search != nil {
				width := float32(260)
				search := ui.Row(c.Key("split-search")).Shrink(1)
				if k.Skin != MacOS {
					search.Width(width).MinWidth(120)
				}
				search.Children(p.Search)
			}
			if p.Toolbar != nil {
				ui.Row(c.Key("split-commands")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(p.Toolbar)
			}
		})
	}

	panes := func() {
		content := k.Surface(c.Key("split-content"), RegionContent).Grow(1).MinWidth(0).MinHeight(0).Clip().AlignItems(ui.Stretch)
		if k.Skin == Windows {
			content.Radius(0, 0, 0, k.Px("borderRadiusXLarge")) // only the leading top corner is round
		}
		content.Children(func() {
			if k.Skin != Windows {
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
		if k.Skin == Windows {
			toolbar()
		}
		ui.Row(c.Key("split-body")).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
			if !overlay && p.Sidebar != nil {
				// Keep the viewport mounted at zero width, preserving navigation
				// state and avoiding a structural jump at the end of the motion.
				sidebar(max(0, curW), false)
			}
			panes()
		})
		if k.Skin == MacOS && p.SidebarHidden != nil && p.SidebarToggleLabel != "" {
			x := 92 + (sidebarW-40-92)*reveal
			ui.Row(c.Key("sidebar-chrome")).Absolute().Left(x).Top(12).Size(32, 32).ZIndex(4).AlignItems(ui.Center).Justify(ui.Center).Children(toggle)
		}
		if overlay && !hidden {
			// Light dismiss: the scrim and Escape ask the sidebar to hide again.
			scrim := ui.ButtonBase(c.Key("split-scrim")).Absolute().Left(0).Top(0).Right(0).Bottom(0).
				Background(k.Color("scrim")).ZIndex(2).Label(p.SidebarToggleLabel).TabStop(false)
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
