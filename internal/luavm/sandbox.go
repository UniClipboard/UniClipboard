package lua

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"
)

// Limits are per-callback budgets and per-state sizing. Zero fields take the
// defaults of lua-runtime-contract.md.
type Limits struct {
	Steps    int64
	Native   int64
	Bytes    int64
	Alloc    int64
	Timeout  time.Duration
	MaxSrc   int
	Compile  time.Duration
	CallSize int
	RegInit  int
	RegMax   int
}

func (l Limits) withDefaults() Limits {
	def := func(v *int64, d int64) {
		if *v <= 0 {
			*v = d
		}
	}
	def(&l.Steps, 1_000_000)
	def(&l.Native, 1_000_000)
	def(&l.Bytes, 8<<20)
	def(&l.Alloc, 16<<20)
	if l.Timeout <= 0 {
		l.Timeout = 200 * time.Millisecond
	}
	if l.Compile <= 0 {
		l.Compile = 200 * time.Millisecond
	}
	if l.MaxSrc <= 0 {
		l.MaxSrc = 256 << 10
	}
	if l.CallSize <= 0 {
		l.CallSize = 128
	}
	if l.RegInit <= 0 {
		l.RegInit = 1024
	}
	if l.RegMax <= 0 {
		l.RegMax = 8192
	}
	return l
}

// Sandbox is one plugin's Lua state. It is not safe for concurrent use; the
// caller serializes callbacks. After any failed callback it is Dead and must be
// discarded.
type Sandbox struct {
	L      *LState
	lim    Limits
	broken error
}

var baseAllow = map[string]bool{
	"assert": true, "error": true, "ipairs": true, "pairs": true, "next": true,
	"pcall": true, "xpcall": true, "select": true, "tonumber": true,
	"tostring": true, "type": true, "unpack": true,
}

// NewSandbox creates a state exposing only the whitelisted libraries.
func NewSandbox(lim Limits) *Sandbox {
	lim = lim.withDefaults()
	L := NewState(Options{CallStackSize: lim.CallSize, RegistrySize: lim.RegInit, RegistryMaxSize: lim.RegMax})
	L.Push(L.NewFunction(OpenBase))
	L.Call(0, 0)
	for _, lib := range []struct {
		name string
		fn   LGFunction
	}{{StringLibName, OpenString}, {TabLibName, OpenTable}, {MathLibName, OpenMath}} {
		L.Push(L.NewFunction(lib.fn))
		L.Push(LString(lib.name))
		L.Call(1, 0)
	}
	g := L.G.Global
	var drop []string
	g.ForEach(func(k, v LValue) {
		if ks, ok := k.(LString); ok {
			switch string(ks) {
			case StringLibName, TabLibName, MathLibName:
				return
			}
			if baseAllow[string(ks)] {
				return
			}
			drop = append(drop, string(ks))
		}
	})
	for _, k := range drop {
		g.RawSetString(k, LNil)
	}
	prune := func(lib string, keep ...string) {
		t, _ := g.RawGetString(lib).(*LTable)
		allow := map[string]bool{}
		for _, k := range keep {
			allow[k] = true
		}
		var rm []string
		t.ForEach(func(k, v LValue) {
			if ks, ok := k.(LString); ok && !allow[string(ks)] {
				rm = append(rm, string(ks))
			}
		})
		for _, k := range rm {
			t.RawSetString(k, LNil)
		}
	}
	prune(StringLibName, "byte", "char", "find", "format", "gmatch", "gsub", "len", "lower", "match", "rep", "reverse", "sub", "upper", "__index")
	prune(TabLibName, "concat", "insert", "remove", "sort")
	prune(MathLibName, "abs", "ceil", "floor", "max", "min", "sqrt", "pow", "fmod", "huge", "pi", "random", "randomseed", "mod", "modf", "exp", "log", "log10", "sin", "cos", "tan", "atan", "atan2", "asin", "acos", "sinh", "cosh", "tanh", "deg", "rad", "frexp", "ldexp")
	// Bounded table helpers: charge work proportional to the table length.
	tbl := g.RawGetString(TabLibName).(*LTable)
	for _, name := range []string{"sort", "concat", "insert", "remove"} {
		inner, _ := tbl.RawGetString(name).(*LFunction)
		if inner == nil {
			continue
		}
		name := name
		fn := inner.GFunction
		tbl.RawSetString(name, L.NewFunction(func(L *LState) int {
			if t, ok := L.Get(1).(*LTable); ok {
				n := t.Len()
				if name == "sort" {
					w := n
					for m := n; m > 1; m >>= 1 {
						w += n
					}
					L.ChargeNative(w)
				} else {
					L.ChargeNative(n)
				}
			}
			return fn(L)
		}))
	}
	u := L.NewTable()
	u.RawSetString("valid", L.NewFunction(func(L *LState) int {
		s := L.CheckString(1)
		L.ChargeBytes(len(s))
		L.Push(LBool(utf8.ValidString(s)))
		return 1
	}))
	u.RawSetString("len", L.NewFunction(func(L *LState) int {
		s := L.CheckString(1)
		L.ChargeBytes(len(s))
		if !utf8.ValidString(s) {
			L.Push(LNil)
			return 1
		}
		L.Push(LNumber(utf8.RuneCountInString(s)))
		return 1
	}))
	u.RawSetString("sub", L.NewFunction(func(L *LState) int {
		s := L.CheckString(1)
		L.ChargeBytes(len(s))
		if !utf8.ValidString(s) {
			L.ArgError(1, "invalid utf-8")
		}
		rs := []rune(s)
		i, j := L.OptInt(2, 1), L.OptInt(3, -1)
		if i < 0 {
			i = len(rs) + i + 1
		}
		if j < 0 {
			j = len(rs) + j + 1
		}
		if i < 1 {
			i = 1
		}
		if j > len(rs) {
			j = len(rs)
		}
		if i > j {
			L.Push(LString(""))
		} else {
			L.Push(LString(string(rs[i-1 : j])))
		}
		return 1
	}))
	g.RawSetString("utf8", u)
	return &Sandbox{L: L, lim: lim}
}

// Errors returned by sandbox operations.
var (
	ErrBroken   = errors.New("lua state is broken")
	ErrTooLarge = errors.New("lua source too large")
	ErrTooDeep  = errors.New("lua source nesting too deep")
)

// BudgetError reports a tripped resource limit.
type BudgetError struct{ Cause error }

func (e *BudgetError) Error() string { return e.Cause.Error() }
func (e *BudgetError) Unwrap() error { return e.Cause }

// Compile loads source into a function. It bounds size and bracket nesting
// before parsing and the parse/compile wall time afterwards.
func (s *Sandbox) Compile(src, name string) (*LFunction, error) {
	if s.broken != nil {
		return nil, ErrBroken
	}
	if len(src) > s.lim.MaxSrc {
		return nil, ErrTooLarge
	}
	depth, max := 0, 0
	for i := 0; i < len(src); i++ {
		switch src[i] {
		case '(', '{', '[':
			depth++
			if depth > max {
				max = depth
			}
		case ')', '}', ']':
			depth--
		}
	}
	if max > 128 {
		return nil, ErrTooDeep
	}
	start := time.Now()
	fn, err := s.L.Load(strings.NewReader(src), name)
	if err != nil {
		return nil, err
	}
	if time.Since(start) > s.lim.Compile {
		return nil, &BudgetError{ErrDeadline}
	}
	return fn, nil
}

// Call runs fn(args...) under a fresh budget derived from ctx. Any failure
// leaves the sandbox broken.
func (s *Sandbox) Call(ctx context.Context, fn LValue, nret int, args ...LValue) (rets []LValue, err error) {
	if s.broken != nil {
		return nil, ErrBroken
	}
	b := &Budget{Steps: s.lim.Steps, Native: s.lim.Native, Bytes: s.lim.Bytes, Alloc: s.lim.Alloc, Deadline: time.Now().Add(s.lim.Timeout), Ctx: ctx}
	s.L.SetBudget(b)
	defer s.L.SetBudget(nil)
	base := s.L.GetTop()
	s.L.Push(fn)
	for _, a := range args {
		s.L.Push(a)
	}
	perr := s.L.PCall(len(args), nret, nil)
	if be := b.Err(); be != nil {
		s.broken = be
		return nil, &BudgetError{be}
	}
	if perr != nil {
		s.broken = perr
		return nil, perr
	}
	for i := 0; i < nret; i++ {
		rets = append(rets, s.L.Get(base+1+i))
	}
	s.L.SetTop(base)
	return rets, nil
}

// Close releases the state.
func (s *Sandbox) Close() {
	if !s.L.IsClosed() {
		s.L.Close()
	}
}

func (s *Sandbox) String() string { return fmt.Sprintf("sandbox(broken=%v)", s.broken) }
