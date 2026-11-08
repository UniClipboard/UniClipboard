package engine

import (
	"context"
	"errors"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/wire"
)

// Actions are user-invoked content transformations. Unlike processors they
// never run on their own, and they never touch the source item: the result is
// committed as a new item and the host decides when it reaches the system
// clipboard (BeginApplyHistory), so what was copied is never replaced silently.

// ActionInfo describes one action a plugin offers.
type ActionInfo struct {
	Key      string   `json:"key"` // plugin ID + "/" + action ID
	PluginID PluginID `json:"plugin_id"`
	ID       string   `json:"id"`
	Title    string   `json:"title"`
	Accepts  []string `json:"accepts"` // "text" in this version
}

// RunActionRequest names an action and the history item to run it on. The
// RequestID is idempotent like CommitContent's: a retry never runs it twice.
type RunActionRequest struct {
	RequestID RequestID
	Key       string
	HistoryID HistoryID
}

// ActionResult is the commit of the new item. HistoryID is always set on
// success, because action results are always persisted.
type ActionResult struct {
	SubmissionResult
}

// actionTimeout bounds one run; tests shorten it.
var actionTimeout = 10 * time.Second

// actionPlugin serializes the actions of one plugin instance.
type actionPlugin struct {
	id   PluginID
	gate chan struct{}
}

func newActionPlugin(id PluginID) *actionPlugin {
	return &actionPlugin{id: id, gate: make(chan struct{}, 1)}
}

type pipelineAction struct {
	plugin  *actionPlugin
	id      string
	title   string
	accepts []string
	run     func(context.Context, *actionContext) error
}

func (a *pipelineAction) key() string { return string(a.plugin.id) + "/" + a.id }

func (a *pipelineAction) info() ActionInfo {
	return ActionInfo{Key: a.key(), PluginID: a.plugin.id, ID: a.id, Title: a.title, Accepts: slices.Clone(a.accepts)}
}

// actionRun is the part of a submission that produces its content: the work
// happens inside the idempotent submission record, never before it.
type actionRun struct {
	act    *pipelineAction
	source HistoryID
}

type actionContext struct {
	mu     sync.Mutex
	active bool
	key    string
	item   pipelineItem
	text   *string
	fault  *Error
}

func (c *actionContext) Key() string { return c.key }

func (c *actionContext) Item() pipelineItem {
	c.mu.Lock()
	defer c.mu.Unlock()
	return clonePipelineItem(c.item)
}

// SetText supplies the one result. A second call, bad UTF-8 or an empty or
// oversized text fails the whole run.
func (c *actionContext) SetText(text string) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	switch {
	case !c.active:
		return newErr(CodeResourceExpired, "the action has ended")
	case c.fault != nil:
		return c.fault
	case c.text != nil:
		c.fault = newErr(CodeInvalidArgument, "the action already produced a result")
	case text == "" || !utf8.ValidString(text):
		c.fault = newErr(CodeInvalidArgument, "the result must be non-empty UTF-8 text")
	case len(text) > wire.MaxPartBytes:
		c.fault = newErr(CodeLimitExceeded, "the result is over the representation limit")
	}
	if c.fault != nil {
		return c.fault
	}
	c.text = &text
	return nil
}

func (e *Engine) findAction(key string) *pipelineAction {
	for _, a := range e.actions {
		if a.key() == key {
			return a
		}
	}
	return nil
}

// ListActions returns the actions that are available, sorted by key.
func (e *Engine) ListActions(ctx context.Context) ([]ActionInfo, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	out := make([]ActionInfo, 0, len(e.actions))
	for _, a := range e.actions {
		out = append(out, a.info())
	}
	slices.SortFunc(out, func(a, b ActionInfo) int { return strings.Compare(a.Key, b.Key) })
	return out, nil
}

// RunAction runs one action on a stored text item and commits the result as a
// new item (origin "action"). Nothing is written to the system clipboard, the
// source is unchanged and a failed run leaves nothing behind. The captured
// processors still annotate the new item.
func (e *Engine) RunAction(ctx context.Context, req RunActionRequest) (ActionResult, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return ActionResult{}, gerr
	}
	defer release()
	if !validExternalSubmissionID(req.RequestID) || req.HistoryID == "" {
		return ActionResult{}, newErr(CodeInvalidArgument, "RequestID and HistoryID are required")
	}
	act := e.findAction(req.Key)
	if act == nil {
		return ActionResult{}, newErr(CodeNotFound, "no such action")
	}
	e.mu.Lock()
	entry, err := e.startSubmissionLocked(SubmitRequest{RequestID: req.RequestID, action: &actionRun{act: act, source: req.HistoryID}})
	e.mu.Unlock()
	if err != nil {
		return ActionResult{}, err
	}
	select {
	case <-entry.done:
		r := entry.result(req.RequestID)
		if r.Error != nil {
			return ActionResult{r}, cloneError(r.Error)
		}
		return ActionResult{r}, nil
	case <-ctx.Done():
		return ActionResult{}, ctxErr(ctx.Err())
	}
}

// produce loads the source, runs the action and returns the new content,
// holding one reference.
func (r *actionRun) produce(ctx context.Context, e *Engine) (*contentObj, *Error) {
	src, lerr := e.loadHistoryContent(r.source)
	if lerr != nil {
		return nil, lerr
	}
	item := describePipelineItem(e.newID(), src, pipelineOrigin{kind: "explicit"}) // the user chose this item
	e.cs.unref(src)
	if item.Type != "text" || !slices.Contains(r.act.accepts, "text") {
		return nil, newErr(CodeUnsupported, "this action does not accept this item")
	}
	callCtx, cancel := context.WithTimeout(ctx, actionTimeout)
	defer cancel()
	select {
	case r.act.plugin.gate <- struct{}{}:
		defer func() { <-r.act.plugin.gate }()
	case <-callCtx.Done():
		if ctx.Err() != nil {
			return nil, ctxErr(ctx.Err())
		}
		return nil, newErr(CodeTimeout, "the action's plugin is busy")
	}
	ac := &actionContext{active: true, key: r.act.key(), item: item}
	err := invokeAction(callCtx, r.act, ac)
	ac.mu.Lock()
	ac.active = false
	text, fault := ac.text, ac.fault
	ac.mu.Unlock()
	switch {
	case ctx.Err() != nil:
		return nil, ctxErr(ctx.Err())
	case fault != nil:
		return nil, fault
	case callCtx.Err() != nil:
		return nil, newErr(CodeTimeout, "the action took too long")
	case err != nil:
		return nil, newErr(CodeInternal, "the action failed") // its message may carry content
	case text == nil:
		return nil, newErr(CodeInvalidArgument, "the action produced no result")
	}
	h, ierr := e.ImportContent(ctx, ContentInput{Representations: []RepresentationInput{{MIME: MimeText, Reader: strings.NewReader(*text), Size: int64(len(*text))}}})
	if ierr != nil {
		var ee *Error
		if errors.As(ierr, &ee) {
			return nil, ee
		}
		return nil, newErr(CodeInternal, "storing the result failed")
	}
	// Take a working reference, then let the temporary lease go.
	c, cerr := e.contentOf(h.LeaseID)
	if cerr != nil {
		return nil, cerr
	}
	e.cs.mu.Lock()
	c.refs++
	e.cs.mu.Unlock()
	_ = e.ReleaseContent(ctx, h.LeaseID)
	return c, nil
}

func invokeAction(ctx context.Context, a *pipelineAction, c *actionContext) (err error) {
	defer func() {
		if recover() != nil {
			err = errors.New("action panicked")
		}
	}()
	return a.run(ctx, c)
}
