package daemonproc

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"
)

type stepRecorder struct {
	calls     []string
	logs      []string
	graceful  error
	exited    bool
	waitErr   error
	termErr   error
	waitGiven time.Duration
	ctxBound  time.Duration
}

func (r *stepRecorder) steps() stopSteps {
	return stopSteps{
		requestGraceful: func(ctx context.Context) error {
			r.calls = append(r.calls, "graceful")
			if d, ok := ctx.Deadline(); ok {
				r.ctxBound = time.Until(d)
			}
			return r.graceful
		},
		waitExit: func(_ uint32, timeout time.Duration) (bool, error) {
			r.calls = append(r.calls, "wait")
			r.waitGiven = timeout
			return r.exited, r.waitErr
		},
		terminate: func(uint32, time.Duration) error {
			r.calls = append(r.calls, "terminate")
			return r.termErr
		},
		logf: func(format string, args ...any) { r.logs = append(r.logs, fmt.Sprintf(format, args...)) },
	}
}

func TestStopSkipsTerminateWhenProcessExitsWithinWindow(t *testing.T) {
	r := &stepRecorder{exited: true}
	if err := stopGracefullyThenTerminate(r.steps(), 42, time.Second); err != nil {
		t.Fatal(err)
	}
	if got := fmt.Sprint(r.calls); got != "[graceful wait]" {
		t.Fatalf("calls = %s, want [graceful wait]", got)
	}
	if r.waitGiven <= 0 || r.waitGiven > GracefulStopWindow {
		t.Fatalf("wait bound = %s, want within (0, %s]", r.waitGiven, GracefulStopWindow)
	}
	if r.ctxBound <= 0 || r.ctxBound > GracefulStopWindow {
		t.Fatalf("request bound = %s, want within (0, %s]", r.ctxBound, GracefulStopWindow)
	}
}

func TestStopTerminatesOnlyAfterWindowWhenProcessStaysAlive(t *testing.T) {
	r := &stepRecorder{exited: false}
	if err := stopGracefullyThenTerminate(r.steps(), 42, time.Second); err != nil {
		t.Fatal(err)
	}
	if got := fmt.Sprint(r.calls); got != "[graceful wait terminate]" {
		t.Fatalf("calls = %s, want [graceful wait terminate]", got)
	}
	if len(r.logs) != 1 {
		t.Fatalf("logs = %q, want one timeout line", r.logs)
	}
}

func TestStopFallsBackToTerminateAndLogsWhenGracefulRequestFails(t *testing.T) {
	r := &stepRecorder{graceful: errors.New("connection refused")}
	if err := stopGracefullyThenTerminate(r.steps(), 42, time.Second); err != nil {
		t.Fatal(err)
	}
	if got := fmt.Sprint(r.calls); got != "[graceful terminate]" {
		t.Fatalf("calls = %s, want [graceful terminate]", got)
	}
	if len(r.logs) != 1 || !strings.Contains(r.logs[0], "connection refused") {
		t.Fatalf("logs = %q, want the request error logged", r.logs)
	}
}

func TestStopFallsBackToTerminateAndLogsWhenWaitFails(t *testing.T) {
	r := &stepRecorder{waitErr: errors.New("access denied")}
	if err := stopGracefullyThenTerminate(r.steps(), 42, time.Second); err != nil {
		t.Fatal(err)
	}
	if got := fmt.Sprint(r.calls); got != "[graceful wait terminate]" {
		t.Fatalf("calls = %s, want [graceful wait terminate]", got)
	}
	if len(r.logs) != 1 || !strings.Contains(r.logs[0], "access denied") {
		t.Fatalf("logs = %q, want the wait error logged", r.logs)
	}
}

func TestStopReturnsTerminateError(t *testing.T) {
	want := errors.New("still running")
	r := &stepRecorder{graceful: errors.New("x"), termErr: want}
	if err := stopGracefullyThenTerminate(r.steps(), 42, time.Second); !errors.Is(err, want) {
		t.Fatalf("err = %v, want %v", err, want)
	}
}

func TestStopRefusesPidZeroWithoutAnyStep(t *testing.T) {
	r := &stepRecorder{}
	if err := stopGracefullyThenTerminate(r.steps(), 0, time.Second); err == nil {
		t.Fatal("want an error for pid 0")
	}
	if len(r.calls) != 0 {
		t.Fatalf("calls = %v, want none", r.calls)
	}
}
