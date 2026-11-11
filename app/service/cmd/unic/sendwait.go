package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/observability"
)

// Frozen completion budgets.
const (
	pollEvery   = 200 * time.Millisecond
	sessionPoll = time.Second
)

// These are variables only so an e2e build can shorten them; production never changes them.
var (
	// connectWindow is how long after a generation starts that eligible paired devices may still connect
	// before a command sends. It applies to every service, so send semantics do not depend on who started it.
	connectWindow = 10 * time.Second
	sendDeadline  = 60 * time.Second // wait for this command's own send operation
	pairingWindow = 2 * time.Minute  // wait for the other device to approve under a temporary service
)

// current is this invocation's service usage. Nil outside a service command.
var current *session

// incompleteError reports work that was committed but did not finish. It exits 4.
type incompleteError struct{ msg string }

func (e *incompleteError) Error() string { return e.msg }

// connectOpportunity gives paired devices a finite chance to connect before the one dispatch decision.
// Startup readiness does not mean peers are reachable. Offline devices cannot hold the command beyond the window.
func connectOpportunity(ctx context.Context, c *api.Client) {
	// The first request acquires the service, so the generation's start time is known only afterwards.
	st, err := c.Status(ctx)
	if err != nil {
		return
	}
	started, ok := current.startedAt()
	if !ok {
		return
	}
	announced := false
	for {
		waiting := 0
		for _, p := range st.Peers {
			if p.Kind == "" && !p.Connected {
				waiting++
			}
		}
		left := connectWindow - time.Since(started)
		if waiting == 0 || left <= 0 {
			return
		}
		if !announced {
			fmt.Fprintf(os.Stderr, "waiting up to %s for %d paired device(s) to connect\n", left.Round(time.Second), waiting)
			announced = true
		}
		select {
		case <-ctx.Done():
			return
		case <-time.After(pollEvery):
		}
		if st, err = c.Status(ctx); err != nil {
			return
		}
	}
}

// awaitSend observes only the operation this command created. It never replays the submission.
// outcome is success, partial, failed, superseded, timeout, interrupted or none (no operation).
func awaitSend(ctx context.Context, c *api.Client, id engine.OperationID) (outcome string, op *engine.Operation, err error) {
	if id == "" {
		return "none", nil, nil
	}
	wctx, cancel := context.WithTimeout(ctx, sendDeadline)
	defer cancel()
	for {
		// Each poll has its own request deadline; the overall wait is wctx.
		v, e := c.Operation(wctx, string(id))
		switch {
		case e == nil && v.Done():
			out := string(v.Outcome)
			if out == "" {
				out = strings.ToLower(string(v.Phase))
			}
			return out, &v, nil
		case e != nil && wctx.Err() == nil:
			return "unknown", nil, e
		case wctx.Err() != nil:
			if ctx.Err() != nil {
				return "interrupted", nil, nil
			}
			return "timeout", nil, nil
		}
		select {
		case <-wctx.Done():
		case <-time.After(pollEvery):
		}
	}
}

// sendIncomplete converts a non-success outcome to the exit-4 error after output was written.
func sendIncomplete(outcome, request string) error {
	switch outcome {
	case "none", "success", "superseded":
		return nil
	case "unknown":
		return &incompleteError{"committed locally, but the send result could not be read"}
	}
	note := ""
	if outcome == "timeout" || outcome == "interrupted" {
		if current.temporary() {
			note = "; the temporary service is stopping, so queued work and referenced files stay in use until this command exits"
		} else {
			note = "; the running service keeps sending (query: unic dev submission " + request + ")"
		}
	}
	return &incompleteError{fmt.Sprintf("committed locally; send %s%s", outcome, note)}
}

// awaitPairing holds a temporary service while the other device approves.
// A persistent owner returns at once because it keeps the pending request alive.
func awaitPairing(ctx context.Context, c *api.Client, p api.PairResult) error {
	if !p.Awaiting || !current.temporary() {
		return nil
	}
	fmt.Fprintf(os.Stderr, "waiting up to %s for %s to approve this device (Ctrl+C to stop)\n", pairingWindow, p.Name)
	wctx, cancel := context.WithTimeout(ctx, pairingWindow)
	defer cancel()
	for {
		st, err := c.Status(wctx)
		switch {
		case err == nil:
			// A device awaiting approval is listed as pending until both sides agree, then as a peer.
			found := false
			for _, pending := range st.Pending {
				found = found || pending.ID == p.ID
			}
			for _, peer := range st.Peers {
				if peer.ID != p.ID {
					continue
				}
				found = true
				if peer.Connected {
					fmt.Printf("approved: connected to %s (%s)\n", peer.Name, peer.ID)
					return nil
				}
			}
			if !found {
				return &incompleteError{"the pairing request was removed or replaced"}
			}
		case ctx.Err() != nil:
			return &incompleteError{"interrupted; approval is still pending and cannot complete after this temporary service stops"}
		case wctx.Err() != nil:
			return &incompleteError{fmt.Sprintf("approval still pending after %s; run a persistent unicd to wait longer", pairingWindow)}
		case errors.Is(err, api.ErrNoDaemon):
			return &incompleteError{"the service stopped before approval; the request may remain pending"}
		default:
			return err
		}
		select {
		case <-wctx.Done():
		case <-time.After(sessionPoll):
		}
	}
}

// holdInvitation keeps a temporary service alive until the invitation ends.
// Replacement by a newer invitation or expiry ends this command only.
func holdInvitation(ctx context.Context, c *api.Client, code api.Code) error {
	if !current.temporary() {
		return nil
	}
	fmt.Fprintf(os.Stderr, "keeping the temporary service running until %s so another device can join (Ctrl+C to stop).\nStart unicd yourself to keep the code usable after this command.\n", code.Expires().Format("15:04:05"))
	wctx, cancel := context.WithDeadline(ctx, code.Expires().Add(time.Second))
	defer cancel()
	for {
		select {
		case <-wctx.Done():
			return nil
		case <-time.After(sessionPoll):
		}
		now, err := c.Code(wctx)
		if err != nil || now.Code != code.Code {
			return nil // used, cancelled, expired or replaced by a newer invitation
		}
	}
}

// holdOverride keeps a temporary service alive for one log-level override window.
func holdOverride(ctx context.Context, c *api.Client, target string, set observability.LogLevelState) error {
	if !current.temporary() {
		return nil
	}
	state := set.Local
	if target == "remote" {
		state = set.Remote
	}
	if state.Expires == nil {
		return nil
	}
	end := time.UnixMilli(*state.Expires)
	fmt.Fprintf(os.Stderr, "keeping the temporary service running until %s for this %s log override (Ctrl+C to stop).\n", end.Format("15:04:05"), target)
	wctx, cancel := context.WithDeadline(ctx, end)
	defer cancel()
	for {
		select {
		case <-wctx.Done():
			return nil
		case <-time.After(sessionPoll):
		}
		now, err := c.LogLevels(wctx)
		if err != nil {
			return nil
		}
		cur := now.Local
		if target == "remote" {
			cur = now.Remote
		}
		if cur.Expires == nil || *cur.Expires != *state.Expires {
			return nil // reset or replaced by a newer override
		}
	}
}
