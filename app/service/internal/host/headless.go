package host

import (
	"context"
	"errors"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func (n *Node) Submission(ctx context.Context, id string) (engine.SubmissionResult, error) {
	n.submissions.mu.Lock()
	r := n.submissions.entries[id]
	if r != nil {
		select {
		case <-r.done:
			out := r.out
			n.submissions.mu.Unlock()
			return out, nil
		default:
			n.submissions.mu.Unlock()
			return engine.SubmissionResult{RequestID: engine.RequestID(id), Phase: engine.SubmissionProcessing}, nil
		}
	}
	n.submissions.mu.Unlock()
	v, e := n.e.GetSubmission(ctx, engine.RequestID(id))
	return v, apiErr0(e)
}
func (n *Node) Operation(ctx context.Context, id string) (engine.Operation, error) {
	v, e := n.e.GetOperation(ctx, engine.OperationID(id))
	return v, apiErr0(e)
}
func (n *Node) Capabilities(ctx context.Context) (engine.Capabilities, error) {
	v, e := n.e.GetCapabilities(ctx)
	return v, apiErr0(e)
}
func (n *Node) Received(ctx context.Context) ([]engine.ReceivedContent, error) {
	v, e := n.e.ListReceived(ctx)
	if v == nil {
		v = []engine.ReceivedContent{}
	}
	return v, apiErr0(e)
}
func (n *Node) DismissReceived(ctx context.Context, id string) error {
	return apiErr0(n.e.DismissReceived(ctx, engine.ReceiptID(id)))
}
func (n *Node) ReceivedContent(ctx context.Context, id string, part int) (api.HistoryContent, error) {
	rs, e := n.e.ListReceived(ctx)
	if e != nil {
		return api.HistoryContent{}, apiErr(e, false)
	}
	for _, r := range rs {
		if string(r.ReceiptID) != id {
			continue
		}
		if part < 0 || part >= len(r.Content.Parts) {
			return api.HistoryContent{}, &api.Error{Status: 404, Reason: "not_found", Message: "no such part"}
		}
		h, e := n.e.RetainContent(ctx, r.Content.ID)
		if e != nil {
			return api.HistoryContent{}, apiErr(e, false)
		}
		release := func() { _ = n.e.ReleaseContent(context.Background(), h.LeaseID) }
		p := h.Content.Parts[part]
		reader, e := n.e.ReadContent(ctx, h.LeaseID, p.ID)
		if e != nil {
			release()
			return api.HistoryContent{}, apiErr(e, false)
		}
		return api.HistoryContent{MIME: p.MIME, Name: p.Name, Size: p.Size, ReadCloser: releasing{reader, release}}, nil
	}
	return api.HistoryContent{}, &api.Error{Status: 404, Reason: "not_found", Message: "receipt expired or absent"}
}
func (n *Node) ExecuteAction(ctx context.Context, id, key string, in api.ActionInput) (out api.ActionOutput, err error) {
	if in.RequestID == "" {
		in.RequestID = randomID()
	}
	if in.Apply {
		caps, e := n.e.GetCapabilities(ctx)
		if e != nil {
			return out, apiErr(e, false)
		}
		switch caps.Host.Apply {
		case engine.AbilityAvailable:
		case engine.AbilityPermissionRequired:
			return out, apiErr(adapterError(engine.CodePermissionDenied), false)
		case engine.AbilityUnsupported:
			return out, apiErr(adapterError(engine.CodeUnsupported), false)
		default:
			return out, apiErr(adapterError(engine.CodeUnavailable), false)
		}
	}
	r, e := n.e.RunAction(ctx, engine.RunActionRequest{RequestID: engine.RequestID(in.RequestID), Key: key, HistoryID: engine.HistoryID(id)})
	if e != nil {
		return out, apiErr(e, false)
	}
	out.Submission = r.SubmissionResult
	out.Item, e = n.GetHistory(ctx, string(r.HistoryID))
	if e != nil {
		return out, e
	}
	if in.Apply {
		e = n.clip.UseHistory(ctx, r.HistoryID)
		if e == nil {
			out.Applied = true
		} else {
			var ee *engine.Error
			if errors.As(e, &ee) {
				out.ApplyError = ee
			} else {
				out.ApplyError = adapterError(engine.CodeUnavailable)
			}
		}
	}
	return out, nil
}
