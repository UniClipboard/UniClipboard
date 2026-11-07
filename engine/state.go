package engine

import (
	"context"
	"sort"
	"strings"
	"time"
)

var coreFormats = []string{MimeText, MimeHTML, MimePNG}

// clone returns a deep copy, so that callers cannot alter engine state.
func (s State) clone() State {
	c := s
	c.Capabilities = s.Capabilities.clone()
	c.Peers = append([]Peer{}, s.Peers...)
	c.Pending = append([]PendingPeer{}, s.Pending...)
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
	if !sy.Text {
		why = append(why, "text_disabled")
		eff.ReadFormats, eff.WriteFormats = without(eff.ReadFormats, MimeText, MimeHTML), without(eff.WriteFormats, MimeText, MimeHTML)
	}
	if !sy.Images {
		why = append(why, "images_disabled")
		eff.ReadFormats, eff.WriteFormats = without(eff.ReadFormats, MimePNG), without(eff.WriteFormats, MimePNG)
	}
	if !sy.Files {
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
	peers, err := e.peers()
	if err != nil {
		return State{}, err
	}
	pend, err := e.pending()
	if err != nil {
		return State{}, err
	}
	now := time.Now()
	e.ops.prune(now, e.res.OperationTTL)
	st := State{
		InstanceID: e.instance, Revision: e.rev, Lifecycle: e.life, Runtime: e.runtime,
		Self:         Self{ID: e.self, Name: e.selfNameLocked()},
		Settings:     e.settings,
		Capabilities: e.capsLocked(),
		Peers:        peers, Pending: pend,
		PairingCode: e.code.state(now),
		Operations:  e.ops.list(),
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
	e.mu.Lock()
	defer e.mu.Unlock()
	if req.ExpectedRevision != e.settings.Revision {
		return SettingsState{}, newErr(CodeConflict, "the settings changed; read them again")
	}
	if req.Value == e.settings.Value {
		return e.settings, nil
	}
	e.settings = SettingsState{Revision: e.settings.Revision + 1, Value: req.Value}
	e.commitLocked(EventSettings, EventCapabilities)
	return e.settings, nil
}

func contains(s []string, x string) bool {
	for _, v := range s {
		if v == x {
			return true
		}
	}
	return false
}
