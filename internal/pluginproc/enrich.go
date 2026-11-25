package pluginproc

import (
	"bytes"
	"context"
	"errors"
	"io"
	"sync"
	"time"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

type enrichM struct{ p *Proxy }

func (m enrichM) Enrich(ctx context.Context, c plugin.EnrichContext) error {
	return m.p.enrich(ctx, c)
}

type enrichAssetStream struct {
	size uint64
	done chan struct{}
	buf  bytes.Buffer
	err  error
}

type enrichCollector struct {
	source        plugin.ActionSource
	ctx           context.Context
	mu            sync.Mutex
	readParts     map[int]bool
	liveParts     int
	partTransfers map[int]*actionPartTransfer
	assets        map[uint64]*enrichAssetStream
	totalAssets   uint64
}

func (p *Proxy) sendTextWithPurpose(s *session, id uint64, purpose, text string) (*rpc.StreamDesc, error) {
	if text == "" {
		return nil, nil
	}
	ss, d, err := s.conn.NewSend(s.conn.Context(), id, purpose, uint64(len(text)))
	if err != nil {
		return nil, err
	}
	go func() {
		if _, err := io.WriteString(ss, text); err == nil {
			_ = ss.Close()
		}
	}()
	return &d, nil
}

func (p *Proxy) enrich(ctx context.Context, ec plugin.EnrichContext) error {
	s := p.current()
	if s == nil {
		return ErrUnavailable
	}
	item := ec.Item()
	gen := p.gen.Add(1)
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	col := &enrichCollector{
		source:        ec,
		ctx:           ctx,
		readParts:     map[int]bool{},
		partTransfers: map[int]*actionPartTransfer{},
		assets:        map[uint64]*enrichAssetStream{},
	}
	timeout := time.Duration(rpc.DefaultEnrichTimeoutMs) * time.Millisecond
	if p.spec.Expected.Enricher != nil && p.spec.Expected.Enricher.TimeoutMS > 0 {
		timeout = time.Duration(p.spec.Expected.Enricher.TimeoutMS) * time.Millisecond
	}
	var res rpc.EnrichResult
	id, err := s.conn.Call(ctx, rpc.CallOptions{
		Method:  rpc.MethodEnrich,
		Timeout: p.callTimeout(ctx, timeout),
		Build: func(id uint64) (any, error) {
			s.enrichments.Store(id, col)
			d, err := p.sendTextWithPurpose(s, id, rpc.PurposeEnrichText, item.Text)
			if err != nil {
				return nil, err
			}
			in := rpc.Enrich{
				Trigger:    ec.Trigger(),
				Generation: rpc.U64(gen),
				Item:       wireItem(item, d),
			}
			for _, part := range ec.Parts() {
				if part.Size < 0 {
					return nil, rpc.NewError(rpc.CodeInvalidArgument)
				}
				in.Parts = append(in.Parts, rpc.ActionPart{
					MIME: part.MIME,
					File: part.File,
					Name: part.Name,
					Size: rpc.U64(part.Size),
				})
			}
			return in, nil
		},
	}, &res)
	defer s.enrichments.Delete(id)
	if err != nil {
		return err
	}
	defer s.conn.ReleaseRoot(id)

	if uint64(res.Generation) != gen {
		return rpc.NewError(rpc.CodeInvalidState)
	}
	if err := res.Validate(); err != nil {
		return err
	}

	out := plugin.EnrichmentResult{
		Status:  res.Status,
		AddTags: append([]string(nil), res.AddTags...),
	}
	for _, doc := range res.SearchDocs {
		out.SearchDocs = append(out.SearchDocs, plugin.EnrichmentSearchDoc{Kind: doc.Kind, Text: doc.Text})
	}
	referenced := map[uint64]bool{}
	for _, ann := range res.Annotations {
		pAnn := plugin.EnrichmentAnnotation{
			Type:        ann.Type,
			PayloadJSON: append([]byte(nil), ann.Payload...),
		}
		for _, ad := range ann.Assets {
			sid := uint64(ad.Stream.StreamID)
			if referenced[sid] {
				return rpc.NewError(rpc.CodeInvalidArgument)
			}
			referenced[sid] = true
			col.mu.Lock()
			st := col.assets[sid]
			col.mu.Unlock()
			if st == nil || st.size != uint64(ad.Stream.Size) {
				return rpc.NewError(rpc.CodeInvalidState)
			}
			select {
			case <-st.done:
			case <-ctx.Done():
				return ctx.Err()
			}
			if st.err != nil || uint64(st.buf.Len()) != st.size {
				return rpc.NewError(rpc.CodeInvalidState)
			}
			pAnn.Assets = append(pAnn.Assets, plugin.EnrichmentAsset{
				Key:  ad.Key,
				MIME: ad.MIME,
				Data: append([]byte(nil), st.buf.Bytes()...),
			})
		}
		out.Annotations = append(out.Annotations, pAnn)
	}
	return ec.SetResult(out)
}

func (p *Proxy) openEnrichAssetStream(s *session, c *rpc.Call, d rpc.StreamDesc) (any, error) {
	v, ok := s.enrichments.Load(c.Parent)
	if !ok {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	col := v.(*enrichCollector)
	sz := uint64(d.Size)
	if sz == 0 || sz > rpc.MaxEnrichAssetBytes {
		return nil, rpc.NewError(rpc.CodeLimitExceeded)
	}
	col.mu.Lock()
	if col.ctx.Err() != nil {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodePermission)
	}
	if len(col.assets) >= rpc.MaxEnrichAnnotations*rpc.MaxEnrichAssetsPerAnnotation || col.totalAssets+sz > rpc.MaxEnrichTotalAssetBytes {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeLimitExceeded)
	}
	if _, exists := col.assets[uint64(d.StreamID)]; exists {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeConflict)
	}
	st := &enrichAssetStream{size: sz, done: make(chan struct{})}
	col.assets[uint64(d.StreamID)] = st
	col.totalAssets += sz
	col.mu.Unlock()

	rs, err := c.Conn.Expect(c.Conn.Context(), c.Parent, d, sz)
	if err != nil {
		return nil, err
	}
	go func() {
		_, err := io.Copy(&st.buf, rs)
		st.err = err
		close(st.done)
	}()
	return nil, nil
}

func (p *Proxy) closeEnrichPart(s *session, ctx context.Context, c *rpc.Call) (any, error) {
	var in rpc.OpenActionPart
	if rpc.Decode(c.Params, &in) != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	value, ok := s.enrichments.Load(c.Parent)
	if !ok {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	col := value.(*enrichCollector)
	col.mu.Lock()
	part := col.partTransfers[in.Index]
	col.mu.Unlock()
	if part == nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	part.cancel()
	select {
	case <-part.done:
		return rpc.Empty{}, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (p *Proxy) openEnrichPart(s *session, _ context.Context, c *rpc.Call) (any, error) {
	var in rpc.OpenActionPart
	if rpc.Decode(c.Params, &in) != nil {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	value, ok := s.enrichments.Load(c.Parent)
	if !ok {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	col := value.(*enrichCollector)
	col.mu.Lock()
	if col.source == nil || col.ctx.Err() != nil {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodePermission)
	}
	parts := col.source.Parts()
	if in.Index < 0 || in.Index >= len(parts) || col.readParts[in.Index] {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	if col.liveParts >= 4 {
		col.mu.Unlock()
		return nil, rpc.NewError(rpc.CodeExhausted)
	}
	partCtx, cancel := context.WithCancel(col.ctx)
	part := &actionPartTransfer{cancel: cancel, done: make(chan struct{})}
	col.readParts[in.Index] = true
	col.partTransfers[in.Index] = part
	col.liveParts++
	col.mu.Unlock()
	release := func() {
		part.once.Do(func() { cancel(); col.mu.Lock(); col.liveParts--; col.mu.Unlock(); close(part.done) })
	}
	rc, err := col.source.OpenPart(partCtx, in.Index)
	if err != nil {
		release()
		return nil, rpc.NewError(rpc.CodeUnavailable)
	}
	stop := context.AfterFunc(partCtx, func() { _ = rc.Close() })
	finish := func() { stop(); _ = rc.Close(); release() }
	size := parts[in.Index].Size
	if size == 0 {
		finish()
		return rpc.StreamDesc{Purpose: rpc.PurposeEnrichPart}, nil
	}
	stream, desc, err := c.Conn.NewSend(partCtx, c.Parent, rpc.PurposeEnrichPart, uint64(size))
	if err != nil {
		finish()
		return nil, err
	}
	go func() {
		defer finish()
		n, err := io.Copy(stream, rc)
		if partCtx.Err() != nil || col.ctx.Err() != nil || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			_ = stream.Close()
			return
		}
		if err == nil && n == size {
			_ = stream.Close()
			return
		}
		_ = c.Conn.Close()
	}()
	return desc, nil
}
