package desktopkit

import (
	"runtime"
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
	canvas := k.workspaceCanvas()
	return layTokens{
		Window: canvas, View: canvas, Card: k.CardSurface(),
		CardStroke: k.Color("separator-border"), Border: k.Color("separator-border"), Shade: k.Color("background-tertiary-default"),
		Hover: k.Color("background-secondary-hover"), Fg: k.Color("text-primary"), Fg2: k.Color("text-secondary"), Fg3: k.Color("text-tertiary"),
		Accent: k.Color("accent-500"), Code: k.Color("accent-950"), Header: k.Color("background-secondary-default"), Detail: canvas,
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
	// ContentScale converts the physical view width to the scaled layout width.
	// Zero preserves unscaled layout.
	ContentScale float32
	// Title of the content at the leading end of the toolbar band; Subtitle is secondary
	// text after it, e.g. the item count.
	Title, Subtitle string
	// SubtitleMuted softly dims Subtitle during a slow foreground refresh instead of
	// blanking it out and causing header flicker.
	SubtitleMuted bool
	// ToolbarLoading draws a subtle 2-DIP indeterminate progress hairline along the
	// bottom edge of the toolbar while a slow query is in flight.
	ToolbarLoading bool
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
	// SearchCentered places Search in the centered omnibar slot of the 52-DIP toolbar
	// between the leading heading and trailing controls (U1 unified search+filter bar).
	SearchCentered bool
	// ToolbarLeading holds controls just before the trailing commands, e.g. the list / grid switch.
	ToolbarLeading func()
	// Toolbar holds controls at the trailing end of the toolbar band, after the search field.
	Toolbar func()
	// ToolbarTrailing holds host-supplied controls after application commands.
	ToolbarTrailing func()
	// DetailOpen is the explicit detail navigation: below SinglePaneWidth the list and the
	// detail share one pane and a back button (labelled BackLabel) closes the detail. Pass the
	// same pointer to the Panes. Nil disables the adaptive behaviour.
	DetailOpen *bool
	BackLabel  string
	// Drawer renders a full-height global inspector drawer anchored to the trailing window
	// edge (covering the titlebar region cleanly without reflowing the body).
	// Set DrawerInset > 0 (e.g. 8) for a floating inset full-height card (D1-b).
	Drawer       func()
	DrawerOpen   bool
	DrawerLabel  string
	DrawerWidth  float32
	DrawerInset  float32
	DrawerRadius float32
}

// TitleBarHeight returns the exact height of the SplitView's top band so a full-height
// global drawer can align its own top bar seam with the main window toolbar.
func (k Kit) TitleBarHeight(c *ui.Context) float32 {
	caption := c.TitleBar()
	h := float32(52)
	if runtime.GOOS == "darwin" && caption.Left > 0 {
		h = max(h, caption.Height)
	}
	return h
}

// SplitViewResult is the element plus the toolbar interactions of the frame.
type SplitViewResult struct {
	ui.Element
	// SidebarToggled is true on the frame the toggle was activated (the pointer is already flipped).
	SidebarToggled bool
}

type drawerPresentation struct {
	initialized bool
	present     bool
}

// SplitView is the adaptive window layout: a sidebar beside the content whose toolbar band
func (k Kit) SplitView(c *ui.Context, p SplitViewProps, body func()) SplitViewResult {
	t := k.layTok()
	caption := c.TitleBar()
	integratedCaption := runtime.GOOS == "darwin" && caption.Left > 0
	headerHeight := float32(52)
	if integratedCaption {
		headerHeight = max(headerHeight, caption.Height)
	}
	hidden := p.SidebarHidden != nil && *p.SidebarHidden
	overlay := p.SidebarOverlay && p.Sidebar != nil
	w, _ := c.Size()
	if p.ContentScale > 0 {
		w /= p.ContentScale
	}
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

	// M5 Layered Depth Slide state for the global trailing drawer (with exit retention).
	drawerState := ui.Local(root, "split-drawer-presentation", func() drawerPresentation { return drawerPresentation{} })
	drawerOpen := p.Drawer != nil && p.DrawerOpen
	if !drawerState.initialized {
		initial := float32(0)
		if drawerOpen {
			initial = 1
			drawerState.present = true
		}
		root.Jump("drawer-visibility", initial)
		drawerState.initialized = true
	}
	drawerTarget := float32(0)
	if drawerOpen {
		drawerState.present = true
		drawerTarget = 1
	}
	drawerDur := 280 * time.Millisecond
	if !drawerOpen {
		drawerDur = 210 * time.Millisecond
	}
	if c.Preferences().ReduceMotion {
		drawerDur = 0
		root.Jump("drawer-visibility", drawerTarget)
	}
	drawerProgress := root.AnimateTo("drawer-visibility", drawerTarget, ui.Animation{
		Duration: drawerDur,
		Ease:     ui.CubicBezier(.32, .72, 0, 1),
	})
	if drawerState.present && !drawerOpen && drawerProgress <= 0.001 {
		drawerState.present = false
		c.Invalidate()
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
		b := ui.ButtonBase(c.Key("split-toggle")).Label(p.SidebarToggleLabel).Tooltip(tip).
			Size(28, 28).Radius(7).Justify(ui.Center).AlignItems(ui.Center).
			FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
		iconColor := k.Color("foreground-icon-secondary")
		if b.Pressed() {
			b.Background(k.Color("background-tertiary-default"))
			iconColor = k.Color("foreground-icon-primary")
		} else if b.Hovered() {
			b.Background(k.Color("background-secondary-hover"))
			iconColor = k.Color("foreground-icon-primary")
		}
		b.Children(func() { k.Icon(c, IconSidebar, 16).TextColor(iconColor).AccessHidden(true) })
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
			if integratedCaption {
				body.Padding(0, 0, 8, 8)
			} else {
				body.Padding(8, 0, 8, 8)
			}

			body.Children(func() {
				if integratedCaption {
					// Reserve only the sidebar's top band. The content toolbar starts at y=0.
					ui.Row(c.Key("split-sidebar-caption")).Height(headerHeight).Shrink(0).DragWindow()
				}
				p.Sidebar()
			})
		})
	}

	toolbar := func() {
		h := headerHeight
		bar := ui.Row(c.Key("split-toolbar")).Height(h).Gap(8).AlignItems(ui.Center).Role(ui.RoleToolbar).
			Label(p.Title)
		bar.Padding(0, 16).Background(t.View).InsetShadow(0, -1, 0, 0, t.Border)

		if integratedCaption {
			// As the sidebar moves, controls must continue clearing the real traffic lights.
			bar.Padding(0, 16, 0, max(16, caption.Left+48-curW))
		}
		bar.DragWindow()
		bar.Children(func() {
			if detail {
				back := k.Button(c.Key("split-back"), p.BackLabel, ButtonProps{Variant: Subtle, Size: SizeSmall, Icon: IconBack})
				if back.Clicked() {
					*p.DetailOpen = false
				}
			}
			if !integratedCaption {
				toggle()
			}
			if p.TitleLeading != nil {
				p.TitleLeading()
			}
			heading := func() {
				h := ui.Row(c.Key("split-heading")).Gap(7).AlignItems(ui.Baseline).MinWidth(0)
				if p.SearchCentered {
					h.MinWidth(68)
				}
				h.Children(func() {
					k.Text(c.Key("t"), p.Title, TextProps{Role: RoleHeadline, Emphasized: true, Truncate: true})

					if p.Subtitle != "" {
						sub := k.Text(c.Key("s"), p.Subtitle, TextProps{Role: RoleCaption, Tone: ToneSecondary, Truncate: true})
						subTarget := float32(1)
						if p.SubtitleMuted {
							subTarget = 0.42
						}
						subAlpha := sub.AnimateTo("sub-a", subTarget, ui.Animation{
							Duration:  140 * time.Millisecond,
							Ease:      ui.EaseOut,
							NonMotion: true,
						})
						if subAlpha < 0.999 {
							sub.Opacity(subAlpha)
						}
					}
				})
			}
			trailingControls := func() {
				if p.ToolbarLeading != nil {
					p.ToolbarLeading()
				}
				if p.Search != nil {
					ui.Row(c.Key("split-search")).Shrink(1).MinWidth(32).Children(p.Search)
				}
				if p.Toolbar != nil {
					ui.Row(c.Key("split-commands")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(p.Toolbar)
				}
			}
			if p.SearchCentered && p.Search != nil {
				heading()
				if !drawerOpen {
					ui.Box(c.Key("split-sp-l")).Grow(1).MinWidth(6)
					sc := ui.Row(c.Key("split-search")).Grow(3).MinWidth(110).MaxWidth(456).Shrink(1).AlignItems(ui.Center)
					if p.ToolbarTrailing != nil {
						sc.MinWidth(0)
					}
					if drawerProgress > 0 {
						sc.Opacity(max(0, 1-drawerProgress*1.4))
					}
					sc.Children(p.Search)
					ui.Box(c.Key("split-sp-r")).Grow(1).MinWidth(6)
					trail := ui.Row(c.Key("split-trailing")).Gap(6).AlignItems(ui.Center).Shrink(0)
					if drawerProgress > 0 {
						trail.Opacity(max(0, 1-drawerProgress*1.4))
					}
					trail.Children(func() {
						if p.ToolbarLeading != nil {
							p.ToolbarLeading()
						}
						if p.Toolbar != nil {
							ui.Row(c.Key("split-commands")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(p.Toolbar)
						}
					})
				} else {
					ui.Box(c.Key("split-spacer")).Grow(1).MinWidth(0)
				}
			} else if integratedCaption {
				// macOS top band: title/count lead; view/search/actions trail.
				heading()
				ui.Box(c.Key("split-spacer")).Grow(1).MinWidth(0)
				if !drawerOpen {
					trail := ui.Row(c.Key("split-trailing")).Gap(8).AlignItems(ui.Center).Shrink(1).MinWidth(0)
					if drawerProgress > 0 {
						trail.Opacity(max(0, 1-drawerProgress*1.4))
					}
					trail.Children(trailingControls)
				}
			} else if drawerOpen {
				heading()
				ui.Box(c.Key("split-spacer")).Grow(1).MinWidth(0)
			} else {
				if p.ToolbarLeading != nil {
					p.ToolbarLeading()
				}
				ui.Row(c.Key("split-center")).Grow(1).MinWidth(0).Justify(ui.Center).Children(heading)
				if p.Search != nil {
					ui.Row(c.Key("split-search")).Shrink(1).MinWidth(32).Children(p.Search)
				}
				if p.Toolbar != nil {
					ui.Row(c.Key("split-commands")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(p.Toolbar)
				}
			}
			if p.ToolbarTrailing != nil {
				ui.Row(c.Key("split-host-controls")).Shrink(0).AlignItems(ui.Center).Children(p.ToolbarTrailing)
			}
			k.TopHairlineProgress(c.Key("toolbar-progress"), p.ToolbarLoading)

		})
	}

	panes := func() {
		content := k.Surface(c.Key("split-content"), RegionContent).Grow(1).MinWidth(0).MinHeight(0).Clip().AlignItems(ui.Stretch)
		// M5 depth recede: gently scale & dim the main canvas as the floating card enters.
		if drawerProgress > 0 {
			sc := float32(1) - 0.012*drawerProgress
			content.Scale(sc, sc).Opacity(float32(1) - 0.16*drawerProgress)
		}

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
		if integratedCaption && p.Sidebar != nil {
			// Keep one toggle outside the inert/animated sidebar, as in v1's mac-split-chrome.
			ui.Row(c.Key("split-native-toggle")).Absolute().Left(max(caption.Left+8, curW-40)).
				Top((headerHeight - 28) / 2).Height(28).ZIndex(4).Children(toggle)
		}
		if p.Drawer != nil && drawerState.present {
			dw := p.DrawerWidth
			if dw <= 0 {
				dw = 436
			}
			if w > 0 {
				dw = min(dw, max(320, w*0.54))
			}
			label := p.DrawerLabel
			if label == "" {
				label = "Detail"
			}
			inset := p.DrawerInset
			drawer := ui.Column(c.Key("split-drawer")).Absolute().
				Top(inset).Right(inset).Bottom(inset).Width(dw).ZIndex(5).
				Background(k.Color("background-primary-default")).Clip().
				Role(ui.RoleRegion).Label(label).AlignItems(ui.Stretch)
			if inset > 0 {
				rad := p.DrawerRadius
				if rad <= 0 {
					rad = 12
				}
				shadow := k.Color("drawer-shadow-inset")
				drawer.Radius(rad).
					Border(1, k.Color("border-button-default")).
					Shadow(0, 18, 48, 0, shadow.Alpha(drawerProgress))
			} else {
				shadow := k.Color("drawer-shadow-edge")
				drawer.InsetShadow(1, 0, 0, 0, t.Border).
					Shadow(-12, 0, 32, 0, shadow.Alpha(drawerProgress))
			}
			// M5 Layered Depth Slide transform: +120px slide + 0.98->1 scale + opacity fade.
			tx := 120 * (1 - drawerProgress)
			sc := float32(0.98) + 0.02*drawerProgress
			drawer.Translate(tx, 0).Scale(sc, sc).Opacity(drawerProgress)
			if !drawerOpen {
				// Non-interactive and hidden from assistive tree while sliding out,
				// allowing immediate re-selection of any grid card mid-exit.
				drawer.PassThrough().AccessHidden(true)
			}
			drawer.Children(p.Drawer)
		}
	})
	return res
}
