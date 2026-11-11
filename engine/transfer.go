package engine

import (
	"context"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/wire"
)

// TransferBackendID names the origin of content that arrived over a temporary
// transfer (0022): a peer without a unic device identity, such as the browser
// page. The content is not tied to a device or a stored channel.
const TransferBackendID PluginID = "unic.transfer"

// transferChannel is the pseudo channel under which such content is listed.
const transferChannel = "transfer"

// TransferPart announces one part of an incoming transfer before any byte of it
// is read. Representations have a MIME type; files have a name.
type TransferPart struct {
	File bool
	MIME string
	Name string
	Size int64
}

// TransferOffer is what a temporary-transfer peer offers.
type TransferOffer struct {
	// TransferID and View identify the offer for duplicate detection.
	TransferID, View string
	Parts            []TransferPart
}

// TransferRepresentation is one fully received format.
type TransferRepresentation struct {
	MIME string
	Data []byte
}

// TransferFile is a received file written under TransferInbox.Dir.
type TransferFile struct {
	Name string
	Path string
	Size int64
}

// TransferContent is everything received, in offer order.
type TransferContent struct {
	Representations []TransferRepresentation
	Files           []TransferFile
}

// TransferInbox is an admitted transfer. The caller writes received files
// below Dir and then calls exactly one of Complete or Abort.
type TransferInbox struct {
	// Dir is private to this transfer and owned by the engine afterwards.
	Dir  string
	done func(clip.Snapshot, string, bool)
}

// AdmitTransfer decides, before any byte is read, whether the engine takes the
// offered content: it must be running, sync on, the content type allowed by the
// transfer backend's policy, the offer new, and storage available. A refusal is
// returned as an error; nothing stays reserved. An accepted offer reserves
// storage until Complete or Abort.
func (e *Engine) AdmitTransfer(ctx context.Context, offer TransferOffer) (*TransferInbox, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	if offer.TransferID == "" || offer.View == "" || len(offer.Parts) == 0 || len(offer.Parts) > wire.MaxParts {
		return nil, newErr(CodeInvalidArgument, "invalid transfer offer")
	}
	var total int64
	parts := make([]receivePart, len(offer.Parts))
	for i, p := range offer.Parts {
		if p.Size < 0 || (!p.File && p.Size == 0) || (p.File && (p.Name == "" || len(p.Name) > 255)) {
			return nil, newErr(CodeInvalidArgument, "invalid transfer offer")
		}
		if (p.File && p.Size > wire.MaxFileBytes) || (!p.File && p.Size > wire.MaxPartBytes) {
			return nil, newErr(CodeLimitExceeded, "transfer part is too large")
		}
		total += p.Size
		parts[i] = receivePart{file: p.File, size: p.Size, mime: strings.ToLower(strings.TrimSpace(p.MIME))}
	}
	if total > wire.MaxTotal {
		return nil, newErr(CodeLimitExceeded, "transfer is too large")
	}
	adm, ok := e.admitReceive(receiveRequest{
		backend: TransferBackendID, channel: transferChannel, ephemeral: true,
		transfer: offer.TransferID, view: offer.View, total: total, parts: parts,
	})
	if !ok {
		return nil, newErr(CodePermissionDenied, "the engine did not accept this transfer")
	}
	dir := filepath.Join(adm.Inbox, randHex(16))
	if err := os.MkdirAll(dir, 0o700); err != nil {
		adm.Done(clip.Snapshot{}, "", false)
		return nil, newErr(CodeUnavailable, "cannot prepare the transfer inbox")
	}
	return &TransferInbox{Dir: dir, done: adm.Done}, nil
}

// Complete hands the received content to the pipeline: processors, history and
// the received list, exactly as for content from a device.
func (in *TransferInbox) Complete(c TransferContent) {
	var snap clip.Snapshot
	for _, r := range c.Representations {
		snap.Parts = append(snap.Parts, clip.Part{Format: r.MIME, Data: r.Data})
	}
	for _, f := range c.Files {
		snap.Files = append(snap.Files, clip.File{Name: f.Name, Path: f.Path, Size: f.Size})
	}
	in.done(snap, in.Dir, true)
}

// Abort drops the transfer and releases what admission reserved.
func (in *TransferInbox) Abort() {
	_ = os.RemoveAll(in.Dir)
	in.done(clip.Snapshot{}, "", false)
}
