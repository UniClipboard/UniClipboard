package host

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/app/service/internal/server"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func doHostHTTP(t *testing.T, h *host, method, path, body string) *httptest.ResponseRecorder {
	t.Helper()
	handler := server.Handler(h.n, h.cfg)
	req := httptest.NewRequest(method, path, strings.NewReader(body))
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)
	return rec
}

func decodeAPIError(t *testing.T, rec *httptest.ResponseRecorder) api.ErrorBody {
	t.Helper()
	var eb api.ErrorBody
	if err := json.Unmarshal(rec.Body.Bytes(), &eb); err != nil {
		t.Fatalf("unmarshal error body %q: %v", rec.Body.String(), err)
	}
	return eb
}

func TestPeerRenameEndToEnd(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)

	st, _ := getStatusHTTP(t, a)
	orig := findPeer(t, st, b.id)
	if orig.Revision == 0 || orig.ReportedName == "" || orig.Fingerprint == "" {
		t.Fatalf("unexpected initial peer: %+v", orig)
	}

	// Set custom name.
	rec := doHostHTTP(t, a, "PATCH", "/v1/peers/"+b.id, fmt.Sprintf(`{"revision":%d,"name":"Work Laptop"}`, orig.Revision))
	if rec.Code != 200 {
		t.Fatalf("PATCH rename = %d %s", rec.Code, rec.Body.String())
	}
	var renamed api.PeerStatus
	if err := json.Unmarshal(rec.Body.Bytes(), &renamed); err != nil {
		t.Fatal(err)
	}
	if renamed.Name != "Work Laptop" || renamed.UserName != "Work Laptop" {
		t.Fatalf("renamed names = name:%q user:%q", renamed.Name, renamed.UserName)
	}
	if renamed.ReportedName != orig.ReportedName {
		t.Fatalf("reported_name = %q, want %q", renamed.ReportedName, orig.ReportedName)
	}
	if renamed.Fingerprint != orig.Fingerprint {
		t.Fatalf("fingerprint = %q, want %q", renamed.Fingerprint, orig.Fingerprint)
	}
	if renamed.Revision <= orig.Revision {
		t.Fatalf("revision = %d, want > %d", renamed.Revision, orig.Revision)
	}

	// Stale revision returns 409 conflict.
	rec = doHostHTTP(t, a, "PATCH", "/v1/peers/"+b.id, fmt.Sprintf(`{"revision":%d,"name":"Stale"}`, orig.Revision))
	if rec.Code != 409 {
		t.Fatalf("stale rename = %d %s, want 409", rec.Code, rec.Body.String())
	}
	if eb := decodeAPIError(t, rec); eb.Reason != "conflict" {
		t.Fatalf("stale rename reason = %q, want conflict", eb.Reason)
	}

	// Validation rejections return 400 invalid_argument.
	invalidBodies := []string{
		fmt.Sprintf(`{"revision":%d}`, renamed.Revision),
		`{"name":"NoRev"}`,
		`{"revision":0,"name":"ZeroRev"}`,
		fmt.Sprintf(`{"revision":%d,"name":"Extra","unknown":1}`, renamed.Revision),
		fmt.Sprintf(`{"revision":%d,"name":"Trail"} {}`, renamed.Revision),
		fmt.Sprintf(`{"revision":%d,"name":"line\nbreak"}`, renamed.Revision),
		fmt.Sprintf(`{"revision":%d,"name":"nul\u0000char"}`, renamed.Revision),
		fmt.Sprintf(`{"revision":%d,"name":"%s"}`, renamed.Revision, strings.Repeat("x", 65)),
		fmt.Sprintf(`{"revision":%d,"name":" leading"}`, renamed.Revision),
	}
	for _, body := range invalidBodies {
		rec := doHostHTTP(t, a, "PATCH", "/v1/peers/"+b.id, body)
		if rec.Code != 400 {
			t.Fatalf("invalid body %q = %d %s, want 400", body, rec.Code, rec.Body.String())
		}
		if eb := decodeAPIError(t, rec); eb.Reason != "invalid_argument" {
			t.Fatalf("invalid body %q reason = %q, want invalid_argument", body, eb.Reason)
		}
	}

	// Clear custom name with "" reverts name to reported_name and omits user_name.
	rec = doHostHTTP(t, a, "PATCH", "/v1/peers/"+b.id, fmt.Sprintf(`{"revision":%d,"name":""}`, renamed.Revision))
	if rec.Code != 200 {
		t.Fatalf("PATCH clear = %d %s", rec.Code, rec.Body.String())
	}
	if strings.Contains(rec.Body.String(), `"user_name"`) {
		t.Fatalf("cleared user_name must be omitted from JSON: %s", rec.Body.String())
	}
	var cleared api.PeerStatus
	if err := json.Unmarshal(rec.Body.Bytes(), &cleared); err != nil {
		t.Fatal(err)
	}
	if cleared.Name != orig.ReportedName || cleared.UserName != "" || cleared.ReportedName != orig.ReportedName {
		t.Fatalf("cleared = %+v, want reverted to %q", cleared, orig.ReportedName)
	}
	if cleared.Revision <= renamed.Revision {
		t.Fatalf("cleared revision = %d, want > %d", cleared.Revision, renamed.Revision)
	}

	st, raw := getStatusHTTP(t, a)
	p := findPeer(t, st, b.id)
	if p.Name != orig.ReportedName || p.UserName != "" || p.Revision != cleared.Revision {
		t.Fatalf("status after clear = %+v", p)
	}
	if strings.Contains(raw, `"user_name"`) {
		t.Fatalf("status JSON should omit empty user_name: %s", raw)
	}

	// Unknown peer returns 404 not_found.
	rec = doHostHTTP(t, a, "PATCH", "/v1/peers/no-such-device", `{"revision":1,"name":"Ghost"}`)
	if rec.Code != 404 {
		t.Fatalf("unknown peer rename = %d %s, want 404", rec.Code, rec.Body.String())
	}
	if eb := decodeAPIError(t, rec); eb.Reason != "not_found" {
		t.Fatalf("unknown peer reason = %q, want not_found", eb.Reason)
	}
}

func TestPeerUnpairCASEndToEnd(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)

	st, _ := getStatusHTTP(t, a)
	orig := findPeer(t, st, b.id)

	// Bump revision via rename so orig.Revision becomes stale.
	updated, err := a.n.RenamePeer(bg, b.id, api.PeerRename{Revision: orig.Revision, Name: "Renamed"})
	if err != nil {
		t.Fatal(err)
	}

	// Invalid or zero revision query returns 400 and does not unpair.
	for _, q := range []string{"0", "-2", "not-a-number"} {
		rec := doHostHTTP(t, a, "DELETE", "/v1/peers/"+b.id+"?revision="+q, "")
		if rec.Code != 400 {
			t.Fatalf("revision=%s: status = %d %s, want 400", q, rec.Code, rec.Body.String())
		}
		if eb := decodeAPIError(t, rec); eb.Reason != "invalid_argument" {
			t.Fatalf("revision=%s: reason = %q, want invalid_argument", q, eb.Reason)
		}
	}

	// Stale revision returns 409 without unpairing.
	rec := doHostHTTP(t, a, "DELETE", fmt.Sprintf("/v1/peers/%s?revision=%d", b.id, orig.Revision), "")
	if rec.Code != 409 {
		t.Fatalf("stale unpair = %d %s, want 409", rec.Code, rec.Body.String())
	}
	if eb := decodeAPIError(t, rec); eb.Reason != "conflict" {
		t.Fatalf("stale unpair reason = %q, want conflict", eb.Reason)
	}
	st, _ = getStatusHTTP(t, a)
	_ = findPeer(t, st, b.id)

	// Unknown peer returns 404.
	rec = doHostHTTP(t, a, "DELETE", "/v1/peers/no-such-device?revision=1", "")
	if rec.Code != 404 {
		t.Fatalf("unknown unpair = %d %s, want 404", rec.Code, rec.Body.String())
	}
	if eb := decodeAPIError(t, rec); eb.Reason != "not_found" {
		t.Fatalf("unknown unpair reason = %q, want not_found", eb.Reason)
	}

	// Matching revision succeeds and unpairs.
	rec = doHostHTTP(t, a, "DELETE", fmt.Sprintf("/v1/peers/%s?revision=%d", b.id, updated.Revision), "")
	if rec.Code != 200 {
		t.Fatalf("matching unpair = %d %s", rec.Code, rec.Body.String())
	}
	st, _ = getStatusHTTP(t, a)
	for _, p := range st.Peers {
		if p.ID == b.id {
			t.Fatalf("peer %q still listed after CAS unpair", b.id)
		}
	}
}

func TestPeerUnpairWithoutRevisionStillWorks(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)

	rec := doHostHTTP(t, a, "DELETE", "/v1/peers/"+b.id, "")
	if rec.Code != 200 {
		t.Fatalf("legacy DELETE /v1/peers = %d %s", rec.Code, rec.Body.String())
	}
	st, _ := getStatusHTTP(t, a)
	for _, p := range st.Peers {
		if p.ID == b.id {
			t.Fatalf("peer %q still listed after legacy unpair", b.id)
		}
	}
}

func TestPeerClientUnixSocketWithRealHost(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)

	sock := filepath.Join(t.TempDir(), "u.sock")
	ln, err := server.Listen(sock)
	if err != nil {
		t.Fatal(err)
	}
	srv := &http.Server{Handler: server.Handler(a.n, a.cfg)}
	go func() { _ = srv.Serve(ln) }()
	t.Cleanup(func() { _ = srv.Close() })

	client := api.NewClient(sock)
	ctx := context.Background()

	st, err := client.Status(ctx)
	if err != nil {
		t.Fatal(err)
	}
	orig := findPeer(t, st, b.id)

	renamed, err := client.RenamePeer(ctx, b.id, api.PeerRename{Revision: orig.Revision, Name: "Studio Mac"})
	if err != nil {
		t.Fatalf("RenamePeer: %v", err)
	}
	if renamed.Name != "Studio Mac" || renamed.UserName != "Studio Mac" || renamed.ReportedName != orig.ReportedName || renamed.Revision <= orig.Revision {
		t.Fatalf("renamed = %+v", renamed)
	}

	if _, err := client.RenamePeer(ctx, b.id, api.PeerRename{Revision: orig.Revision, Name: "Stale"}); api.ReasonOf(err) != "conflict" {
		t.Fatalf("stale RenamePeer = %v (reason=%q)", err, api.ReasonOf(err))
	}

	cleared, err := client.RenamePeer(ctx, b.id, api.PeerRename{Revision: renamed.Revision, Name: ""})
	if err != nil {
		t.Fatalf("RenamePeer clear: %v", err)
	}
	if cleared.Name != orig.ReportedName || cleared.UserName != "" || cleared.Revision <= renamed.Revision {
		t.Fatalf("cleared = %+v", cleared)
	}

	if err := client.UnpairAt(ctx, b.id, orig.Revision); api.ReasonOf(err) != "conflict" {
		t.Fatalf("stale UnpairAt = %v (reason=%q)", err, api.ReasonOf(err))
	}

	if err := client.UnpairAt(ctx, b.id, cleared.Revision); err != nil {
		t.Fatalf("UnpairAt: %v", err)
	}

	st, err = client.Status(ctx)
	if err != nil {
		t.Fatal(err)
	}
	for _, p := range st.Peers {
		if p.ID == b.id {
			t.Fatalf("peer %q still listed after client UnpairAt", b.id)
		}
	}
}
