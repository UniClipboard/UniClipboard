package host

import (
	"context"
	"encoding/json"
	"errors"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/app/service/internal/server"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
)

type failingLeaveBackend struct {
	chanBackend
}

func (f *failingLeaveBackend) Spec() plugin.Spec {
	s := f.chanBackend.Spec()
	s.ID = "test.wx.revoke"
	s.RemoteRevocation = true
	return s
}

func (f *failingLeaveBackend) LeaveChannel(context.Context, string) error {
	return errors.New("secret-backend-rpc-token-must-not-leak")
}

func getStatusHTTP(t *testing.T, h *host) (api.Status, string) {
	t.Helper()
	handler := server.Handler(h.n, h.cfg)
	req := httptest.NewRequest("GET", "/v1/status", nil)
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)
	if rec.Code != 200 {
		t.Fatalf("GET /v1/status = %d %s", rec.Code, rec.Body.String())
	}
	var st api.Status
	if err := json.Unmarshal(rec.Body.Bytes(), &st); err != nil {
		t.Fatalf("unmarshal status: %v (%s)", err, rec.Body.String())
	}
	return st, rec.Body.String()
}

func findPeer(t *testing.T, st api.Status, id string) api.PeerStatus {
	t.Helper()
	for _, p := range st.Peers {
		if p.ID == id {
			return p
		}
	}
	t.Fatalf("peer %q not found in %+v", id, st.Peers)
	return api.PeerStatus{}
}

func assertNoSecretsInStatusJSON(t *testing.T, raw string, extraSecrets ...string) {
	t.Helper()
	lower := strings.ToLower(raw)
	for _, forbidden := range []string{
		`"addr"`,
		`"address"`,
		`"pairing_ticket"`,
		`"remote_context"`,
		`"private_key"`,
		`"credential"`,
		`"rpc_token"`,
		`"token"`,
		"127.0.0.1:",
	} {
		if strings.Contains(lower, forbidden) {
			t.Fatalf("GET /v1/status leaked %q: %s", forbidden, raw)
		}
	}
	for _, secret := range extraSecrets {
		if secret != "" && strings.Contains(raw, secret) {
			t.Fatalf("GET /v1/status leaked secret value: %s", raw)
		}
	}
	var payload struct {
		Peers []map[string]any `json:"peers"`
	}
	if err := json.Unmarshal([]byte(raw), &payload); err != nil {
		t.Fatalf("unmarshal raw status: %v", err)
	}
	allowedPeerKeys := map[string]bool{
		"bindings":          true,
		"id":                true,
		"name":              true,
		"user_name":         true,
		"reported_name":     true,
		"fingerprint":       true,
		"connected":         true,
		"last_seen":         true,
		"kind":              true,
		"backend_id":        true,
		"enabled":           true,
		"auto_apply":        true,
		"remote_revocation": true,
		"revision":          true,
		"last_error":        true,
	}
	for _, peer := range payload.Peers {
		for k := range peer {
			if !allowedPeerKeys[k] {
				t.Fatalf("unexpected peer JSON field %q in %s", k, raw)
			}
		}
	}
}

func TestStatusPeerProjectionTrustedDevice(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)

	ticketA := addr(t, a)
	ticketB := addr(t, b)

	dev, err := a.n.e.GetDevice(bg, b.id)
	if err != nil {
		t.Fatal(err)
	}
	if dev.Fingerprint == "" || dev.Revision == 0 || dev.ReportedName == "" {
		t.Fatalf("unexpected device fixture: %+v", dev)
	}

	st, raw := getStatusHTTP(t, a)
	assertNoSecretsInStatusJSON(t, raw, ticketA, ticketB)

	p := findPeer(t, st, b.id)
	if p.Fingerprint != dev.Fingerprint {
		t.Fatalf("fingerprint = %q, want %q", p.Fingerprint, dev.Fingerprint)
	}
	if p.Revision != dev.Revision || p.Revision == 0 {
		t.Fatalf("revision = %d, want non-zero %d", p.Revision, dev.Revision)
	}
	if p.UserName != "" {
		t.Fatalf("user_name = %q, want empty before rename", p.UserName)
	}
	if p.ReportedName != dev.ReportedName {
		t.Fatalf("reported_name = %q, want %q", p.ReportedName, dev.ReportedName)
	}
	if p.Name != dev.DisplayName() || p.Name != dev.ReportedName {
		t.Fatalf("name = %q, want DisplayName() %q", p.Name, dev.DisplayName())
	}
	if strings.Contains(raw, `"user_name"`) || strings.Contains(raw, `"last_error"`) {
		t.Fatalf("empty omitempty fields should be omitted: %s", raw)
	}

	renamed, err := a.n.e.RenameDevice(bg, engine.RenameDeviceRequest{
		ID:               b.id,
		ExpectedRevision: dev.Revision,
		Name:             "Work Laptop",
	})
	if err != nil {
		t.Fatal(err)
	}

	st, raw = getStatusHTTP(t, a)
	assertNoSecretsInStatusJSON(t, raw, ticketA, ticketB)

	p = findPeer(t, st, b.id)
	if p.Name != "Work Laptop" {
		t.Fatalf("name after rename = %q, want %q", p.Name, "Work Laptop")
	}
	if p.UserName != "Work Laptop" {
		t.Fatalf("user_name after rename = %q, want %q", p.UserName, "Work Laptop")
	}
	if p.ReportedName != dev.ReportedName {
		t.Fatalf("reported_name after rename = %q, want %q", p.ReportedName, dev.ReportedName)
	}
	if p.Fingerprint != dev.Fingerprint {
		t.Fatalf("fingerprint after rename = %q, want %q", p.Fingerprint, dev.Fingerprint)
	}
	if p.Revision != renamed.Revision || p.Revision <= dev.Revision {
		t.Fatalf("revision after rename = %d, want %d (> %d)", p.Revision, renamed.Revision, dev.Revision)
	}
}

func TestStatusPeerProjectionExternalChannel(t *testing.T) {
	fb := &failingLeaveBackend{}
	h := newHostWith(t, false, []engine.PluginInstance{{Plugin: fb}})
	settle(h)

	const remoteCtxSecret = "secret-remote-context-group-999"
	ch, err := h.n.e.AddChannel(bg, engine.AddChannelRequest{
		BackendID:     "test.wx.revoke",
		RemoteContext: []byte(remoteCtxSecret),
		ReportedName:  "WeType Group",
	})
	if err != nil {
		t.Fatal(err)
	}

	st, raw := getStatusHTTP(t, h)
	assertNoSecretsInStatusJSON(t, raw, remoteCtxSecret)

	p := findPeer(t, st, ch.ID)
	if p.Kind != "external" || p.BackendID != "test.wx.revoke" || !p.RemoteRevocation {
		t.Fatalf("channel projection = %+v", p)
	}
	if p.Name != "WeType Group" || p.UserName != "" || p.ReportedName != "WeType Group" {
		t.Fatalf("names = name:%q user:%q reported:%q", p.Name, p.UserName, p.ReportedName)
	}
	if p.Fingerprint != "" || p.LastError != "" || p.Revision == 0 {
		t.Fatalf("unexpected channel fields: %+v", p)
	}

	custom := "My WeType"
	updated, err := h.n.UpdateChannel(bg, ch.ID, api.ChannelUpdate{Revision: p.Revision, Name: &custom})
	if err != nil {
		t.Fatal(err)
	}
	if updated.Name != custom || updated.UserName != custom || updated.ReportedName != "WeType Group" {
		t.Fatalf("updated channel = %+v", updated)
	}

	// Failing remote leave records stable LastError "remove_failed" without leaking raw backend text.
	if err := h.n.RemoveChannel(bg, ch.ID, updated.Revision); err == nil {
		t.Fatal("expected RemoveChannel failure from failingLeaveBackend")
	}

	st, raw = getStatusHTTP(t, h)
	assertNoSecretsInStatusJSON(t, raw, remoteCtxSecret, "secret-backend-rpc-token-must-not-leak")

	p = findPeer(t, st, ch.ID)
	if p.Name != custom || p.UserName != custom || p.ReportedName != "WeType Group" {
		t.Fatalf("names after rename/remove failure = name:%q user:%q reported:%q", p.Name, p.UserName, p.ReportedName)
	}
	if p.LastError != "remove_failed" {
		t.Fatalf("last_error = %q, want %q", p.LastError, "remove_failed")
	}
	if p.Enabled {
		t.Fatalf("channel should be disabled after failed remote leave: %+v", p)
	}
	if p.Fingerprint != "" {
		t.Fatalf("external channel must not expose fingerprint: %q", p.Fingerprint)
	}
}
