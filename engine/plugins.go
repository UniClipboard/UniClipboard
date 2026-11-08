package engine

import (
	"context"
	"errors"
	"fmt"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/pluginproc"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// PluginInstance is one plugin in the processing order of Options.Plugins.
// Set either Plugin (runs inside the host process, e.g. built-ins) or Process
// (a Go plugin run as a child process over the plugin RPC protocol).
type PluginInstance struct {
	Plugin  plugin.Plugin
	Process *PluginProcess
	Config  plugin.Config
	// Disabled keeps the plugin in the list, with its description checked and
	// its ID reserved, but does not start it or let it see anything.
	Disabled bool
	// Events the plugin subscribes to. Processors take clipboard.captured and
	// clipboard.received (default: both); observers take clipboard.committed,
	// updated, deleted and expired (default: all four). When the list is not
	// empty, every capability the plugin has needs at least one of its events.
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
	backends, unsupported := 0, false
	var pending []syncBackend // installed only when the whole list loads
	var started []plugin.Plugin
	defer func() {
		if err != nil {
			for _, p := range slices.Backward(started) {
				_ = p.Close(context.WithoutCancel(ctx))
			}
			e.stopObservers()
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
		if in.Disabled {
			continue
		}
		if slices.Contains(spec.Capabilities, plugin.CapSyncBackend) {
			if backends++; backends > e.res.MaxSyncBackends {
				return newErr(CodeLimitExceeded, "too many sync backends are enabled")
			}
			sb, ok := in.Plugin.(plugin.SyncBackend)
			if !ok || (in.Process != nil && !spec.ExternalChannels) {
				unsupported = true // out-of-process backends need the RPC business layer
				continue
			}
			if _, leaves := in.Plugin.(plugin.ChannelLeaver); spec.RemoteRevocation && !leaves {
				return invalidPlugin(spec.ID, "remote revocation needs LeaveChannel")
			}
			if slices.ContainsFunc(e.backends, func(b syncBackend) bool { return string(b.ID()) == spec.ID }) {
				return invalidPlugin(spec.ID, "a sync backend with this ID already exists")
			}
			host := &pluginHost{e: e, id: PluginID(spec.ID)}
			if ierr := in.Plugin.Init(ctx, host, in.Config); ierr != nil {
				return invalidPlugin(spec.ID, "Init failed")
			}
			pb := &pluginBackend{e: e, id: PluginID(spec.ID), kind: TransportKind(spec.TransportKind), sb: sb, spec: spec}
			if ra, ok := in.Plugin.(plugin.ReceiverAware); ok && spec.ExternalChannels {
				ra.SetReceiver(pb)
			}
			started = append(started, in.Plugin)
			if serr := in.Plugin.Start(ctx); serr != nil {
				return invalidPlugin(spec.ID, "Start failed")
			}
			pending = append(pending, pb)
			continue
		}
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
	if unsupported {
		return newErr(CodeUnsupported, "sync backend plugins are not supported yet")
	}
	e.plugins = started
	e.backends = append(e.backends, pending...)
	return nil
}

func validateSpec(s plugin.Spec, in PluginInstance) *Error {
	switch {
	case len(s.ID) > 128 || !pluginIDRe.MatchString(s.ID):
		return invalidPlugin(s.ID, "invalid ID")
	case s.ID == string(BackendTailcat):
		return invalidPlugin(s.ID, "builtin.tailcat is reserved")
	case s.APIVersion != 1:
		return invalidPlugin(s.ID, "unsupported API version")
	case s.Version == "" || len(s.Version) > 64:
		return invalidPlugin(s.ID, "invalid version")
	}
	var proc, act, obs, backend bool
	for _, c := range s.Capabilities {
		switch c {
		case plugin.CapSyncBackend:
			backend = true
		case plugin.CapObserver:
			obs = true
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
	if !backend && (s.ExternalChannels || s.RemoteRevocation) {
		return invalidPlugin(s.ID, "only sync backends declare channels")
	}
	if s.RemoteRevocation && !s.ExternalChannels {
		return invalidPlugin(s.ID, "remote revocation applies to external channels")
	}
	switch {
	case backend && s.TransportKind != plugin.TransportPeerSessions && s.TransportKind != plugin.TransportObjectStore:
		return invalidPlugin(s.ID, "a sync backend declares peer_sessions or object_store")
	case !backend && s.TransportKind != "":
		return invalidPlugin(s.ID, "only sync backends declare a transport kind")
	}
	if _, ok := in.Plugin.(plugin.Observer); obs != ok {
		return invalidPlugin(s.ID, "capability observer does not match the implementation")
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
	var procEv, obsEv int
	for _, ev := range in.Events {
		switch {
		case ev == plugin.EventCaptured || ev == plugin.EventReceived:
			procEv++
		case slices.Contains(observerEvents, ev):
			obsEv++
		default:
			return invalidPlugin(s.ID, "unknown event")
		}
	}
	if len(in.Events) != len(slices.Compact(slices.Sorted(slices.Values(in.Events)))) {
		return invalidPlugin(s.ID, "an event is listed twice")
	}
	if len(in.Events) != 0 && ((proc && procEv == 0) || (obs && obsEv == 0)) {
		return invalidPlugin(s.ID, "no subscribed event for one of its capabilities")
	}
	if (!proc && procEv != 0) || (!obs && obsEv != 0) {
		return invalidPlugin(s.ID, "an event does not match its capabilities")
	}
	return nil
}

func (e *Engine) registerPlugin(in PluginInstance, spec plugin.Spec) {
	if p, ok := in.Plugin.(plugin.Processor); ok {
		events := []pipelineEvent{pipelineCaptured, pipelineReceived}
		if len(in.Events) != 0 {
			events = nil
			for _, ev := range in.Events {
				if ev == plugin.EventCaptured || ev == plugin.EventReceived {
					events = append(events, pipelineEvent(ev))
				}
			}
		}
		e.processors = append(e.processors, newPipelineProcessor(spec.ID, events, in.OnError == "block_sync", func(ctx context.Context, c *processingContext) error {
			return p.Process(ctx, processView{c})
		}))
	}
	if o, ok := in.Plugin.(plugin.Observer); ok {
		events := observerEvents
		if len(in.Events) != 0 {
			events = slices.DeleteFunc(slices.Clone(in.Events), func(ev string) bool { return !slices.Contains(observerEvents, ev) })
		}
		e.observers = append(e.observers, newPipelineObserver(spec.ID, events, o.OnEvent))
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
	e.stopObservers()
	for _, p := range slices.Backward(e.plugins) {
		_ = p.Close(ctx)
	}
	e.plugins = nil
}

// RestartPlugin starts a process plugin again after it gave up (state
// "unavailable") following repeated crashes. In-process plugins and plugins
// that are running are not restartable.
func (e *Engine) RestartPlugin(ctx context.Context, id PluginID) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	i := slices.IndexFunc(e.plugins, func(p plugin.Plugin) bool { return p.Spec().ID == string(id) })
	if i < 0 {
		return newErr(CodeNotFound, "no such plugin")
	}
	r, ok := e.plugins[i].(interface{ Restart(context.Context) error })
	if !ok {
		return newErr(CodeUnsupported, "this plugin runs inside the host and cannot be restarted")
	}
	if err := r.Restart(ctx); err != nil {
		var re *rpc.Error
		switch {
		case errors.As(err, &re) && re.Code == rpc.CodeInvalidState:
			return newErr(CodeInvalidState, "the plugin is not unavailable")
		case ctx.Err() != nil:
			return ctxErr(ctx.Err())
		}
		return newErr(CodeUnavailable, "the plugin did not start")
	}
	return nil
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
