package desktopui

import (
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

func (p *quickPanelState) imagePreview(c *ui.Context, k desktopkit.Kit, a qpPalette, e qpEntry) {
	if p.imageID != e.id {
		p.imageID = e.id
		p.actualSize = false
		p.imagePan = ui.Point{}
		p.imageDragging = false
	}
	img := p.quickImage(c, e)
	stage := ui.Column(c).FillWidth().Grow(1).MinHeight(0).Clip().Background(a.muted).Label(e.title)
	bounds := stage.Bounds()
	width, height := float32(800), float32(500)
	if p.live != nil {
		width, height = p.quickImageSize(e)
	}
	clamp := func() {
		maxX := max(float32(0), (width-bounds.W)/2)
		maxY := max(float32(0), (height-bounds.H)/2)
		p.imagePan.X = max(-maxX, min(maxX, p.imagePan.X))
		p.imagePan.Y = max(-maxY, min(maxY, p.imagePan.Y))
	}
	toggle := func() { p.actualSize = !p.actualSize; p.imagePan = ui.Point{}; p.imageDragging = false; p.update() }
	stage.HandleInput(func(ev ui.InputEvent) bool {
		if img == nil {
			return false
		}
		switch ev.Kind {
		case ui.InputPointerDown:
			if ev.Button != 0 {
				return false
			}
			if ev.Clicks == 2 {
				toggle()
				return true
			}
			if p.actualSize {
				p.imageDragging = true
				p.imageDragStart = ui.Point{X: ev.X, Y: ev.Y}
				p.imagePanStart = p.imagePan
				return true
			}
		case ui.InputPointerMove:
			if p.imageDragging {
				p.imagePan = ui.Point{X: p.imagePanStart.X + ev.X - p.imageDragStart.X, Y: p.imagePanStart.Y + ev.Y - p.imageDragStart.Y}
				clamp()
				p.update()
				return true
			}
		case ui.InputPointerUp:
			if p.imageDragging {
				p.imageDragging = false
				return true
			}
		}
		return false
	})
	stage.Children(func() {
		if img == nil {
			k.ShimmerBox(c.Key("img-preview-skel"), 0).Fill()
			return
		}
		if p.actualSize {
			ui.Image(c, img).Size(width, height).Absolute().LeftPercent(50).TopPercent(50).Margin(-height/2+p.imagePan.Y, 0, 0, -width/2+p.imagePan.X)
		} else {
			ui.Image(c, img).Fill().Fit(ui.Contain).Label(e.title)
		}
		target := float32(0)
		if stage.Hovered() {
			target = 1
		}
		opacity := stage.Animate("image-overlay", target, 140*time.Millisecond)
		{
			format := "PNG"
			if e.kind == "文件" {
				if ext := qpFileExtensionBadge(e); ext != "" {
					format = ext
				}
			}
			meta := fmt.Sprintf(p.tr("%.0f × %.0f · PNG · 来自 %s"), width, height, e.source)
			if format != "PNG" {
				meta = strings.Replace(meta, " · PNG · ", " · "+format+" · ", 1)
			}
			ui.Row(c).Absolute().Top(12).Left(12).Padding(5, 8).Radius(6).Background(a.background).Opacity(opacity).Children(func() {
				ui.Text(c, meta).FontSize(11).TextColor(a.quiet)
			})
			label := p.tr("原始尺寸")
			if p.actualSize {
				label = p.tr("完整显示")
			}
			ui.ButtonBase(c.Key("zoom")).Label(label).Disabled(opacity <= 0).Absolute().Right(12).Bottom(12).Height(28).PaddingX(10).Radius(6).Background(a.background).Opacity(opacity).Children(func() { ui.Text(c, label).FontSize(12) }).OnClick(toggle)
		}
	})
	if e.kind == "文件" {
		if caption := qpSingleFileName(e); caption != "" {
			ui.Row(c.Key("img-file-caption")).Height(30).Shrink(0).Padding(0, 12).Gap(6).BorderWidth(1, 0, 0, 0).BorderColor(a.border).AlignItems(ui.Center).Children(func() {
				k.Icon(c, desktopkit.IconFile, 13).TextColor(a.quiet)
				ui.Text(c, caption).FontSize(11.5 * zoomScale(p.textZoom)).TextColor(a.quiet).SingleLine().Grow(1).MinWidth(0)
			})
		}
	}
}
