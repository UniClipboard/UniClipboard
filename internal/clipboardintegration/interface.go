// Package clipboardintegration implements the host observation/apply state machine.
// It contains no graphical toolkit. Production desktop clients use an IPC Engine.
package clipboardintegration

import (
	"context"
	"crypto/rand"
	"io"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type Board interface {
	Read() (clip.Snapshot, error)
	Write(clip.Snapshot) error
}
type Engine interface {
	ImportContent(context.Context, engine.ContentInput) (engine.ContentHandle, error)
	RetainContent(context.Context, engine.ContentID) (engine.ContentHandle, error)
	ReleaseContent(context.Context, engine.LeaseID) error
	ReadContent(context.Context, engine.LeaseID, engine.PartID) (io.ReadCloser, error)
	LocalFilePath(context.Context, engine.LeaseID, engine.PartID) (string, error)
	FileReference(context.Context, engine.LeaseID, engine.PartID) (string, error)
	ObserveClipboard(context.Context, engine.Observation) (engine.ObservationResult, error)
	BeginApply(context.Context, engine.ApplyRequest) (engine.ApplyTicket, error)
	BeginApplyHistory(context.Context, engine.ApplyHistoryRequest) (engine.ApplyTicket, error)
	ReportApplyResult(context.Context, engine.ApplyResult) error
	ListReceived(context.Context) ([]engine.ReceivedContent, error)
	DismissReceived(context.Context, engine.ReceiptID) error
	ReconcileClipboardFiles(context.Context, []string) error
	SubmitContent(context.Context, engine.SubmitRequest) (engine.OperationID, error)
	SyncRestoredHistory(context.Context, engine.ApplyToken, engine.LeaseID) (engine.RestoreSyncResult, error)
}
type Local struct{ *engine.Engine }

func (l Local) ReadContent(ctx context.Context, a engine.LeaseID, b engine.PartID) (io.ReadCloser, error) {
	return l.Engine.ReadContent(ctx, a, b)
}
func New(e Engine, b Board, poll time.Duration, emit func(api.Event)) *Clipboard {
	if poll == 0 {
		poll = 400 * time.Millisecond
	}
	if emit == nil {
		emit = func(api.Event) {}
	}
	return &Clipboard{e: e, board: b, poll: poll, emit: emit, logf: func(string, ...any) {}, name: func(context.Context, engine.DeviceID) string { return "" }, channelName: func(context.Context, string) string { return "" }}
}
func (c *Clipboard) Read(ctx context.Context) (clip.Snapshot, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.board.Read()
}
func randID() string { return rand.Text()[:16] }
func readPart(ctx context.Context, e Engine, l engine.LeaseID, p engine.PartID) ([]byte, error) {
	r, err := e.ReadContent(ctx, l, p)
	if err != nil {
		return nil, err
	}
	defer func() { _ = r.Close() }()
	return io.ReadAll(r)
}

func asCode(err error) string { return "unavailable" }
