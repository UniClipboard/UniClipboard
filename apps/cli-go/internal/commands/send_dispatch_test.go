package commands

import (
	"errors"
	"os"
	"testing"
	"testing/synctest"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

func TestSendVirtualTimeDispatch(t *testing.T) {
	tests := []struct {
		name         string
		budget       uint64
		resend       bool
		setupDelay   time.Duration
		setupCode    int
		onlineAt     time.Duration
		offline      bool
		queryError   bool
		noPeerFlag   bool
		allOffline   bool
		dispatchCode int
		wantCode     int
		wantWait     bool
		elapsed      time.Duration
	}{
		{name: "V01_ready_once", budget: 60, wantWait: true},
		{name: "V02_late_once", budget: 60, wantWait: true, onlineAt: 4 * time.Second, elapsed: 4 * time.Second},
		{name: "V03_timeout_zero_sends", budget: 60, wantWait: true, offline: true, wantCode: exitcode.Error, elapsed: 60 * time.Second},
		{name: "V04_all_targets_late", budget: 60, wantWait: true, onlineAt: 4 * time.Second, elapsed: 4 * time.Second},
		{name: "V06_broadcast_one_online", budget: 60, wantWait: true, noPeerFlag: true, offline: true},
		{name: "V07_broadcast_all_offline", budget: 60, wantWait: true, noPeerFlag: true, allOffline: true, offline: true, wantCode: exitcode.Error, elapsed: 60 * time.Second},
		{name: "V09_query_error_dispatch_error", budget: 60, wantWait: true, queryError: true, dispatchCode: exitcode.Error, wantCode: exitcode.Error},
		{name: "V10_daemon_failure", budget: 60, setupCode: 5, wantCode: 5},
		{name: "V10_lease_failure", budget: 60, setupCode: exitcode.Error, wantCode: exitcode.Error},
		{name: "V11_shared_budget", budget: 60, wantWait: true, setupDelay: 57 * time.Second, offline: true, wantCode: exitcode.Error, elapsed: 60 * time.Second},
		{name: "V14_zero_budget", offline: true},
		{name: "V14_resend", budget: 60, resend: true, offline: true, setupDelay: 2 * time.Second, elapsed: 2 * time.Second},
		{name: "V15_dispatch_failure_no_retry", budget: 60, wantWait: true, dispatchCode: exitcode.Error, wantCode: exitcode.Error},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				start := time.Now()
				requested := []string{"a", "b"}
				if tc.noPeerFlag {
					requested = nil
				}
				plan := newSendWaitPlan(tc.budget, tc.resend, requested)
				deadline := time.Time{}
				if tc.budget > 0 {
					deadline = start.Add(time.Duration(tc.budget) * time.Second)
				}
				if plan.deadline != deadline {
					t.Fatalf("deadline=%v, want %v", plan.deadline, deadline)
				}
				connectCalls, waitCalls, dispatchCalls, releases := 0, 0, 0, 0
				client := &daemonclient.Client{}
				code, cancelled := dispatchSend(plan, make(chan os.Signal), func(got time.Time) preparedDispatch {
					connectCalls++
					if got != deadline {
						t.Errorf("connect deadline=%v, want %v", got, deadline)
					}
					time.Sleep(tc.setupDelay)
					return preparedDispatch{client: client, release: func() { releases++ }, code: tc.setupCode}
				}, func(peers []string, got time.Time, budget uint64) int {
					waitCalls++
					if got != deadline || budget != tc.budget || len(peers) != len(requested) || (len(peers) > 0 && (peers[0] != "a" || peers[1] != "b")) {
						t.Errorf("wait inputs differ from original plan")
					}
					reason := pollSendTargets(peers, got, func() ([]peerSnapshot, error) {
						if tc.queryError {
							return nil, errors.New("query failed")
						}
						ready := !tc.offline && time.Since(start) >= tc.onlineAt
						return []peerSnapshot{{PeerID: "a", IsPaired: true, Connected: !tc.allOffline}, {PeerID: "b", IsPaired: true, Connected: ready}}, nil
					}, func() {
						if dispatchCalls != 0 {
							t.Error("dispatched while waiting")
						}
					})
					if reason != "" {
						return exitcode.Error
					}
					return exitcode.Success
				}, func(got *daemonclient.Client) int {
					dispatchCalls++
					if got != client || releases != 0 {
						t.Error("client or lease ownership lost before dispatch")
					}
					return tc.dispatchCode
				})
				wantDispatch := 1
				if tc.setupCode != 0 || (tc.offline && tc.wantWait && (!tc.noPeerFlag || tc.allOffline)) {
					wantDispatch = 0
				}
				wantRelease := 1
				if tc.setupCode != 0 {
					wantRelease = 0
				}
				wantWait := 0
				if tc.wantWait {
					wantWait = 1
				}
				if code != tc.wantCode || cancelled || connectCalls != 1 || waitCalls != wantWait || dispatchCalls != wantDispatch || releases != wantRelease || time.Since(start) != tc.elapsed {
					t.Fatalf("code=%d cancelled=%v connect=%d wait=%d dispatch=%d releases=%d elapsed=%v", code, cancelled, connectCalls, waitCalls, dispatchCalls, releases, time.Since(start))
				}
			})
		})
	}
}

func TestSendVirtualTimeCancellation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		interrupts := make(chan os.Signal, 1)
		started := make(chan struct{})
		resume := make(chan struct{})
		done := make(chan struct{})
		go func() { <-started; interrupts <- os.Interrupt }()
		dispatches := 0
		code, cancelled := dispatchSend(newSendWaitPlan(60, false, nil), interrupts, func(time.Time) preparedDispatch {
			close(started)
			<-resume
			close(done)
			return preparedDispatch{code: exitcode.Error}
		}, func([]string, time.Time, uint64) int { t.Error("wait after failed preparation"); return 0 }, func(*daemonclient.Client) int { dispatches++; return 0 })
		if code != exitcode.Error || !cancelled || dispatches != 0 {
			t.Fatalf("code=%d cancelled=%v dispatches=%d", code, cancelled, dispatches)
		}
		close(resume)
		<-done
		synctest.Wait()
	})
}

func TestSendVirtualTimeCancellationWhileWaiting(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		interrupts := make(chan os.Signal, 1)
		waiting := make(chan struct{})
		resume := make(chan struct{})
		go func() { <-waiting; interrupts <- os.Interrupt }()
		dispatches, releases := 0, 0
		code, cancelled := dispatchSend(newSendWaitPlan(60, false, []string{"a"}), interrupts, func(time.Time) preparedDispatch {
			return preparedDispatch{release: func() { releases++ }, code: exitcode.Success}
		}, func([]string, time.Time, uint64) int {
			close(waiting)
			<-resume
			return exitcode.Error
		}, func(*daemonclient.Client) int { dispatches++; return 0 })
		if code != exitcode.Error || !cancelled || dispatches != 0 {
			t.Fatalf("code=%d cancelled=%v dispatches=%d", code, cancelled, dispatches)
		}
		// Preserve the existing background preparation lifetime after cancellation.
		close(resume)
		synctest.Wait()
		if releases != 1 || dispatches != 0 {
			t.Fatalf("releases=%d dispatches=%d", releases, dispatches)
		}
	})
}
