package mobile

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/cloudauth"
	"github.com/uniclipboard/unic/internal/plugins/tailcat"
)

func TestMobileRelaySettingsSecretPersistenceAndRedaction(t *testing.T) {
	dataDir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	cfgJSON, _ := json.Marshal(map[string]any{
		"data_dir":    dataDir,
		"device_name": "TestPhone",
	})
	eng, err := NewEngine(string(cfgJSON))
	if err != nil {
		t.Fatalf("NewEngine: %v", err)
	}

	if got := eng.ActiveSendCount(); got != 0 {
		t.Fatalf("ActiveSendCount = %d, want 0", got)
	}

	rawState, err := eng.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var st engine.State
	if err := json.Unmarshal([]byte(rawState), &st); err != nil {
		t.Fatal(err)
	}
	val := st.Settings.Value
	val.Sync.Relay.Enabled = true
	val.Sync.Relay.URL = "https://relay.selfhost.example"
	val.Sync.Relay.MaxBytes = 50 << 20

	// Set relay settings + write-only relay_access_key.
	updReq, _ := json.Marshal(map[string]any{
		"expected_revision": st.Settings.Revision,
		"value":             val,
		"relay_access_key":  "my-mobile-secret-key",
	})
	updRaw, err := eng.UpdateSettings(string(updReq))
	if err != nil {
		t.Fatalf("UpdateSettings(set key): %v", err)
	}
	if strings.Contains(updRaw, "my-mobile-secret-key") {
		t.Fatalf("plaintext relay_access_key leaked in UpdateSettings response: %s", updRaw)
	}
	var updSt engine.SettingsState
	if err := json.Unmarshal([]byte(updRaw), &updSt); err != nil {
		t.Fatal(err)
	}
	if !updSt.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("expected HasAccessKey=true, got %+v", updSt.Value.Sync.Relay)
	}
	// Verify mobile-settings.json does NOT contain the plaintext secret.
	savedSettings, err := os.ReadFile(filepath.Join(dataDir, "mobile-settings.json"))
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(savedSettings), "my-mobile-secret-key") {
		t.Fatalf("plaintext relay_access_key leaked into mobile-settings.json")
	}

	// Update an unrelated setting omitting relay_access_key: secret must be preserved.
	val2 := updSt.Value
	val2.DeviceName = "TestPhoneRenamed"
	updReq2, _ := json.Marshal(map[string]any{
		"expected_revision": updSt.Revision,
		"value":             val2,
	})
	updRaw2, err := eng.UpdateSettings(string(updReq2))
	if err != nil {
		t.Fatalf("UpdateSettings(preserve key): %v", err)
	}
	var updSt2 engine.SettingsState
	if err := json.Unmarshal([]byte(updRaw2), &updSt2); err != nil {
		t.Fatal(err)
	}
	if !updSt2.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("expected HasAccessKey preserved across unrelated update")
	}
	_ = eng.Close(nil)

	// Reopen Engine on the same dataDir: persisted secret must reload cleanly.
	eng2, err := NewEngine(string(cfgJSON))
	if err != nil {
		t.Fatalf("reopen NewEngine: %v", err)
	}
	defer func() { _ = eng2.Close(nil) }()

	rawState2, err := eng2.GetState()
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(rawState2, "my-mobile-secret-key") {
		t.Fatalf("plaintext relay_access_key leaked in GetState after reopen")
	}
	var st2 engine.State
	if err := json.Unmarshal([]byte(rawState2), &st2); err != nil {
		t.Fatal(err)
	}
	if !st2.Settings.Value.Sync.Relay.Enabled ||
		st2.Settings.Value.Sync.Relay.URL != "https://relay.selfhost.example" ||
		st2.Settings.Value.Sync.Relay.MaxBytes != 50<<20 ||
		!st2.Settings.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("unexpected reloaded relay settings: %+v", st2.Settings.Value.Sync.Relay)
	}

	// Explicitly clear relay_access_key with "".
	clearReq, _ := json.Marshal(map[string]any{
		"expected_revision": st2.Settings.Revision,
		"value":             st2.Settings.Value,
		"relay_access_key":  "",
	})
	clearRaw, err := eng2.UpdateSettings(string(clearReq))
	if err != nil {
		t.Fatalf("UpdateSettings(clear key): %v", err)
	}
	var clearSt engine.SettingsState
	if err := json.Unmarshal([]byte(clearRaw), &clearSt); err != nil {
		t.Fatal(err)
	}
	if clearSt.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("expected HasAccessKey=false after explicit clear")
	}
}

func TestMobileCloudSessionManagedRelayNonDefaultOriginAndLogout(t *testing.T) {
	dataDir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}

	var mu sync.Mutex
	var authCode string
	var issuedCount int
	var deleteCalled atomic.Int32
	var logoutCalled atomic.Int32
	advertisedRelay := "https://custom-official-relay.example:8443"

	cloudSrv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch r.URL.Path {
		case "/api/auth/token":
			var body map[string]string
			_ = json.NewDecoder(r.Body).Decode(&body)
			mu.Lock()
			authCode = body["code"]
			mu.Unlock()
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token":  "tok-access-secret-1",
				"refresh_token": "tok-refresh-secret-1",
				"expires_in":    3600,
				"user": map[string]any{
					"clerk_user_id": "user_1",
					"email":         "alice@example.com",
				},
			})
		case "/api/v1/device-credentials/relay":
			switch r.Method {
			case http.MethodPost:
				mu.Lock()
				issuedCount++
				idx := issuedCount
				mu.Unlock()
				_ = idx
				w.WriteHeader(http.StatusCreated)
				_ = json.NewEncoder(w).Encode(map[string]any{
					"id":              "dc-mob-1",
					"name":            "TestPhone",
					"key_prefix":      "uk_live_mob1",
					"credential":      "uk_live_mob1_secret_credential_1",
					"relay_url":       advertisedRelay,
					"expires_at_ms":   time.Now().Add(time.Hour).UnixMilli(),
					"rotate_after_ms": time.Now().Add(30 * time.Minute).UnixMilli(),
				})
			case http.MethodDelete:
				deleteCalled.Add(1)
				w.WriteHeader(http.StatusNoContent)
			default:
				http.Error(w, "method", http.StatusMethodNotAllowed)
			}
		case "/api/auth/revoke":
			logoutCalled.Add(1)
			w.WriteHeader(http.StatusNoContent)
		default:
			http.NotFound(w, r)
		}
	}))
	defer cloudSrv.Close()

	cfgJSON, _ := json.Marshal(map[string]any{
		"data_dir":    dataDir,
		"device_name": "TestPhone",
	})
	eng, err := newEngineWithOptions(string(cfgJSON), tailcat.New(), cloudSrv.Client(), nil)
	if err != nil {
		t.Fatalf("newEngineWithOptions: %v", err)
	}
	defer func() { _ = eng.Close(nil) }()

	// Signed-out official mode without manual key rejects official relay enablement.
	rawState, err := eng.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var st engine.State
	if err := json.Unmarshal([]byte(rawState), &st); err != nil {
		t.Fatal(err)
	}
	enVal := st.Settings.Value
	enVal.Sync.Relay.Enabled = true
	enReq, _ := json.Marshal(map[string]any{
		"expected_revision": st.Settings.Revision,
		"value":             enVal,
	})
	guardedRaw, err := eng.UpdateSettings(string(enReq))
	if err != nil {
		t.Fatalf("UpdateSettings while signed out: %v", err)
	}
	var guardedSt engine.SettingsState
	if err := json.Unmarshal([]byte(guardedRaw), &guardedSt); err != nil {
		t.Fatal(err)
	}
	if guardedSt.Value.Sync.Relay.Enabled {
		t.Fatalf("expected signed-out official relay without fallback to stay disabled")
	}

	// FinishCloudLogin completes PKCE exchange via internal/cloudauth, enables official relay,
	// provisions scoped device credential, and preserves sync.relay.url == "" and HasAccessKey == false.
	loginReq, _ := json.Marshal(map[string]any{
		"cloud_url":         cloudSrv.URL,
		"code":              "pkce-code-123",
		"code_verifier":     "pkce-verifier-123456789012345678901234567890",
		"redirect_uri":      "uniclipboard://auth/callback",
		"device_name":       "TestPhone",
		"auto_enable_relay": true,
	})
	statusRaw, err := eng.FinishCloudLogin(string(loginReq))
	if err != nil {
		t.Fatalf("FinishCloudLogin: %v", err)
	}
	mu.Lock()
	gotCode := authCode
	mu.Unlock()
	if gotCode != "pkce-code-123" {
		t.Fatalf("code exchanged = %q, want pkce-code-123", gotCode)
	}
	for _, secret := range []string{"tok-access-secret-1", "tok-refresh-secret-1", "uk_live_mob1_secret_credential_1"} {
		if strings.Contains(statusRaw, secret) {
			t.Fatalf("FinishCloudLogin leaked secret %q", secret)
		}
	}
	var status cloudauth.StatusSnapshot
	if err := json.Unmarshal([]byte(statusRaw), &status); err != nil {
		t.Fatal(err)
	}
	if !status.SignedIn || status.State != cloudauth.SessionStateActive || status.Account != "alice@example.com" || status.RelayURL != advertisedRelay {
		t.Fatalf("unexpected Cloud status after login: %+v", status)
	}

	afterLoginRaw, err := eng.GetState()
	if err != nil {
		t.Fatal(err)
	}
	for _, secret := range []string{"tok-access-secret-1", "tok-refresh-secret-1", "uk_live_mob1_secret_credential_1", advertisedRelay} {
		if strings.Contains(afterLoginRaw, secret) {
			t.Fatalf("GetState leaked %q: %s", secret, afterLoginRaw)
		}
	}
	var afterSt engine.State
	if err := json.Unmarshal([]byte(afterLoginRaw), &afterSt); err != nil {
		t.Fatal(err)
	}
	if !afterSt.Settings.Value.Sync.Relay.Enabled || afterSt.Settings.Value.Sync.Relay.URL != "" || afterSt.Settings.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("unexpected official relay settings after login: %+v", afterSt.Settings.Value.Sync.Relay)
	}
	if _, statErr := os.Stat(filepath.Join(dataDir, "mobile-relay-secret")); !os.IsNotExist(statErr) {
		t.Fatalf("expected no mobile-relay-secret file for Cloud-managed credential, got stat err=%v", statErr)
	}
	savedSettings, err := os.ReadFile(filepath.Join(dataDir, "mobile-settings.json"))
	if err != nil {
		t.Fatal(err)
	}
	for _, leak := range []string{"tok-access-secret-1", "tok-refresh-secret-1", "uk_live_mob1_secret_credential_1", advertisedRelay} {
		if strings.Contains(string(savedSettings), leak) {
			t.Fatalf("mobile-settings.json leaked %q", leak)
		}
	}

	// Host provider only sends Cloud credential to the exact Cloud-advertised HTTPS origin.
	if k, allow := eng.cloudRelay.AccessKey(advertisedRelay); !allow || k != "uk_live_mob1_secret_credential_1" {
		t.Fatalf("AccessKey(advertised) = (%q, %v), want managed credential", k, allow)
	}
	if k, allow := eng.cloudRelay.AccessKey("https://evil.example"); allow || k != "" {
		t.Fatalf("AccessKey(unrelated origin) = (%q, %v), want denied", k, allow)
	}

	// Reopen Engine on same dataDir: Cloud session reloads cleanly with URL still empty.
	_ = eng.Close(nil)
	eng2, err := newEngineWithOptions(string(cfgJSON), tailcat.New(), cloudSrv.Client(), nil)
	if err != nil {
		t.Fatalf("reopen Engine: %v", err)
	}
	defer func() { _ = eng2.Close(nil) }()
	reopenStatusRaw, err := eng2.GetCloudStatus()
	if err != nil {
		t.Fatal(err)
	}
	var reopenStatus cloudauth.StatusSnapshot
	if err := json.Unmarshal([]byte(reopenStatusRaw), &reopenStatus); err != nil {
		t.Fatal(err)
	}
	if !reopenStatus.SignedIn || reopenStatus.RelayURL != advertisedRelay {
		t.Fatalf("reopened Cloud status mismatch: %+v", reopenStatus)
	}
	reopenStateRaw, err := eng2.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var reopenSt engine.State
	if err := json.Unmarshal([]byte(reopenStateRaw), &reopenSt); err != nil {
		t.Fatal(err)
	}
	if !reopenSt.Settings.Value.Sync.Relay.Enabled || reopenSt.Settings.Value.Sync.Relay.URL != "" || reopenSt.Settings.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("reopened settings leaked advertised url/secret: %+v", reopenSt.Settings.Value.Sync.Relay)
	}

	// Switching to self-hosted clears Cloud device credential best-effort and isolates origin.
	shVal := reopenSt.Settings.Value
	shVal.Sync.Relay.URL = "https://selfhost.example"
	shReq, _ := json.Marshal(map[string]any{
		"expected_revision": reopenSt.Settings.Revision,
		"value":             shVal,
		"relay_access_key":  "selfhost-only-key",
	})
	shRaw, err := eng2.UpdateSettings(string(shReq))
	if err != nil {
		t.Fatalf("UpdateSettings(switch to selfhost): %v", err)
	}
	var shSt engine.SettingsState
	if err := json.Unmarshal([]byte(shRaw), &shSt); err != nil {
		t.Fatal(err)
	}
	if !shSt.Value.Sync.Relay.HasAccessKey || shSt.Value.Sync.Relay.URL != "https://selfhost.example" {
		t.Fatalf("unexpected self-hosted state: %+v", shSt.Value.Sync.Relay)
	}
	if k, allow := eng2.cloudRelay.AccessKey("https://selfhost.example"); !allow || k != "selfhost-only-key" {
		t.Fatalf("AccessKey(selfhost) = (%q, %v), want selfhost-only-key", k, allow)
	}
	if k, allow := eng2.cloudRelay.AccessKey(advertisedRelay); allow || k != "" {
		t.Fatalf("AccessKey(advertised after switch) = (%q, %v), want denied", k, allow)
	}

	// Switch back to official mode + LogoutCloud deletes session and disables official relay when unbacked.
	offVal := shSt.Value
	offVal.Sync.Relay.URL = ""
	offVal.Sync.Relay.Enabled = true
	offReq, _ := json.Marshal(map[string]any{
		"expected_revision": shSt.Revision,
		"value":             offVal,
		"relay_access_key":  "",
	})
	offRaw, err := eng2.UpdateSettings(string(offReq))
	if err != nil {
		t.Fatalf("UpdateSettings(back to official): %v", err)
	}
	var offSt engine.SettingsState
	if err := json.Unmarshal([]byte(offRaw), &offSt); err != nil {
		t.Fatal(err)
	}
	if !offSt.Value.Sync.Relay.Enabled {
		t.Fatalf("expected official relay re-enabled while signed in")
	}

	outRaw, err := eng2.LogoutCloud()
	if err != nil {
		t.Fatalf("LogoutCloud: %v", err)
	}
	var outStatus cloudauth.StatusSnapshot
	if err := json.Unmarshal([]byte(outRaw), &outStatus); err != nil {
		t.Fatal(err)
	}
	if outStatus.SignedIn || outStatus.State != "signed_out" {
		t.Fatalf("unexpected Cloud status after logout: %+v", outStatus)
	}
	if deleteCalled.Load() == 0 || logoutCalled.Load() == 0 {
		t.Fatalf("expected Cloud revoke/logout calls, got delete=%d logout=%d", deleteCalled.Load(), logoutCalled.Load())
	}
	finalStateRaw, err := eng2.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var finalSt engine.State
	if err := json.Unmarshal([]byte(finalStateRaw), &finalSt); err != nil {
		t.Fatal(err)
	}
	if finalSt.Settings.Value.Sync.Relay.Enabled {
		t.Fatalf("expected official relay disabled after LogoutCloud without manual fallback")
	}
}

func TestMobileCloudOutageHoldNonBlockingRotationAndRevocation(t *testing.T) {
	dataDir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}

	var mu sync.Mutex
	now := time.Unix(1700000000, 0)
	nowFn := func() time.Time {
		mu.Lock()
		defer mu.Unlock()
		return now
	}
	advance := func(d time.Duration) {
		mu.Lock()
		now = now.Add(d)
		mu.Unlock()
	}

	advertisedRelay := "https://edge-relay.example"
	var cloudMode atomic.Int32 // 0=ok old, 1=503 slow gate, 2=ok new, 3=revoked 401
	slowStarted := make(chan struct{}, 1)
	slowRelease := make(chan struct{})

	cloudSrv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		if r.URL.Path != "/api/v1/device-credentials/relay" || r.Method != http.MethodPost {
			http.NotFound(w, r)
			return
		}
		switch cloudMode.Load() {
		case 1:
			select {
			case slowStarted <- struct{}{}:
			default:
			}
			<-slowRelease
			w.WriteHeader(http.StatusServiceUnavailable)
			_, _ = w.Write([]byte(`{"error":"service_unavailable"}`))
		case 2:
			curNow := nowFn()
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":              "dc-new",
				"key_prefix":      "uk_new",
				"credential":      "cred-new",
				"relay_url":       advertisedRelay,
				"expires_at_ms":   curNow.Add(time.Hour).UnixMilli(),
				"rotate_after_ms": curNow.Add(30 * time.Minute).UnixMilli(),
			})
		case 3:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"error":"invalid_grant"}`))
		default:
			http.Error(w, "unexpected", http.StatusInternalServerError)
		}
	}))
	defer cloudSrv.Close()
	defer func() {
		select {
		case <-slowRelease:
		default:
			close(slowRelease)
		}
	}()

	initialSess := cloudauth.Session{
		State:        cloudauth.SessionStateActive,
		CloudURL:     cloudSrv.URL,
		AccessToken:  "access-valid",
		RefreshToken: "refresh-valid",
		User:         cloudauth.User{Email: "bob@example.com"},
		DeviceCredential: &cloudauth.DeviceCredential{
			ID:            "dc-old",
			KeyPrefix:     "uk_old",
			Credential:    "cred-old",
			RelayURL:      advertisedRelay,
			ExpiresAtMs:   now.Add(10 * time.Minute).UnixMilli(),
			RotateAfterMs: now.Add(-1 * time.Minute).UnixMilli(),
		},
	}
	if err := cloudauth.SaveSession(dataDir, initialSess); err != nil {
		t.Fatal(err)
	}
	if err := saveSettingsAtomic(filepath.Join(dataDir, mobileSettingsFilename), func() engine.Settings {
		s := engine.DefaultSettings()
		s.Sync.Relay.Enabled = true
		return s
	}()); err != nil {
		t.Fatal(err)
	}

	// Put Cloud into blocking slow-503 mode while cred-old is still unexpired.
	cloudMode.Store(1)
	cfgJSON, _ := json.Marshal(map[string]any{
		"data_dir":    dataDir,
		"device_name": "Phone",
	})
	eng, err := newEngineWithOptions(string(cfgJSON), tailcat.New(), cloudSrv.Client(), nowFn)
	if err != nil {
		t.Fatalf("newEngineWithOptions: %v", err)
	}
	defer func() { _ = eng.Close(nil) }()

	if err := eng.Start(nil); err != nil {
		t.Fatalf("Start: %v", err)
	}
	<-slowStarted

	// AccessKey must NOT block on in-flight rotation while cred-old is still valid.
	done := make(chan string, 1)
	go func() {
		k, allow := eng.cloudRelay.AccessKey(advertisedRelay)
		if allow {
			done <- k
		} else {
			done <- ""
		}
	}()
	select {
	case k := <-done:
		if k != "cred-old" {
			t.Fatalf("AccessKey during slow rotation = %q, want cred-old", k)
		}
	case <-time.After(500 * time.Millisecond):
		t.Fatalf("AccessKey blocked on slow Cloud rotation")
	}

	// Release slow 503 failure, advance past cred-old expiry: AccessKey must deny,
	// while user's Sync.Relay.Enabled intent stays true during outage.
	close(slowRelease)
	advance(15 * time.Minute)
	if k, allow := eng.cloudRelay.AccessKey(advertisedRelay); allow || k != "" {
		t.Fatalf("expected expired credential denied during outage, got (%q, %v)", k, allow)
	}
	stRaw, err := eng.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var st engine.State
	if err := json.Unmarshal([]byte(stRaw), &st); err != nil {
		t.Fatal(err)
	}
	if !st.Settings.Value.Sync.Relay.Enabled {
		t.Fatalf("expected user relay.enabled preserved during temporary 503 outage")
	}

	// Recover Cloud 200 -> EnsureCloudRelay / rotation grants cred-new.
	cloudMode.Store(2)
	if _, err := eng.EnsureCloudRelay(); err != nil {
		t.Fatalf("EnsureCloudRelay after 503 recovery: %v", err)
	}
	if k, allow := eng.cloudRelay.AccessKey(advertisedRelay); !allow || k != "cred-new" {
		t.Fatalf("AccessKey after recovery = (%q, %v), want cred-new", k, allow)
	}

	// Authoritative revocation transitions session to reauth_required and disables official relay.
	cloudMode.Store(3)
	if retry := eng.cloudRelay.OnUnauthorized(context.Background(), advertisedRelay, "cred-new"); retry {
		t.Fatalf("expected OnUnauthorized=false after authoritative revocation")
	}
	statusRaw, err := eng.GetCloudStatus()
	if err != nil {
		t.Fatal(err)
	}
	var status cloudauth.StatusSnapshot
	if err := json.Unmarshal([]byte(statusRaw), &status); err != nil {
		t.Fatal(err)
	}
	if !status.ReauthRequired || status.SignedIn || status.State != cloudauth.SessionStateReauthRequired {
		t.Fatalf("expected reauth_required status after revocation, got %+v", status)
	}
	if k, allow := eng.cloudRelay.AccessKey(advertisedRelay); allow || k != "" {
		t.Fatalf("expected denied after revocation, got (%q, %v)", k, allow)
	}
	deadline := time.Now().Add(3 * time.Second)
	for {
		revStateRaw, err := eng.GetState()
		if err != nil {
			t.Fatal(err)
		}
		var revSt engine.State
		if err := json.Unmarshal([]byte(revStateRaw), &revSt); err != nil {
			t.Fatal(err)
		}
		if !revSt.Settings.Value.Sync.Relay.Enabled {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("expected official relay disabled after authoritative revocation")
		}
		time.Sleep(20 * time.Millisecond)
	}
}

type recordedRelayReq struct {
	Path   string
	SSE    bool
	HasKey bool
	Key    string
}

func startSyntheticLoopbackRelay(t *testing.T) (*httptest.Server, func(prefix string) []recordedRelayReq, <-chan struct{}) {
	t.Helper()
	var mu sync.Mutex
	var reqs []recordedRelayReq
	notifyCh := make(chan struct{}, 32)

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		key := r.Header.Get("X-Unic-Relay-Key")
		_, hasKey := r.Header["X-Unic-Relay-Key"]
		isSSE := strings.Contains(r.URL.Path, "/events")
		mu.Lock()
		reqs = append(reqs, recordedRelayReq{
			Path:   r.URL.Path,
			SSE:    isSSE,
			HasKey: hasKey,
			Key:    key,
		})
		mu.Unlock()
		select {
		case notifyCh <- struct{}{}:
		default:
		}
		if isSSE {
			w.Header().Set("Content-Type", "text/event-stream")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(": ok\n\n"))
			if f, ok := w.(http.Flusher); ok {
				f.Flush()
			}
			<-r.Context().Done()
			return
		}
		w.Header().Set("Content-Type", "application/json")
		switch {
		case strings.HasSuffix(r.URL.Path, "/v2/profiles/register"):
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`{"ok":true}`))
		case strings.HasSuffix(r.URL.Path, "/v2/mailbox") || strings.Contains(r.URL.Path, "/v2/mailbox/"):
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`{"items":[]}`))
		default:
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`{}`))
		}
	}))
	t.Cleanup(srv.Close)

	snapshot := func(prefix string) []recordedRelayReq {
		mu.Lock()
		defer mu.Unlock()
		var out []recordedRelayReq
		for _, rec := range reqs {
			if prefix == "" || strings.HasPrefix(rec.Path, prefix) {
				out = append(out, rec)
			}
		}
		return out
	}
	return srv, snapshot, notifyCh
}

func waitForRelayTraffic(t *testing.T, snapshot func(string) []recordedRelayReq, notify <-chan struct{}, prefix string, wantCtrl, wantSSE int) []recordedRelayReq {
	t.Helper()
	deadline := time.Now().Add(8 * time.Second)
	for {
		recs := snapshot(prefix)
		ctrlCount, sseCount := 0, 0
		for _, r := range recs {
			if r.SSE {
				sseCount++
			} else {
				ctrlCount++
			}
		}
		if ctrlCount >= wantCtrl && sseCount >= wantSSE {
			return recs
		}
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for relay traffic on prefix=%q (got ctrl=%d sse=%d, want ctrl>=%d sse>=%d)", prefix, ctrlCount, sseCount, wantCtrl, wantSSE)
		}
		select {
		case <-notify:
		case <-time.After(50 * time.Millisecond):
		}
	}
}

func TestMobileRelayURLChangeClearsManualKeyControlPlaneSSEAndReopen(t *testing.T) {
	dataDir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	srvA, snapA, notifyA := startSyntheticLoopbackRelay(t)
	srvB, snapB, notifyB := startSyntheticLoopbackRelay(t)

	cfgJSON, _ := json.Marshal(map[string]any{
		"data_dir":    dataDir,
		"device_name": "LoopPhone",
	})
	eng, err := NewEngine(string(cfgJSON))
	if err != nil {
		t.Fatalf("NewEngine: %v", err)
	}
	if err := eng.SetRuntimeState("active"); err != nil {
		t.Fatal(err)
	}
	if err := eng.Start(nil); err != nil {
		t.Fatal(err)
	}

	rawSt, err := eng.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var st engine.State
	if err := json.Unmarshal([]byte(rawSt), &st); err != nil {
		t.Fatal(err)
	}

	// 1. Set URL A + /base1 with key-1 and verify control plane + SSE receive key-1.
	val := st.Settings.Value
	val.Sync.Relay.Enabled = true
	val.Sync.Relay.URL = srvA.URL + "/base1"
	req1, _ := json.Marshal(map[string]any{
		"expected_revision": st.Settings.Revision,
		"value":             val,
		"relay_access_key":  "key-1",
	})
	raw1, err := eng.UpdateSettings(string(req1))
	if err != nil {
		t.Fatalf("UpdateSettings(base1 + key-1): %v", err)
	}
	var st1 engine.SettingsState
	if err := json.Unmarshal([]byte(raw1), &st1); err != nil {
		t.Fatal(err)
	}
	if !st1.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("expected HasAccessKey=true on /base1")
	}
	recsBase1 := waitForRelayTraffic(t, snapA, notifyA, "/base1/", 1, 1)
	for _, r := range recsBase1 {
		if r.Key != "key-1" {
			t.Fatalf("/base1 request expected key-1, got %+v", r)
		}
	}

	// Cold reopen preserves /base1 + key-1.
	_ = eng.Close(nil)
	eng2, err := NewEngine(string(cfgJSON))
	if err != nil {
		t.Fatalf("reopen eng2: %v", err)
	}
	if err := eng2.SetRuntimeState("active"); err != nil {
		t.Fatal(err)
	}
	if err := eng2.Start(nil); err != nil {
		t.Fatal(err)
	}
	raw2, err := eng2.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var full2 engine.State
	if err := json.Unmarshal([]byte(raw2), &full2); err != nil {
		t.Fatal(err)
	}
	if !full2.Settings.Value.Sync.Relay.HasAccessKey || full2.Settings.Value.Sync.Relay.URL != srvA.URL+"/base1" {
		t.Fatalf("unexpected reopened state: %+v", full2.Settings.Value.Sync.Relay)
	}

	// 2. Change base path on SAME host/port (/base1 -> /base2) omitting relay_access_key:
	//    old key-1 MUST be cleared before /base2 receives any request, in memory, disk, and UI projection.
	valBase2 := full2.Settings.Value
	valBase2.Sync.Relay.URL = srvA.URL + "/base2"
	reqBase2, _ := json.Marshal(map[string]any{
		"expected_revision": full2.Settings.Revision,
		"value":             valBase2,
	})
	rawBase2, err := eng2.UpdateSettings(string(reqBase2))
	if err != nil {
		t.Fatalf("UpdateSettings(/base2): %v", err)
	}
	var stBase2 engine.SettingsState
	if err := json.Unmarshal([]byte(rawBase2), &stBase2); err != nil {
		t.Fatal(err)
	}
	if stBase2.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("expected HasAccessKey=false after changing base path without key")
	}
	if _, statErr := os.Stat(filepath.Join(dataDir, "mobile-relay-secret")); !os.IsNotExist(statErr) {
		t.Fatalf("expected mobile-relay-secret removed after URL change, got err=%v", statErr)
	}
	recsBase2 := waitForRelayTraffic(t, snapA, notifyA, "/base2/", 1, 1)
	for _, r := range recsBase2 {
		if r.Key != "" || r.HasKey {
			t.Fatalf("old key leaked to /base2: %+v", r)
		}
	}

	// Cold reopen on /base2 stays cleared and key-free.
	_ = eng2.Close(nil)
	eng3, err := NewEngine(string(cfgJSON))
	if err != nil {
		t.Fatalf("reopen eng3: %v", err)
	}
	defer func() { _ = eng3.Close(nil) }()
	if err := eng3.SetRuntimeState("active"); err != nil {
		t.Fatal(err)
	}
	if err := eng3.Start(nil); err != nil {
		t.Fatal(err)
	}
	raw3, err := eng3.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var full3 engine.State
	if err := json.Unmarshal([]byte(raw3), &full3); err != nil {
		t.Fatal(err)
	}
	if full3.Settings.Value.Sync.Relay.HasAccessKey || full3.Settings.Value.Sync.Relay.URL != srvA.URL+"/base2" {
		t.Fatalf("reopen after URL change unexpected: %+v", full3.Settings.Value.Sync.Relay)
	}

	// 3. Save explicit key-2 on /base2, then re-save SAME normalized URL (/base2/) omitting key: preserves key-2.
	reqKey2, _ := json.Marshal(map[string]any{
		"expected_revision": full3.Settings.Revision,
		"value":             full3.Settings.Value,
		"relay_access_key":  "key-2",
	})
	rawKey2, err := eng3.UpdateSettings(string(reqKey2))
	if err != nil {
		t.Fatalf("UpdateSettings(key-2): %v", err)
	}
	var stKey2 engine.SettingsState
	if err := json.Unmarshal([]byte(rawKey2), &stKey2); err != nil {
		t.Fatal(err)
	}
	valSameNorm := stKey2.Value
	valSameNorm.Sync.Relay.URL = srvA.URL + "/base2/"
	valSameNorm.Sync.Relay.MaxBytes = 32 << 20
	reqSameNorm, _ := json.Marshal(map[string]any{
		"expected_revision": stKey2.Revision,
		"value":             valSameNorm,
	})
	rawSameNorm, err := eng3.UpdateSettings(string(reqSameNorm))
	if err != nil {
		t.Fatalf("UpdateSettings(same normalized URL): %v", err)
	}
	var stSameNorm engine.SettingsState
	if err := json.Unmarshal([]byte(rawSameNorm), &stSameNorm); err != nil {
		t.Fatal(err)
	}
	if !stSameNorm.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("expected HasAccessKey=true preserved on same normalized URL save")
	}

	// 4. Error during URL save (invalid URL or revision conflict) MUST NOT wipe existing key-2.
	valInvalid := stSameNorm.Value
	valInvalid.Sync.Relay.URL = "not-a-valid-url"
	reqInvalid, _ := json.Marshal(map[string]any{
		"expected_revision": stSameNorm.Revision,
		"value":             valInvalid,
	})
	if _, err := eng3.UpdateSettings(string(reqInvalid)); err == nil {
		t.Fatalf("expected invalid relay URL save to fail")
	}
	if gotSecret, proven, _ := loadRelaySecretForTarget(filepath.Join(dataDir, "mobile-relay-secret"), srvA.URL+"/base2"); !proven || gotSecret != "key-2" {
		t.Fatalf("invalid URL save corrupted mobile-relay-secret: got (%q, %v), want key-2", gotSecret, proven)
	}
	valConflict := stSameNorm.Value
	valConflict.Sync.Relay.URL = srvB.URL + "/base1"
	reqConflict, _ := json.Marshal(map[string]any{
		"expected_revision": stSameNorm.Revision + 999,
		"value":             valConflict,
	})
	if _, err := eng3.UpdateSettings(string(reqConflict)); err == nil {
		t.Fatalf("expected stale revision save to fail")
	}
	if gotSecret, proven, _ := loadRelaySecretForTarget(filepath.Join(dataDir, "mobile-relay-secret"), srvA.URL+"/base2"); !proven || gotSecret != "key-2" {
		t.Fatalf("conflict save corrupted mobile-relay-secret: got (%q, %v), want key-2", gotSecret, proven)
	}

	// 5. Change PORT (srvA -> srvB) WITH explicit key-3 in the SAME save: uses key-3, never key-2.
	valPortB := stSameNorm.Value
	valPortB.Sync.Relay.URL = srvB.URL + "/base1"
	reqPortB, _ := json.Marshal(map[string]any{
		"expected_revision": stSameNorm.Revision,
		"value":             valPortB,
		"relay_access_key":  "key-3",
	})
	rawPortB, err := eng3.UpdateSettings(string(reqPortB))
	if err != nil {
		t.Fatalf("UpdateSettings(srvB + key-3): %v", err)
	}
	var stPortB engine.SettingsState
	if err := json.Unmarshal([]byte(rawPortB), &stPortB); err != nil {
		t.Fatal(err)
	}
	recsPortB := waitForRelayTraffic(t, snapB, notifyB, "/base1/", 1, 1)
	for _, r := range recsPortB {
		if r.Key != "key-3" {
			t.Fatalf("srvB request expected key-3 (and never key-2), got %+v", r)
		}
	}

	// 6. Change PORT back to srvA (/otherportcheck) WITHOUT explicit key -> clears key-3.
	valPortA := stPortB.Value
	valPortA.Sync.Relay.URL = srvA.URL + "/portcheck"
	reqPortA, _ := json.Marshal(map[string]any{
		"expected_revision": stPortB.Revision,
		"value":             valPortA,
	})
	rawPortA, err := eng3.UpdateSettings(string(reqPortA))
	if err != nil {
		t.Fatalf("UpdateSettings(portcheck without key): %v", err)
	}
	var stPortA engine.SettingsState
	if err := json.Unmarshal([]byte(rawPortA), &stPortA); err != nil {
		t.Fatal(err)
	}
	if stPortA.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("expected HasAccessKey=false after changing port without key")
	}
	recsPortCheck := waitForRelayTraffic(t, snapA, notifyA, "/portcheck/", 1, 1)
	for _, r := range recsPortCheck {
		if r.Key != "" || r.HasKey {
			t.Fatalf("old key-3 leaked after port change: %+v", r)
		}
	}

	// 7. Re-set key-4, then restore official mode ("") without explicit key: clears key-4 & disables unbacked official relay.
	reqKey4, _ := json.Marshal(map[string]any{
		"expected_revision": stPortA.Revision,
		"value":             stPortA.Value,
		"relay_access_key":  "key-4",
	})
	rawKey4, err := eng3.UpdateSettings(string(reqKey4))
	if err != nil {
		t.Fatal(err)
	}
	var stKey4 engine.SettingsState
	if err := json.Unmarshal([]byte(rawKey4), &stKey4); err != nil {
		t.Fatal(err)
	}
	valOff := stKey4.Value
	valOff.Sync.Relay.URL = ""
	reqOff, _ := json.Marshal(map[string]any{
		"expected_revision": stKey4.Revision,
		"value":             valOff,
	})
	rawOff, err := eng3.UpdateSettings(string(reqOff))
	if err != nil {
		t.Fatalf("UpdateSettings(restore official): %v", err)
	}
	var stOff engine.SettingsState
	if err := json.Unmarshal([]byte(rawOff), &stOff); err != nil {
		t.Fatal(err)
	}
	if stOff.Value.Sync.Relay.HasAccessKey || stOff.Value.Sync.Relay.Enabled || stOff.Value.Sync.Relay.URL != "" {
		t.Fatalf("expected official restore cleared key and disabled signed-out official relay, got %+v", stOff.Value.Sync.Relay)
	}
	if _, statErr := os.Stat(filepath.Join(dataDir, "mobile-relay-secret")); !os.IsNotExist(statErr) {
		t.Fatalf("expected mobile-relay-secret removed after official restore, got err=%v", statErr)
	}
}

func TestMobileCloudSlowRenewalDoesNotBlockUpdateSettingsOrDeadlock(t *testing.T) {
	dataDir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}

	now := time.Unix(1700000000, 0)
	slowStarted := make(chan struct{}, 1)
	slowRelease := make(chan struct{})
	var closeOnce sync.Once
	releaseSlow := func() {
		closeOnce.Do(func() { close(slowRelease) })
	}
	defer releaseSlow()

	cloudSrv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost:
			select {
			case slowStarted <- struct{}{}:
			default:
			}
			<-slowRelease
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":              "dc-new-slow",
				"key_prefix":      "uk_slow",
				"credential":      "cred-slow-new",
				"relay_url":       "https://official-edge.example",
				"expires_at_ms":   now.Add(time.Hour).UnixMilli(),
				"rotate_after_ms": now.Add(30 * time.Minute).UnixMilli(),
			})
		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodDelete:
			w.WriteHeader(http.StatusNoContent)
		default:
			http.NotFound(w, r)
		}
	}))
	defer cloudSrv.Close()

	if err := cloudauth.SaveSession(dataDir, cloudauth.Session{
		State:        cloudauth.SessionStateActive,
		CloudURL:     cloudSrv.URL,
		AccessToken:  "access-valid",
		RefreshToken: "refresh-valid",
		User:         cloudauth.User{Email: "carol@example.com"},
		DeviceCredential: &cloudauth.DeviceCredential{
			ID:            "dc-old",
			KeyPrefix:     "uk_old",
			Credential:    "cred-old",
			RelayURL:      "https://official-edge.example",
			ExpiresAtMs:   now.Add(20 * time.Minute).UnixMilli(),
			RotateAfterMs: now.Add(-1 * time.Minute).UnixMilli(),
		},
	}); err != nil {
		t.Fatal(err)
	}
	if err := saveSettingsAtomic(filepath.Join(dataDir, mobileSettingsFilename), func() engine.Settings {
		s := engine.DefaultSettings()
		s.Sync.Relay.Enabled = true
		return s
	}()); err != nil {
		t.Fatal(err)
	}

	cfgJSON, _ := json.Marshal(map[string]any{
		"data_dir":    dataDir,
		"device_name": "SlowPhone",
	})
	eng, err := newEngineWithOptions(string(cfgJSON), tailcat.New(), cloudSrv.Client(), func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		releaseSlow()
		_ = eng.Close(nil)
	}()
	if err := eng.Start(nil); err != nil {
		t.Fatal(err)
	}

	<-slowStarted

	rawSt, err := eng.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var st engine.State
	if err := json.Unmarshal([]byte(rawSt), &st); err != nil {
		t.Fatal(err)
	}

	// Saving settings (e.g. updating max_bytes or switching to self-hosted) MUST complete
	// quickly without blocking on the in-flight Cloud credential renewal or deadlocking.
	val := st.Settings.Value
	val.DeviceName = "SlowPhoneUpdated"
	val.Sync.Relay.URL = "http://127.0.0.1:45678/custom"
	updReq, _ := json.Marshal(map[string]any{
		"expected_revision": st.Settings.Revision,
		"value":             val,
		"relay_access_key":  "selfhost-during-slow-rotate",
	})

	type updRes struct {
		raw string
		err error
	}
	resCh := make(chan updRes, 1)
	go func() {
		r, uerr := eng.UpdateSettings(string(updReq))
		resCh <- updRes{raw: r, err: uerr}
	}()

	select {
	case res := <-resCh:
		if res.err != nil {
			t.Fatalf("UpdateSettings during slow renewal failed: %v", res.err)
		}
		var saved engine.SettingsState
		if err := json.Unmarshal([]byte(res.raw), &saved); err != nil {
			t.Fatal(err)
		}
		if saved.Value.Sync.Relay.URL != "http://127.0.0.1:45678/custom" || !saved.Value.Sync.Relay.HasAccessKey {
			t.Fatalf("unexpected saved settings during slow renewal: %+v", saved.Value.Sync.Relay)
		}
	case <-time.After(1 * time.Second):
		t.Fatalf("UpdateSettings blocked on slow Cloud renewal")
	}

	releaseSlow()
	time.Sleep(100 * time.Millisecond)

	// Reopening confirms user's self-hosted choice was not clobbered when slow renewal completed.
	_ = eng.Close(nil)
	eng2, err := newEngineWithOptions(string(cfgJSON), tailcat.New(), cloudSrv.Client(), func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = eng2.Close(nil) }()
	rawAfter, err := eng2.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var stAfter engine.State
	if err := json.Unmarshal([]byte(rawAfter), &stAfter); err != nil {
		t.Fatal(err)
	}
	if stAfter.Settings.Value.Sync.Relay.URL != "http://127.0.0.1:45678/custom" || !stAfter.Settings.Value.Sync.Relay.HasAccessKey {
		t.Fatalf("slow renewal clobbered self-hosted settings: %+v", stAfter.Settings.Value.Sync.Relay)
	}
}

func TestMobileCloudReal401RevocationFromRelayLoopNoDeadlock(t *testing.T) {
	dataDir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}

	relaySrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = w.Write([]byte(`{"error":"unauthorized"}`))
	}))
	defer relaySrv.Close()

	now := time.Now()
	cloudSrv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		if r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"error":"invalid_grant"}`))
			return
		}
		http.NotFound(w, r)
	}))
	defer cloudSrv.Close()

	if err := cloudauth.SaveSession(dataDir, cloudauth.Session{
		State:        cloudauth.SessionStateActive,
		CloudURL:     cloudSrv.URL,
		AccessToken:  "access-valid",
		RefreshToken: "refresh-valid",
		User:         cloudauth.User{Email: "dave@example.com"},
		DeviceCredential: &cloudauth.DeviceCredential{
			ID:            "dc-401",
			KeyPrefix:     "uk_401",
			Credential:    "cred-401",
			RelayURL:      relaySrv.URL,
			ExpiresAtMs:   now.Add(time.Hour).UnixMilli(),
			RotateAfterMs: now.Add(30 * time.Minute).UnixMilli(),
		},
	}); err != nil {
		t.Fatal(err)
	}
	if err := saveSettingsAtomic(filepath.Join(dataDir, mobileSettingsFilename), func() engine.Settings {
		s := engine.DefaultSettings()
		s.Sync.Relay.Enabled = true
		return s
	}()); err != nil {
		t.Fatal(err)
	}

	cfgJSON, _ := json.Marshal(map[string]any{
		"data_dir":    dataDir,
		"device_name": "Loop401Phone",
	})
	eng, err := newEngineWithOptions(string(cfgJSON), tailcat.New(), cloudSrv.Client(), nil)
	if err != nil {
		t.Fatal(err)
	}
	if err := eng.SetRuntimeState("active"); err != nil {
		t.Fatal(err)
	}
	if err := eng.Start(nil); err != nil {
		t.Fatal(err)
	}

	deadline := time.Now().Add(5 * time.Second)
	for {
		rawSt, gerr := eng.GetState()
		if gerr != nil {
			t.Fatal(gerr)
		}
		var st engine.State
		if err := json.Unmarshal([]byte(rawSt), &st); err != nil {
			t.Fatal(err)
		}
		rawCloud, cerr := eng.GetCloudStatus()
		if cerr != nil {
			t.Fatal(cerr)
		}
		var cst cloudauth.StatusSnapshot
		if err := json.Unmarshal([]byte(rawCloud), &cst); err != nil {
			t.Fatal(err)
		}
		if !st.Settings.Value.Sync.Relay.Enabled && cst.ReauthRequired {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for non-deadlocking 401 revocation reconcile (relay.enabled=%v reauth=%v)",
				st.Settings.Value.Sync.Relay.Enabled, cst.ReauthRequired)
		}
		time.Sleep(25 * time.Millisecond)
	}

	closeDone := make(chan error, 1)
	go func() { closeDone <- eng.Close(nil) }()
	select {
	case err := <-closeDone:
		if err != nil {
			t.Fatalf("Close after 401 revocation failed: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatalf("Close deadlocked after real 401 revocation from relay loop")
	}
}
