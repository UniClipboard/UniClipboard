package engine

import (
	"bytes"
	"context"
	"image"
	_ "image/jpeg"
	"image/png"
	"io"

	_ "golang.org/x/image/webp"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/directory"
	"github.com/uniclipboard/unic/plugin/rpc"
)

const (
	maxWeTypeImageBytes  = rpc.MaxSyncBytes
	maxWeTypeImagePixels = 16 << 20
)

func weTypeBackend(backend syncBackend) *pluginBackend {
	p, ok := backend.(*pluginBackend)
	if !ok || !p.spec.ExternalChannels || p.spec.ID != "unic.wxime" {
		return nil
	}
	return p
}

// weTypeImageSet projects a single image file into the WeType send view.
// The original content owns the file metadata, history, and other send views.
func (e *Engine) weTypeImageSet(ctx context.Context, c *contentObj) (sendSet, *Error) {
	var file *partRec
	for _, part := range c.parts {
		if part.desc.MIME == directory.MIME || part.desc.MIME == directory.ReferenceMIME {
			return sendSet{}, nil
		}
		if part.desc.Kind == PartFile {
			if file != nil {
				return sendSet{}, nil
			}
			file = part
		}
	}
	if file == nil {
		return sendSet{}, nil
	}
	pngBytes := weTypePNG(c.fileImagePNG)
	if pngBytes == nil {
		if file.desc.Size <= 0 || file.desc.Size > maxWeTypeImageBytes {
			return sendSet{}, nil
		}
		r, err := e.openPart(ctx, file)
		if err != nil {
			return sendSet{}, err
		}
		defer r.Close()
		data, readErr := io.ReadAll(io.LimitReader(ctxReader{ctx, r}, file.desc.Size+1))
		if readErr != nil {
			return sendSet{}, sourceErr(ctx, readErr)
		}
		if int64(len(data)) != file.desc.Size {
			return sendSet{}, newErr(CodeSourceChanged, "the image file changed during reading")
		}
		pngBytes = weTypePNG(data)
		if pngBytes == nil {
			return sendSet{}, nil
		}
	}
	if err := ctx.Err(); err != nil {
		return sendSet{}, ctxErr(err)
	}
	imagePart := &partRec{data: pngBytes, desc: PartDescriptor{
		Kind: PartRepresentation, MIME: MimePNG, Size: int64(len(pngBytes)),
	}}
	snapshot := clip.Snapshot{Parts: []clip.Part{{Format: MimePNG, Data: pngBytes}}}
	return sendSet{c: c, parts: []*partRec{imagePart}, hash: snapshot.Hash(), total: imagePart.desc.Size}, nil
}

func weTypePNG(data []byte) []byte {
	if len(data) == 0 || len(data) > maxWeTypeImageBytes {
		return nil
	}
	cfg, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || cfg.Width > maxWeTypeImagePixels/cfg.Height {
		return nil
	}
	switch format {
	case "png", "jpeg", "webp":
	default:
		return nil
	}
	img, _, err := image.Decode(bytes.NewReader(data))
	if err != nil || img.Bounds().Dx() != cfg.Width || img.Bounds().Dy() != cfg.Height {
		return nil
	}
	if format == "png" {
		return data
	}
	var out weTypePNGBuffer
	if err := png.Encode(&out, img); err != nil {
		return nil
	}
	return out.Bytes()
}

type weTypePNGBuffer struct{ bytes.Buffer }

func (out *weTypePNGBuffer) Write(data []byte) (int, error) {
	if len(data) > maxWeTypeImageBytes-out.Len() {
		return 0, io.ErrShortWrite
	}
	return out.Buffer.Write(data)
}
