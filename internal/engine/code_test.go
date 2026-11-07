package engine

import (
	"context"
	"testing"
	"time"

	"unic/internal/rendezvous"
	"unic/internal/rendezvous/rvtest"
)

func codeNodes(t *testing.T, ids ...string) (*rvtest.Server, []node) {
	t.Helper()
	srv := rvtest.New()
	t.Cleanup(srv.Close)
	var ns []node
	for _, id := range ids {
		ns = append(ns, newNodeWith(t, id, &rendezvous.Client{URL: srv.URL, HTTP: srv.Client()}))
	}
	time.Sleep(20 * time.Millisecond) // let Run install its context
	return srv, ns
}

func TestJoinByCodeStillNeedsApproval(t *testing.T) {
	srv, ns := codeNodes(t, "aaa", "bbb")
	a, b := ns[0], ns[1]
	ctx := context.Background()

	code, err := a.e.IssueCode(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if cur, ok := a.e.CurrentCode(); !ok || cur.Code != code.Code {
		t.Fatalf("CurrentCode = %+v, %v", cur, ok)
	}

	p, awaiting, err := b.e.Join(ctx, code.Display())
	if err != nil {
		t.Fatal(err)
	}
	if !awaiting || p.ID != "aaa" {
		t.Fatalf("Join = %+v awaiting %v; the code must not grant trust", p, awaiting)
	}

	// a sees b as a request that came through its code, and the code is spent.
	ps := pending(a)
	if len(ps) != 1 || ps[0].ID != "bbb" || !ps[0].ByCode {
		t.Fatalf("a's pending = %+v", ps)
	}
	if _, ok := a.e.CurrentCode(); ok {
		t.Fatal("the code is still live after a device used it")
	}
	eventually(t, "code consumed", func() bool { return srv.WasConsumed(code.Code) })

	// Both devices show the same fingerprint.
	if ps[0].Fingerprint != b.e.Fingerprint("aaa") || len(ps[0].Fingerprint) != 19 {
		t.Fatalf("fingerprints differ: a shows %q, b shows %q", ps[0].Fingerprint, b.e.Fingerprint("aaa"))
	}

	// Nothing syncs until a approves.
	time.Sleep(200 * time.Millisecond)
	if connected(a, b) {
		t.Fatal("connected before approval")
	}
	if _, err := a.e.Approve("bbb"); err != nil {
		t.Fatal(err)
	}
	eventually(t, "connected", func() bool { return connected(a, b) })
}

func TestStaleCodeTokenIsJustADirectRequest(t *testing.T) {
	_, ns := codeNodes(t, "aaa", "bbb")
	a, b := ns[0], ns[1]
	ctx := context.Background()

	if _, err := a.e.IssueCode(ctx); err != nil {
		t.Fatal(err)
	}
	// b found a's address some other way and presents a token a never issued.
	_, _, addr := a.e.Self()
	if _, awaiting, err := b.e.addPeer(ctx, addr, rendezvous.NewToken()); err != nil || !awaiting {
		t.Fatalf("addPeer = %v, %v", awaiting, err)
	}
	if ps := pending(a); len(ps) != 1 || ps[0].ByCode {
		t.Fatalf("a's pending = %+v", ps)
	}
	if _, ok := a.e.CurrentCode(); !ok {
		t.Fatal("a wrong token used up the code")
	}
}

func TestNewCodeReplacesTheOld(t *testing.T) {
	srv, ns := codeNodes(t, "aaa", "bbb")
	a, b := ns[0], ns[1]
	ctx := context.Background()

	first, err := a.e.IssueCode(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := a.e.IssueCode(ctx); err != nil {
		t.Fatal(err)
	}
	eventually(t, "old code consumed", func() bool { return srv.WasConsumed(first.Code) })
	if _, _, err := b.e.Join(ctx, first.Code); rendezvous.ReasonOf(err) != rendezvous.Expired {
		t.Fatalf("Join with replaced code: %v", err)
	}
	a.e.CancelCode()
	if _, ok := a.e.CurrentCode(); ok {
		t.Fatal("code still live after CancelCode")
	}
}

func TestOwnCodeIsRefused(t *testing.T) {
	_, ns := codeNodes(t, "aaa")
	a := ns[0]
	code, err := a.e.IssueCode(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := a.e.Join(context.Background(), code.Code); err == nil {
		t.Fatal("joined its own code")
	}
}

func TestFingerprintIsSymmetric(t *testing.T) {
	if Fingerprint("aaa", "bbb") != Fingerprint("bbb", "aaa") {
		t.Fatal("fingerprint depends on the side")
	}
	if Fingerprint("aaa", "bbb") == Fingerprint("aaa", "ccc") {
		t.Fatal("fingerprint ignores the peer")
	}
}
