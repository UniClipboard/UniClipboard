package engine

import (
	"context"
	"encoding/json"
	"log"
	"log/slog"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/rendezvous"
)

// drainEvents returns everything a subscription delivers until it goes quiet.
func drainEvents(t *testing.T, sub *Subscription) string {
	t.Helper()
	var sb strings.Builder
	for {
		ctx, cancel := timeoutCtx(200 * time.Millisecond)
		ev, err := sub.Next(ctx)
		cancel()
		if err != nil {
			return sb.String()
		}
		js, _ := json.Marshal(ev)
		sb.Write(js)
	}
}

func jsonOf(v any) string { b, _ := json.Marshal(v); return string(b) }

func captureLogs(t *testing.T) (*lockedBuf, func(*Options)) {
	t.Helper()
	var logs lockedBuf
	prev := log.Writer()
	log.SetOutput(&logs)
	t.Cleanup(func() { log.SetOutput(prev) })
	h := slog.NewTextHandler(&logs, &slog.HandlerOptions{Level: slog.LevelDebug})
	return &logs, func(o *Options) { o.Logger = slog.New(h) }
}

func assertNone(t *testing.T, sinks map[string]string, secrets map[string]string) {
	t.Helper()
	for sink, out := range sinks {
		for what, s := range secrets {
			if s != "" && strings.Contains(out, s) {
				t.Errorf("%s carries the %s", sink, what)
			}
		}
	}
}

// Clipboard text is readable only through an explicit content read. A
// sentinel copied, committed, synced, received and left in history must not
// show up in logs, events, operation records, state, history listings or the
// errors of a failing backend.
func TestClipboardBodyOnlyAppearsInContentReads(t *testing.T) {
	logs, mutate := captureLogs(t)
	const body = "BODY-SENTINEL-7f3a9c"
	bad := &fakeBackend{id: "test.bad", pol: allTypes, fail: newErr(CodeUnavailable, "backend unavailable")}
	a := newContentNode(t, func(o *Options) { mutate(o); o.extraBackends = append(o.extraBackends, bad) })
	b := newContentNode(t, mutate)
	start(t, a)
	start(t, b)
	subA, _ := a.e.SubscribeEvents(bg)
	subB, _ := b.e.SubscribeEvents(bg)
	defer func() { _ = subA.Close(); _ = subB.Close() }()
	s := state(t, a.e).Settings
	s.Value.BackendPolicies["test.bad"] = allTypes
	if _, err := a.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value}); err != nil {
		t.Fatal(err)
	}
	pairTwo(t, a, b)

	h := importText(t, a.e, body)
	r, err := a.e.CommitContent(bg, CommitRequest{RequestID: "sentinel", LeaseID: h.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	op := waitSend(t, a.e, r.OperationID)
	waitReceived(t, b.e, 1)
	hist, _ := a.e.ListHistory(bg, ListHistoryRequest{Limit: 50})
	histB, _ := b.e.ListHistory(bg, ListHistoryRequest{Limit: 50})
	_ = a.e.ReleaseContent(bg, h.LeaseID)

	assertNone(t, map[string]string{
		"logs": logs.String(), "events A": drainEvents(t, subA), "events B": drainEvents(t, subB),
		"submission result": jsonOf(r), "operation": jsonOf(op),
		"history A": jsonOf(hist), "history B": jsonOf(histB),
		"state A": jsonOf(state(t, a.e)), "state B": jsonOf(state(t, b.e)),
	}, map[string]string{"clipboard text": body})
	if resultOf(r, "test.bad").Error == nil {
		t.Fatal("the failing backend's error was expected in the result")
	}
}

// A full pairing and send over real backend processes: the transport address,
// the pairing ticket and its token stay out of logs, events, operations and
// state, and a chatty plugin's stderr is not forwarded to the host's stderr.
func TestRealProcessesKeepSecretsOutOfEveryOutput(t *testing.T) {
	logs, mutate := captureLogs(t)
	const noise = "STDERR-SENTINEL-c41d"
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	prev := os.Stderr
	os.Stderr = w
	t.Cleanup(func() { os.Stderr = prev; _ = r.Close() })

	newNode := func() node {
		return newContentNode(t, func(o *Options) {
			mutate(o)
			inst := e2eInstance()
			inst.Process.Env = append(inst.Process.Env, "UNIC_TEST_STDERR="+noise)
			o.Plugins = []PluginInstance{inst}
		})
	}
	na, nb := newNode(), newNode()
	start(t, na)
	start(t, nb)
	subA, _ := na.e.SubscribeEvents(bg)
	subB, _ := nb.e.SubscribeEvents(bg)
	defer func() { _ = subA.Close(); _ = subB.Close() }()
	idA, idB := na.e.ident.DeviceID(), nb.e.ident.DeviceID()

	tk, err := na.e.ExportPairingTicketVia(bg, "e2e.tcp")
	if err != nil {
		t.Fatal(err)
	}
	parsed, ok := rendezvous.ParseText(tk.Ticket)
	if !ok {
		t.Fatal("cannot parse the ticket")
	}
	op, err := nb.e.JoinByTicket(bg, JoinTicketRequest{RequestID: "join", Ticket: tk.Ticket})
	if err != nil {
		t.Fatal(err)
	}
	joined := waitOp(t, nb.e, op, PhaseSucceeded)
	approveDevice(t, na.e, idB)
	approveDevice(t, nb.e, idA)
	var sent Operation
	eventually(t, "delivered", func() bool {
		id, err := submit(na.e, importText(t, na.e, "hello"))
		if err != nil {
			return false
		}
		sent = waitSend(t, na.e, id)
		return len(sent.Targets) == 1 && sent.Targets[0].State == TargetSent && len(received(nb.e)) > 0
	})
	// A failed join surfaces its error to the host; it must not echo the address.
	badOp, _ := nb.e.JoinByTicket(bg, JoinTicketRequest{RequestID: "bad", Ticket: testTicketText()})
	failed := waitOp(t, nb.e, badOp, PhaseFailed)

	_ = w.Close()
	_ = r.SetReadDeadline(time.Now().Add(2 * time.Second)) // a child that inherited the pipe keeps it open
	var stderr strings.Builder
	buf := make([]byte, 4096)
	for {
		n, err := r.Read(buf)
		stderr.Write(buf[:n])
		if err != nil {
			break
		}
	}
	secrets := map[string]string{
		"ticket": tk.Ticket, "ticket address": string(parsed.Access), "ticket token": string(parsed.Token),
		"pairing token (base64)": b64.EncodeToString(parsed.Token),
	}
	assertNone(t, map[string]string{
		"logs": logs.String(), "events A": drainEvents(t, subA), "events B": drainEvents(t, subB),
		"pairing operation": jsonOf(joined), "send operation": jsonOf(sent), "failed operation": jsonOf(failed),
		"state A": jsonOf(state(t, na.e)), "state B": jsonOf(state(t, nb.e)),
		"devices A": jsonOf(must(na.e.ListDevices(bg))), "devices B": jsonOf(must(nb.e.ListDevices(bg))),
		"host stderr": stderr.String(),
	}, secrets)
	if strings.Contains(logs.String(), noise) || strings.Contains(stderr.String(), noise) {
		t.Error("a plugin's stderr reached the host's logs or stderr")
	}
}

func must[T any](v T, err error) T {
	if err != nil {
		panic(err)
	}
	return v
}

func timeoutCtx(d time.Duration) (context.Context, context.CancelFunc) {
	return context.WithTimeout(bg, d)
}
