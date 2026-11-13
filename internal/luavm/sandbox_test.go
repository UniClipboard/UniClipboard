package lua

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

func run(t *testing.T, lim Limits, src string) (*Sandbox, []LValue, error) {
	t.Helper()
	s := NewSandbox(lim)
	t.Cleanup(s.Close)
	fn, err := s.Compile(src, "t")
	if err != nil {
		return s, nil, err
	}
	r, err := s.Call(context.Background(), fn, 1)
	return s, r, err
}

func TestSandboxBasics(t *testing.T) {
	_, r, err := run(t, Limits{}, `local t = {} for i=1,10 do t[#t+1]=i*2 end return table.concat(t, ",") .. utf8.sub("héllo", 2, 3) .. ("x"):rep(3)`)
	if err != nil || r[0].String() != "2,4,6,8,10,12,14,16,18,20élxxx" {
		t.Fatalf("%v %v", r, err)
	}
}

func TestSandboxWhitelist(t *testing.T) {
	for _, g := range []string{"os", "io", "debug", "package", "require", "load", "loadstring", "dofile", "print", "collectgarbage", "getfenv", "setfenv", "rawget", "rawset", "setmetatable", "getmetatable", "newproxy", "coroutine", "module", "_G"} {
		_, r, err := run(t, Limits{}, `return `+g)
		if err != nil || r[0] != LNil {
			t.Errorf("%s exposed: %v %v", g, r, err)
		}
	}
	_, r, _ := run(t, Limits{}, `return string.dump, string.gfind, table.maxn`)
	_ = r
	_, r, err := run(t, Limits{}, `return string.dump`)
	if err != nil || r[0] != LNil {
		t.Fatal("string.dump exposed")
	}
}

func budgetErr(t *testing.T, lim Limits, src string, want error) {
	t.Helper()
	// Keep resource-budget assertions independent of scheduler speed.
	// Deadline coverage supplies its own explicit timeout.
	if lim.Timeout == 0 {
		lim.Timeout = time.Minute
	}
	s, _, err := run(t, lim, src)
	var be *BudgetError
	if !errors.As(err, &be) || !errors.Is(err, want) {
		t.Fatalf("%q: want %v got %v", src, want, err)
	}
	if _, err := s.Call(context.Background(), LNil, 0); !errors.Is(err, ErrBroken) {
		t.Fatalf("not broken: %v", err)
	}
}

func TestSandboxBudgets(t *testing.T) {
	budgetErr(t, Limits{}, `while true do end`, ErrStepBudget)
	budgetErr(t, Limits{Steps: 1 << 40, Timeout: 30 * time.Millisecond}, `while true do end`, ErrDeadline)
	// pcall cannot recover an abort
	budgetErr(t, Limits{Steps: 1000}, `local n=0 while true do pcall(function() while true do n=n+1 end end) end`, ErrStepBudget)
	budgetErr(t, Limits{}, `for i=1,100 do local s = ("a"):rep(500000) end`, ErrByteBudget)
	budgetErr(t, Limits{Native: 1000}, `return (("a"):rep(5000)):find(".-.-.-.-b")`, ErrNativeBudget)
	budgetErr(t, Limits{Alloc: 1 << 20}, `local s="x" for i=1,40 do s = s .. s end return s`, ErrAllocBudget)
}

func TestSandboxCancelAndLimits(t *testing.T) {
	s := NewSandbox(Limits{Steps: 1 << 40, Timeout: time.Minute})
	defer s.Close()
	fn, _ := s.Compile(`while true do end`, "t")
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, err := s.Call(ctx, fn, 0); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
	s2 := NewSandbox(Limits{})
	defer s2.Close()
	if _, err := s2.Compile(strings.Repeat("x", 300<<10), "t"); !errors.Is(err, ErrTooLarge) {
		t.Fatal(err)
	}
	if _, err := s2.Compile("return "+strings.Repeat("(", 200)+"1"+strings.Repeat(")", 200), "t"); !errors.Is(err, ErrTooDeep) {
		t.Fatal(err)
	}
}

func TestSandboxPlainError(t *testing.T) {
	_, _, err := run(t, Limits{}, `error("boom")`)
	var be *BudgetError
	if err == nil || errors.As(err, &be) {
		t.Fatalf("want ordinary error, got %v", err)
	}
	budgetStack, _, err := run(t, Limits{}, `local function f() return f() + 1 end return f()`)
	_ = budgetStack
	if err == nil {
		t.Fatal("infinite recursion must fail")
	}
}
