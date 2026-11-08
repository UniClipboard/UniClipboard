package lua

import (
	"context"
	"errors"
	"time"
)

// Budget limits one callback. It is shared by every thread of a Global and is
// sticky: once any limit trips, every later instruction and every metered native
// operation raises again, so a script-level pcall cannot recover an abort.
type Budget struct {
	Steps    int64 // remaining VM instructions
	Native   int64 // remaining native iterations (pattern matching, etc.)
	Bytes    int64 // remaining bytes a native operation may scan or copy
	Alloc    int64 // remaining logical allocation units (table slots, closures, string bytes)
	Deadline time.Time
	Ctx      context.Context

	tick int
	err  error
}

var (
	ErrStepBudget   = errors.New("instruction budget exhausted")
	ErrNativeBudget = errors.New("native work budget exhausted")
	ErrByteBudget   = errors.New("scan/copy budget exhausted")
	ErrAllocBudget  = errors.New("allocation budget exhausted")
	ErrDeadline     = errors.New("deadline exceeded")
)

// Err returns the sticky abort cause, or nil.
func (b *Budget) Err() error { return b.err }

func (b *Budget) abort(err error) error {
	if b.err == nil {
		b.err = err
	}
	return b.err
}

// step charges one VM instruction.
func (b *Budget) step() error {
	if b.err != nil {
		return b.err
	}
	b.Steps--
	if b.Steps < 0 {
		return b.abort(ErrStepBudget)
	}
	b.tick++
	if b.tick&1023 == 0 {
		return b.poll()
	}
	return nil
}

func (b *Budget) poll() error {
	if !b.Deadline.IsZero() && time.Now().After(b.Deadline) {
		return b.abort(ErrDeadline)
	}
	if b.Ctx != nil {
		if e := b.Ctx.Err(); e != nil {
			return b.abort(e)
		}
	}
	return nil
}

// ChargeNative charges n native iterations; ChargeBytes n scanned/copied bytes;
// ChargeAlloc n logical allocation units. Each raises on exhaustion.
func (b *Budget) chargeNative(n int64) error {
	if b.err != nil {
		return b.err
	}
	b.Native -= n
	if b.Native < 0 {
		return b.abort(ErrNativeBudget)
	}
	b.tick++
	if b.tick&1023 == 0 {
		return b.poll()
	}
	return nil
}

func (b *Budget) chargeBytes(n int64) error {
	if b.err != nil {
		return b.err
	}
	b.Bytes -= n
	if b.Bytes < 0 {
		return b.abort(ErrByteBudget)
	}
	return b.poll()
}

func (b *Budget) chargeAlloc(n int64) error {
	if b.err != nil {
		return b.err
	}
	b.Alloc -= n
	if b.Alloc < 0 {
		return b.abort(ErrAllocBudget)
	}
	return nil
}

// SetBudget installs b (nil removes it). Call before running code.
func (ls *LState) SetBudget(b *Budget) { ls.G.budget = b }

// Budget returns the installed budget or nil.
func (ls *LState) Budget() *Budget { return ls.G.budget }

// metered raises if the sticky budget has tripped, or applies charge.
func (ls *LState) charge(f func(*Budget) error) {
	if b := ls.G.budget; b != nil {
		if err := f(b); err != nil {
			ls.raiseAbort(err)
		}
	}
}

func (ls *LState) raiseAbort(err error) {
	ls.RaiseError("%s", err.Error())
}

// ChargeBytes is exported for whitelisted native wrappers.
func (ls *LState) ChargeBytes(n int) {
	ls.charge(func(b *Budget) error { return b.chargeBytes(int64(n)) })
}
func (ls *LState) ChargeNative(n int) {
	ls.charge(func(b *Budget) error { return b.chargeNative(int64(n)) })
}
func (ls *LState) ChargeAlloc(n int) {
	ls.charge(func(b *Budget) error { return b.chargeAlloc(int64(n)) })
}
