package engine

import (
	"io"
	"sort"
	"time"
)

// Live send progress. Bytes are counted where the backend reads a part
// from the engine, so every backend gets it without a protocol change. The
// progress stays in memory and is merged into history items while the send runs.

const progressInterval = 250 * time.Millisecond

// progressSink is implemented by hosts that show live part progress. Hosts
// without it (test fakes) simply get none.
type progressSink interface {
	InitProgress(k targetKey, parts []*partRec)
	ReportPart(k targetKey, index int, state string, done int64)
}

func (h *dispatchHost) InitProgress(k targetKey, parts []*partRec) {
	if h.op != nil {
		h.e.initProgress(h.op.op.ID, k, parts)
	}
}

func (h *dispatchHost) ReportPart(k targetKey, index int, state string, done int64) {
	if h.op != nil {
		h.e.reportPart(h.op.op.ID, k, index, state, done)
	}
}

// initProgress lists the parts of a target as waiting.
func (e *Engine) initProgress(id OperationID, k targetKey, parts []*partRec) {
	e.mu.Lock()
	defer e.mu.Unlock()
	o := e.ops.byID[id]
	if o == nil {
		return
	}
	if o.progress == nil {
		o.progress = map[targetKey]*targetProgress{}
	}
	tp := &targetProgress{items: make([]HistoryTargetItem, len(parts))}
	for i, p := range parts {
		tp.items[i] = HistoryTargetItem{Index: i, Kind: p.desc.Kind, Name: p.desc.Name, State: "waiting", TotalBytes: p.desc.Size}
	}
	o.progress[k] = tp
}

// reportPart records the state of one part. Byte-only changes are published at
// most every progressInterval; a state change is published at once.
func (e *Engine) reportPart(id OperationID, k targetKey, index int, state string, done int64) {
	e.mu.Lock()
	o := e.ops.byID[id]
	var tp *targetProgress
	if o != nil {
		tp = o.progress[k]
	}
	if tp == nil || index < 0 || index >= len(tp.items) {
		e.mu.Unlock()
		return
	}
	it := &tp.items[index]
	changed := it.State != state
	it.State, it.CompletedBytes = state, done
	now := time.Now()
	publish := o.history != "" && (changed || now.Sub(tp.last) >= progressInterval)
	if publish {
		tp.last = now
	}
	hid := o.history
	e.mu.Unlock()
	if publish {
		e.historyChanged(string(hid))
	}
}

// countingPart counts the bytes a backend reads from one part.
type countingPart struct {
	io.ReadCloser
	report func(state string, done int64)
	done   int64
	last   time.Time
}

func (c *countingPart) Read(p []byte) (int, error) {
	n, err := c.ReadCloser.Read(p)
	c.done += int64(n)
	switch {
	case err == io.EOF:
		c.report("completed", c.done)
	case n > 0 && time.Since(c.last) >= progressInterval:
		c.last = time.Now()
		c.report("transferring", c.done)
	}
	return n, err
}

// overlayProgress merges the live state of running sends into a history item.
func (e *Engine) overlayProgress(out *HistoryItem) {
	e.mu.Lock()
	defer e.mu.Unlock()
	for _, o := range e.ops.byID {
		if o.history != out.ID || o.op.Done() {
			continue
		}
		for _, t := range o.op.Targets {
			k := targetKey{t.BackendID, t.TargetKind, t.TargetID}
			idx := -1
			for i, x := range out.Targets {
				if x.BackendID == k.backend && x.TargetKind == k.kind && x.TargetID == k.id {
					idx = i
				}
			}
			if idx < 0 {
				out.Targets = append(out.Targets, HistoryTargetState{BackendID: t.BackendID, TargetKind: t.TargetKind, TargetID: t.TargetID})
				idx = len(out.Targets) - 1
			}
			row := &out.Targets[idx]
			row.State, row.ErrorCode = t.State, ""
			if t.Error != nil {
				row.ErrorCode = t.Error.Code
			}
			row.Items = nil
			if tp := o.progress[k]; tp != nil {
				row.Items = append([]HistoryTargetItem(nil), tp.items...)
				if t.State == TargetFailed {
					for i := range row.Items {
						if row.Items[i].State == "transferring" {
							row.Items[i].State = "failed"
						}
					}
				}
			}
		}
	}
	sort.SliceStable(out.Targets, func(i, j int) bool {
		a, b := out.Targets[i], out.Targets[j]
		if a.BackendID != b.BackendID {
			return a.BackendID < b.BackendID
		}
		if a.TargetKind != b.TargetKind {
			return a.TargetKind < b.TargetKind
		}
		return a.TargetID < b.TargetID
	})
}
