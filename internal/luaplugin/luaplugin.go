// Package luaplugin adapts a Lua source file to the plugin interfaces. The
// script runs in a luavm sandbox with per-callback budgets. Errors that cross
// back to the engine carry only a fixed category, never script messages or
// clipboard text.
package luaplugin

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"sync"
	"time"

	lua "github.com/uniclipboard/unic/internal/luavm"
	"github.com/uniclipboard/unic/plugin"
)

// Spec is what the host declares; the script's table must agree with it.
type Spec struct {
	ID           string
	Version      string
	APIVersion   int
	Capabilities []string
	Source       string
}

// Failure categories surfaced to the engine.
var (
	ErrResource    = errors.New("lua plugin: resource exhausted")
	ErrFailed      = errors.New("lua plugin: callback failed")
	ErrUnavailable = errors.New("lua plugin: unavailable until reloaded")
)

type base struct {
	spec  Spec
	pspec plugin.Spec

	mu      sync.Mutex // serializes callbacks and swaps
	sb      *lua.Sandbox
	tbl     *lua.LTable
	host    plugin.Host
	cfg     plugin.Config
	started bool
	dead    bool
}

// New loads the script and validates its table against spec. No callbacks of
// the plugin run yet except the chunk itself.
func New(spec Spec) (plugin.Plugin, error) {
	b := &base{spec: spec}
	sb, tbl, ps, err := b.load(context.Background(), spec.Source)
	if err != nil {
		return nil, err
	}
	b.sb, b.tbl, b.pspec = sb, tbl, ps
	has := func(c string) bool { return slices.Contains(spec.Capabilities, c) }
	p, a, o := has(plugin.CapProcessor), has(plugin.CapAction), has(plugin.CapObserver)
	pm, am, om := procM{b}, actM{b}, obsM{b}
	switch {
	case p && a && o:
		return struct {
			*base
			procM
			actM
			obsM
		}{b, pm, am, om}, nil
	case p && a:
		return struct {
			*base
			procM
			actM
		}{b, pm, am}, nil
	case p && o:
		return struct {
			*base
			procM
			obsM
		}{b, pm, om}, nil
	case a && o:
		return struct {
			*base
			actM
			obsM
		}{b, am, om}, nil
	case p:
		return struct {
			*base
			procM
		}{b, pm}, nil
	case a:
		return struct {
			*base
			actM
		}{b, am}, nil
	case o:
		return struct {
			*base
			obsM
		}{b, om}, nil
	}
	sb.Close()
	return nil, errors.New("lua plugin: needs processor, action or observer")
}

func (b *base) load(ctx context.Context, src string) (*lua.Sandbox, *lua.LTable, plugin.Spec, error) {
	sb := lua.NewSandbox(lua.Limits{})
	fail := func(err error) (*lua.Sandbox, *lua.LTable, plugin.Spec, error) {
		sb.Close()
		return nil, nil, plugin.Spec{}, err
	}
	fn, err := sb.Compile(src, "="+b.spec.ID)
	if err != nil {
		return fail(categorize(err))
	}
	rets, err := sb.Call(ctx, fn, 1)
	if err != nil {
		return fail(categorize(err))
	}
	t, ok := rets[0].(*lua.LTable)
	if !ok {
		return fail(fmt.Errorf("%w: script must return the plugin table", ErrFailed))
	}
	str := func(k string) string { s, _ := t.RawGetString(k).(lua.LString); return string(s) }
	num := func(k string) int { n, _ := t.RawGetString(k).(lua.LNumber); return int(n) }
	var caps []string
	if ct, ok := t.RawGetString("capabilities").(*lua.LTable); ok {
		for i := 1; i <= ct.Len(); i++ {
			if s, ok := ct.RawGetInt(i).(lua.LString); ok {
				caps = append(caps, string(s))
			}
		}
	}
	slices.Sort(caps)
	want := slices.Sorted(slices.Values(b.spec.Capabilities))
	if str("id") != b.spec.ID || str("version") != b.spec.Version || num("api_version") != b.spec.APIVersion || !slices.Equal(caps, want) {
		return fail(fmt.Errorf("%w: plugin table does not match its declaration", ErrFailed))
	}
	ps := plugin.Spec{ID: b.spec.ID, Version: b.spec.Version, APIVersion: b.spec.APIVersion, Capabilities: slices.Clone(b.spec.Capabilities)}
	if slices.Contains(want, plugin.CapAction) {
		at, _ := t.RawGetString("actions").(*lua.LTable)
		if at != nil {
			for i := 1; i <= at.Len(); i++ {
				e, ok := at.RawGetInt(i).(*lua.LTable)
				if !ok {
					return fail(fmt.Errorf("%w: bad action entry", ErrFailed))
				}
				as := plugin.ActionSpec{}
				if s, ok := e.RawGetString("id").(lua.LString); ok {
					as.ID = string(s)
				}
				if s, ok := e.RawGetString("title").(lua.LString); ok {
					as.Title = string(s)
				}
				if ac, ok := e.RawGetString("accepts").(*lua.LTable); ok {
					for j := 1; j <= ac.Len(); j++ {
						if s, ok := ac.RawGetInt(j).(lua.LString); ok {
							as.Accepts = append(as.Accepts, string(s))
						}
					}
				}
				ps.Actions = append(ps.Actions, as)
			}
		}
	}
	return sb, t, ps, nil
}

func categorize(err error) error {
	var be *lua.BudgetError
	switch {
	case errors.As(err, &be), errors.Is(err, lua.ErrTooLarge), errors.Is(err, lua.ErrTooDeep):
		return ErrResource
	case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
		return err
	case errors.Is(err, lua.ErrBroken):
		return ErrUnavailable
	}
	return ErrFailed
}

func (b *base) Spec() plugin.Spec {
	s := b.pspec
	s.Capabilities = slices.Clone(s.Capabilities)
	s.Actions = slices.Clone(s.Actions)
	return s
}

// callLocked calls tbl[name](args...) if the method exists. The caller holds mu.
func (b *base) callLocked(ctx context.Context, name string, args ...lua.LValue) error {
	if b.dead {
		return ErrUnavailable
	}
	fn, ok := b.tbl.RawGetString(name).(*lua.LFunction)
	if !ok {
		return nil
	}
	_, err := b.sb.Call(ctx, fn, 0, args...)
	if err != nil {
		b.dead = true
		return categorize(err)
	}
	return nil
}

func (b *base) hostTable(L *lua.LState, host plugin.Host, ctx func() context.Context) *lua.LTable {
	t := L.NewTable()
	t.RawSetString("notify", L.NewFunction(func(L *lua.LState) int {
		msg := L.CheckString(2)
		if host != nil && len(msg) <= 512 {
			host.Notify(ctx(), msg)
		}
		return 0
	}))
	return t
}

func (b *base) Init(ctx context.Context, host plugin.Host, cfg plugin.Config) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.host, b.cfg = host, cfg
	return b.initLocked(ctx)
}

func (b *base) initLocked(ctx context.Context) error {
	L := b.sb.L
	c, err := toLua(L, map[string]any(b.cfg), 0)
	if err != nil {
		return plugin.ErrInvalidConfig
	}
	return b.callLocked(ctx, "init", b.hostTable(L, b.host, func() context.Context { return context.Background() }), c)
}

func (b *base) Start(ctx context.Context) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if err := b.callLocked(ctx, "start"); err != nil {
		return err
	}
	b.started = true
	return nil
}

func (b *base) Close(ctx context.Context) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.started && !b.dead {
		_ = b.callLocked(ctx, "close")
	}
	b.started = false
	b.dead = true
	b.sb.Close()
	return nil
}

// Reload replaces the script (empty src: reload the original source). The
// new state is built, initialised and started before it replaces the old one;
// on failure the old state keeps serving — unless it was already dead, in
// which case the plugin stays unavailable. In-flight callbacks finish first.
func (b *base) Reload(ctx context.Context, src string) error {
	if src == "" {
		src = b.spec.Source
	}
	b.mu.Lock()
	nb := &base{spec: b.spec, host: b.host, cfg: b.cfg}
	started := b.started
	b.mu.Unlock()
	nb.spec.Source = src
	sb, tbl, ps, err := nb.load(ctx, src)
	if err != nil {
		return err
	}
	nb.sb, nb.tbl = sb, tbl
	if !slices.EqualFunc(ps.Actions, b.pspec.Actions, func(a, c plugin.ActionSpec) bool {
		return a.ID == c.ID && a.Title == c.Title && slices.Equal(a.Accepts, c.Accepts)
	}) {
		sb.Close()
		return fmt.Errorf("%w: actions cannot change on reload", ErrFailed)
	}
	if err := nb.initLocked(ctx); err != nil {
		sb.Close()
		return err
	}
	if started {
		if err := nb.callLocked(ctx, "start"); err != nil {
			sb.Close()
			return err
		}
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	old := b.sb
	b.sb, b.tbl, b.dead = sb, tbl, false
	b.spec.Source = src
	old.Close()
	return nil
}

// --- processor ---

type procM struct{ b *base }

func (m procM) Process(ctx context.Context, pc plugin.ProcessContext) error {
	b := m.b
	b.mu.Lock()
	defer b.mu.Unlock()
	L := b.sb.L
	t := L.NewTable()
	t.RawSetString("event", lua.LString(pc.Event()))
	t.RawSetString("item", itemTable(L, pc.Item()))
	t.RawSetString("policy", policyTable(L, pc.Policy()))
	wrap := func(f func(*lua.LState) error) *lua.LFunction {
		return L.NewFunction(func(L *lua.LState) int {
			if err := f(L); err != nil {
				L.RaiseError("%s", "context call failed")
			}
			return 0
		})
	}
	t.RawSetString("add_tag", wrap(func(L *lua.LState) error { return pc.AddTag(L.CheckString(2)) }))
	t.RawSetString("remove_tag", wrap(func(L *lua.LState) error { return pc.RemoveTag(L.CheckString(2)) }))
	t.RawSetString("set_persist", wrap(func(L *lua.LState) error { return pc.SetPersist(L.CheckBool(2)) }))
	t.RawSetString("set_sync", wrap(func(L *lua.LState) error { return pc.SetSync(L.CheckBool(2)) }))
	t.RawSetString("set_expire", wrap(func(L *lua.LState) error {
		ms := L.CheckInt64(2)
		return pc.SetExpire(time.Duration(ms) * time.Millisecond)
	}))
	return b.callLocked(ctx, "process", t)
}

// --- action ---

type actM struct{ b *base }

func (m actM) Run(ctx context.Context, ac plugin.ActionContext) error {
	b := m.b
	b.mu.Lock()
	defer b.mu.Unlock()
	L := b.sb.L
	t := L.NewTable()
	t.RawSetString("action", lua.LString(ac.Action()))
	t.RawSetString("item", itemTable(L, ac.Item()))
	t.RawSetString("set_text", L.NewFunction(func(L *lua.LState) int {
		if err := ac.SetText(L.CheckString(2)); err != nil {
			L.RaiseError("%s", "context call failed")
		}
		return 0
	}))
	t.RawSetString("notify", L.NewFunction(func(L *lua.LState) int {
		if msg := L.CheckString(2); b.host != nil && len(msg) <= 512 {
			b.host.Notify(ctx, msg)
		}
		return 0
	}))
	return b.callLocked(ctx, "run_action", t)
}

// --- observer ---

type obsM struct{ b *base }

func (m obsM) OnEvent(ctx context.Context, ev plugin.Event) error {
	b := m.b
	b.mu.Lock()
	defer b.mu.Unlock()
	L := b.sb.L
	t := L.NewTable()
	t.RawSetString("kind", lua.LString(ev.Kind))
	t.RawSetString("item_id", lua.LString(ev.ItemID))
	t.RawSetString("at_ms", lua.LNumber(ev.AtMs))
	return b.callLocked(ctx, "on_event", t)
}

func itemTable(L *lua.LState, i plugin.Item) *lua.LTable {
	t := L.NewTable()
	t.RawSetString("id", lua.LString(i.ID))
	t.RawSetString("type", lua.LString(i.Type))
	t.RawSetString("text", lua.LString(i.Text))
	f := L.NewTable()
	for _, s := range i.Formats {
		f.Append(lua.LString(s))
	}
	t.RawSetString("formats", f)
	fs := L.NewTable()
	for _, x := range i.Files {
		e := L.NewTable()
		e.RawSetString("name", lua.LString(x.Name))
		e.RawSetString("size", lua.LNumber(x.Size))
		fs.Append(e)
	}
	t.RawSetString("files", fs)
	o := L.NewTable()
	o.RawSetString("kind", lua.LString(i.Origin.Kind))
	o.RawSetString("device_id", lua.LString(i.Origin.DeviceID))
	o.RawSetString("plugin_id", lua.LString(i.Origin.PluginID))
	t.RawSetString("origin", o)
	return t
}

func policyTable(L *lua.LState, p plugin.Policy) *lua.LTable {
	t := L.NewTable()
	t.RawSetString("persist", lua.LBool(p.Persist))
	t.RawSetString("sync", lua.LBool(p.Sync))
	tags := L.NewTable()
	for _, s := range p.Tags {
		tags.Append(lua.LString(s))
	}
	t.RawSetString("tags", tags)
	t.RawSetString("expire_ms", lua.LNumber(p.ExpireAfter.Milliseconds()))
	return t
}

// toLua converts JSON-shaped config values.
func toLua(L *lua.LState, v any, depth int) (lua.LValue, error) {
	if depth > 16 {
		return nil, errors.New("too deep")
	}
	switch x := v.(type) {
	case nil:
		return lua.LNil, nil
	case bool:
		return lua.LBool(x), nil
	case string:
		return lua.LString(x), nil
	case float64:
		return lua.LNumber(x), nil
	case int:
		return lua.LNumber(x), nil
	case int64:
		return lua.LNumber(x), nil
	case []any:
		t := L.NewTable()
		for _, e := range x {
			lv, err := toLua(L, e, depth+1)
			if err != nil {
				return nil, err
			}
			t.Append(lv)
		}
		return t, nil
	case map[string]any:
		t := L.NewTable()
		for k, e := range x {
			lv, err := toLua(L, e, depth+1)
			if err != nil {
				return nil, err
			}
			t.RawSetString(k, lv)
		}
		return t, nil
	}
	return nil, errors.New("unsupported config value")
}
