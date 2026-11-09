package engine

import (
	"context"
	"reflect"
	"sort"
	"strings"
	"time"
)

var coreFormats = []string{MimeText, MimeHTML, MimePNG}

// clone returns a deep copy, so that callers cannot alter engine state.
func (s State) clone() State {
	c := s
	c.Capabilities = s.Capabilities.clone()
	c.Devices = make([]Device, len(s.Devices))
	for i, d := range s.Devices {
		d.Bindings = append([]DeviceBinding(nil), d.Bindings...)
		c.Devices[i] = d
	}
	if s.ActiveApply != nil {
		a := *s.ActiveApply
		c.ActiveApply = &a
	}
	c.Received = make([]ReceivedContent, len(s.Received))
	for i, r := range s.Received {
		r.Content.Parts = append([]PartDescriptor{}, r.Content.Parts...)
		c.Received[i] = r
	}
	c.Operations = make([]Operation, len(s.Operations))
	for i, o := range s.Operations {
		c.Operations[i] = o.clone()
	}
	return c
}

func (o Operation) clone() Operation {
	if o.Pairing != nil {
		p := *o.Pairing
		o.Pairing = &p
	}
	if o.Error != nil {
		e := *o.Error
		o.Error = &e
	}
	if o.Targets != nil {
		ts := make([]TargetStatus, len(o.Targets))
		for i, t := range o.Targets {
			if t.Error != nil {
				e := *t.Error
				t.Error = &e
			}
			ts[i] = t
		}
		o.Targets = ts
	}
	return o
}

func (h HostCapabilities) clone() HostCapabilities {
	h.ReadFormats = append([]string{}, h.ReadFormats...)
	h.WriteFormats = append([]string{}, h.WriteFormats...)
	return h
}

func (c Capabilities) clone() Capabilities {
	c.Host = c.Host.clone()
	c.Effective = c.Effective.clone()
	c.CoreFormats = append([]string{}, c.CoreFormats...)
	c.Restrictions = append([]string(nil), c.Restrictions...)
	return c
}

// normalizeHost validates and canonicalizes a host's declaration.
func normalizeHost(h HostCapabilities) (HostCapabilities, *Error) {
	ab := func(a Ability) (Ability, bool) {
		switch a {
		case "":
			return AbilityUnsupported, true
		case AbilityAvailable, AbilityPermissionRequired, AbilityUnavailable, AbilityUnsupported:
			return a, true
		}
		return a, false
	}
	var ok1, ok2 bool
	if h.Observe, ok1 = ab(h.Observe); !ok1 {
		return h, newErr(CodeInvalidArgument, "unknown observe ability")
	}
	if h.Apply, ok2 = ab(h.Apply); !ok2 {
		return h, newErr(CodeInvalidArgument, "unknown apply ability")
	}
	norm := func(in []string) ([]string, bool) {
		seen := map[string]bool{}
		out := []string{}
		for _, f := range in {
			f = strings.ToLower(strings.TrimSpace(f))
			if f == "" || len(f) > 127 || !strings.Contains(f, "/") || strings.ContainsAny(f, " \t\r\n") {
				return nil, false
			}
			if !seen[f] {
				seen[f] = true
				out = append(out, f)
			}
		}
		sort.Strings(out)
		return out, true
	}
	var ok bool
	if h.ReadFormats, ok = norm(h.ReadFormats); !ok {
		return h, newErr(CodeInvalidArgument, "invalid MIME type in ReadFormats")
	}
	if h.WriteFormats, ok = norm(h.WriteFormats); !ok {
		return h, newErr(CodeInvalidArgument, "invalid MIME type in WriteFormats")
	}
	return h, nil
}

func intersect(a, b []string) []string {
	in := map[string]bool{}
	for _, x := range b {
		in[x] = true
	}
	out := []string{}
	for _, x := range a {
		if in[x] {
			out = append(out, x)
		}
	}
	return out
}

func without(in []string, drop ...string) []string {
	out := []string{}
	for _, x := range in {
		keep := true
		for _, d := range drop {
			keep = keep && x != d
		}
		if keep {
			out = append(out, x)
		}
	}
	return out
}

// capsLocked computes what the engine will really do. e.mu must be held.
func (e *Engine) capsLocked() Capabilities {
	h := e.host.clone()
	eff := HostCapabilities{
		ReadFormats: intersect(h.ReadFormats, coreFormats), WriteFormats: intersect(h.WriteFormats, coreFormats),
		ReadFiles: h.ReadFiles, WriteFiles: h.WriteFiles, Observe: h.Observe, Apply: h.Apply,
	}
	var why []string
	sy := e.settings.Value.Sync
	blocked := false
	if e.life != LifecycleRunning {
		why, blocked = append(why, "not_running"), true
	}
	if e.runtime == RuntimeSuspended {
		why, blocked = append(why, "suspended"), true
	}
	if !sy.Enabled {
		why, blocked = append(why, "sync_disabled"), true
	}
	if !e.typeAllowedLocked("text") {
		why = append(why, "text_disabled")
		eff.ReadFormats, eff.WriteFormats = without(eff.ReadFormats, MimeText, MimeHTML), without(eff.WriteFormats, MimeText, MimeHTML)
	}
	if !e.typeAllowedLocked("images") {
		why = append(why, "images_disabled")
		eff.ReadFormats, eff.WriteFormats = without(eff.ReadFormats, MimePNG), without(eff.WriteFormats, MimePNG)
	}
	if !e.typeAllowedLocked("files") {
		why = append(why, "files_disabled")
		eff.ReadFiles, eff.WriteFiles = false, false
	}
	if blocked {
		eff.ReadFormats, eff.WriteFormats, eff.ReadFiles, eff.WriteFiles = []string{}, []string{}, false, false
		if eff.Observe == AbilityAvailable {
			eff.Observe = AbilityUnavailable
		}
		if eff.Apply == AbilityAvailable {
			eff.Apply = AbilityUnavailable
		}
	}
	return Capabilities{Host: h, CoreFormats: append([]string{}, coreFormats...), Effective: eff, Restrictions: why}
}

func (e *Engine) selfNameLocked() string {
	if n := strings.TrimSpace(e.settings.Value.DeviceName); n != "" {
		return n
	}
	return e.defName
}

// stateLocked assembles the snapshot. e.mu must be held, which makes the
// revision and the content one consistent point.
func (e *Engine) stateLocked() (State, *Error) {
	devices, err := e.deviceList()
	if err != nil {
		return State{}, err
	}
	now := time.Now()
	e.ops.prune(now, e.res.OperationTTL)
	hrev, herr := e.st.HistoryRevision()
	if herr != nil {
		return State{}, retryable(newErr(CodeUnavailable, "the history database failed"))
	}
	st := State{HistoryRevision: hrev,
		InstanceID: e.instance, Revision: e.rev, Lifecycle: e.life, Runtime: e.runtime,
		Self:         Self{ID: DeviceID(e.ident.DeviceID()), Name: e.selfNameLocked()},
		Settings:     e.settingsLocked(),
		Capabilities: e.capsLocked(),
		Devices:      devices,
		PairingCode:  e.code.state(now),
		Operations:   e.ops.list(),
		Received:     e.receivedLocked(now),
	}
	if t := e.ticket; t != nil {
		st.ActiveApply = &ActiveApply{Kind: ApplyKindReceived, ReceiptID: t.receipt, ContentID: t.content}
		if t.history != "" {
			st.ActiveApply.Kind, st.ActiveApply.HistoryID = ApplyKindHistory, t.history
		}
	}
	return st.clone(), nil
}

// GetState returns a consistent snapshot without secrets (no pairing code, no
// address, no clipboard content).
func (e *Engine) GetState(ctx context.Context) (State, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return State{}, gerr
	}
	defer release()
	e.mu.Lock()
	defer e.mu.Unlock()
	st, err := e.stateLocked()
	if err != nil {
		return State{}, err
	}
	return st, nil
}

// GetCapabilities returns the declared, supported and effective abilities.
func (e *Engine) GetCapabilities(ctx context.Context) (Capabilities, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return Capabilities{}, gerr
	}
	defer release()
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.capsLocked(), nil
}

// UpdateHostCapabilities replaces the host's declaration, for example after
// the user grants a permission. It changes no trust and starts no clipboard
// access.
func (e *Engine) UpdateHostCapabilities(ctx context.Context, caps HostCapabilities) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	h, herr := normalizeHost(caps)
	if herr != nil {
		return herr
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	e.host = h
	if h.Apply != AbilityAvailable {
		e.killTicketLocked()
	}
	e.commitLocked(EventCapabilities)
	return nil
}

// UpdateSettings replaces the settings as a whole after validating all of
// them; on any error nothing changes. A name change reaches other devices on
// their next connection, a limit change applies to transfers that start
// afterwards.
func (e *Engine) UpdateSettings(ctx context.Context, req UpdateSettingsRequest) (SettingsState, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return SettingsState{}, gerr
	}
	defer release()
	if verr := req.Value.validate(); verr != nil {
		return SettingsState{}, verr
	}
	if req.Value.Limits.MaxTotalBytes > e.res.MaxStoredBytes {
		return SettingsState{}, newErr(CodeInvalidArgument, "limits.max_total_bytes exceeds the resource policy")
	}
	req.Value.DeviceName = strings.TrimSpace(req.Value.DeviceName)
	req.Value.RendezvousURL = strings.TrimRight(req.Value.RendezvousURL, "/")
	value, perr := normalizePolicies(req.Value, e.backendIDs())
	if perr != nil {
		return SettingsState{}, perr
	}
	defer e.syncPeerStates() // after the unlock below: sync.enabled, limits and policies reach out-of-process backends
	e.mu.Lock()
	defer e.mu.Unlock()
	if req.ExpectedRevision != e.settings.Revision {
		return SettingsState{}, newErr(CodeConflict, "the settings changed; read them again")
	}
	if reflect.DeepEqual(value, e.settings.Value) {
		return e.settingsLocked(), nil
	}
	e.settings = SettingsState{Revision: e.settings.Revision + 1, Value: value}
	if !req.Value.Sync.Enabled && (e.ticket == nil || e.ticket.history == "") {
		e.killTicketLocked() // a stored record is local: pausing sync does not void its application
	}
	e.commitLocked(EventSettings, EventCapabilities)
	return e.settingsLocked(), nil
}

// settingsLocked returns a copy that shares nothing with the live settings.
func (e *Engine) settingsLocked() SettingsState {
	return SettingsState{Revision: e.settings.Revision, Value: e.settings.Value.clone()}
}

func (e *Engine) backendIDs() []PluginID {
	ids := make([]PluginID, len(e.syncBackends))
	for i, b := range e.syncBackends {
		ids[i] = b.ID()
	}
	return ids
}

// normalizePolicies checks the backend policies against the assembled
// backends and returns settings with the complete effective map.
func normalizePolicies(s Settings, ids []PluginID) (Settings, *Error) {
	out := make(map[PluginID]BackendSyncPolicy, len(ids))
	for _, id := range ids {
		out[id] = DefaultBackendPolicy()
	}
	for id, p := range s.BackendPolicies {
		if _, ok := out[id]; !ok {
			return s, newErr(CodeInvalidArgument, "backend_policies: unknown sync backend")
		}
		out[id] = p
	}
	s.BackendPolicies = out
	return s, nil
}

// policyLocked is the effective policy of a backend. e.mu held.
func (e *Engine) policyLocked(id PluginID) BackendSyncPolicy {
	if p, ok := e.settings.Value.BackendPolicies[id]; ok {
		return p
	}
	return DefaultBackendPolicy()
}

func (e *Engine) policyOf(id PluginID) BackendSyncPolicy {
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.policyLocked(id)
}

// typeAllowedLocked reports whether some backend syncs this kind of content.
// With no backend nothing is restricted: capture and history do not depend on
// sync policy. kind is "text", "images" or "files". e.mu held.
func (e *Engine) typeAllowedLocked(kind string) bool {
	if len(e.syncBackends) == 0 {
		return true
	}
	for _, b := range e.syncBackends {
		p := e.policyLocked(b.ID())
		if kind == "text" && p.Text || kind == "images" && p.Images || kind == "files" && p.Files {
			return true
		}
	}
	return false
}

func contains(s []string, x string) bool {
	for _, v := range s {
		if v == x {
			return true
		}
	}
	return false
}
