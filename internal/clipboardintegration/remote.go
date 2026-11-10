package clipboardintegration

import (
	"context"
	"errors"
	"io"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type Remote struct {
	Client  *api.Client
	Session string
}

func (r *Remote) call(ctx context.Context, m string, in, out any) error {
	request, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	return r.Client.AdapterCall(request, r.Session, m, in, out)
}
func (r *Remote) ImportContent(ctx context.Context, in engine.ContentInput) (out engine.ContentHandle, err error) {
	var s clip.Snapshot
	for _, p := range in.Representations {
		if p.Size > 64<<20 {
			return out, &engine.Error{Code: engine.CodeLimitExceeded, Message: "representation too large"}
		}
		var b []byte
		b, err = io.ReadAll(io.LimitReader(p.Reader, p.Size+1))
		if err != nil {
			return
		}
		if int64(len(b)) != p.Size {
			return out, &engine.Error{Code: engine.CodeSourceChanged, Message: "representation changed"}
		}
		s.Parts = append(s.Parts, clip.Part{Format: p.MIME, Data: b})
	}
	for _, f := range in.Files {
		ref, ok := f.Source.(interface{ PersistentRef() string })
		if !ok {
			return out, &engine.Error{Code: engine.CodeUnsupported, Message: "adapter files require persistent references"}
		}
		s.Files = append(s.Files, clip.File{Name: f.Name, Size: f.Size, Path: ref.PersistentRef()})
	}
	err = r.call(ctx, "import", s, &out)
	return
}
func (r *Remote) ReadContent(ctx context.Context, l engine.LeaseID, p engine.PartID) (io.ReadCloser, error) {
	return r.Client.AdapterContent(ctx, r.Session, l, p)
}

// RunRemote reconnects with a fresh baseline. Commands with uncertain results are never replayed.
func RunRemote(ctx context.Context, c *api.Client, b Board, emit func(api.Event)) error {
	registration := api.AdapterRegistration{LaunchID: randID()}
	for ctx.Err() == nil {
		var session string
		if err := c.AdapterCall(ctx, "", "register", registration, &session); err != nil {
			select {
			case <-ctx.Done():
				return nil
			case <-time.After(time.Second):
				continue
			}
		}
		run, stop := context.WithCancel(ctx)
		r := &Remote{c, session}
		caps := engine.HostCapabilities{ReadFormats: []string{engine.MimeText, engine.MimeHTML, engine.MimePNG}, WriteFormats: []string{engine.MimeText, engine.MimeHTML, engine.MimePNG}, ReadFiles: true, WriteFiles: true, Observe: engine.AbilityAvailable, Apply: engine.AbilityAvailable}
		if err := r.call(run, "capabilities", caps, nil); err != nil {
			stop()
			continue
		}
		activity := emit
		if activity == nil {
			activity = func(ev api.Event) {
				if ev.Kind == "sent" {
					_ = r.call(run, "activity", ev, nil)
				}
			}
		}
		cb := New(r, b, 400*time.Millisecond, activity)
		cb.Initialize(run)
		done := make(chan struct{})

		heart := make(chan struct{})
		go func() {
			defer close(heart)
			t := time.NewTicker(2 * time.Second)
			defer t.Stop()
			for {
				select {
				case <-run.Done():
					return
				case <-t.C:
					if r.call(run, "heartbeat", nil, nil) != nil {
						stop()
						return
					}
				}
			}
		}()
		// Establish the real baseline before accepting commands.
		var oldReceipts []engine.ReceiptID
		if cb.Ready() && r.call(run, "ready", nil, &oldReceipts) == nil {
			// Startup was acknowledged. Future reconnects are not new launches,
			// including reconnects to a replacement service generation.
			registration.LaunchID = ""
			cb.IgnoreReceipts(oldReceipts)
			go func() { defer close(done); cb.Run(run) }()
		} else {
			close(done)
			stop()
		}
		for run.Err() == nil {
			var cmd api.DesktopCommand
			if err := r.call(run, "next", nil, &cmd); err != nil {
				stop()
				break
			}
			if cmd.ID == "" {
				continue
			}
			result := api.DesktopResult{ID: cmd.ID}
			var err error
			switch cmd.Kind {
			case "read":
				result.Snapshot, err = cb.Read(run)
			case "copy":
				err = cb.Send(run, cmd.Snapshot)
			case "apply":
				err = cb.UseHistory(run, cmd.HistoryID)
			case "restore":
				result.Restore, err = cb.RestoreHistory(run, cmd.HistoryID, cmd.RequestID)
			default:
				err = &engine.Error{Code: engine.CodeUnsupported, Message: "unknown desktop command"}
			}
			if err != nil {
				var ee *engine.Error
				if errors.As(err, &ee) {
					result.Error = ee
				} else {
					result.Error = &engine.Error{Code: engine.CodeUnavailable, Message: "clipboard operation failed"}
				}
			}
			if r.call(run, "result", result, nil) != nil {
				stop()
				break
			}
		}
		stop()
		<-done
		<-heart
		closeCtx, cancel := context.WithTimeout(context.Background(), time.Second)
		_ = r.call(closeCtx, "close", nil, nil)
		cancel()
	}
	return nil
}

func (r *Remote) RetainContent(ctx context.Context, id engine.ContentID) (out engine.ContentHandle, err error) {
	err = r.call(ctx, "retain", id, &out)
	return
}

func (r *Remote) ReleaseContent(ctx context.Context, id engine.LeaseID) error {
	return r.call(ctx, "release", id, nil)
}

func (r *Remote) ObserveClipboard(ctx context.Context, v engine.Observation) (out engine.ObservationResult, err error) {
	err = r.call(ctx, "observe", v, &out)
	return
}

func (r *Remote) BeginApply(ctx context.Context, v engine.ApplyRequest) (out engine.ApplyTicket, err error) {
	err = r.call(ctx, "apply", v, &out)
	return
}

func (r *Remote) BeginApplyHistory(ctx context.Context, v engine.ApplyHistoryRequest) (out engine.ApplyTicket, err error) {
	err = r.call(ctx, "apply-history", v, &out)
	return
}

func (r *Remote) ReportApplyResult(ctx context.Context, v engine.ApplyResult) error {
	return r.call(ctx, "report", v, nil)
}

func (r *Remote) ListReceived(ctx context.Context) (out []engine.ReceivedContent, err error) {
	err = r.call(ctx, "received", nil, &out)
	return
}

func (r *Remote) DismissReceived(ctx context.Context, id engine.ReceiptID) error {
	return r.call(ctx, "dismiss", id, nil)
}

func (r *Remote) ReconcileClipboardFiles(ctx context.Context, v []string) error {
	return r.call(ctx, "reconcile", v, nil)
}

func (r *Remote) SubmitContent(ctx context.Context, v engine.SubmitRequest) (out engine.OperationID, err error) {
	err = r.call(ctx, "submit", v, &out)
	return
}

func (r *Remote) SyncRestoredHistory(ctx context.Context, token engine.ApplyToken, lease engine.LeaseID) (out engine.RestoreSyncResult, err error) {
	err = r.call(ctx, "restore-sync", struct {
		Token engine.ApplyToken
		Lease engine.LeaseID
	}{token, lease}, &out)
	return
}

func (r *Remote) LocalFilePath(ctx context.Context, lease engine.LeaseID, part engine.PartID) (out string, err error) {
	err = r.call(ctx, "path", struct {
		Lease engine.LeaseID
		Part  engine.PartID
	}{lease, part}, &out)
	return
}

func (r *Remote) FileReference(ctx context.Context, lease engine.LeaseID, part engine.PartID) (out string, err error) {
	err = r.call(ctx, "reference", struct {
		Lease engine.LeaseID
		Part  engine.PartID
	}{lease, part}, &out)
	return
}
