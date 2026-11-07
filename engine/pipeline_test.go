package engine

import (
	"context"
	"errors"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func pipelineInput(t *testing.T, e *Engine, text string) *contentObj {
	t.Helper()
	h := importText(t, e, text)
	c, err := e.contentOf(h.LeaseID)
	if err != nil {
		t.Fatal(err)
	}
	e.cs.mu.Lock()
	c.refs++
	e.cs.mu.Unlock()
	if err := e.ReleaseContent(bg, h.LeaseID); err != nil {
		t.Fatal(err)
	}
	return c
}

func TestPipelineAtomicPatchesAndOwnership(t *testing.T) {
	n := newContentNode(t, nil)
	var ended *processingContext
	n.e.processors = []*pipelineProcessor{
		newPipelineProcessor("example.first", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error {
			ended = c
			if err := c.AddTag("keep"); err != nil {
				return err
			}
			return c.SetText("changed")
		}),
		newPipelineProcessor("example.broken", []pipelineEvent{pipelineCaptured}, true, func(_ context.Context, c *processingContext) error {
			if err := c.SetText("must not survive"); err != nil {
				return err
			}
			if err := c.RemoveTag("keep"); err != nil {
				return err
			}
			return errors.New("private body and path must not escape")
		}),
		newPipelineProcessor("example.last", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error {
			if c.Item().Text != "changed" || len(c.Policy().Tags) != 1 {
				t.Error("failed patch was not rolled back")
			}
			snapshot := c.Policy()
			snapshot.Tags[0] = "outside"
			if err := c.SetSync(true); err != nil {
				return err
			}
			return c.SetExpire(time.Minute)
		}),
	}
	out, err := n.e.processContent(bg, pipelineCaptured, pipelineInput(t, n.e, "original"), pipelineOrigin{kind: "explicit"})
	if err != nil {
		t.Fatal(err)
	}
	if out.item.Text != "changed" || out.policy.Sync || !out.policy.Persist || out.policy.Tags[0] != "keep" || out.policy.ExpireAfter != time.Minute {
		t.Fatalf("unexpected prepared item/policy: %+v %+v", out.item, out.policy)
	}
	if out.item.Origin.pluginID != "example.first" || out.item.AutomationDepth != 1 {
		t.Fatalf("wrong provenance: %+v", out.item)
	}
	if code(ended.SetText("late")) != CodeResourceExpired {
		t.Fatal("ended context accepted mutation")
	}
	if stored(n.e) != int64(len("changed")) {
		t.Fatalf("temporary content leaked: %d", stored(n.e))
	}
	n.e.cs.unref(out.c)
	if stored(n.e) != 0 {
		t.Fatal("prepared reference leaked")
	}
}

func TestPipelineIgnoredMethodFailureRollsBackPatch(t *testing.T) {
	n := newContentNode(t, nil)
	n.e.processors = []*pipelineProcessor{
		newPipelineProcessor("example.invalid", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error {
			if err := c.SetPersist(false); err != nil {
				return err
			}
			_ = c.SetExpire(time.Nanosecond)
			return nil
		}),
	}
	out, err := n.e.processContent(bg, pipelineCaptured, pipelineInput(t, n.e, "original"), pipelineOrigin{kind: "explicit"})
	if err != nil {
		t.Fatal(err)
	}
	defer n.e.cs.unref(out.c)
	if !out.policy.Persist || out.policy.ExpireAfter != 0 {
		t.Fatal("ignored method error committed a partial patch")
	}
}

func TestPipelineTextClearsHTMLAndRestoresOrigin(t *testing.T) {
	n := newContentNode(t, nil)
	n.e.processors = []*pipelineProcessor{
		newPipelineProcessor("example.change", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error { return c.SetText("new") }),
		newPipelineProcessor("example.restore", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error { return c.SetText("old") }),
	}
	out, err := n.e.processContent(bg, pipelineCaptured, pipelineInput(t, n.e, "old"), pipelineOrigin{kind: "explicit"})
	if err != nil {
		t.Fatal(err)
	}
	if out.item.Origin.kind != "explicit" || out.item.AutomationDepth != 0 {
		t.Fatal("restored content changed origin")
	}
	n.e.cs.unref(out.c)
	h, importErr := n.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, "old"), rep(MimeHTML, "<b>old</b>")}})
	if importErr != nil {
		t.Fatal(importErr)
	}
	c, _ := n.e.contentOf(h.LeaseID)
	n.e.cs.mu.Lock()
	c.refs++
	n.e.cs.mu.Unlock()
	if err := n.e.ReleaseContent(bg, h.LeaseID); err != nil {
		t.Fatal(err)
	}
	out, err = n.e.processContent(bg, pipelineCaptured, c, pipelineOrigin{kind: "explicit"})
	if err != nil {
		t.Fatal(err)
	}
	defer n.e.cs.unref(out.c)
	if len(out.c.parts) != 1 || out.item.Origin.pluginID != "example.restore" || out.item.AutomationDepth != 1 {
		t.Fatal("stale HTML survived or representation change was lost")
	}
}

func TestPipelineDepthLimitSkipsProcessors(t *testing.T) {
	n := newContentNode(t, nil)
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("example.never", []pipelineEvent{pipelineCaptured}, false, func(context.Context, *processingContext) error {
		t.Error("fourth automation round ran")
		return nil
	})}
	out, err := n.e.processContent(bg, pipelineCaptured, pipelineInput(t, n.e, "same"), pipelineOrigin{kind: "plugin", depth: 3})
	if err != nil {
		t.Fatal(err)
	}
	defer n.e.cs.unref(out.c)
	if out.item.AutomationDepth != 3 {
		t.Fatal("depth changed while skipping")
	}
}

func TestPipelineIngressesProcessBeforeDispatch(t *testing.T) {
	a, b := newContentNode(t, withHost), newContentNode(t, nil)
	var captured, receivedCalls atomic.Int32
	a.e.processors = []*pipelineProcessor{newPipelineProcessor("example.capture", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error {
		captured.Add(1)
		return c.SetText(strings.ToUpper(c.Item().Text))
	})}
	b.e.processors = []*pipelineProcessor{newPipelineProcessor("example.receive", []pipelineEvent{pipelineReceived}, false, func(_ context.Context, c *processingContext) error {
		receivedCalls.Add(1)
		if c.Item().Origin.kind != "remote_device" || c.Item().Origin.deviceID != a.e.self {
			t.Error("unauthenticated receive origin")
		}
		return c.SetText(c.Item().Text + "!")
	})}
	start(t, a)
	start(t, b)
	pairTwo(t, a, b)
	h := importText(t, a.e, "explicit")
	id, err := submit(a.e, h)
	if err != nil {
		t.Fatal(err)
	}
	waitSend(t, a.e, id)
	r := waitReceived(t, b.e, 1)
	if textOf(t, b.e, r[0]) != "EXPLICIT!" {
		t.Fatal("explicit ingress bypassed processing")
	}
	host := newHost(t, a.e)
	host.must(ObserveBaseline, "baseline")
	host.must(ObserveChange, "observed")
	r = waitReceived(t, b.e, 2)
	if textOf(t, b.e, r[0]) != "OBSERVED!" {
		t.Fatal("observation ingress bypassed processing")
	}
	if captured.Load() != 2 || receivedCalls.Load() != 2 || len(received(a.e)) != 0 {
		t.Fatal("duplicate processing or remote forwarding")
	}
	if err := a.e.ReleaseContent(bg, h.LeaseID); err != nil {
		t.Fatal(err)
	}
}

func TestPipelineProcessesLocalChangesWithSyncDisabled(t *testing.T) {
	n := newContentNode(t, withHost)
	var calls atomic.Int32
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("example.local", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error {
		calls.Add(1)
		return c.AddTag("local")
	})}
	start(t, n)
	updateSettings(t, n.e, func(s *Settings) { s.Sync.Enabled = false })
	h := newHost(t, n.e)
	h.must(ObserveBaseline, "baseline")
	if h.must(ObserveChange, "local") != DispDisabled || calls.Load() != 1 {
		t.Fatal("sync disabled skipped local processing")
	}
	h.must(ObserveChange, "local")
	if calls.Load() != 1 {
		t.Fatal("unchanged observation reran processors")
	}
	if stored(n.e) != 0 {
		t.Fatal("local-only processing retained content")
	}
}

func TestPipelineCoalescesConcurrentSubmissions(t *testing.T) {
	n := newContentNode(t, nil)
	entered, proceed := make(chan struct{}), make(chan struct{})
	var calls atomic.Int32
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("example.once", []pipelineEvent{pipelineCaptured}, false, func(ctx context.Context, c *processingContext) error {
		if calls.Add(1) == 1 {
			close(entered)
		}
		select {
		case <-proceed:
			return c.SetText("final")
		case <-ctx.Done():
			return ctx.Err()
		}
	})}
	start(t, n)
	l := newLegacy(t, n)
	h := importText(t, n.e, "original")
	req := SubmitRequest{RequestID: "concurrent", LeaseID: h.LeaseID, Origin: OriginExplicit}
	ids := make(chan OperationID, 8)
	var wg sync.WaitGroup
	for range 8 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			id, err := n.e.SubmitContent(bg, req)
			if err != nil {
				t.Error(err)
			}
			ids <- id
		}()
	}
	<-entered
	// A callback must not monopolize the engine state lock.
	ctx, cancel := context.WithTimeout(bg, time.Second)
	defer cancel()
	if _, err := n.e.GetState(ctx); err != nil {
		t.Fatal(err)
	}
	if l.count() != 0 {
		t.Fatal("content dispatched before processing finished")
	}
	close(proceed)
	wg.Wait()
	close(ids)
	var first OperationID
	for id := range ids {
		if first == "" {
			first = id
		}
		if id != first {
			t.Fatal("different operations for one request")
		}
	}
	waitSend(t, n.e, first)
	eventually(t, "rewritten content", func() bool { return l.text() == "final" })
	if calls.Load() != 1 || l.count() != 1 {
		t.Fatal("concurrent request ran twice")
	}
	if err := n.e.ReleaseContent(bg, h.LeaseID); err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.SubmitContent(bg, req); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 {
		t.Fatal("completed retry reran processing")
	}
}

func TestPipelineSuspendDiscardsLatePatch(t *testing.T) {
	n := newContentNode(t, nil)
	entered := make(chan struct{})
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("example.late", []pipelineEvent{pipelineCaptured}, false, func(ctx context.Context, c *processingContext) error {
		close(entered)
		<-ctx.Done()
		return c.SetText("late success")
	})}
	start(t, n)
	h := importText(t, n.e, "original")
	done := make(chan error, 1)
	go func() { _, err := submit(n.e, h); done <- err }()
	<-entered
	if err := n.e.SetRuntimeState(bg, RuntimeSuspended); err != nil {
		t.Fatal(err)
	}
	if err := <-done; code(err) != CodeCancelled {
		t.Fatalf("late patch: %v", err)
	}
	if err := n.e.ReleaseContent(bg, h.LeaseID); err != nil {
		t.Fatal(err)
	}
	if stored(n.e) != 0 || len(state(t, n.e).Operations) != 0 {
		t.Fatal("cancelled processing published or retained content")
	}
}

func TestPipelineFaultsDiscardWholePatch(t *testing.T) {
	for _, mode := range []string{"panic", "timeout", "quota"} {
		t.Run(mode, func(t *testing.T) {
			n := newContentNode(t, func(o *Options) {
				o.Settings = DefaultSettings()
				o.Settings.Limits = Limits{MaxFileBytes: 64, MaxTotalBytes: 64}
				o.Resources.MaxStoredBytes = 64
			})
			n.e.processors = []*pipelineProcessor{newPipelineProcessor("example.fault", []pipelineEvent{pipelineCaptured}, true, func(ctx context.Context, c *processingContext) error {
				if err := c.SetPersist(false); err != nil {
					return err
				}
				switch mode {
				case "panic":
					panic("private text")
				case "timeout":
					<-ctx.Done()
				}
				return c.SetText(strings.Repeat("x", 63))
			})}
			out, err := n.e.processContent(bg, pipelineCaptured, pipelineInput(t, n.e, "base"), pipelineOrigin{kind: "explicit"})
			if err != nil {
				t.Fatal(err)
			}
			if out.item.Text != "base" || !out.policy.Persist || out.policy.Sync || stored(n.e) != 4 {
				t.Fatal("failed/timed-out patch changed content, policy or quota")
			}
			n.e.cs.unref(out.c)
			if stored(n.e) != 0 {
				t.Fatal("failed patch leaked content")
			}
		})
	}
}

func TestPipelineReceiveRechecksAdmissionAfterProcessing(t *testing.T) {
	for _, mode := range []string{"policy", "trust"} {
		t.Run(mode, func(t *testing.T) {
			a, b := newContentNode(t, nil), newContentNode(t, nil)
			entered, proceed, finished := make(chan struct{}), make(chan struct{}), make(chan struct{})
			b.e.processors = []*pipelineProcessor{newPipelineProcessor("example.receive", []pipelineEvent{pipelineReceived}, false, func(ctx context.Context, c *processingContext) error {
				close(entered)
				defer close(finished)
				select {
				case <-proceed:
					return c.SetText("prepared")
				case <-ctx.Done():
					return ctx.Err()
				}
			})}
			start(t, a)
			start(t, b)
			pairTwo(t, a, b)
			h := importText(t, a.e, "original")
			id, err := submit(a.e, h)
			if err != nil {
				t.Fatal(err)
			}
			<-entered
			if mode == "policy" {
				updateSettings(t, b.e, policy(BackendTailcat, func(p *BackendSyncPolicy) { p.Text = false }))
			} else if err := b.e.Unpair(bg, a.e.self); err != nil {
				t.Fatal(err)
			}
			close(proceed)
			<-finished
			waitSend(t, a.e, id)
			eventually(t, "discarded receive releases its reservation", func() bool { return stored(b.e) == 0 })
			if len(received(b.e)) != 0 {
				t.Fatal("revoked receive was published")
			}
			if err := a.e.ReleaseContent(bg, h.LeaseID); err != nil {
				t.Fatal(err)
			}
		})
	}
}
