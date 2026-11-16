package desktopui

import (
	"bytes"
	"context"
	"image"
	"image/png"
	"io"
	"time"

	"github.com/disintegration/imaging"
	"github.com/egoist/mygo/ui"
)

const thumbnailMemoryBudget = 32 << 20

// Content requests may run concurrently, but only one image owns encoded and
// full-resolution decode scratch space at a time, across all desktop windows.
var imageDecodeGate = make(chan struct{}, 1)

// One shared owner follows an image through cache, projections and selection.
// An edge of zero denotes the original-resolution selected preview.
type historyImage struct {
	bitmap        *ui.Bitmap
	width, height int
	edge          int
	firstFrame    time.Time
	released      bool
}

// Release on a later frame: the preceding frame has already rendered. mygo
// keeps encoded data and recreates pixels after texture loss or CPU rendering.
func (p *historyImage) source(c *ui.Context) ui.ImageSource {
	if !p.released {
		if p.firstFrame.IsZero() {
			p.firstFrame = c.Now()
			c.After(time.Millisecond)
		} else if !p.firstFrame.Equal(c.Now()) {
			p.bitmap.ReleasePixels()
			p.released = true
		}
	}
	return p.bitmap
}

func thumbnailEdge(count int) int {
	edge := 256
	// CPU base/mips and a reconstructed PNG fit within three RGBA rasters,
	// with an additional per-image allowance for the small-image headers.
	for edge > 16 && (12*edge*edge+1024)*max(1, count) > thumbnailMemoryBudget {
		edge /= 2
	}
	return edge
}

func (p *historyImage) matches(edge int) bool {
	return (p.edge == 0 && edge == 0) || (p.edge > 0 && edge > 0 && p.edge <= edge)
}

func imageDimensions(v entry) (float32, float32) {
	if v.picture != nil {
		return float32(v.picture.width), float32(v.picture.height)
	}
	return ui.ImageSize(v.image)
}

// Preserve the original payload and oriented dimensions. Only neighbors are
// re-encoded into a bounded thumbnail; the selected image uses mygo's decoder.
func readPreviewImage(ctx context.Context, r io.Reader, edge int) (*historyImage, string) {
	select {
	case <-ctx.Done():
		return nil, "读取内容失败。"
	case imageDecodeGate <- struct{}{}:
	}
	defer func() { <-imageDecodeGate }()
	if ctx.Err() != nil {
		return nil, "读取内容失败。"
	}
	const limit = 32 << 20
	raw, err := io.ReadAll(io.LimitReader(r, limit+1))
	if err != nil {
		return nil, "读取内容失败。"
	}
	if len(raw) > limit {
		return nil, "内容超过预览大小限制。"
	}
	cfg, _, err := image.DecodeConfig(bytes.NewReader(raw))
	if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || int64(cfg.Width) > 16_000_000/int64(cfg.Height) {
		return nil, "图片无法预览。"
	}
	p := &historyImage{edge: edge}
	if edge > 0 {
		decoded, err := imaging.Decode(bytes.NewReader(raw), imaging.AutoOrientation(true))
		if err != nil {
			return nil, "图片无法预览。"
		}
		p.width, p.height = decoded.Bounds().Dx(), decoded.Bounds().Dy()
		thumb := imaging.Fit(decoded, edge, edge, imaging.Lanczos)
		var encoded bytes.Buffer
		encoder := png.Encoder{CompressionLevel: png.BestSpeed}
		if err := encoder.Encode(&encoded, thumb); err != nil {
			return nil, "图片无法预览。"
		}
		raw = encoded.Bytes()
	}
	p.bitmap, err = ui.DecodeBitmap(raw)
	if err != nil {
		return nil, "图片无法预览。"
	}
	if edge == 0 {
		p.width, p.height = p.bitmap.Size()
	}
	if ctx.Err() != nil {
		return nil, "读取内容失败。"
	}
	return p, ""
}
