// Package setupui renders the BoardUI v2 first-install setup and offline v1 migration flow.
package setupui

import (
	"runtime"
	"sync"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
	"github.com/uniclipboard/unic/app/desktop/internal/appicons"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/internal/v1migration"
)

type Model struct {
	Colors                                                  *boardui.ColorTheme
	Stage, Source, Error, Language                          string
	Detecting, Found, PendingStart, Entering, MigrationOnly bool
	Progress                                                v1migration.Progress
	Report                                                  v1migration.Report
}

type Fields struct{ Passphrase string }

type Actions struct{ Begin, Redetect, Pick, Start, Unlock, Stop, Enter func() }

var brandLogoSVG = sync.OnceValue(func() *ui.SVG {
	return ui.MustParseSVG(appicons.LogoSVG)
})

const compactBreakpoint float32 = 700

// macCaptionBand returns the native macOS traffic-light top band height when
// setup owns the window chrome directly (TitleBarHidden on macOS).
func macCaptionBand(c *ui.Context) float32 {
	bar := c.TitleBar()
	if runtime.GOOS == "darwin" && bar.Left > 0 {
		return max(bar.Height, 44)
	}
	return 0
}

// Render is shared by the live Desktop setup flow and software-view acceptance.
func Render(c *ui.Context, m Model, fields *Fields, a Actions) {
	k := desktopkit.Kit{Dark: c.Theme().Dark, Colors: m.Colors}
	tr := func(s string) string { return i18n.Translate(m.Language, s) }
	w, _ := c.Size()
	compact := w > 0 && w < compactBreakpoint
	topBand := macCaptionBand(c)

	k.UiRoot(c, func() {
		if m.Stage == "welcome" {
			renderWelcome(c, k, m, a, tr, compact, topBand)
			return
		}
		renderMigration(c, k, m, fields, a, tr, compact, topBand)
	})
}

type setupColors struct {
	workspace        ui.Color
	sidebar          ui.Color
	card             ui.Color
	cardSubtle       ui.Color
	inset            ui.Color
	sep              ui.Color
	cardBorder       ui.Color
	activeStepBg     ui.Color
	activeStepBorder ui.Color
	textSub          ui.Color
	textMuted        ui.Color
	accentSoft       ui.Color
	accentEdge       ui.Color
	accentInk        ui.Color
	okBg             ui.Color
	okFg             ui.Color
	okDot            ui.Color
	okEdge           ui.Color
	warnBg           ui.Color
	warnPillBg       ui.Color
	warnFg           ui.Color
	warnBody         ui.Color
	warnEdge         ui.Color
	errBg            ui.Color
	errFg            ui.Color
	errEdge          ui.Color
	shadowCard       ui.Color
	shadowFloat      ui.Color
}

func paletteFor(k desktopkit.Kit) setupColors {
	if k.Dark {
		return setupColors{
			workspace:        k.WorkspaceSurface(),
			sidebar:          k.Color("background-secondary-default"),
			card:             k.CardSurface(),
			cardSubtle:       k.Color("background-secondary-default").MixOklab(k.CardSurface(), .45),
			inset:            ui.RGB(24, 24, 29),
			sep:              ui.RGBA(255, 255, 255, .085),
			cardBorder:       ui.RGBA(255, 255, 255, .105),
			activeStepBg:     ui.RGBA(255, 255, 255, .08),
			activeStepBorder: ui.RGBA(255, 255, 255, .14),
			textSub:          ui.RGB(172, 174, 184),
			textMuted:        ui.RGB(134, 136, 148),
			accentSoft:       ui.RGBA(43, 127, 255, .16),
			accentEdge:       ui.RGBA(77, 160, 255, .36),
			accentInk:        ui.RGB(124, 184, 255),
			okBg:             ui.RGBA(94, 165, 0, .20),
			okFg:             ui.RGB(163, 230, 53),
			okDot:            ui.RGB(132, 204, 22),
			okEdge:           ui.RGBA(132, 204, 22, .34),
			warnBg:           ui.RGBA(245, 158, 11, .11),
			warnPillBg:       ui.RGBA(245, 158, 11, .20),
			warnFg:           ui.RGB(251, 191, 36),
			warnBody:         ui.RGB(228, 200, 140),
			warnEdge:         ui.RGBA(245, 158, 11, .28),
			errBg:            ui.RGBA(244, 63, 94, .14),
			errFg:            ui.RGB(253, 164, 175),
			errEdge:          ui.RGBA(244, 63, 94, .32),
			shadowCard:       ui.RGBA(0, 0, 0, .28),
			shadowFloat:      ui.RGBA(0, 0, 0, .44),
		}
	}
	return setupColors{
		workspace:        k.WorkspaceSurface(),
		sidebar:          k.Color("background-secondary-default"),
		card:             k.CardSurface(),
		cardSubtle:       ui.RGB(247, 247, 249),
		inset:            ui.RGB(241, 241, 245),
		sep:              ui.RGB(234, 234, 239),
		cardBorder:       ui.RGB(226, 226, 232),
		activeStepBg:     k.CardSurface(),
		activeStepBorder: ui.RGB(222, 222, 229),
		textSub:          ui.RGB(96, 96, 108),
		textMuted:        ui.RGB(124, 124, 136),
		accentSoft:       ui.RGB(240, 246, 255),
		accentEdge:       ui.RGB(191, 216, 255),
		accentInk:        ui.RGB(21, 93, 252),
		okBg:             ui.RGB(234, 249, 208),
		okFg:             ui.RGB(47, 87, 0),
		okDot:            ui.RGB(94, 165, 0),
		okEdge:           ui.RGB(197, 232, 142),
		warnBg:           ui.RGB(255, 251, 235),
		warnPillBg:       ui.RGB(254, 243, 199),
		warnFg:           ui.RGB(146, 64, 14),
		warnBody:         ui.RGB(120, 53, 15),
		warnEdge:         ui.RGB(253, 230, 138),
		errBg:            ui.RGB(255, 241, 242),
		errFg:            ui.RGB(190, 18, 60),
		errEdge:          ui.RGB(254, 205, 211),
		shadowCard:       ui.RGBA(0, 0, 0, .04),
		shadowFloat:      ui.RGBA(0, 0, 0, .09),
	}
}

func brandLogo(c *ui.Context, k desktopkit.Kit, size, radius float32, elevated bool) ui.Element {
	bg := ui.RGB(17, 18, 21)
	rim := ui.RGBA(0, 0, 0, .10)
	if k.Dark {
		bg = ui.RGB(24, 25, 31)
		rim = ui.RGBA(255, 255, 255, .16)
	}
	box := ui.Box(c).Size(size, size).Radius(radius).Clip().Shrink(0).Background(bg).Border(1, rim)
	if elevated {
		shadow := ui.RGBA(0, 0, 0, .14)
		if k.Dark {
			shadow = ui.RGBA(0, 0, 0, .42)
		}
		box.Draw(func(pt *ui.Painter, rc ui.Rect) {
			pt.Shadow(rc, radius, 0, 6, 18, 0, shadow)
		})
	}
	box.Children(func() {
		ui.Image(c, brandLogoSVG()).Size(size, size).Fit(ui.Contain).AccessHidden(true)
	})
	return box
}

type pillTone int

const (
	pillNeutral pillTone = iota
	pillPositive
	pillInfo
	pillWarning
	pillError
)

func statusPill(c *ui.Context, k desktopkit.Kit, p setupColors, tone pillTone, label string, dot bool) ui.Element {
	bg, fg, dotCol := k.Color("background-tertiary-default"), p.textSub, p.textMuted
	if k.Dark {
		bg = ui.RGBA(255, 255, 255, .09)
	}
	switch tone {
	case pillPositive:
		bg, fg, dotCol = p.okBg, p.okFg, p.okDot
	case pillInfo:
		bg, fg, dotCol = p.accentSoft, p.accentInk, k.Color("accent-500")
	case pillWarning:
		bg, fg, dotCol = p.warnPillBg, p.warnFg, p.warnFg
	case pillError:
		bg, fg, dotCol = p.errBg, p.errFg, p.errFg
	}
	return ui.Row(c).Height(22).PaddingX(8).Radius(11).Gap(5).AlignItems(ui.Center).Shrink(0).Background(bg).Children(func() {
		if dot {
			ui.Box(c).Size(6, 6).Radius(3).Background(dotCol).Shrink(0).AccessHidden(true)
		}
		k.Text(c, label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11).TextColor(fg).NoWrap()
	})
}

func tagChip(c *ui.Context, k desktopkit.Kit, color desktopkit.TagColor, label string) ui.Element {
	bg, ink := k.TagTint(color)
	dot := k.SwatchColor(color)
	if k.Dark && (color == "" || color == "gray") {
		bg = ui.RGBA(255, 255, 255, .10)
		ink = ui.RGB(190, 192, 202)
		dot = ui.RGB(160, 162, 174)
	}
	return ui.Row(c).Height(21).PaddingX(7).Radius(10.5).Gap(4).AlignItems(ui.Center).Shrink(0).Background(bg).Children(func() {
		ui.Box(c).Size(5, 5).Radius(2.5).Background(dot).Shrink(0).AccessHidden(true)
		k.Text(c, label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(10.8).TextColor(ink).NoWrap()
	})
}

func semanticBanner(c *ui.Context, k desktopkit.Kit, p setupColors, tone desktopkit.NoticeTone, icon desktopkit.IconName, title, body string) ui.Element {
	bg, border, fg := p.cardSubtle, p.cardBorder, p.textSub
	titleCol := k.Color("text-primary")
	switch tone {
	case desktopkit.ToneWarning:
		bg, border, fg, titleCol = p.warnBg, p.warnEdge, p.warnBody, p.warnFg
	case desktopkit.ToneError:
		bg, border, fg, titleCol = p.errBg, p.errEdge, p.errFg, p.errFg
	case desktopkit.ToneInfo:
		bg, border, fg, titleCol = p.accentSoft, p.accentEdge, p.accentInk, p.accentInk
	}
	return ui.Row(c).FillWidth().Padding(11, 13).Radius(10).Gap(9).AlignItems(ui.Start).Background(bg).Border(1, border).Children(func() {
		k.Icon(c, icon, 15).TextColor(titleCol).Margin(1, 0, 0)
		ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
			if title != "" {
				k.Text(c, title, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12.2).TextColor(titleCol)
			}
			if body != "" {
				k.Text(c, body, desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.8).TextColor(fg)
			}
		})
	})
}

func iconTile(c *ui.Context, k desktopkit.Kit, size, radius float32, bg, fg ui.Color, icon desktopkit.IconName, iconSize float32) ui.Element {
	return ui.Box(c).Size(size, size).Radius(radius).Background(bg).Center().Shrink(0).Children(func() {
		k.Icon(c, icon, iconSize).TextColor(fg)
	})
}

func actionButton(c *ui.Context, k desktopkit.Kit, key, label string, variant desktopkit.ButtonVariant, size desktopkit.ControlSize, icon desktopkit.IconName, disabled, busy bool, action func()) {
	kitSize := size
	if variant != desktopkit.Link && size == desktopkit.SizeMedium {
		// Use compact 32px BoardUI small geometry in setup toolbars/footers so
		// buttons stay in proportion with 12-13px desktop dialog copy.
		kitSize = desktopkit.SizeSmall
	}
	b := k.Button(c.Key(key), label, desktopkit.ButtonProps{
		Variant:  variant,
		Size:     kitSize,
		Icon:     icon,
		Disabled: disabled || action == nil,
		Busy:     busy,
	})
	if variant != desktopkit.Link {
		switch size {
		case desktopkit.SizeLarge:
			b.MinWidth(148).Height(38).Radius(9).Justify(ui.Center)
		case desktopkit.SizeMedium:
			b.MinWidth(92).Justify(ui.Center)
		}
	}
	if b.Clicked() && action != nil && !disabled && !busy {
		action()
	}
}

// compactSecondaryButton renders a 28px inline secondary button matching BoardUI
// card controls (such as "选择其他目录") without overpowering 13px card headings.
func compactSecondaryButton(c *ui.Context, k desktopkit.Kit, p setupColors, key, label string, disabled bool, action func()) {
	off := disabled || action == nil
	b := ui.ButtonBase(c.Key(key)).Label(label).Disabled(off).DimDisabled(false).
		Height(28).Padding(0, 11).Radius(7).AlignItems(ui.Center).Justify(ui.Center).Shrink(0)
	bg := p.card
	edge := p.cardBorder
	ink := k.Color("text-primary")
	if b.Hovered() && !off {
		bg = p.cardSubtle
		edge = k.Color("border-button-hover")
	}
	if b.Pressed() && !off {
		bg = p.inset
	}
	if off {
		ink = p.textMuted
		b.Opacity(.55)
	}
	b.Background(bg).Border(1, edge).FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2).Children(func() {
		k.Text(c.Key("t"), label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12).TextColor(ink).SingleLine()
	})
	if b.Clicked() && !off {
		action()
	}
}
