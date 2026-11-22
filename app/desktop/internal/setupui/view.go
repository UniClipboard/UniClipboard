// Package setupui renders the BoardUI v2 first-install setup and offline v1 migration flow.
package setupui

import (
	"sync"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/appicons"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/internal/v1migration"
)

type Model struct {
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

// Render is shared by the live Desktop setup flow and software-view acceptance.
func Render(c *ui.Context, m Model, fields *Fields, a Actions) {
	k := desktopkit.For(c.Theme().Dark)
	tr := func(s string) string { return i18n.Translate(m.Language, s) }
	w, _ := c.Size()
	compact := w > 0 && w < compactBreakpoint

	k.UiRoot(c, func() {
		if m.Stage == "welcome" {
			renderWelcome(c, k, m, a, tr, compact)
			return
		}
		renderMigration(c, k, m, fields, a, tr, compact)
	})
}

type setupColors struct {
	workspace   ui.Color
	sidebar     ui.Color
	card        ui.Color
	cardSubtle  ui.Color
	inset       ui.Color
	sep         ui.Color
	cardBorder  ui.Color
	accentSoft  ui.Color
	accentEdge  ui.Color
	accentInk   ui.Color
	okBg        ui.Color
	okFg        ui.Color
	okDot       ui.Color
	okEdge      ui.Color
	warnBg      ui.Color
	warnPillBg  ui.Color
	warnFg      ui.Color
	warnEdge    ui.Color
	errBg       ui.Color
	errFg       ui.Color
	errEdge     ui.Color
	shadowCard  ui.Color
	shadowFloat ui.Color
}

func paletteFor(k desktopkit.Kit) setupColors {
	if k.Dark {
		return setupColors{
			workspace:   k.WorkspaceSurface(),
			sidebar:     k.Color("background-secondary-default"),
			card:        k.CardSurface(),
			cardSubtle:  k.Color("background-secondary-default").MixOklab(k.CardSurface(), .45),
			inset:       k.Color("background-full").MixOklab(k.Color("background-secondary-default"), .5),
			sep:         k.Color("separator-border"),
			cardBorder:  k.Color("border-button-default").Alpha(.65),
			accentSoft:  k.Color("accent-500").Alpha(.16),
			accentEdge:  k.Color("accent-400").Alpha(.38),
			accentInk:   k.Color("status-blue-text"),
			okBg:        k.Color("notification-success-background"),
			okFg:        k.Color("notification-success-foreground"),
			okDot:       k.Color("status-lime-text"),
			okEdge:      k.Color("status-lime-text").Alpha(.36),
			warnBg:      k.Color("status-yellow-background"),
			warnPillBg:  k.Color("status-yellow-background"),
			warnFg:      k.Color("status-yellow-text"),
			warnEdge:    k.Color("status-yellow-text").Alpha(.35),
			errBg:       k.Color("background-tertiary-error"),
			errFg:       k.Color("notification-error-foreground"),
			errEdge:     k.Color("border-error-default").Alpha(.42),
			shadowCard:  ui.RGBA(0, 0, 0, .28),
			shadowFloat: ui.RGBA(0, 0, 0, .46),
		}
	}
	return setupColors{
		workspace:   k.WorkspaceSurface(),
		sidebar:     k.Color("background-secondary-default"),
		card:        k.CardSurface(),
		cardSubtle:  k.Color("background-secondary-default"),
		inset:       k.Color("background-tertiary-default").MixOklab(k.CardSurface(), .45),
		sep:         k.Color("separator-border"),
		cardBorder:  k.Color("border-button-default"),
		accentSoft:  k.Color("accent-50"),
		accentEdge:  k.Color("accent-200"),
		accentInk:   k.Color("accent-600"),
		okBg:        k.Color("notification-success-background"),
		okFg:        k.Color("state-success-text"),
		okDot:       k.Color("notification-success-foreground"),
		okEdge:      k.Color("state-success-base"),
		warnBg:      k.Color("status-yellow-background").MixOklab(k.CardSurface(), .72),
		warnPillBg:  k.Color("status-yellow-background").MixOklab(k.CardSurface(), .35),
		warnFg:      k.Color("status-yellow-text"),
		warnEdge:    k.Color("status-yellow-background"),
		errBg:       k.Color("notification-error-background").MixOklab(k.CardSurface(), .35),
		errFg:       k.Color("text-destructive"),
		errEdge:     k.Color("notification-error-background"),
		shadowCard:  ui.RGBA(0, 0, 0, .04),
		shadowFloat: ui.RGBA(0, 0, 0, .09),
	}
}

func brandLogo(c *ui.Context, k desktopkit.Kit, size, radius float32, elevated bool) ui.Element {
	rim := ui.RGBA(0, 0, 0, .08)
	if k.Dark {
		rim = ui.RGBA(255, 255, 255, .18)
	}
	box := ui.Box(c).Size(size, size).Radius(radius).Clip().Shrink(0).Background(ui.RGB(0, 0, 0)).Border(1, rim)
	if elevated {
		shadow := ui.RGBA(0, 0, 0, .14)
		if k.Dark {
			shadow = ui.RGBA(0, 0, 0, .48)
		}
		box.Draw(func(pt *ui.Painter, rc ui.Rect) {
			pt.Shadow(rc, radius, 0, 6, 16, 0, shadow)
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
	bg, fg, dotCol := k.Color("background-tertiary-default"), k.Color("text-secondary"), k.Color("text-tertiary")
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
		ink = ui.RGB(185, 185, 194)
		dot = ui.RGB(160, 160, 170)
	}
	return ui.Row(c).Height(20).PaddingX(7).Radius(10).Gap(4).AlignItems(ui.Center).Shrink(0).Background(bg).Children(func() {
		ui.Box(c).Size(5, 5).Radius(2.5).Background(dot).Shrink(0).AccessHidden(true)
		k.Text(c, label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(10.5).TextColor(ink).NoWrap()
	})
}

func semanticBanner(c *ui.Context, k desktopkit.Kit, p setupColors, tone desktopkit.NoticeTone, icon desktopkit.IconName, title, body string) ui.Element {
	bg, border, fg := p.cardSubtle, p.cardBorder, k.Color("text-secondary")
	titleCol := k.Color("text-primary")
	switch tone {
	case desktopkit.ToneWarning:
		bg, border, fg, titleCol = p.warnBg, p.warnEdge, p.warnFg, p.warnFg
	case desktopkit.ToneError:
		bg, border, fg, titleCol = p.errBg, p.errEdge, p.errFg, p.errFg
	case desktopkit.ToneInfo:
		bg, border, fg, titleCol = p.accentSoft, p.accentEdge, p.accentInk, p.accentInk
	}
	return ui.Row(c).FillWidth().Padding(10, 12).Radius(10).Gap(9).AlignItems(ui.Start).Background(bg).Border(1, border).Children(func() {
		k.Icon(c, icon, 15).TextColor(titleCol).Margin(1, 0, 0)
		ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
			if title != "" {
				k.Text(c, title, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12).TextColor(titleCol)
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
	b := k.Button(c.Key(key), label, desktopkit.ButtonProps{
		Variant:  variant,
		Size:     size,
		Icon:     icon,
		Disabled: disabled || action == nil,
		Busy:     busy,
	})
	if b.Clicked() && action != nil && !disabled && !busy {
		action()
	}
}
