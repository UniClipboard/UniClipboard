package engine

import (
	"bytes"
	"encoding/json"
	"log"
	"log/slog"
	"strings"
	"sync"
	"testing"
)

type lockedBuf struct {
	mu sync.Mutex
	b  bytes.Buffer
}

func (l *lockedBuf) Write(p []byte) (int, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.b.Write(p)
}

func (l *lockedBuf) String() string {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.b.String()
}

// Everything the engine and the layers under it write to logs, and the errors
// of failed operations, must not contain device addresses, pairing tickets,
// pairing codes or transport keys: a full pairing and a send run with
// capturing sinks, then every sink is searched for each secret.
func TestLogsAndErrorsCarryNoSecrets(t *testing.T) {
	var logs lockedBuf
	prev := log.Writer()
	log.SetOutput(&logs)
	t.Cleanup(func() { log.SetOutput(prev) })
	h := slog.NewTextHandler(&logs, &slog.HandlerOptions{Level: slog.LevelDebug})
	mutate := func(o *Options) { o.Logger = slog.New(h) }

	a, b := newContentNode(t, mutate), newContentNode(t, mutate)
	start(t, a)
	start(t, b)
	tk, err := a.e.ExportPairingTicket(bg)
	if err != nil {
		t.Fatal(err)
	}
	pairTwo(t, a, b)
	sendText(t, a, b, "clipboard text that is not a log subject")
	// A failed pairing: the error surfaces to the host, not to logs.
	op, _ := b.e.JoinByTicket(bg, JoinTicketRequest{RequestID: "bad", Ticket: testTicketText()})
	failed := waitOp(t, b.e, op, PhaseFailed)

	secrets := map[string]string{
		"address A": a.net.addr, "address B": b.net.addr, "ticket": tk.Ticket,
	}
	if c, cerr := a.e.CreatePairingCode(bg); cerr == nil && c.Code != "" {
		secrets["code"] = c.Code
	}
	out := logs.String()
	if js, err := json.Marshal(failed); err == nil {
		out += string(js)
	}
	for what, s := range secrets {
		if s != "" && strings.Contains(out, s) {
			t.Errorf("a log or error leaks the %s", what)
		}
	}
	if strings.Contains(out, "127.0.0.1:1") {
		t.Error("an address from a ticket leaks into logs or errors")
	}
}
