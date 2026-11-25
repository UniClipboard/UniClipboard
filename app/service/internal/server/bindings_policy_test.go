package server

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type stubBindingBackend struct {
	fake
	lastID   string
	lastEdit api.BindingEdit
	out      engine.DeviceBinding
	err      error
}

func (s *stubBindingBackend) SetBindingSync(_ context.Context, id string, in api.BindingEdit) (engine.DeviceBinding, error) {
	s.lastID = id
	s.lastEdit = in
	return s.out, s.err
}

func (s *stubBindingBackend) RemoveBinding(_ context.Context, _ string, _ int64) error {
	return s.err
}

func TestPatchBindingPolicyRoutes(t *testing.T) {
	wantPol := engine.BindingSyncPolicy{
		Text:   engine.SyncDirectionBidirectional,
		Images: engine.SyncDirectionSendOnly,
		Files:  engine.SyncDirectionDisabled,
	}
	b := &stubBindingBackend{
		out: engine.DeviceBinding{
			ID:          "b-1",
			InstanceID:  "builtin.tailcat",
			State:       engine.BindingAuthorized,
			SyncEnabled: true,
			Policy:      wantPol,
			Revision:    3,
		},
	}
	mux := http.NewServeMux()
	bindingRoutes(mux, b)

	// Policy-only PATCH succeeds without requiring enabled.
	body := `{"revision":2,"policy":{"text":"bidirectional","images":"send_only","files":"disabled"}}`
	rec := httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodPatch, "/v1/bindings/b-1", bytes.NewBufferString(body))
	mux.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("policy-only PATCH status = %d, body = %s", rec.Code, rec.Body.String())
	}
	if b.lastID != "b-1" || b.lastEdit.Revision != 2 || b.lastEdit.Enabled != nil || b.lastEdit.Policy == nil || *b.lastEdit.Policy != wantPol {
		t.Fatalf("unexpected backend edit: id=%q edit=%+v", b.lastID, b.lastEdit)
	}
	var got engine.DeviceBinding
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil || got.Policy != wantPol {
		t.Fatalf("response = %+v, err=%v", got, err)
	}

	// Invalid direction is rejected with 400.
	recBad := httptest.NewRecorder()
	mux.ServeHTTP(recBad, httptest.NewRequest(http.MethodPatch, "/v1/bindings/b-1",
		bytes.NewBufferString(`{"revision":3,"policy":{"text":"invalid","images":"bidirectional","files":"bidirectional"}}`)))
	if recBad.Code != http.StatusBadRequest {
		t.Fatalf("invalid policy status = %d, want 400", recBad.Code)
	}

	// Empty edit (neither enabled nor policy) is rejected with 400.
	recEmpty := httptest.NewRecorder()
	mux.ServeHTTP(recEmpty, httptest.NewRequest(http.MethodPatch, "/v1/bindings/b-1",
		bytes.NewBufferString(`{"revision":3}`)))
	if recEmpty.Code != http.StatusBadRequest {
		t.Fatalf("empty edit status = %d, want 400", recEmpty.Code)
	}
}
