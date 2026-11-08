package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/app/desktop/internal/config"
)

// fake is a Backend that records what the handler asked of it.
type fake struct {
	Backend
	approveID, approveFP string
	query                HistoryQuery
	delID                string
	delRev               uint64
	actID, actKey        string
	err                  error
}

func (f *fake) Status(context.Context) (Status, error) { return Status{ID: "me"}, f.err }
func (f *fake) Pending(context.Context) ([]PendingPeer, error) {
	return nil, f.err
}
func (f *fake) Approve(_ context.Context, id, fp string) (PeerStatus, error) {
	f.approveID, f.approveFP = id, fp
	return PeerStatus{ID: id, Name: "n"}, f.err
}

func do(t *testing.T, h http.Handler, method, path, body string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(method, path, strings.NewReader(body))
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

func TestListsAreNeverNull(t *testing.T) {
	h := Handler(&fake{}, config.Static(config.Default()))
	var st map[string]any
	rec := do(t, h, "GET", "/v1/status", "")
	if err := json.Unmarshal(rec.Body.Bytes(), &st); err != nil {
		t.Fatal(err)
	}
	if st["peers"] == nil || st["pending"] == nil {
		t.Fatalf("clients iterate these: %s", rec.Body)
	}
	if rec := do(t, h, "GET", "/v1/pending", ""); strings.TrimSpace(rec.Body.String()) != "[]" {
		t.Fatalf("pending = %s", rec.Body)
	}
}

func TestApproveCarriesTheFingerprintTheUserSaw(t *testing.T) {
	f := &fake{}
	h := Handler(f, config.Static(config.Default()))
	if rec := do(t, h, "POST", "/v1/pending/abc/approve", `{"fingerprint":"AAAA BBBB CCCC DDDD"}`); rec.Code != 200 {
		t.Fatalf("approve = %d %s", rec.Code, rec.Body)
	}
	if f.approveID != "abc" || f.approveFP != "AAAA BBBB CCCC DDDD" {
		t.Fatalf("backend got %q %q", f.approveID, f.approveFP)
	}
	// Old clients send no body: the check is skipped, as before.
	do(t, h, "POST", "/v1/pending/abc/approve", "")
	if f.approveFP != "" {
		t.Fatalf("fingerprint = %q", f.approveFP)
	}
}

func TestBackendChoosesTheStatus(t *testing.T) {
	f := &fake{err: &Error{Message: "the fingerprint does not match", Status: 409}}
	h := Handler(f, config.Static(config.Default()))
	rec := do(t, h, "POST", "/v1/pending/abc/approve", `{"fingerprint":"x"}`)
	if rec.Code != 409 || !strings.Contains(rec.Body.String(), "does not match") {
		t.Fatalf("approve = %d %s", rec.Code, rec.Body)
	}
	f.err = &Error{Message: "expired", Reason: "expired", Status: 404}
	rec = do(t, h, "POST", "/v1/pending/abc/approve", "")
	var body errBody
	json.Unmarshal(rec.Body.Bytes(), &body)
	if rec.Code != 404 || body.Reason != "expired" {
		t.Fatalf("reason lost: %d %+v", rec.Code, body)
	}
}

func (f *fake) ListHistory(_ context.Context, q HistoryQuery) (HistoryPage, error) {
	f.query = q
	return HistoryPage{}, f.err
}
func (f *fake) DeleteHistory(_ context.Context, id string, rev uint64) error {
	f.delID, f.delRev = id, rev
	return f.err
}

func TestHistoryRoutesPassQueryAndNeverReturnNull(t *testing.T) {
	f := &fake{}
	h := Handler(f, config.Static(config.Default()))
	rec := do(t, h, "GET", "/v1/history?limit=5&kind=text&kind=image&tag=a&device=d1&from=10&before=20&cursor=c", "")
	if rec.Code != 200 || !strings.Contains(rec.Body.String(), `"items":[]`) {
		t.Fatalf("%d %s", rec.Code, rec.Body)
	}
	q := f.query
	if q.Limit != 5 || len(q.Kinds) != 2 || q.Tags[0] != "a" || q.DeviceID != "d1" || q.FromMs != 10 || q.BeforeMs != 20 || q.Cursor != "c" {
		t.Fatalf("query = %+v", q)
	}
	if rec := do(t, h, "GET", "/v1/history?limit=x", ""); rec.Code != 400 {
		t.Fatalf("bad limit = %d", rec.Code)
	}
	if rec := do(t, h, "DELETE", "/v1/history/abc", ""); rec.Code != 400 {
		t.Fatalf("delete without revision = %d", rec.Code)
	}
	if rec := do(t, h, "DELETE", "/v1/history/abc?revision=3", ""); rec.Code != 200 || f.delID != "abc" || f.delRev != 3 {
		t.Fatalf("delete = %d %q %d", rec.Code, f.delID, f.delRev)
	}
}

func (f *fake) Actions(context.Context) ([]Action, error) { return nil, f.err }
func (f *fake) RunAction(_ context.Context, id, key string) (HistoryItem, error) {
	f.actID, f.actKey = id, key
	return HistoryItem{ID: "new"}, f.err
}

func TestActionRoutes(t *testing.T) {
	f := &fake{}
	h := Handler(f, config.Static(config.Default()))
	if rec := do(t, h, "GET", "/v1/actions", ""); rec.Code != 200 || !strings.Contains(rec.Body.String(), `"actions":[]`) {
		t.Fatalf("%d %s", rec.Code, rec.Body)
	}
	rec := do(t, h, "POST", "/v1/history/abc/actions/builtin.text/upper", "")
	if rec.Code != 200 || f.actID != "abc" || f.actKey != "builtin.text/upper" || !strings.Contains(rec.Body.String(), `"id":"new"`) {
		t.Fatalf("%d %s %q %q", rec.Code, rec.Body, f.actID, f.actKey)
	}
}
