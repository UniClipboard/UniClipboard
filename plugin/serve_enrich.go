package plugin

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"slices"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin/rpc"
)

func (s *server) enrich(ctx context.Context, c *rpc.Call) (any, error) {
	enr, ok := s.p.(Enricher)
	if !ok {
		return nil, rpc.NewError(rpc.CodeUnsupported)
	}
	var in rpc.Enrich
	if err := rpc.Decode(c.Params, &in); err != nil || in.Validate() != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if err := s.started(); err != nil {
		return nil, err
	}
	if s.spec.Enricher == nil || !slices.Contains(s.spec.Enricher.Triggers, in.Trigger) {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	text, err := s.readText(ctx, c, in.Item.Text)
	if err != nil {
		return nil, err
	}
	enrichLife, cancelEnrich := context.WithCancel(ctx)
	defer cancelEnrich()
	ec := &enrichCtx{
		trigger: in.Trigger,
		item:    publicItem(in.Item, text),
		parts:   in.Parts,
		call:    c,
		ctx:     enrichLife,
	}
	if err := safely(func() error { return enr.Enrich(ctx, ec) }); err != nil {
		return nil, err
	}
	if !ec.set {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	out := rpc.EnrichResult{
		Generation: in.Generation,
		Status:     ec.res.Status,
		AddTags:    slices.Clone(ec.res.AddTags),
	}
	for _, doc := range ec.res.SearchDocs {
		out.SearchDocs = append(out.SearchDocs, rpc.EnrichSearchDoc{Kind: doc.Kind, Text: doc.Text})
	}
	for _, ann := range ec.res.Annotations {
		wireAnn := rpc.EnrichAnnotation{
			Type:    ann.Type,
			Payload: append(json.RawMessage(nil), ann.PayloadJSON...),
		}
		for _, asset := range ann.Assets {
			ss, desc, err := c.Conn.NewSend(ctx, c.ID, rpc.PurposeEnrichAsset, uint64(len(asset.Data)))
			if err != nil {
				return nil, err
			}
			if _, err := c.Conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodStream, Parent: c.ID, Timeout: 5 * time.Second, Params: desc}, nil); err != nil {
				return nil, err
			}
			if _, err := ss.Write(asset.Data); err != nil {
				return nil, err
			}
			if err := ss.Close(); err != nil {
				return nil, err
			}
			wireAnn.Assets = append(wireAnn.Assets, rpc.EnrichAssetDesc{
				Key:    asset.Key,
				MIME:   asset.MIME,
				Stream: desc,
			})
		}
		out.Annotations = append(out.Annotations, wireAnn)
	}
	if err := out.Validate(); err != nil {
		return nil, err
	}
	return out, nil
}

type enrichCtx struct {
	trigger string
	item    Item
	parts   []rpc.ActionPart
	call    *rpc.Call
	ctx     context.Context
	res     EnrichmentResult
	set     bool
}

func (e *enrichCtx) Trigger() string { return e.trigger }
func (e *enrichCtx) Item() Item      { return e.item }

func (e *enrichCtx) Parts() []ActionPart {
	out := make([]ActionPart, len(e.parts))
	for i, p := range e.parts {
		out[i] = ActionPart{MIME: p.MIME, File: p.File, Name: p.Name, Size: int64(p.Size)}
	}
	return out
}

func (e *enrichCtx) OpenPart(ctx context.Context, index int) (io.ReadCloser, error) {
	if index < 0 || index >= len(e.parts) || e.ctx.Err() != nil {
		return nil, ErrInvalidArgument
	}
	merged, cancel := context.WithCancel(e.ctx)
	stop := context.AfterFunc(ctx, cancel)
	var desc rpc.StreamDesc
	_, err := e.call.Conn.Call(merged, rpc.CallOptions{
		Method:  rpc.MethodOpenEnrichPart,
		Parent:  e.call.ID,
		Timeout: 30 * time.Second,
		Params:  rpc.OpenActionPart{Index: index},
	}, &desc)
	if err != nil {
		stop()
		cancel()
		return nil, err
	}
	if desc.Purpose != rpc.PurposeEnrichPart || uint64(desc.Size) != uint64(e.parts[index].Size) {
		stop()
		cancel()
		return nil, ErrInvalidArgument
	}
	var reader io.ReadCloser
	if desc.Size == 0 && desc.StreamID == 0 {
		reader = io.NopCloser(bytes.NewReader(nil))
	} else {
		reader, err = e.call.Conn.Expect(merged, e.call.ID, desc, uint64(e.parts[index].Size))
	}
	if err != nil {
		stop()
		cancel()
		return nil, err
	}
	out := &actionSourceReader{ReadCloser: reader, done: func() { stop(); cancel() }}
	out.complete.Store(e.parts[index].Size == 0)
	out.release = func() error {
		if e.ctx.Err() != nil {
			return nil
		}
		closeCtx, stopClose := context.WithTimeout(e.ctx, 5*time.Second)
		defer stopClose()
		_, err := e.call.Conn.Call(closeCtx, rpc.CallOptions{
			Method:  rpc.MethodCloseEnrichPart,
			Parent:  e.call.ID,
			Timeout: 5 * time.Second,
			Params:  rpc.OpenActionPart{Index: index},
		}, nil)
		return err
	}
	return out, nil
}

func (e *enrichCtx) SetResult(res EnrichmentResult) error {
	if e.set {
		return ErrInvalidArgument
	}
	switch res.Status {
	case EnrichStatusSkipped:
		if len(res.SearchDocs) != 0 || len(res.AddTags) != 0 || len(res.Annotations) != 0 {
			return ErrInvalidArgument
		}
	case EnrichStatusOK:
		if len(res.SearchDocs) > rpc.MaxEnrichSearchDocs {
			return ErrInvalidArgument
		}
		totalSearch := 0
		for _, d := range res.SearchDocs {
			if !rpc.ValidEnrichDocKind(d.Kind) || d.Text == "" || !utf8.ValidString(d.Text) || len(d.Text) > rpc.MaxEnrichSearchDocBytes {
				return ErrInvalidArgument
			}
			totalSearch += len(d.Text)
			if totalSearch > rpc.MaxEnrichSearchTotalBytes {
				return ErrInvalidArgument
			}
		}
		if len(res.AddTags) > rpc.MaxTags {
			return ErrInvalidArgument
		}
		for _, t := range res.AddTags {
			if !validTag(t) {
				return ErrInvalidArgument
			}
		}
		if len(res.Annotations) > rpc.MaxEnrichAnnotations {
			return ErrInvalidArgument
		}
		totalAssets := 0
		for _, a := range res.Annotations {
			if !rpc.ValidAnnotationType(a.Type) || len(a.PayloadJSON) == 0 || len(a.PayloadJSON) > rpc.MaxEnrichAnnotationPayloadBytes || !json.Valid(a.PayloadJSON) {
				return ErrInvalidArgument
			}
			if len(a.Assets) > rpc.MaxEnrichAssetsPerAnnotation {
				return ErrInvalidArgument
			}
			for _, asset := range a.Assets {
				if !rpc.ValidEnrichAssetKey(asset.Key) || !rpc.ValidEnrichAssetMIME(asset.MIME) || len(asset.Data) == 0 || len(asset.Data) > rpc.MaxEnrichAssetBytes {
					return ErrInvalidArgument
				}
				totalAssets += len(asset.Data)
				if totalAssets > rpc.MaxEnrichTotalAssetBytes {
					return ErrInvalidArgument
				}
			}
		}
	default:
		return ErrInvalidArgument
	}
	e.res, e.set = res, true
	return nil
}
