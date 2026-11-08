package engine

import (
	"context"
	"fmt"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/pluginproc"
	"github.com/uniclipboard/unic/plugin"
)

// PluginInstance is one plugin in the processing order of Options.Plugins.
// Set either Plugin (runs inside the host process, e.g. built-ins) or Process
// (a Go plugin run as a child process over the plugin RPC protocol).
type PluginInstance struct {
	Plugin  plugin.Plugin
	Process *PluginProcess
	Config  plugin.Config
	// Events a processor subscribes to; empty means captured and received.
	Events []string
	// OnError is what a failed processor means for syncing: "continue"
	// (default) or "block_sync".
	OnError string
}

// PluginProcess describes a plugin executable. The engine verifies the
// digest, starts it with a minimal environment, and requires its handshake
// to present exactly Spec.
type PluginProcess struct {
	Path   string // absolute path
	SHA256 string // hex digest the executable must match
	Args   []string
	Env    []string // extra KEY=VALUE; nothing else is inherited
	Dir    string   // private data directory and working directory
	Spec   plugin.Spec
	// Timeouts override the defaults when non-zero; mainly for tests.
	Options pluginproc.Options
}

var (
	pluginIDRe = regexp.MustCompile(`^[a-z][a-z0-9]*(\.[a-z][a-z0-9_-]*)+$`)
	actionIDRe = regexp.MustCompile(`^[a-z][a-z0-9_-]*$`)
)

func invalidPlugin(id, why string) *Error {
	return newErr(CodeInvalidArgument, fmt.Sprintf("plugin %q: %s", id, why))
}

// loadPlugins validates, initializes and starts each plugin, registering its
// processor and actions. On failure everything already started is closed.
func (e *Engine) loadPlugins(ctx context.Context, list []PluginInstance) (err *Error) {
	seen := map[string]bool{}
	var started []plugin.Plugin
	defer func() {
		if err != nil {
			for _, p := range slices.Backward(started) {
				_ = p.Close(context.WithoutCancel(ctx))
			}
			e.processors, e.actions = nil, nil
		}
	}()
	for _, in := range list {
		if in.Process != nil {
			if in.Plugin != nil {
				return newErr(CodeInvalidArgument, "set Plugin or Process, not both")
			}
			pp := in.Process
			in.Plugin = pluginproc.New(pluginproc.Spec{Path: pp.Path, SHA256: pp.SHA256, Args: pp.Args, Env: pp.Env, Dir: pp.Dir,
				Expected: pp.Spec, Events: in.Events}, pp.Options)
		}
		if in.Plugin == nil {
			return newErr(CodeInvalidArgument, "nil plugin")
		}
		switch in.OnError {
		case "", "continue", "block_sync":
		default:
			return newErr(CodeInvalidArgument, "unknown on_error policy")
		}
		spec := in.Plugin.Spec()
		if verr := validateSpec(spec, in); verr != nil {
			return verr
		}
		if seen[spec.ID] {
			return invalidPlugin(spec.ID, "listed twice")
		}
		seen[spec.ID] = true
		host := &pluginHost{e: e, id: PluginID(spec.ID)}
		if ierr := in.Plugin.Init(ctx, host, in.Config); ierr != nil {
			return invalidPlugin(spec.ID, "Init failed") // its message could carry anything
		}
		started = append(started, in.Plugin)
		if serr := in.Plugin.Start(ctx); serr != nil {
			return invalidPlugin(spec.ID, "Start failed")
		}
		e.registerPlugin(in, spec)
	}
	e.plugins = started
	return nil
}

func validateSpec(s plugin.Spec, in PluginInstance) *Error {
	switch {
	case len(s.ID) > 128 || !pluginIDRe.MatchString(s.ID):
		return invalidPlugin(s.ID, "invalid ID")
	case strings.HasPrefix(s.ID, "sync."):
		return invalidPlugin(s.ID, "the sync. prefix is reserved")
	case s.APIVersion != 1:
		return invalidPlugin(s.ID, "unsupported API version")
	case s.Version == "" || len(s.Version) > 64:
		return invalidPlugin(s.ID, "invalid version")
	}
	var proc, act bool
	for _, c := range s.Capabilities {
		switch c {
		case plugin.CapProcessor:
			proc = true
		case plugin.CapAction:
			act = true
		default:
			return invalidPlugin(s.ID, "unsupported capability")
		}
	}
	if len(s.Capabilities) != len(slices.Compact(slices.Sorted(slices.Values(s.Capabilities)))) {
		return invalidPlugin(s.ID, "a capability is listed twice")
	}
	if _, ok := in.Plugin.(plugin.Processor); proc != ok {
		return invalidPlugin(s.ID, "capability processor does not match the implementation")
	}
	if _, ok := in.Plugin.(plugin.Actor); act != ok {
		return invalidPlugin(s.ID, "capability action does not match the implementation")
	}
	if act != (len(s.Actions) != 0) {
		return invalidPlugin(s.ID, "actions must be declared exactly when the plugin has the action capability")
	}
	ids := map[string]bool{}
	for _, a := range s.Actions {
		switch {
		case len(a.ID) > 64 || !actionIDRe.MatchString(a.ID) || ids[a.ID]:
			return invalidPlugin(s.ID, "invalid or repeated action ID")
		case a.Title == "" || len(a.Title) > 64 || !utf8.ValidString(a.Title) || strings.ContainsFunc(a.Title, func(r rune) bool { return r < 0x20 || r == 0x7f }):
			return invalidPlugin(s.ID, "invalid action title")
		case !slices.Equal(a.Accepts, []string{"text"}):
			return invalidPlugin(s.ID, "this version only accepts text")
		}
		ids[a.ID] = true
	}
	for _, ev := range in.Events {
		if ev != plugin.EventCaptured && ev != plugin.EventReceived {
			return invalidPlugin(s.ID, "unknown event")
		}
	}
	return nil
}

func (e *Engine) registerPlugin(in PluginInstance, spec plugin.Spec) {
	if p, ok := in.Plugin.(plugin.Processor); ok {
		events := []pipelineEvent{pipelineCaptured, pipelineReceived}
		if len(in.Events) != 0 {
			events = nil
			for _, ev := range in.Events {
				events = append(events, pipelineEvent(ev))
			}
		}
		e.processors = append(e.processors, newPipelineProcessor(spec.ID, events, in.OnError == "block_sync", func(ctx context.Context, c *processingContext) error {
			return p.Process(ctx, processView{c})
		}))
	}
	if a, ok := in.Plugin.(plugin.Actor); ok {
		ap := newActionPlugin(PluginID(spec.ID))
		for _, as := range spec.Actions {
			e.actions = append(e.actions, &pipelineAction{plugin: ap, id: as.ID, title: as.Title, accepts: slices.Clone(as.Accepts),
				run: func(ctx context.Context, c *actionContext) error { return a.Run(ctx, actionView{c, as.ID}) }})
		}
	}
}

func (e *Engine) closePlugins(ctx context.Context) {
	for _, p := range slices.Backward(e.plugins) {
		_ = p.Close(ctx)
	}
	e.plugins = nil
}

func publicItem(i pipelineItem) plugin.Item {
	out := plugin.Item{ID: i.ID, Type: i.Type, Text: i.Text, Formats: slices.Clone(i.Formats),
		Origin: plugin.Origin{Kind: i.Origin.kind, DeviceID: string(i.Origin.deviceID), PluginID: i.Origin.pluginID}}
	for _, f := range i.Files {
		out.Files = append(out.Files, plugin.File{Name: f.Name, Size: f.Size})
	}
	return out
}

type processView struct{ c *processingContext }

func (v processView) Event() string     { return string(v.c.Event()) }
func (v processView) Item() plugin.Item { return publicItem(v.c.Item()) }
func (v processView) Policy() plugin.Policy {
	p := v.c.Policy()
	return plugin.Policy{Persist: p.Persist, Sync: p.Sync, Tags: p.Tags, ExpireAfter: p.ExpireAfter}
}
func (v processView) AddTag(t string) error           { return v.c.AddTag(t) }
func (v processView) RemoveTag(t string) error        { return v.c.RemoveTag(t) }
func (v processView) SetPersist(b bool) error         { return v.c.SetPersist(b) }
func (v processView) SetSync(b bool) error            { return v.c.SetSync(b) }
func (v processView) SetExpire(d time.Duration) error { return v.c.SetExpire(d) }

type actionView struct {
	c  *actionContext
	id string
}

func (v actionView) Action() string         { return v.id }
func (v actionView) Item() plugin.Item      { return publicItem(v.c.Item()) }
func (v actionView) SetText(s string) error { return v.c.SetText(s) }

// pluginHost serves Notify: short, rate limited, never logged.
type pluginHost struct {
	e    *Engine
	id   PluginID
	mu   sync.Mutex
	last time.Time
}

func (h *pluginHost) Notify(_ context.Context, message string) {
	if h.e.opts.PluginNotify == nil {
		return
	}
	h.mu.Lock()
	now := time.Now()
	ok := now.Sub(h.last) >= time.Second
	if ok {
		h.last = now
	}
	h.mu.Unlock()
	if !ok {
		return
	}
	if len(message) > 256 {
		message = strings.ToValidUTF8(message[:256], "")
	}
	h.e.opts.PluginNotify(h.id, message)
}
