package engine

import "context"

// ProductEvent describes a completed product action using finite categories.
// It contains no identity, request ID, content, path or arbitrary property map.
// The host owns consent/configuration, installation identity and delivery.
type ProductEvent struct {
	Name           string
	Product        string
	Result         string
	Code           Code
	ContentKind    DiagnosticContentKind
	ActionCategory string
}

func (e *Engine) productEvent(ctx context.Context, event ProductEvent) {
	if e.opts.ProductEvents == nil {
		return
	}
	defer func() { _ = recover() }()
	e.opts.ProductEvents(ctx, event)
}
