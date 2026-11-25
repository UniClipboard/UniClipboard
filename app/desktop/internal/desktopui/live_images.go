package desktopui

import (
	"bufio"
	"bytes"
	"context"
	"image"
	"image/png"
	"io"
	"time"

	"github.com/disintegration/imaging"
	"github.com/egoist/mygo/ui"
	_ "golang.org/x/image/webp"

	api "github.com/uniclipboard/unic/internal/localapi"
)

const thumbnailMemoryBudget = 32 << 20

// One shared owner follows an image through cache, projections and selection.
// An edge of zero denotes the original-resolution selected preview.
type historyImage struct {
	bitmap        *ui.Bitmap
	width, height int
	edge          int
	firstFrame    time.Time
	released      bool
}

type bitmapPreview struct {
	source  ui.ImageSource
	history *historyImage
}

func (e entry) bitmapPreview() bitmapPreview {
	if e.picture != nil {
		return bitmapPreview{source: e.picture.bitmap, history: e.picture}
	}
	return bitmapPreview{source: e.image}
}

func (image bitmapPreview) drawOver() func(*ui.Painter, ui.Rect) {
	if image.history != nil && !image.history.released {
		return image.history.afterAcceleratedPaint
	}
	return nil
}

func (image bitmapPreview) element(c *ui.Context) ui.Element {
	element := ui.Image(c, image.source)
	if drawOver := image.drawOver(); drawOver != nil {
		return element.DrawOver(drawOver)
	}
	return element
}

// Release on a later accelerated frame so the preceding frame can upload the
// pixels. mygo retains the encoded data for texture recovery.
func (p *historyImage) afterAcceleratedPaint(paint *ui.Painter, _ ui.Rect) {
	if p.released {
		return
	}
	if !paint.Accelerated() {
		// Software drawing still needs the pixels. Releasing them here would
		// cause mygo to decode the same image again.
		p.firstFrame = time.Time{}
		return
	}
	now := paint.Now()
	if p.firstFrame.IsZero() {
		p.firstFrame = now
		paint.After(time.Millisecond)
	} else if !p.firstFrame.Equal(now) {
		p.bitmap.ReleasePixels()
		p.released = true
	}
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
	if ctx.Err() != nil {
		return nil, "读取内容失败。"
	}
	var sizeHint int64
	if content, ok := r.(api.HistoryContent); ok {
		sizeHint = content.Size
	}
	r = previewContextReader{ctx: ctx, reader: r}
	const limit = 32 << 20
	var raw []byte
	var err error
	var cfg image.Config
	p := &historyImage{edge: edge}
	if edge > 0 {
		// Do not let a hostile dimension header grow its replay buffer past
		// the existing encoded-input budget. The extra overflow byte is only
		// enabled after DecodeConfig has finished retaining the header.
		limited := &io.LimitedReader{R: r, N: limit}
		// Keep codec-sized reads from becoming separate local-socket reads.
		// The shared limit stays beneath this buffer, including replay/drain.
		buffered := bufio.NewReaderSize(limited, 64<<10)
		var header bytes.Buffer
		cfg, _, err = image.DecodeConfig(io.TeeReader(buffered, &header))
		if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || int64(cfg.Width) > 16_000_000/int64(cfg.Height) {
			return nil, "图片无法预览。"
		}
		limited.N++
		decoded, err := imaging.Decode(io.MultiReader(bytes.NewReader(header.Bytes()), buffered), imaging.AutoOrientation(true))
		if err != nil {
			return nil, "图片无法预览。"
		}
		if _, err := io.Copy(io.Discard, buffered); err != nil {
			return nil, "读取内容失败。"
		}
		if limited.N <= 0 {
			return nil, "内容超过预览大小限制。"
		}
		header = bytes.Buffer{}
		if ctx.Err() != nil {
			return nil, "读取内容失败。"
		}
		p.width, p.height = decoded.Bounds().Dx(), decoded.Bounds().Dy()
		thumb := imaging.Fit(decoded, edge, edge, imaging.Lanczos)
		if ctx.Err() != nil {
			return nil, "读取内容失败。"
		}
		var encoded bytes.Buffer
		encoder := png.Encoder{CompressionLevel: png.BestSpeed}
		if err := encoder.Encode(&encoded, thumb); err != nil {
			return nil, "图片无法预览。"
		}
		raw = encoded.Bytes()
	} else {
		if sizeHint > 0 && sizeHint <= limit {
			// Reserve the known input once, with bounded socket read requests.
			// Keep the actual byte limit independent of this optional hint.
			var encoded bytes.Buffer
			encoded.Grow(int(sizeHint))
			// Expose only Write so CopyBuffer keeps its fixed read buffer instead
			// of delegating to Buffer.ReadFrom's whole-capacity read requests.
			_, err = io.CopyBuffer(struct{ io.Writer }{&encoded}, io.LimitReader(r, limit+1), make([]byte, 64<<10))
			raw = encoded.Bytes()
		} else {
			raw, err = io.ReadAll(io.LimitReader(r, limit+1))
		}
		if err != nil {
			return nil, "读取内容失败。"
		}
		if len(raw) > limit {
			return nil, "内容超过预览大小限制。"
		}
		cfg, _, err = image.DecodeConfig(bytes.NewReader(raw))
		if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || int64(cfg.Width) > 16_000_000/int64(cfg.Height) {
			return nil, "图片无法预览。"
		}
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

// Codec reads observe cancellation even when their source is already buffered.
// Full decode/resize calls remain bounded by the existing pixel limit.
type previewContextReader struct {
	ctx    context.Context
	reader io.Reader
}

func (r previewContextReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(p)
}
