package engine

import (
	"context"
	"errors"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"
)

// These are private processing types. RPC/Lua registration, durable commit and
// the public CommitContent API are added in later steps, not emulated here.
type pipelineEvent string

const (
	pipelineCaptured pipelineEvent = "clipboard.captured"
	pipelineReceived pipelineEvent = "clipboard.received"
)

type pipelineOrigin struct {
	kind     string
	deviceID DeviceID
	pluginID string // set when kind is "action"
}

type pipelineFile struct {
	Name string
	Size int64
}

type pipelineItem struct {
	ID      string
	Type    string
	Text    string
	Formats []string
	Files   []pipelineFile
	Origin  pipelineOrigin
}

type pipelinePolicy struct {
	Persist     bool
	Sync        bool
	Tags        []string
	ExpireAfter time.Duration // resolved against the eventual commit time
}

// A processor is installed before Start and immutable afterwards. A channel
// serializes this instance across all ingress paths without holding Engine.mu.
type pipelineProcessor struct {
	id        string
	events    []pipelineEvent
	blockSync bool
	gate      chan struct{}
	process   func(context.Context, *processingContext) error
}

func newPipelineProcessor(id string, events []pipelineEvent, blockSync bool, fn func(context.Context, *processingContext) error) *pipelineProcessor {
	return &pipelineProcessor{id: id, events: slices.Clone(events), blockSync: blockSync, gate: make(chan struct{}, 1), process: fn}
}

// processedContent owns exactly one content reference. Publication/dispatch
// acquires its own reference before this working reference is released.
type processedContent struct {
	c       *contentObj
	item    pipelineItem
	policy  pipelinePolicy
	blocked bool
}

type processingContext struct {
	mu     sync.Mutex
	active bool
	event  pipelineEvent
	item   pipelineItem
	policy pipelinePolicy
	fault  *Error // method failure invalidates the whole patch, even if ignored
}

func clonePipelineItem(i pipelineItem) pipelineItem {
	i.Formats = slices.Clone(i.Formats)
	i.Files = slices.Clone(i.Files)
	return i
}

func clonePipelinePolicy(p pipelinePolicy) pipelinePolicy {
	p.Tags = slices.Clone(p.Tags)
	return p
}

func (c *processingContext) Event() pipelineEvent { return c.event }

func (c *processingContext) Item() pipelineItem {
	c.mu.Lock()
	defer c.mu.Unlock()
	return clonePipelineItem(c.item)
}

func (c *processingContext) Policy() pipelinePolicy {
	c.mu.Lock()
	defer c.mu.Unlock()
	return clonePipelinePolicy(c.policy)
}

func (c *processingContext) change(fn func() *Error) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.active {
		return newErr(CodeResourceExpired, "the processing context has ended")
	}
	if c.fault != nil {
		return c.fault
	}
	c.fault = fn()
	if c.fault != nil {
		return c.fault
	}
	return nil
}

func (c *processingContext) AddTag(tag string) error {
	return c.change(func() *Error {
		if tag == "" || !utf8.ValidString(tag) || len(tag) > 64 {
			return newErr(CodeInvalidArgument, "invalid tag")
		}
		if !slices.Contains(c.policy.Tags, tag) {
			if len(c.policy.Tags) == 32 {
				return newErr(CodeLimitExceeded, "too many tags")
			}
			c.policy.Tags = append(c.policy.Tags, tag)
			slices.Sort(c.policy.Tags)
		}
		return nil
	})
}

func (c *processingContext) RemoveTag(tag string) error {
	return c.change(func() *Error {
		if tag == "" || !utf8.ValidString(tag) || len(tag) > 64 {
			return newErr(CodeInvalidArgument, "invalid tag")
		}
		c.policy.Tags = slices.DeleteFunc(c.policy.Tags, func(v string) bool { return v == tag })
		return nil
	})
}

func (c *processingContext) SetPersist(v bool) error {
	return c.change(func() *Error { c.policy.Persist = v; return nil })
}

func (c *processingContext) SetSync(v bool) error {
	return c.change(func() *Error { c.policy.Sync = v; return nil })
}

func (c *processingContext) SetExpire(v time.Duration) error {
	return c.change(func() *Error {
		if v <= 0 || v%time.Millisecond != 0 {
			return newErr(CodeInvalidArgument, "expiry must be positive whole milliseconds")
		}
		c.policy.ExpireAfter = v
		return nil
	})
}

func describePipelineItem(id string, c *contentObj, origin pipelineOrigin) pipelineItem {
	i := pipelineItem{ID: id, Type: "text", Origin: origin}
	for _, p := range c.parts {
		if p.desc.Kind == PartFile {
			i.Files = append(i.Files, pipelineFile{Name: p.desc.Name, Size: p.desc.Size})
		} else {
			i.Formats = append(i.Formats, p.desc.MIME)
			if p.desc.MIME == MimeText {
				i.Text = string(p.data)
			}
		}
	}
	if len(i.Files) != 0 {
		i.Type = "files"
	} else if slices.Contains(i.Formats, MimePNG) {
		i.Type = "image"
	}
	return i
}

// processContent consumes one reference, returning it (or a replacement) on
// success. Failure releases it. Engine.mu and the content-store mutex are never
// held during callbacks. Observation admission does not wait for processing.
func (e *Engine) processContent(ctx context.Context, event pipelineEvent, c *contentObj, origin pipelineOrigin) (out *processedContent, failure *Error) {
	w := &processedContent{c: c, item: describePipelineItem(e.newID(), c, origin), policy: pipelinePolicy{Persist: !e.opts.noPersistDefault, Sync: true}}
	defer func() {
		if failure != nil {
			e.cs.unref(w.c)
		}
	}()
	for _, p := range e.processors {
		if !slices.Contains(p.events, event) {
			continue
		}
		if err := ctx.Err(); err != nil {
			return nil, ctxErr(err)
		}
		callCtx, cancel := context.WithTimeout(ctx, 200*time.Millisecond)
		select {
		case p.gate <- struct{}{}:
		case <-callCtx.Done():
			cancel()
			if ctx.Err() != nil {
				return nil, ctxErr(ctx.Err())
			}
			w.blocked = w.blocked || p.blockSync
			continue
		}
		pc := &processingContext{active: true, event: event, item: clonePipelineItem(w.item), policy: clonePipelinePolicy(w.policy)}
		err := invokeProcessor(callCtx, p, pc)
		pc.mu.Lock()
		pc.active = false
		if err == nil {
			if pc.fault != nil {
				err = pc.fault
			} else {
				err = callCtx.Err()
			}
		}
		if err == nil {
			w.item, w.policy = clonePipelineItem(pc.item), clonePipelinePolicy(pc.policy)
		} else {
			w.blocked = w.blocked || p.blockSync
		}
		pc.mu.Unlock()
		<-p.gate
		cancel()
	}
	if err := ctx.Err(); err != nil {
		return nil, ctxErr(err)
	}
	if w.blocked {
		w.policy.Sync = false
	}
	return w, nil
}

// Private callbacks are trusted cooperative Go adapters, not an in-process
// third-party plugin API. RPC cancellation/process termination is added in P4.
func invokeProcessor(ctx context.Context, p *pipelineProcessor, c *processingContext) (err error) {
	defer func() {
		if recover() != nil {
			err = errors.New("processor failed")
		}
	}()
	return p.process(ctx, c)
}

func validExternalSubmissionID(id RequestID) bool {
	return id != "" && !strings.HasPrefix(string(id), "obs:") && !strings.HasPrefix(string(id), "recv:")
}
