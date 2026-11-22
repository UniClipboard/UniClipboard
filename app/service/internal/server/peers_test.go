package server

import (
	"context"
	"encoding/json"
	"net/http"
	"path/filepath"
	"strings"
	"testing"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type peerFake struct {
	fake
	renameCalls  int
	renameID     string
	renameIn     PeerRename
	renameRes    PeerStatus
	renameErr    error
	unpairCalls  int
	unpairID     string
	unpairErr    error
	unpairAtCall int
	unpairAtID   string
	unpairAtRev  int64
	unpairAtErr  error
}

func (f *peerFake) RenamePeer(_ context.Context, id string, in PeerRename) (PeerStatus, error) {
	f.renameCalls++
	f.renameID, f.renameIn = id, in
	if f.renameErr != nil {
		return PeerStatus{}, f.renameErr
	}
	res := f.renameRes
	if res.ID == "" {
		res.ID = id
	}
	if res.Revision == 0 {
		res.Revision = in.Revision + 1
	}
	res.UserName = in.Name
	if in.Name != "" {
		res.Name = in.Name
	} else {
		res.Name = res.ReportedName
	}
	return res, nil
}

func (f *peerFake) Unpair(_ context.Context, id string) error {
	f.unpairCalls++
	f.unpairID = id
	return f.unpairErr
}

func (f *peerFake) UnpairAt(_ context.Context, id string, revision int64) error {
	f.unpairAtCall++
	f.unpairAtID, f.unpairAtRev = id, revision
	return f.unpairAtErr
}

func decodeErrorBody(t *testing.T, raw []byte) errBody {
	t.Helper()
	var body errBody
	if err := json.Unmarshal(raw, &body); err != nil {
		t.Fatalf("unmarshal error body %q: %v", string(raw), err)
	}
	return body
}

func TestPeerRenameRouteValidationAndDispatch(t *testing.T) {
	f := &peerFake{
		renameRes: PeerStatus{
			ID:           "dev-1",
			ReportedName: "Host-A",
			Fingerprint:  "AAAA BBBB CCCC DDDD",
		},
	}
	h := Handler(f, config.Static(config.Default()))

	rec := do(t, h, "PATCH", "/v1/peers/dev-1", `{"revision":3,"name":"Work Laptop"}`)
	if rec.Code != 200 {
		t.Fatalf("rename = %d %s", rec.Code, rec.Body)
	}
	if f.renameCalls != 1 || f.renameID != "dev-1" || f.renameIn.Revision != 3 || f.renameIn.Name != "Work Laptop" {
		t.Fatalf("unexpected call: calls=%d id=%q in=%+v", f.renameCalls, f.renameID, f.renameIn)
	}
	var got PeerStatus
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatal(err)
	}
	if got.Name != "Work Laptop" || got.UserName != "Work Laptop" || got.ReportedName != "Host-A" || got.Revision != 4 {
		t.Fatalf("got = %+v", got)
	}

	// Clearing custom name with explicit "".
	rec = do(t, h, "PATCH", "/v1/peers/dev-1", `{"revision":4,"name":""}`)
	if rec.Code != 200 {
		t.Fatalf("clear rename = %d %s", rec.Code, rec.Body)
	}
	if f.renameIn.Revision != 4 || f.renameIn.Name != "" {
		t.Fatalf("clear call = %+v", f.renameIn)
	}
	if strings.Contains(rec.Body.String(), `"user_name"`) {
		t.Fatalf("cleared user_name should be omitted: %s", rec.Body)
	}
	var cleared PeerStatus
	if err := json.Unmarshal(rec.Body.Bytes(), &cleared); err != nil {
		t.Fatal(err)
	}
	if cleared.Name != "Host-A" || cleared.UserName != "" || cleared.ReportedName != "Host-A" || cleared.Revision != 5 {
		t.Fatalf("cleared = %+v", cleared)
	}

	// Reject malformed / incomplete / unknown / trailing / oversized bodies.
	beforeCalls := f.renameCalls
	oversized := `{"revision":5,"name":"` + strings.Repeat("a", 4096) + `"}`
	badBodies := []string{
		"",
		"{",
		`{"revision":5}`,
		`{"revision":5,"name":null}`,
		`{"name":"Work"}`,
		`{"revision":0,"name":"Work"}`,
		`{"revision":-1,"name":"Work"}`,
		`{"revision":5,"name":"Work","extra":true}`,
		`{"revision":5,"name":"Work"} {}`,
		`{"revision":5,"name":"Work"} trailing`,
		oversized,
	}
	for _, body := range badBodies {
		rec := do(t, h, "PATCH", "/v1/peers/dev-1", body)
		if rec.Code != 400 {
			t.Fatalf("body %q: status = %d, want 400 (%s)", body[:min(len(body), 64)], rec.Code, rec.Body)
		}
		if eb := decodeErrorBody(t, rec.Body.Bytes()); eb.Reason != "invalid_argument" {
			t.Fatalf("body %q: reason = %q, want invalid_argument", body[:min(len(body), 64)], eb.Reason)
		}
	}
	if f.renameCalls != beforeCalls {
		t.Fatalf("invalid bodies invoked RenamePeer: calls %d -> %d", beforeCalls, f.renameCalls)
	}

	// Propagate backend error status and reason.
	for _, tc := range []struct {
		status int
		reason string
	}{
		{409, "conflict"},
		{404, "not_found"},
		{400, "invalid_argument"},
	} {
		f.renameErr = &Error{Status: tc.status, Reason: tc.reason, Message: tc.reason}
		rec := do(t, h, "PATCH", "/v1/peers/dev-1", `{"revision":5,"name":"Ok"}`)
		if rec.Code != tc.status {
			t.Fatalf("status = %d, want %d (%s)", rec.Code, tc.status, rec.Body)
		}
		if eb := decodeErrorBody(t, rec.Body.Bytes()); eb.Reason != tc.reason {
			t.Fatalf("reason = %q, want %q", eb.Reason, tc.reason)
		}
	}

	// Unsupported backend returns 501.
	hNoRename := Handler(&fake{}, config.Static(config.Default()))
	rec = do(t, hNoRename, "PATCH", "/v1/peers/dev-1", `{"revision":1,"name":"Ok"}`)
	if rec.Code != 501 {
		t.Fatalf("unsupported rename = %d %s", rec.Code, rec.Body)
	}
	if eb := decodeErrorBody(t, rec.Body.Bytes()); eb.Reason != "unsupported" {
		t.Fatalf("unsupported reason = %q, want unsupported", eb.Reason)
	}
}

func TestPeerUnpairRouteCASAndLegacy(t *testing.T) {
	f := &peerFake{}
	h := Handler(f, config.Static(config.Default()))

	// Omitted revision preserves legacy Unpair.
	rec := do(t, h, "DELETE", "/v1/peers/dev-1", "")
	if rec.Code != 200 || f.unpairCalls != 1 || f.unpairID != "dev-1" || f.unpairAtCall != 0 {
		t.Fatalf("legacy unpair = %d calls=%d/%d id=%q", rec.Code, f.unpairCalls, f.unpairAtCall, f.unpairID)
	}

	// Invalid / non-positive revision query returns 400 without calling backend.
	for _, q := range []string{"0", "-1", "abc", "1.5"} {
		rec := do(t, h, "DELETE", "/v1/peers/dev-1?revision="+q, "")
		if rec.Code != 400 {
			t.Fatalf("revision=%s: status = %d, want 400 (%s)", q, rec.Code, rec.Body)
		}
		if eb := decodeErrorBody(t, rec.Body.Bytes()); eb.Reason != "invalid_argument" {
			t.Fatalf("revision=%s: reason = %q, want invalid_argument", q, eb.Reason)
		}
	}
	if f.unpairCalls != 1 || f.unpairAtCall != 0 {
		t.Fatalf("invalid revision queries invoked backend: %d/%d", f.unpairCalls, f.unpairAtCall)
	}

	// Valid revision calls UnpairAt.
	rec = do(t, h, "DELETE", "/v1/peers/dev-1?revision=7", "")
	if rec.Code != 200 || f.unpairAtCall != 1 || f.unpairAtID != "dev-1" || f.unpairAtRev != 7 {
		t.Fatalf("cas unpair = %d calls=%d id=%q rev=%d", rec.Code, f.unpairAtCall, f.unpairAtID, f.unpairAtRev)
	}

	// Conflict and NotFound statuses flow through fail(w, 500, err).
	f.unpairAtErr = &Error{Status: 409, Reason: "conflict", Message: "stale"}
	rec = do(t, h, "DELETE", "/v1/peers/dev-1?revision=6", "")
	if rec.Code != 409 || decodeErrorBody(t, rec.Body.Bytes()).Reason != "conflict" {
		t.Fatalf("conflict = %d %s", rec.Code, rec.Body)
	}
	f.unpairAtErr = &Error{Status: 404, Reason: "not_found", Message: "missing"}
	rec = do(t, h, "DELETE", "/v1/peers/dev-1?revision=6", "")
	if rec.Code != 404 || decodeErrorBody(t, rec.Body.Bytes()).Reason != "not_found" {
		t.Fatalf("not_found = %d %s", rec.Code, rec.Body)
	}

	// Unsupported UnpairAt returns 501 when ?revision= is provided, while legacy still works.
	hNoCAS := Handler(&legacyOnlyFake{}, config.Static(config.Default()))
	rec = do(t, hNoCAS, "DELETE", "/v1/peers/dev-1?revision=2", "")
	if rec.Code != 501 || decodeErrorBody(t, rec.Body.Bytes()).Reason != "unsupported" {
		t.Fatalf("unsupported cas unpair = %d %s", rec.Code, rec.Body)
	}
	rec = do(t, hNoCAS, "DELETE", "/v1/peers/dev-1", "")
	if rec.Code != 200 {
		t.Fatalf("legacy fallback = %d %s", rec.Code, rec.Body)
	}
}

type legacyOnlyFake struct{ fake }

func (f *legacyOnlyFake) Unpair(context.Context, string) error { return nil }

func TestPeerClientUnixSocketRoundTrip(t *testing.T) {
	f := &peerFake{
		renameRes: PeerStatus{
			ReportedName: "Reported",
			Fingerprint:  "1111 2222 3333 4444",
		},
	}
	sock := filepath.Join(t.TempDir(), "u.sock")
	ln, err := Listen(sock)
	if err != nil {
		t.Fatal(err)
	}
	srv := &http.Server{Handler: Handler(f, config.Static(config.Default()))}
	go func() { _ = srv.Serve(ln) }()
	t.Cleanup(func() { _ = srv.Close() })

	c := NewClient(sock)
	ctx := context.Background()

	// Include characters that need URL path escaping to verify PathEscape round-trip.
	const peerID = "peer/a b%c"
	got, err := c.RenamePeer(ctx, peerID, PeerRename{Revision: 3, Name: "Desk"})
	if err != nil {
		t.Fatalf("RenamePeer: %v", err)
	}
	if f.renameID != peerID || f.renameIn.Revision != 3 || f.renameIn.Name != "Desk" {
		t.Fatalf("server got id=%q in=%+v", f.renameID, f.renameIn)
	}
	if got.ID != peerID || got.Name != "Desk" || got.UserName != "Desk" || got.ReportedName != "Reported" || got.Revision != 4 {
		t.Fatalf("got = %+v", got)
	}

	// Clearing custom name sends explicit empty string.
	cleared, err := c.RenamePeer(ctx, peerID, PeerRename{Revision: 4, Name: ""})
	if err != nil {
		t.Fatalf("RenamePeer clear: %v", err)
	}
	if cleared.Name != "Reported" || cleared.UserName != "" || cleared.Revision != 5 {
		t.Fatalf("cleared = %+v", cleared)
	}

	// Rename conflict error reason propagates.
	f.renameErr = &Error{Status: 409, Reason: "conflict", Message: "stale"}
	if _, err := c.RenamePeer(ctx, peerID, PeerRename{Revision: 3, Name: "Old"}); api.ReasonOf(err) != "conflict" {
		t.Fatalf("RenamePeer conflict err = %v (reason=%q)", err, api.ReasonOf(err))
	}
	f.renameErr = nil

	// UnpairAt round-trip and error propagation.
	if err := c.UnpairAt(ctx, peerID, 5); err != nil {
		t.Fatalf("UnpairAt: %v", err)
	}
	if f.unpairAtID != peerID || f.unpairAtRev != 5 {
		t.Fatalf("server got id=%q rev=%d", f.unpairAtID, f.unpairAtRev)
	}

	f.unpairAtErr = &Error{Status: 409, Reason: "conflict", Message: "stale"}
	if err := c.UnpairAt(ctx, peerID, 4); api.ReasonOf(err) != "conflict" {
		t.Fatalf("UnpairAt conflict err = %v (reason=%q)", err, api.ReasonOf(err))
	}
}
