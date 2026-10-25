package main

import (
	"errors"
	"sync"
	"sync/atomic"
	"testing"
)

func TestExitIntentPreservesFirstDecision(t *testing.T) {
	for _, keep := range []bool{false, true} {
		var e exitIntent
		if !e.begin(keep) || e.begin(!keep) || e.keepDaemon.Load() != keep {
			t.Fatalf("later exit changed keep-daemon=%t", keep)
		}
		started, err := e.restart(func() error { t.Fatal("restarted after quit"); return nil })
		if started || err != nil {
			t.Fatal(started, err)
		}
	}
}

func TestFailedRestartLeavesHostUsable(t *testing.T) {
	var e exitIntent
	failure := errors.New("launch failed")
	started, err := e.restart(func() error { return failure })
	if started || err != failure || e.quitting.Load() {
		t.Fatal(started, err)
	}
	started, err = e.restart(func() error { return nil })
	if !started || err != nil || !e.keepDaemon.Load() || !e.quitting.Load() {
		t.Fatal(started, err)
	}
	if e.begin(false) {
		t.Fatal("later quit overwrote committed restart")
	}
}

func TestConcurrentRestartStartsExactlyOneReplacement(t *testing.T) {
	var e exitIntent
	var launched atomic.Int32
	var wg sync.WaitGroup
	for range 32 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := e.restart(func() error { launched.Add(1); return nil })
			if err != nil {
				t.Error(err)
			}
		}()
	}
	wg.Wait()
	if launched.Load() != 1 || !e.keepDaemon.Load() {
		t.Fatal(launched.Load())
	}
}

func TestConcurrentQuitAndRestartHaveOneConsistentOutcome(t *testing.T) {
	for range 100 {
		var e exitIntent
		var launched atomic.Int32
		start := make(chan struct{})
		var wg sync.WaitGroup
		wg.Add(2)
		go func() {
			defer wg.Done()
			<-start
			e.begin(false)
		}()
		go func() {
			defer wg.Done()
			<-start
			_, _ = e.restart(func() error { launched.Add(1); return nil })
		}()
		close(start)
		wg.Wait()
		if !e.quitting.Load() || e.keepDaemon.Load() != (launched.Load() == 1) {
			t.Fatal("shutdown decision disagrees with whether a replacement was launched")
		}
	}
}
