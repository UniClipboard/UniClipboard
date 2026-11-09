package engine

import (
	"context"
	"errors"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func upperAction(p *actionPlugin, calls *atomic.Int32) *pipelineAction {
	return &pipelineAction{plugin: p, id: "upper", title: "Upper case", accepts: []string{"text"},
		run: func(_ context.Context, c *actionContext) error {
			if calls != nil {
				calls.Add(1)
			}
			return c.SetText(strings.ToUpper(c.Item().Text))
		}}
}

func historyTotal(t *testing.T, e *Engine) int {
	t.Helper()
	p, err := e.ListHistory(bg, ListHistoryRequest{})
	if err != nil {
		t.Fatal(err)
	}
	return len(p.Items)
}

func historyText(t *testing.T, e *Engine, id HistoryID) string {
	t.Helper()
	h, err := e.RetainHistoryContent(bg, id)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.ReleaseContent(bg, h.LeaseID) }()
	return readAll(t, e, h.LeaseID, h.Content.Parts[0].ID)
}

func TestActionCommitsANewItemAndLeavesTheSource(t *testing.T) {
	n := newContentNode(t, nil)
	pl := newActionPlugin("example.text")
	n.e.actions = []*pipelineAction{upperAction(pl, nil)}
	start(t, n)
	src := commitText(t, n.e, "src", "hello")
	r, err := n.e.RunAction(bg, RunActionRequest{RequestID: "run", Key: "example.text/upper", HistoryID: src.HistoryID})
	if err != nil {
		t.Fatal(err)
	}
	if r.Phase != SubmissionCommitted || r.HistoryID == "" || r.HistoryID == src.HistoryID {
		t.Fatalf("%+v", r)
	}
	it, err := n.e.GetItem(bg, r.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	if it.Origin.Kind != OriginKindAction || it.Origin.PluginID != "example.text" || it.Kind != HistoryText {
		t.Fatalf("%+v", it.Origin)
	}
	if historyText(t, n.e, r.HistoryID) != "HELLO" || historyText(t, n.e, src.HistoryID) != "hello" {
		t.Fatal("wrong result, or the source was changed")
	}
}

func TestActionResultIsAnnotatedAndAlwaysPersisted(t *testing.T) {
	n := newContentNode(t, nil)
	n.e.actions = []*pipelineAction{upperAction(newActionPlugin("example.text"), nil)}
	start(t, n)
	src := commitText(t, n.e, "src", "mail me@example.com")
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("example.tag", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error {
		if err := c.AddTag("seen"); err != nil {
			return err
		}
		return c.SetPersist(false)
	})}
	r, err := n.e.RunAction(bg, RunActionRequest{RequestID: "run", Key: "example.text/upper", HistoryID: src.HistoryID})
	if err != nil || r.HistoryID == "" {
		t.Fatalf("%+v %v", r, err)
	}
	it, _ := n.e.GetItem(bg, r.HistoryID)
	if len(it.Tags) != 1 || it.Tags[0] != "seen" {
		t.Fatalf("the new item was not annotated: %v", it.Tags)
	}
}

func TestActionRetriesNeverRunTwice(t *testing.T) {
	n := newContentNode(t, nil)
	var calls atomic.Int32
	n.e.actions = []*pipelineAction{upperAction(newActionPlugin("example.text"), &calls)}
	start(t, n)
	src := commitText(t, n.e, "src", "hello")
	req := RunActionRequest{RequestID: "once", Key: "example.text/upper", HistoryID: src.HistoryID}
	a, err := n.e.RunAction(bg, req)
	if err != nil {
		t.Fatal(err)
	}
	b, err := n.e.RunAction(bg, req)
	if err != nil || b.HistoryID != a.HistoryID || calls.Load() != 1 {
		t.Fatalf("retry ran again: %+v %v calls=%d", b, err, calls.Load())
	}
	if got, err := n.e.GetSubmission(bg, "once"); err != nil || got.HistoryID != a.HistoryID {
		t.Fatal(got, err)
	}
	req.HistoryID = "other"
	if _, err := n.e.RunAction(bg, req); code(err) != CodeConflict {
		t.Fatalf("RequestID reused for another source: %v", err)
	}
	if historyTotal(t, n.e) != 2 {
		t.Fatal("a retry stored another item")
	}
}

func TestActionFailuresLeaveNothingBehind(t *testing.T) {
	old := actionTimeout
	actionTimeout = 50 * time.Millisecond
	t.Cleanup(func() { actionTimeout = old })
	n := newContentNode(t, nil)
	pl := newActionPlugin("example.bad")
	mk := func(id string, f func(context.Context, *actionContext) error) *pipelineAction {
		return &pipelineAction{plugin: pl, id: id, title: id, accepts: []string{"text"}, run: f}
	}
	n.e.actions = []*pipelineAction{
		mk("error", func(context.Context, *actionContext) error { return errors.New("private body") }),
		mk("panic", func(context.Context, *actionContext) error { panic("private body") }),
		mk("none", func(context.Context, *actionContext) error { return nil }),
		mk("twice", func(_ context.Context, c *actionContext) error {
			_ = c.SetText("a")
			return c.SetText("b")
		}),
		mk("empty", func(_ context.Context, c *actionContext) error { return c.SetText("") }),
		mk("badutf8", func(_ context.Context, c *actionContext) error { return c.SetText("\xff") }),
		mk("slow", func(ctx context.Context, c *actionContext) error { <-ctx.Done(); return c.SetText("late") }),
		mk("ignored", func(_ context.Context, c *actionContext) error { _ = c.SetText(""); return nil }),
	}
	start(t, n)
	src := commitText(t, n.e, "src", "hello")
	want := map[string]Code{"error": CodeInternal, "panic": CodeInternal, "none": CodeInvalidArgument, "twice": CodeInvalidArgument,
		"empty": CodeInvalidArgument, "badutf8": CodeInvalidArgument, "slow": CodeTimeout, "ignored": CodeInvalidArgument}
	for id, c := range want {
		r, err := n.e.RunAction(bg, RunActionRequest{RequestID: RequestID("f-" + id), Key: "example.bad/" + id, HistoryID: src.HistoryID})
		if code(err) != c || r.Phase != SubmissionFailed {
			t.Errorf("%s: %v %+v", id, err, r)
		}
		if err != nil && strings.Contains(err.Error(), "private") {
			t.Errorf("%s leaked its message", id)
		}
	}
	if historyTotal(t, n.e) != 1 || stored(n.e) != 0 {
		t.Fatalf("failed runs left items or content: %d %d", historyTotal(t, n.e), stored(n.e))
	}
}

func TestActionRejectsBadTargets(t *testing.T) {
	n := newContentNode(t, nil)
	pl := newActionPlugin("example.text")
	n.e.actions = []*pipelineAction{
		upperAction(pl, nil),
		{plugin: pl, id: "files", title: "Files only", accepts: []string{"files"}, run: func(context.Context, *actionContext) error { return nil }},
	}
	start(t, n)
	src := commitText(t, n.e, "src", "hello")
	for name, tc := range map[string]struct {
		req  RunActionRequest
		want Code
	}{
		"no action":   {RunActionRequest{RequestID: "a", Key: "example.text/nope", HistoryID: src.HistoryID}, CodeNotFound},
		"no item":     {RunActionRequest{RequestID: "b", Key: "example.text/upper", HistoryID: "missing"}, CodeNotFound},
		"wrong kind":  {RunActionRequest{RequestID: "c", Key: "example.text/files", HistoryID: src.HistoryID}, CodeUnsupported},
		"no id":       {RunActionRequest{RequestID: "d", Key: "example.text/upper"}, CodeInvalidArgument},
		"no request":  {RunActionRequest{Key: "example.text/upper", HistoryID: src.HistoryID}, CodeInvalidArgument},
		"internal id": {RunActionRequest{RequestID: "obs:x", Key: "example.text/upper", HistoryID: src.HistoryID}, CodeInvalidArgument},
	} {
		if _, err := n.e.RunAction(bg, tc.req); code(err) != tc.want {
			t.Errorf("%s: %v", name, err)
		}
	}
	list, err := n.e.ListActions(bg)
	if err != nil || len(list) != 2 || list[0].Key != "example.text/files" || list[1].Title != "Upper case" {
		t.Fatalf("%+v %v", list, err)
	}
}

// Stop cancels an action that is running; it leaves no item behind.
func TestStopCancelsARunningActionAndLeavesNothing(t *testing.T) {
	n := newContentNode(t, nil)
	pl := newActionPlugin("example.slow")
	started := make(chan struct{})
	n.e.actions = []*pipelineAction{{plugin: pl, id: "hang", title: "hang", accepts: []string{"text"},
		run: func(ctx context.Context, c *actionContext) error {
			close(started)
			<-ctx.Done()
			return c.SetText("late")
		}}}
	start(t, n)
	src := commitText(t, n.e, "src", "hello")
	done := make(chan error, 1)
	go func() {
		_, err := n.e.RunAction(bg, RunActionRequest{RequestID: "hang", Key: "example.slow/hang", HistoryID: src.HistoryID})
		done <- err
	}()
	<-started
	if err := n.e.Stop(bg); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("the cancelled action reported success")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Stop did not cancel the running action")
	}
	if stored(n.e) != 0 {
		t.Fatal("a cancelled action leaked content")
	}
}
