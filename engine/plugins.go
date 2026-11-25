package engine

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/pluginproc"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// PluginInstance is one plugin in the processing order of Options.Plugins.
// Set either Plugin (runs inside the host process, e.g. built-ins) or Process
// (a Go plugin run as a child process over the plugin RPC protocol).
type PluginInstance struct {
	// ID is the local runtime instance. Empty uses the implementation ID.
	// Distinct IDs are supported for external Go sync backends only.
	ID      PluginID
	Plugin  plugin.Plugin
	Process *PluginProcess
	Config  plugin.Config
	// Disabled keeps the plugin in the list, with its description checked and
	// its ID reserved, but does not start it or let it see anything.
	Disabled bool
	// ImageFilesAsImages opts an external process into PNG projection of one image file.
	ImageFilesAsImages bool
	// Events the plugin subscribes to. Processors take clipboard.captured and
	// clipboard.received (default: both); observers take clipboard.committed,
	// updated, deleted and expired (default: all four). When the list is not
	// empty, every capability the plugin has needs at least one of its events.
	Events []string
	// Builtin marks a plugin the host assembled itself (builtin.tailcat). Only
	// a Builtin instance may carry the reserved built-in ID.
	Builtin bool
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

func (in PluginInstance) instanceID() string {
	if in.ID != "" {
		return string(in.ID)
	}
	if in.Plugin != nil {
		return in.Plugin.Spec().ID
	}
	if in.Process != nil {
		return in.Process.Spec.ID
	}
	return ""
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
	e.pluginInstances = make(map[PluginID]plugin.Plugin)
	backends, unsupported := 0, false
	var pending []syncBackend // installed only when the whole list loads
	var peers []*peerProcBackend
	var started []plugin.Plugin
	defer func() {
		if err != nil {
			for _, p := range slices.Backward(started) {
				_ = p.Close(context.WithoutCancel(ctx))
			}
			e.stopObservers()
			e.processors, e.actions, e.enrichers = nil, nil, nil
		}
	}()
	for _, in := range list {
		id := in.instanceID()
		if in.Process != nil {
			if in.Plugin != nil {
				return newErr(CodeInvalidArgument, "set Plugin or Process, not both")
			}
			pp := in.Process
			dir := pp.Dir
			if !in.Disabled && slices.Contains(pp.Spec.Capabilities, plugin.CapSyncBackend) {
				if dir != "" {
					return invalidPlugin(pp.Spec.ID, "the engine owns a backend's directory")
				}
				if dir, err = e.backendDir(id); err != nil {
					return err
				}
			}
			in.Plugin = pluginproc.New(pluginproc.Spec{Path: pp.Path, SHA256: pp.SHA256, Args: pp.Args, Env: pp.Env, Dir: dir,
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
		if id != spec.ID && (in.Process == nil || !slices.Contains(spec.Capabilities, plugin.CapSyncBackend) || in.Builtin) {
			return invalidPlugin(id, "distinct instances require an external Go sync backend")
		}
		if len(id) > 128 || !pluginIDRe.MatchString(id) || strings.HasPrefix(id, "builtin.") && id != spec.ID {
			return invalidPlugin(id, "invalid instance ID")
		}
		if seen[id] {
			return invalidPlugin(spec.ID, "listed twice")
		}
		seen[id] = true
		if in.Disabled {
			continue
		}
		e.pluginInstances[PluginID(id)] = in.Plugin
		if slices.Contains(spec.Capabilities, plugin.CapSyncBackend) {
			if err := e.bindBackendInstance(id, spec.ID, in.Config); err != nil {
				return err
			}
			if backends++; backends > e.res.MaxSyncBackends {
				return newErr(CodeLimitExceeded, "too many sync backends are enabled")
			}
			sb, ok := in.Plugin.(plugin.SyncBackend)
			peerAware, isPeer := in.Plugin.(plugin.PeerBackend)
			isPeer = isPeer && !spec.ExternalChannels && spec.TransportKind == plugin.TransportPeerSessions
			if (!ok && !isPeer) || (in.Process != nil && !spec.ExternalChannels && !isPeer) { // a peer backend sends through PeerBackend, not Submit
				unsupported = true // out-of-process backends need the RPC business layer
				continue
			}
			if _, leaves := in.Plugin.(plugin.ChannelLeaver); spec.RemoteRevocation && !leaves {
				return invalidPlugin(spec.ID, "remote revocation needs LeaveChannel")
			}
			if slices.ContainsFunc(e.syncBackends, func(b syncBackend) bool { return string(b.ID()) == id }) {
				return invalidPlugin(spec.ID, "a sync backend with this ID already exists")
			}
			if in.Process == nil { // process backends took theirs above
				dir, derr := e.backendDir(id)
				if derr != nil {
					return derr
				}
				if da, ok := in.Plugin.(plugin.DirAware); ok {
					da.SetDir(dir)
				}
			}
			host := &pluginHost{e: e, id: PluginID(id)}
			if ierr := in.Plugin.Init(ctx, host, in.Config); ierr != nil {
				var re *rpc.Error // a stable code is safe to show; free text is not
				if in.Builtin && errors.As(ierr, &re) {
					return invalidPlugin(spec.ID, "Init failed: "+re.Code)
				}
				return invalidPlugin(spec.ID, "Init failed")
			}
			pb := &pluginBackend{e: e, id: PluginID(id), kind: TransportKind(spec.TransportKind), sb: sb, spec: spec, imageFilesAsImages: in.ImageFilesAsImages}
			if ra, ok := in.Plugin.(plugin.ReceiverAware); ok && spec.ExternalChannels {
				ra.SetReceiver(pb)
			}
			started = append(started, in.Plugin)
			var pp *peerProcBackend
			if isPeer {
				pp = &peerProcBackend{e: e, id: PluginID(id), implementation: spec.ID, pb: peerAware, bindingID: randHex(32)}
				if aa, ok := in.Plugin.(plugin.PeerAuthorityAware); ok {
					aa.SetPeerAuthority(pp)
				}
				if ra, ok := in.Plugin.(plugin.PeerReceiverAware); ok {
					ra.SetPeerReceiver(pp)
				}
				if la, ok := in.Plugin.(plugin.PeerLinkCallbackAware); ok {
					la.SetLinkStatusChanged(func(peerID string) {
						if peerID != "" {
							e.deviceChanged(peerID)
						}
					})
				}
				if berr := peerAware.Bind(ctx, pp.bindMsg()); berr != nil {
					return invalidPlugin(spec.ID, "Bind failed")
				}
				if perr := pp.push(ctx); perr != nil {
					return invalidPlugin(spec.ID, "the initial state was refused")
				}
			}
			if serr := in.Plugin.Start(ctx); serr != nil {
				return invalidPlugin(spec.ID, "Start failed")
			}
			if pp != nil {
				pending = append(pending, pp)
				peers = append(peers, pp)
				continue
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
	e.syncBackends = append(e.syncBackends, pending...)
	e.peerProcs = append(e.peerProcs, peers...)
	return nil
}

// backendDir prepares the private directory of a sync backend instance and
// holds its runtime.lock until the engine closes. A second engine on the same
// data directory, or a second instance with the same ID, gets CodeInUse.
func (e *Engine) backendDir(id string) (string, *Error) {
	if len(id) > 128 || !pluginIDRe.MatchString(id) {
		return "", invalidPlugin(id, "invalid ID")
	}
	dir, lock, err := openInstanceDir(e.opts.DataDir, id)
	if err != nil {
		if errors.Is(err, errLocked) {
			return "", newErr(CodeInUse, "the backend instance is used by another process")
		}
		return "", newErr(CodeUnavailable, "cannot prepare the backend directory")
	}
	e.instLocks = append(e.instLocks, lock)
	return dir, nil
}

func validateSpec(s plugin.Spec, in PluginInstance) *Error {
	switch {
	case len(s.ID) > 128 || !pluginIDRe.MatchString(s.ID):
		return invalidPlugin(s.ID, "invalid ID")
	case s.ID == string(BackendTailcat) && !in.Builtin:
		return invalidPlugin(s.ID, "builtin.tailcat is reserved")
	case s.APIVersion != 1:
		return invalidPlugin(s.ID, "unsupported API version")
	case s.Version == "" || len(s.Version) > 64:
		return invalidPlugin(s.ID, "invalid version")
	}
	if in.ImageFilesAsImages && (in.Process == nil || !s.ExternalChannels || !slices.Contains(s.Capabilities, plugin.CapSyncBackend)) {
		return invalidPlugin(s.ID, "image file projection requires an external Go sync backend")
	}
	if s.SettingsSHA256 != "" {
		digest, err := hex.DecodeString(s.SettingsSHA256)
		if err != nil || len(digest) != sha256.Size || s.SettingsSHA256 != hex.EncodeToString(digest) {
			return invalidPlugin(s.ID, "invalid settings schema digest")
		}
	}
	var proc, act, obs, backend, enr bool
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
		case plugin.CapEnricher:
			enr = true
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
	if _, ok := in.Plugin.(plugin.Enricher); enr != ok {
		return invalidPlugin(s.ID, "capability enricher does not match the implementation")
	}
	if enr != (s.Enricher != nil) {
		return invalidPlugin(s.ID, "enricher spec must be declared exactly when the plugin has the enricher capability")
	}
	if s.Enricher != nil {
		if verr := validateEnricherSpec(s.ID, *s.Enricher); verr != nil {
			return verr
		}
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
		case a.TimeoutMS < 0 || a.TimeoutMS > 300000:
			return invalidPlugin(s.ID, "action timeout is outside 0..300000ms")
		case len(a.Accepts) == 0 || len(a.Accepts) > 3:
			return invalidPlugin(s.ID, "invalid action input types")
		case len(a.Placements) == 0 || len(a.Placements) > 3:
			return invalidPlugin(s.ID, "invalid action placements")
		case len(a.PostActions) == 0 || len(a.PostActions) > 4:
			return invalidPlugin(s.ID, "invalid action post actions")
		}
		seenKinds := map[string]bool{}
		for _, kind := range a.Accepts {
			if (kind != "text" && kind != "image" && kind != "files") || seenKinds[kind] {
				return invalidPlugin(s.ID, "invalid or repeated action input type")
			}
			seenKinds[kind] = true
		}
		seenPlacements := map[string]bool{}
		for _, placement := range a.Placements {
			switch placement {
			case plugin.ActionPlacementHistoryItemMenu, plugin.ActionPlacementHistoryToolbar, plugin.ActionPlacementQuickPanel:
			default:
				return invalidPlugin(s.ID, "invalid action placement")
			}
			if seenPlacements[placement] {
				return invalidPlugin(s.ID, "repeated action placement")
			}
			seenPlacements[placement] = true
		}
		seenPost := map[string]bool{}
		for _, post := range a.PostActions {
			switch post {
			case plugin.ActionPostCopy, plugin.ActionPostNotify, plugin.ActionPostPresent, plugin.ActionPostSelect, plugin.ActionPostNone:
			default:
				return invalidPlugin(s.ID, "invalid action post action")
			}
			if seenPost[post] {
				return invalidPlugin(s.ID, "repeated action post action")
			}
			seenPost[post] = true
		}
		if seenPost[plugin.ActionPostNone] && len(a.PostActions) != 1 {
			return invalidPlugin(s.ID, "post action none cannot be combined with other post actions")
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
		e.registerActions(spec, a)
	}
	if enr, ok := in.Plugin.(plugin.Enricher); ok {
		e.registerEnricher(spec, enr)
	}
}

func validateEnricherSpec(id string, es plugin.EnricherSpec) *Error {
	if es.TimeoutMS < 0 || es.TimeoutMS > rpc.MaxEnrichTimeoutMs {
		return invalidPlugin(id, "enricher timeout is outside 0..120000ms")
	}
	if len(es.Accepts) == 0 || len(es.Accepts) > 3 {
		return invalidPlugin(id, "invalid enricher accepts")
	}
	seenAccept := map[string]bool{}
	for _, k := range es.Accepts {
		if (k != "text" && k != "image" && k != "files") || seenAccept[k] {
			return invalidPlugin(id, "invalid or repeated enricher input type")
		}
		seenAccept[k] = true
	}
	if len(es.Triggers) == 0 || len(es.Triggers) > 3 {
		return invalidPlugin(id, "invalid enricher triggers")
	}
	seenTrig := map[string]bool{}
	for _, t := range es.Triggers {
		if (t != plugin.EnrichTriggerOnCommit && t != plugin.EnrichTriggerOnDemand && t != plugin.EnrichTriggerManual) || seenTrig[t] {
			return invalidPlugin(id, "invalid or repeated enricher trigger")
		}
		seenTrig[t] = true
	}
	if len(es.RequiredTags) > 16 {
		return invalidPlugin(id, "too many enricher required tags")
	}
	seenTag := map[string]bool{}
	for _, t := range es.RequiredTags {
		if t == "" || len(t) > rpc.MaxTagBytes || !utf8.ValidString(t) || seenTag[t] {
			return invalidPlugin(id, "invalid or repeated enricher required tag")
		}
		seenTag[t] = true
	}
	if len(es.Annotations) > 16 {
		return invalidPlugin(id, "too many enricher annotation types")
	}
	seenAnn := map[string]bool{}
	for _, a := range es.Annotations {
		if !rpc.ValidAnnotationType(a) || seenAnn[a] {
			return invalidPlugin(id, "invalid or repeated enricher annotation type")
		}
		seenAnn[a] = true
	}
	return nil
}

func (e *Engine) registerEnricher(spec plugin.Spec, enr plugin.Enricher) {
	e.enrichers = append(e.enrichers, newPipelineEnricher(spec, enr))
}

func (e *Engine) registerActions(spec plugin.Spec, a plugin.Actor) {
	ap := newActionPlugin(PluginID(spec.ID))
	for _, as := range spec.Actions {
		e.actions = append(e.actions, &pipelineAction{plugin: ap, id: as.ID, title: as.Title, accepts: slices.Clone(as.Accepts), placements: slices.Clone(as.Placements), postActions: slices.Clone(as.PostActions), timeout: time.Duration(as.TimeoutMS) * time.Millisecond,
			run: func(ctx context.Context, c *actionContext) error { return a.Run(ctx, actionView{c, as.ID}) }})
	}
}

func (e *Engine) bindBackendInstance(id, implementation string, cfg plugin.Config) *Error {
	binding, err := backendInstanceBinding(id, implementation, cfg)
	if err != nil {
		return err
	}
	return e.bindBackendInstances([]store.BackendInstance{binding})
}

func backendInstanceBinding(id, implementation string, cfg plugin.Config) (store.BackendInstance, *Error) {
	config, err := json.Marshal(cfg)
	if err != nil {
		return store.BackendInstance{}, invalidPlugin(id, "configuration must contain JSON values")
	}
	digest := sha256.Sum256(config)
	return store.BackendInstance{ID: id, Implementation: implementation, ConfigHash: hex.EncodeToString(digest[:])}, nil
}

func (e *Engine) bindBackendInstances(bindings []store.BackendInstance) *Error {
	if err := e.st.BindBackendInstances(bindings); err != nil {
		if errors.Is(err, store.ErrDeviceConflict) {
			return newErr(CodeConflict, "backend implementation changed; create a new instance")
		}
		return newErr(CodeUnavailable, "cannot bind backend configuration")
	}
	return nil
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
	e.pluginStartMu.Lock()
	defer e.pluginStartMu.Unlock()
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	e.mu.Lock()
	p, exists := e.pluginInstances[id]
	e.mu.Unlock()
	if !exists {
		return newErr(CodeNotFound, "no such plugin")
	}
	r, ok := p.(interface{ Restart(context.Context) error })
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
	out := plugin.Item{SourceApplication: i.SourceApplication.Clone(), ID: i.ID, Type: i.Type, Text: i.Text, Formats: slices.Clone(i.Formats),
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

func (v actionView) Parts() []plugin.ActionPart { return v.c.Parts() }
func (v actionView) OpenPart(ctx context.Context, index int) (io.ReadCloser, error) {
	return v.c.OpenPart(ctx, index)
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
