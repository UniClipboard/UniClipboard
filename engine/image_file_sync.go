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
	maxImageFileBytes  = rpc.MaxSyncBytes
	maxImageFilePixels = 16 << 20
)

// WantsClipboardFileImage reports whether an automatic clipboard copy can use
// an optional PNG alternative for a file. The host calls it before reading
// the additional image data.
func (e *Engine) WantsClipboardFileImage(ctx context.Context) (bool, error) {
	release, err := e.enter(ctx)
	if err != nil {
		return false, err
	}
	defer release()
	return e.wantsClipboardFileImage(), nil
}

func (e *Engine) wantsClipboardFileImage() bool {
	e.mu.Lock()
	if e.life != LifecycleRunning || e.runtime != RuntimeActive || e.stopReq ||
		!e.settings.Value.Sync.Enabled || !e.settings.Value.Sync.Automatic {
		e.mu.Unlock()
		return false
	}
	var eligible []*pluginBackend
	for _, b := range e.syncBackends {
		if p, ok := b.(*pluginBackend); ok && p.imageFilesAsImages &&
			!p.preparing.Load() && e.policyLocked(p.id).Images {
			eligible = append(eligible, p)
		}
	}
	e.mu.Unlock()
	for _, p := range eligible {
		if !p.preparing.Load() && len(p.enabledChannels()) > 0 {
			return true
		}
	}
	return false
}

// imageFileSendSet projects a single image file into a backend send view.
func (e *Engine) imageFileSendSet(ctx context.Context, c *contentObj) (sendSet, *Error) {
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
	pngBytes := normalizeImageFilePNG(c.fileImagePNG)
	if pngBytes == nil {
		if file.desc.Size <= 0 || file.desc.Size > maxImageFileBytes {
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
		pngBytes = normalizeImageFilePNG(data)
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

func normalizeImageFilePNG(data []byte) []byte {
	if len(data) == 0 || len(data) > maxImageFileBytes {
		return nil
	}
	cfg, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || cfg.Width > maxImageFilePixels/cfg.Height {
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
	var out imageFilePNGBuffer
	if err := png.Encode(&out, img); err != nil {
		return nil
	}
	return out.Bytes()
}

type imageFilePNGBuffer struct{ bytes.Buffer }

func (out *imageFilePNGBuffer) Write(data []byte) (int, error) {
	if len(data) > maxImageFileBytes-out.Len() {
		return 0, io.ErrShortWrite
	}
	return out.Buffer.Write(data)
}
