package desktopui

import (
	"math"
	"strings"
	"time"
	"unicode"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

const (
	qpPanelWidth             = 380
	qpPanelHeight            = 366
	qpSatellitePreviewWidth  = 400
	qpSatelliteMenuWidth     = 380
	qpSatelliteImageHeight   = 310
	qpSatelliteDefaultHeight = 240
	qpSatelliteMinHeight     = 104
	qpSatelliteMaxHeight     = 480
	qpSatelliteBorder        = 1
	qpSatelliteMenuPad       = 12
	qpSatelliteMenuListPad   = 6
	qpSatelliteMenuRowHeight = 30
	qpSatelliteMenuRowGap    = 2
	qpSatelliteHeaderPadTop  = 12
	qpSatelliteHeaderPadSide = 16
	qpSatelliteHeaderPadBot  = 8
	qpSatelliteBodyPadTop    = 4
	qpSatelliteBodyPadSide   = 16
	qpSatelliteBodyPadBot    = 16
	qpSatelliteBodyGap       = 8
	qpSatelliteFileIconSize  = 24
	qpSatelliteFileRowGap    = 8
)

func (p *quickPanelState) satelliteSize() (width, height int) {
	scale := zoomScale(p.windowZoom)
	textScale := zoomScale(p.textZoom)
	minHeight := max(1, int(math.Ceil(float64(qpSatelliteMinHeight*scale))))
	maxHeight := max(minHeight, int(math.Ceil(float64(qpSatelliteMaxHeight*scale))))

	if p.menu != "" {
		width = max(1, int(math.Round(float64(qpSatelliteMenuWidth*scale))))
		title := p.tr("操作")
		if p.menu == "devices" {
			title = p.tr("发送到")
		}
		headerW := max(float32(1), float32(width)-2*qpSatelliteBorder-2*qpSatelliteMenuPad)
		headerH := p.measureTextHeight(headerW, title, "", 11*textScale, 0, float32(maxHeight)) + 2*qpSatelliteMenuPad
		footerH := p.measureTextHeight(headerW, p.tr("↑↓ 选择 · ⏎ 执行 · esc 返回"), "", 11*textScale, 0, float32(maxHeight)) + 20
		rows := len(p.actions())
		listH := float32(2*qpSatelliteMenuListPad + rows*qpSatelliteMenuRowHeight + max(0, rows-1)*qpSatelliteMenuRowGap)
		total := float32(2*qpSatelliteBorder) + headerH + listH + footerH
		height = max(minHeight, min(maxHeight, int(math.Ceil(float64(total)))))
		return width, height
	}

	width = max(1, int(math.Round(float64(qpSatellitePreviewWidth*scale))))
	e := p.previewEntry()
	if e == nil {
		height = max(minHeight, min(maxHeight, int(math.Round(float64(qpSatelliteDefaultHeight*scale)))))
		return width, height
	}
	if p.quickHasImagePreview(*e) {
		height = max(minHeight, min(maxHeight, int(math.Round(float64(qpSatelliteImageHeight*scale)))))
		return width, height
	}

	title := p.tr(e.kind) + " · " + e.age + p.tr(" · 来自 ") + e.source
	headerW := max(float32(1), float32(width)-2*qpSatelliteBorder-2*qpSatelliteHeaderPadSide)
	headerH := p.measureTextHeight(headerW, title, "", 11*textScale, 0, float32(maxHeight)) + qpSatelliteHeaderPadTop + qpSatelliteHeaderPadBot

	bodyW := max(float32(1), float32(width)-2*qpSatelliteBorder-2*qpSatelliteBodyPadSide)
	bodyPad := float32(qpSatelliteBodyPadTop + qpSatelliteBodyPadBot)
	var bodyH float32
	switch e.kind {
	case "富文本":
		if p.live != nil {
			bodyH = p.measureTextHeight(bodyW, e.text, "", 14*textScale, 0, float32(maxHeight))
		} else {
			bodyH = 200
		}
	case "链接":
		titleH := p.measureTextHeight(bodyW, e.title, "", 14*textScale, 700, float32(maxHeight))
		textH := p.measureTextHeight(bodyW, e.text, "", 12*textScale, 0, float32(maxHeight))
		bodyH = titleH + qpSatelliteBodyGap + textH
	case "文件":
		files := strings.Split(e.text, "\n")
		fileW := max(float32(1), bodyW-qpSatelliteFileIconSize-qpSatelliteFileRowGap)
		for i, file := range files {
			if i > 0 {
				bodyH += qpSatelliteBodyGap
			}
			rowH := max(float32(qpSatelliteFileIconSize), p.measureTextHeight(fileW, file, "", 13*textScale, 0, float32(maxHeight)))
			bodyH += rowH
			if bodyH >= float32(maxHeight) {
				break
			}
		}
	default:
		font, size := "", 14*textScale
		if e.kind == "代码" {
			font, size = "monospace", 12*textScale
		}
		bodyH = p.measureTextHeight(bodyW, e.text, font, size, 0, float32(maxHeight))
	}

	total := float32(2*qpSatelliteBorder) + headerH + bodyPad + bodyH
	height = max(minHeight, min(maxHeight, int(math.Ceil(float64(total)))))
	return width, height
}

func (p *quickPanelState) measureTextHeight(width float32, text, font string, size float32, weight int, maxHeight float32) float32 {
	if text == "" {
		return 0
	}
	const maxMeasureLines = 80
	const maxMeasureRunes = 12000
	sample, lineOverflow := truncateForMeasure(text, maxMeasureLines, maxMeasureRunes)
	if lineOverflow {
		return maxHeight
	}
	if p.measureText != nil {
		if _, h := p.measureText(width, ui.Span{Text: sample, Font: font, Size: size, Weight: weight}); h > 0 {
			return h
		}
	}
	return estimateWrappedTextHeight(width, sample, size)
}

func truncateForMeasure(s string, maxLines, maxRunes int) (string, bool) {
	lines, runes := 0, 0
	for i, r := range s {
		if r == '\n' {
			lines++
			if lines >= maxLines {
				return s[:i], true
			}
		}
		runes++
		if runes >= maxRunes {
			return s[:i], false
		}
	}
	return s, false
}

func estimateWrappedTextHeight(width float32, text string, size float32) float32 {
	if size <= 0 {
		size = 14
	}
	if width <= 0 {
		width = 1
	}
	lines := 0
	for _, para := range strings.Split(text, "\n") {
		if para == "" {
			lines++
			continue
		}
		var paraW float32
		for _, r := range para {
			switch {
			case r >= 0x1100 && (unicode.In(r, unicode.Han, unicode.Hiragana, unicode.Katakana, unicode.Hangul) || (r >= 0xFF00 && r <= 0xFFEF) || (r >= 0x3000 && r <= 0x303F)):
				paraW += size
			default:
				paraW += size * 0.6
			}
		}
		wrapped := int(math.Ceil(float64(paraW / width)))
		lines += max(1, wrapped)
	}
	return float32(max(1, lines)) * size * 1.45
}

func openQuickWindow(p *quickPanelState, closed func()) *mygo.Window {
	p.quit = mygo.App.Quit

	title := "unic · 快捷面板演示"
	if p.live != nil {
		title = "unic · 快捷面板"
	}
	history := mygo.NewWindow(mygo.WindowOptions{
		Title: title, Width: qpPanelWidth, Height: qpPanelHeight, UseContentSize: true,
		Frameless: true, DisableResize: true, AlwaysOnTop: true,
		Content: ui.View(p.panel),
	})
	satellite := mygo.NewFlyout(mygo.FlyoutOptions{
		Parent: history, Width: qpSatellitePreviewWidth, Height: qpSatelliteDefaultHeight, Hidden: true, Focusable: false,
		Placement: mygo.PlacementRight, Gap: 8, Shadow: true, CornerRadius: 12,
		Anchor: mygo.Rectangle{X: 0, Y: 54, Width: qpPanelWidth, Height: 30}, Content: ui.View(p.satellite),
	})
	windowZoom := 0
	syncWindow := func() {
		if !p.visible {
			p.satelliteWidth, p.satelliteHeight = 0, 0
			satellite.Hide()
			history.Hide()
			return
		}
		scale := zoomScale(p.windowZoom)
		panelWidth := int(qpPanelWidth * scale)
		if p.windowZoom != windowZoom {
			windowZoom = p.windowZoom
			history.SetContentSize(panelWidth, int(qpPanelHeight*scale))
		}
		if p.menu == "" && p.previewEntry() == nil {
			p.satelliteWidth, p.satelliteHeight = 0, 0
			satellite.Hide()
			return
		}
		width, height := p.satelliteSize()
		p.satelliteWidth, p.satelliteHeight = width, height
		anchor := p.anchor
		if anchor.H <= 0 {
			anchor = ui.Rect{Y: 54, W: qpPanelWidth, H: 30}
		}
		// Use the full parent width so the satellite follows a row vertically
		// but never overlaps the history panel horizontally.
		satellite.SetAnchor(mygo.Rectangle{X: 0, Y: int(anchor.Y), Width: panelWidth, Height: int(anchor.H)})
		satellite.SetSize(width, height)
		satellite.Invalidate()
		satellite.ShowInactive()
	}
	p.onUpdate = syncWindow
	history.OnMove(syncWindow)
	p.nativeFocused = history.IsFocused
	ignoreBlurUntil := time.Now().Add(300 * time.Millisecond)
	history.OnBlur(func() {
		p.blurDue = time.Now().Add(100 * time.Millisecond)
		if p.blurDue.Before(ignoreBlurUntil) {
			p.blurDue = ignoreBlurUntil
		}
		history.Invalidate()
	})
	offActivate := func() {}
	if p.live == nil {
		offActivate = mygo.App.OnActivate(func(hasVisibleWindows bool) {
			if !hasVisibleWindows {
				p.visible = true
				p.expanded = false
				p.menu = ""
				p.applySearch()
				history.Show()
				history.Invalidate()
			}
		})
	}

	history.OnClosed(func() {
		offActivate()
		p.onUpdate = nil
		p.panelInvalidate = nil
		p.satelliteInvalidate = nil
		p.nativeFocused = nil
		if closed != nil {
			closed()
		}
	})
	return history
}
