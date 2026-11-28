package e2e

import (
	"bytes"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"
)

type relayRequestRecord struct {
	Target   string `json:"target"`
	Method   string `json:"method"`
	Path     string `json:"path"`
	KeyLabel string `json:"key_label"`
	Status   int    `json:"status"`
}

// TestCloudRelayCredentialLifecycle exercises the production unic and unicd
// binaries across OAuth login, timed silent rotation, relay 401 rotation +
// cooldown, cross-origin redirect blocking, official<->selfhost revocation &
// re-issuance, signed-out official disablement, config.toml hot reload and
// restart, manual fallback preservation on logout, and authoritative Cloud
// revocation -> reauth_required transition.
func TestCloudRelayCredentialLifecycle(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}

	work := shortTemp(t)
	userHome := filepath.Join(work, "home")
	configHome := filepath.Join(work, "cfg")
	dataHome := filepath.Join(work, "data")
	unicHome := filepath.Join(work, "u")
	boardDir := filepath.Join(work, "board")
	for _, d := range []string{userHome, configHome, dataHome, unicHome, boardDir} {
		if err := os.MkdirAll(d, 0o700); err != nil {
			t.Fatal(err)
		}
	}

	var mu sync.Mutex
	knownSecrets := map[string]string{}
	addSecret := func(label, raw string) {
		mu.Lock()
		defer mu.Unlock()
		if raw != "" {
			knownSecrets[raw] = label
		}
	}
	labelKey := func(raw string) string {
		if raw == "" {
			return "none"
		}
		mu.Lock()
		defer mu.Unlock()
		if lbl, ok := knownSecrets[raw]; ok {
			return lbl
		}
		return "unknown"
	}
	reCredTokens := regexp.MustCompile(`(uc_dc_|uc_ak_|Bearer )[A-Za-z0-9_-]+`)
	redactSecrets := func(text string) string {
		mu.Lock()
		out := text
		for secret, lbl := range knownSecrets {
			if secret != "" {
				out = strings.ReplaceAll(out, secret, "<redacted:"+lbl+">")
			}
		}
		mu.Unlock()
		return reCredTokens.ReplaceAllString(out, "<redacted:token>")
	}

	// ---- Local mock Relays (Official, SelfHost-A, SelfHost-B, EvilRedirect) ----
	var relayRecords []relayRequestRecord
	rejectKeys := map[string]bool{}
	var redirectURLOnce string
	var activeSSE []chan struct{}
	closeAllSSE := func() {
		mu.Lock()
		chans := activeSSE
		activeSSE = nil
		mu.Unlock()
		for _, ch := range chans {
			close(ch)
		}
	}

	newMockRelay := func(target string, tlsEnabled bool) *httptest.Server {
		h := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			key := r.Header.Get("X-Unic-Relay-Key")
			lbl := labelKey(key)

			if target == "evil-redirect" {
				mu.Lock()
				relayRecords = append(relayRecords, relayRequestRecord{
					Target: target, Method: r.Method, Path: r.URL.Path, KeyLabel: "REDIRECTED:" + lbl, Status: http.StatusForbidden,
				})
				mu.Unlock()
				w.WriteHeader(http.StatusForbidden)
				return
			}

			if strings.HasPrefix(target, "selfhost") && strings.HasPrefix(key, "uc_dc_") {
				w.WriteHeader(http.StatusForbidden)
				mu.Lock()
				relayRecords = append(relayRecords, relayRequestRecord{
					Target: target, Method: r.Method, Path: r.URL.Path, KeyLabel: "LEAKED:" + lbl, Status: http.StatusForbidden,
				})
				mu.Unlock()
				return
			}

			mu.Lock()
			rejected := rejectKeys[key] || (target == "official" && key == "")
			redir := ""
			if target == "official" && redirectURLOnce != "" && !rejected {
				redir = redirectURLOnce
				redirectURLOnce = ""
			}
			mu.Unlock()
			if rejected {
				mu.Lock()
				relayRecords = append(relayRecords, relayRequestRecord{
					Target: target, Method: r.Method, Path: r.URL.Path, KeyLabel: lbl, Status: http.StatusUnauthorized,
				})
				mu.Unlock()
				http.Error(w, "unauthorized", http.StatusUnauthorized)
				return
			}
			if redir != "" {
				mu.Lock()
				relayRecords = append(relayRecords, relayRequestRecord{
					Target: target, Method: r.Method, Path: r.URL.Path, KeyLabel: lbl, Status: http.StatusTemporaryRedirect,
				})
				mu.Unlock()
				http.Redirect(w, r, redir, http.StatusTemporaryRedirect)
				return
			}

			if strings.HasSuffix(r.URL.Path, "/events") {
				mu.Lock()
				relayRecords = append(relayRecords, relayRequestRecord{
					Target: target, Method: r.Method, Path: "/events", KeyLabel: lbl, Status: http.StatusOK,
				})
				ch := make(chan struct{})
				activeSSE = append(activeSSE, ch)
				mu.Unlock()

				w.Header().Set("Content-Type", "text/event-stream")
				w.WriteHeader(http.StatusOK)
				_, _ = io.WriteString(w, ": keepalive\n\n")
				if f, ok := w.(http.Flusher); ok {
					f.Flush()
				}
				select {
				case <-r.Context().Done():
				case <-ch:
				}
				return
			}

			if strings.HasSuffix(r.URL.Path, "/transfers") && r.Method == http.MethodGet {
				mu.Lock()
				relayRecords = append(relayRecords, relayRequestRecord{
					Target: target, Method: r.Method, Path: "/transfers", KeyLabel: lbl, Status: http.StatusOK,
				})
				mu.Unlock()
				_ = json.NewEncoder(w).Encode([]any{})
				return
			}

			mu.Lock()
			relayRecords = append(relayRecords, relayRequestRecord{
				Target: target, Method: r.Method, Path: r.URL.Path, KeyLabel: lbl, Status: http.StatusNoContent,
			})
			mu.Unlock()
			w.WriteHeader(http.StatusNoContent)
		})
		if tlsEnabled {
			return httptest.NewTLSServer(h)
		}
		return httptest.NewServer(h)
	}

	officialRelay := newMockRelay("official", true)
	t.Cleanup(func() {
		closeAllSSE()
		officialRelay.Close()
	})
	selfHostA := newMockRelay("selfhost-a", false)
	t.Cleanup(func() {
		closeAllSSE()
		selfHostA.Close()
	})
	selfHostB := newMockRelay("selfhost-b", false)
	t.Cleanup(func() {
		closeAllSSE()
		selfHostB.Close()
	})
	evilRedirect := newMockRelay("evil-redirect", false)
	t.Cleanup(func() {
		evilRedirect.Close()
	})

	// ---- Local mock Cloud ----
	var refreshSeq, credSeq int
	var curRefresh, curAccess string
	var transientFailCount int
	var revokeMode bool
	var revokedCredCount, revokedSessCount, credIssueCalls int
	rotateDelay := 10 * time.Minute
	credTTL := time.Hour
	var savedChallenge, savedRedirect string
	var curDeviceCred string
	var slowRotateEntered chan struct{}
	var slowRotateRelease chan struct{}

	cloudSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/cli/authorize":
			q := r.URL.Query()
			mu.Lock()
			savedChallenge = q.Get("code_challenge")
			savedRedirect = q.Get("redirect_uri")
			mu.Unlock()
			target := savedRedirect + "?code=e2e-code-1&state=" + q.Get("state")
			http.Redirect(w, r, target, http.StatusSeeOther)

		case r.URL.Path == "/api/auth/token":
			mu.Lock()
			defer mu.Unlock()
			refreshSeq++
			curAccess = fmt.Sprintf("e2e-access-%d", refreshSeq)
			curRefresh = fmt.Sprintf("e2e-refresh-%d", refreshSeq)
			knownSecrets[curAccess] = fmt.Sprintf("access_%d", refreshSeq)
			knownSecrets[curRefresh] = fmt.Sprintf("refresh_%d", refreshSeq)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token":  curAccess,
				"refresh_token": curRefresh,
				"token_type":    "Bearer",
				"expires_in":    900,
				"user": map[string]any{
					"clerkUserId": "user_e2e",
					"email":       "e2e-user@example.com",
				},
			})

		case r.URL.Path == "/api/auth/refresh":
			var body struct {
				RefreshToken string `json:"refresh_token"`
			}
			_ = json.NewDecoder(r.Body).Decode(&body)
			mu.Lock()
			defer mu.Unlock()
			if revokeMode {
				w.WriteHeader(http.StatusUnauthorized)
				_ = json.NewEncoder(w).Encode(map[string]string{"error": "invalid_grant"})
				return
			}
			if transientFailCount > 0 {
				transientFailCount--
				http.Error(w, "temporary unavailable", http.StatusServiceUnavailable)
				return
			}
			if body.RefreshToken != curRefresh {
				w.WriteHeader(http.StatusUnauthorized)
				_ = json.NewEncoder(w).Encode(map[string]string{"error": "invalid_grant"})
				return
			}
			refreshSeq++
			curAccess = fmt.Sprintf("e2e-access-%d", refreshSeq)
			curRefresh = fmt.Sprintf("e2e-refresh-%d", refreshSeq)
			knownSecrets[curAccess] = fmt.Sprintf("access_%d", refreshSeq)
			knownSecrets[curRefresh] = fmt.Sprintf("refresh_%d", refreshSeq)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token":  curAccess,
				"refresh_token": curRefresh,
				"expires_in":    900,
			})

		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodPost:
			mu.Lock()
			credIssueCalls++
			var enteredCh, releaseCh chan struct{}
			if credSeq >= 1 && slowRotateEntered != nil {
				enteredCh = slowRotateEntered
				releaseCh = slowRotateRelease
				slowRotateEntered = nil
				slowRotateRelease = nil
			}
			mu.Unlock()
			if enteredCh != nil {
				close(enteredCh)
				select {
				case <-releaseCh:
				case <-r.Context().Done():
					return
				}
			}
			mu.Lock()
			defer mu.Unlock()
			if revokeMode {
				w.WriteHeader(http.StatusUnauthorized)
				_ = json.NewEncoder(w).Encode(map[string]string{"error": "session_revoked"})
				return
			}
			if transientFailCount > 0 {
				transientFailCount--
				http.Error(w, "temporary unavailable", http.StatusServiceUnavailable)
				return
			}
			if r.Header.Get("Authorization") != "Bearer "+curAccess {
				w.WriteHeader(http.StatusUnauthorized)
				_ = json.NewEncoder(w).Encode(map[string]string{"error": "expired_token"})
				return
			}
			credSeq++
			secret := fmt.Sprintf("uc_dc_c%d_secretvalue_%d", credSeq, credSeq)
			prefix := fmt.Sprintf("uc_dc_c%d", credSeq)
			curDeviceCred = secret
			knownSecrets[secret] = fmt.Sprintf("dc_%d", credSeq)
			now := time.Now()
			rotAt := now.Add(rotateDelay)
			rotateDelay = 10 * time.Minute
			expAt := now.Add(credTTL)
			credTTL = time.Hour
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":                 fmt.Sprintf("dc-id-%d", credSeq),
				"key_prefix":         prefix,
				"credential":         secret,
				"scopes":             []string{"relay"},
				"relay_url":          officialRelay.URL,
				"max_transfer_bytes": 25 << 20,
				"expires_at_ms":      expAt.UnixMilli(),
				"rotate_after_ms":    rotAt.UnixMilli(),
			})

		case r.URL.Path == "/api/v1/device-credentials/relay" && r.Method == http.MethodDelete:
			mu.Lock()
			revokedCredCount++
			mu.Unlock()
			w.WriteHeader(http.StatusNoContent)

		case r.URL.Path == "/api/auth/revoke":
			mu.Lock()
			revokedSessCount++
			mu.Unlock()
			w.WriteHeader(http.StatusNoContent)

		default:
			http.NotFound(w, r)
		}
	}))
	defer cloudSrv.Close()
	_ = savedChallenge

	addSecret("manual_key", "uc_ak_manual_fallback_secret")
	addSecret("selfhost_a_key", "selfhost-secret-A-value")
	addSecret("selfhost_a2_key", "selfhost-secret-A2-value")

	var baseEnv []string
	for _, e := range os.Environ() {
		switch {
		case strings.HasPrefix(e, "UNIC_"), strings.HasPrefix(e, "UC_PROFILE="), strings.HasPrefix(e, "HOME="),
			strings.HasPrefix(e, "XDG_"), strings.HasPrefix(e, "LOCALAPPDATA="), strings.HasPrefix(e, "PATH="):
			continue
		}
		baseEnv = append(baseEnv, e)
	}
	caPath := filepath.Join(work, "test-ca.pem")
	var caBuf bytes.Buffer
	if cert := officialRelay.Certificate(); cert != nil {
		_ = pem.Encode(&caBuf, &pem.Block{Type: "CERTIFICATE", Bytes: cert.Raw})
	}
	if err := os.WriteFile(caPath, caBuf.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}

	baseEnv = append(baseEnv,
		"PATH="+binDir+string(os.PathListSeparator)+os.Getenv("PATH"),
		"HOME="+userHome,
		"XDG_CONFIG_HOME="+configHome,
		"XDG_DATA_HOME="+dataHome,
		"LOCALAPPDATA="+dataHome,
		"UNIC_HOME="+unicHome,
		"UNIC_SOCK="+filepath.Join(unicHome, "unic.sock"),
		"UNIC_RENDEZVOUS_URL=http://127.0.0.1:1",
		"UNIC_CLOUD_URL="+cloudSrv.URL,
		"UNIC_RELAY_URL="+officialRelay.URL,
		"SSL_CERT_FILE="+caPath,
		"UNIC_MOCK_CLIPBOARD_DIR="+boardDir,
	)

	var transcript []string
	redactArgs := func(args []string) string {
		redacted := make([]string, len(args))
		copy(redacted, args)
		for i := 0; i+1 < len(redacted); i++ {
			if redacted[i] == "sync.relay.access_key" {
				redacted[i+1] = "<redacted>"
			}
		}
		return strings.Join(redacted, " ")
	}
	runCLI := func(args ...string) (int, string, string) {
		t.Helper()
		cmd := exec.Command(filepath.Join(binDir, "unic"), args...)
		cmd.Env = baseEnv
		var stdout, stderr bytes.Buffer
		cmd.Stdout, cmd.Stderr = &stdout, &stderr
		err := cmd.Run()
		code := 0
		if err != nil {
			if ee, ok := err.(*exec.ExitError); ok {
				code = ee.ExitCode()
			} else {
				t.Fatalf("exec unic (%s) failed: err=%v", redactArgs(args), redactSecrets(err.Error()))
			}
		}
		transcript = append(transcript, fmt.Sprintf("$ unic %s -> exit=%d", redactArgs(args), code))
		return code, stdout.String(), stderr.String()
	}

	assertNoSecretLeak := func(label, text string) {
		t.Helper()
		mu.Lock()
		defer mu.Unlock()
		for secret, name := range knownSecrets {
			if secret != "" && strings.Contains(text, secret) {
				t.Fatalf("secret %s leaked in %s", name, label)
			}
		}
	}

	waitFor := func(desc string, timeout time.Duration, cond func() bool) {
		t.Helper()
		deadline := time.Now().Add(timeout)
		for time.Now().Before(deadline) {
			if cond() {
				return
			}
			time.Sleep(50 * time.Millisecond)
		}
		t.Fatalf("timed out waiting for %s", desc)
	}

	hasRelayEvent := func(target, path, keyLabel string) bool {
		mu.Lock()
		defer mu.Unlock()
		for _, rec := range relayRecords {
			if rec.Target == target && rec.Path == path && rec.KeyLabel == keyLabel {
				return true
			}
		}
		return false
	}

	countTargetRequests := func(target string) int {
		mu.Lock()
		defer mu.Unlock()
		n := 0
		for _, rec := range relayRecords {
			if rec.Target == target {
				n++
			}
		}
		return n
	}

	relayEventsSince := func(startIdx int, target string) []relayRequestRecord {
		mu.Lock()
		defer mu.Unlock()
		var out []relayRequestRecord
		for i := startIdx; i < len(relayRecords); i++ {
			if relayRecords[i].Target == target {
				out = append(out, relayRecords[i])
			}
		}
		return out
	}

	recordCount := func() int {
		mu.Lock()
		defer mu.Unlock()
		return len(relayRecords)
	}

	loginViaCLI := func(extraArgs ...string) string {
		t.Helper()
		args := append([]string{"cloud", "login", "--no-browser", "--json"}, extraArgs...)
		cmd := exec.Command(filepath.Join(binDir, "unic"), args...)
		cmd.Env = baseEnv
		stderrBuf := &syncBuffer{}
		var stdout bytes.Buffer
		cmd.Stdout, cmd.Stderr = &stdout, stderrBuf
		if err := cmd.Start(); err != nil {
			t.Fatalf("start unic cloud login failed: %v", redactSecrets(err.Error()))
		}
		reURL := regexp.MustCompile(`https?://[^\s]+/cli/authorize\?[^\s]+`)
		var authURL string
		waitFor("authorize URL printed by unic cloud login", 5*time.Second, func() bool {
			m := reURL.FindString(stderrBuf.String())
			if m != "" {
				authURL = m
				return true
			}
			return false
		})
		resp, err := http.Get(authURL)
		if err != nil {
			t.Fatalf("GET authURL failed")
		}
		_ = resp.Body.Close()
		if err := cmd.Wait(); err != nil {
			t.Fatalf("unic cloud login failed: stderr_len=%d stdout_len=%d err=%s", len(stderrBuf.String()), len(stdout.String()), redactSecrets(err.Error()))
		}
		assertNoSecretLeak("login stdout", stdout.String())
		assertNoSecretLeak("login stderr", stderrBuf.String())
		transcript = append(transcript, fmt.Sprintf("$ unic %s -> exit=0", redactArgs(args)))
		return stdout.String()
	}

	// ---- Step 1: Initialize profile, disable telemetry, start unicd ----
	if code, _, _ := runCLI("init", "--fresh", "--name", "CloudE2E", "--json"); code != 0 {
		t.Fatalf("init failed: exit=%d", code)
	}
	cfgPath := filepath.Join(unicHome, "config.toml")
	cfgRaw, err := os.ReadFile(cfgPath)
	if err != nil {
		t.Fatal(err)
	}
	updatedCfg := strings.Replace(string(cfgRaw), "[telemetry]\n  enabled = true", "[telemetry]\n  enabled = false", 1)
	if updatedCfg == string(cfgRaw) {
		updatedCfg = string(cfgRaw) + "\n[telemetry]\nenabled = false\n"
	}
	if err := os.WriteFile(cfgPath, []byte(updatedCfg), 0o600); err != nil {
		t.Fatal(err)
	}

	t.Cleanup(func() {
		_, _, _ = runCLI("daemon", "stop")
		closeAllSSE()
	})
	if code, _, _ := runCLI("daemon", "start", "--json"); code != 0 {
		t.Fatalf("daemon start failed: exit=%d", code)
	}

	// ---- Step 2: Login while unicd is already running; dc_1 issued with short rotate_after_ms ----
	slowEntered := make(chan struct{})
	slowRelease := make(chan struct{})
	mu.Lock()
	rotateDelay = 800 * time.Millisecond
	slowRotateEntered = slowEntered
	slowRotateRelease = slowRelease
	mu.Unlock()
	loginOut := loginViaCLI()
	var loginParsed struct {
		SignedIn         bool   `json:"signed_in"`
		State            string `json:"state"`
		CredentialPrefix string `json:"credential_prefix"`
	}
	if err := json.Unmarshal([]byte(loginOut), &loginParsed); err != nil || !loginParsed.SignedIn || loginParsed.CredentialPrefix != "uc_dc_c1" {
		t.Fatalf("expected uc_dc_c1 login JSON, got signed_in=%v state=%q prefix=%q err=%v", loginParsed.SignedIn, loginParsed.State, loginParsed.CredentialPrefix, err)
	}
	cfgAfterLogin, err := os.ReadFile(cfgPath)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(cfgAfterLogin), "access_key") || strings.Contains(string(cfgAfterLogin), "uc_dc_") {
		t.Fatalf("config.toml unexpectedly contains access_key=%v managed_prefix=%v",
			strings.Contains(string(cfgAfterLogin), "access_key"),
			strings.Contains(string(cfgAfterLogin), "uc_dc_"))
	}

	waitFor("official relay SSE sees dc_1", 6*time.Second, func() bool {
		return hasRelayEvent("official", "/events", "dc_1")
	})

	// ---- Step 3: Non-blocking reconnect with still-valid dc_1 while dc_1->dc_2 slow rotation is in flight ----
	// Wait until RotateIfDue has entered the Cloud issue request (holding its slow rotation lock).
	select {
	case <-slowEntered:
	case <-time.After(8 * time.Second):
		close(slowRelease)
		t.Fatalf("timed out waiting for background silent rotation dc_1 -> dc_2 to enter Cloud")
	}

	duringSlowMark := recordCount()
	closeAllSSE()
	waitFor("control-plane and SSE reconnect with still-valid dc_1 while slow Cloud rotation is blocked", 4*time.Second, func() bool {
		evs := relayEventsSince(duringSlowMark, "official")
		hasCtrl := false
		hasSSE := false
		for _, ev := range evs {
			if ev.KeyLabel == "dc_1" && ev.Status == http.StatusOK {
				if ev.Path == "/events" {
					hasSSE = true
				} else {
					hasCtrl = true
				}
			}
		}
		return hasCtrl && hasSSE
	})
	duringSlowEvents := relayEventsSince(duringSlowMark, "official")
	close(slowRelease)

	waitFor("unic cloud status updates to uc_dc_c2 after releasing slow silent rotation", 8*time.Second, func() bool {
		code, out, _ := runCLI("cloud", "status", "--json")
		return code == 0 && strings.Contains(out, `"credential_prefix":"uc_dc_c2"`)
	})
	afterSlowMark := recordCount()
	closeAllSSE()
	waitFor("control-plane and SSE reconnect with dc_2 after slow rotation completes", 6*time.Second, func() bool {
		evs := relayEventsSince(afterSlowMark, "official")
		hasCtrl := false
		hasSSE := false
		for _, ev := range evs {
			if ev.KeyLabel == "dc_2" && ev.Status == http.StatusOK {
				if ev.Path == "/events" {
					hasSSE = true
				} else {
					hasCtrl = true
				}
			}
		}
		return hasCtrl && hasSSE
	})
	afterSlowEvents := relayEventsSince(afterSlowMark, "official")

	// ---- Step 4: Relay 401 invalidates dc_2 -> unicd rotates to dc_3 & retries ----
	mu.Lock()
	for k, lbl := range knownSecrets {
		if lbl == "dc_2" {
			rejectKeys[k] = true
		}
	}
	mu.Unlock()
	closeAllSSE()
	waitFor("official relay recovers from 401 with dc_3", 8*time.Second, func() bool {
		return hasRelayEvent("official", "/events", "dc_3")
	})

	// ---- Step 4b: Cross-origin redirect must be blocked without forwarding X-Unic-Relay-Key ----
	mu.Lock()
	redirectURLOnce = evilRedirect.URL + "/stolen"
	mu.Unlock()
	closeAllSSE()
	waitFor("official relay reconnects after blocked 307 redirect", 6*time.Second, func() bool {
		return hasRelayEvent("official", "/events", "dc_3")
	})
	if n := countTargetRequests("evil-redirect"); n != 0 {
		t.Fatalf("expected 0 requests forwarded on cross-origin redirect, got %d", n)
	}

	// ---- Step 5: Persistent 401 + transient Cloud 503 respects 30s cooldown & stays active ----
	mu.Lock()
	transientFailCount = 4
	callsBeforeCooldown := credIssueCalls
	for k, lbl := range knownSecrets {
		if lbl == "dc_3" {
			rejectKeys[k] = true
		}
	}
	mu.Unlock()
	closeAllSSE()
	time.Sleep(1500 * time.Millisecond)
	mu.Lock()
	callsDuringCooldown := credIssueCalls - callsBeforeCooldown
	delete(rejectKeys, curDeviceCred)
	transientFailCount = 0
	mu.Unlock()
	if callsDuringCooldown > 2 {
		t.Fatalf("expected cooldown to bound Cloud retries on repeated 401, got %d issue calls", callsDuringCooldown)
	}
	if code, out, _ := runCLI("cloud", "status", "--json"); code != 0 || !strings.Contains(out, `"state":"active"`) {
		t.Fatalf("transient Cloud failure must keep session active: exit=%d", code)
	}

	// ---- Step 6: Official -> SelfHost-A revokes/clears managed credential while keeping Cloud session active ----
	mu.Lock()
	revokedBeforeLeave := revokedCredCount
	mu.Unlock()
	if code, _, _ := runCLI("config", "set", "sync.relay.url", fmt.Sprintf("%q", selfHostA.URL)); code != 0 {
		t.Fatalf("set selfhostA url failed: exit=%d", code)
	}
	waitFor("leaving official mode revokes managed credential on Cloud", 5*time.Second, func() bool {
		mu.Lock()
		revDone := revokedCredCount > revokedBeforeLeave
		mu.Unlock()
		return revDone
	})
	sessionPath := filepath.Join(unicHome, "cloud-session.json")
	if sessRaw, rerr := os.ReadFile(sessionPath); rerr != nil {
		t.Fatalf("read session after leaving official: %v", rerr)
	} else {
		var sessObj struct {
			State            string          `json:"state"`
			DeviceCredential json.RawMessage `json:"device_credential"`
		}
		if jerr := json.Unmarshal(sessRaw, &sessObj); jerr != nil || sessObj.State != "active" || len(sessObj.DeviceCredential) != 0 {
			t.Fatalf("expected active session with cleared device_credential after leaving official, got state=%q has_dc=%v", sessObj.State, len(sessObj.DeviceCredential) != 0)
		}
	}

	if code, _, _ := runCLI("config", "set", "sync.relay.access_key", `"selfhost-secret-A-value"`); code != 0 {
		t.Fatalf("set selfhostA key failed: exit=%d", code)
	}
	waitFor("selfhost-a SSE uses selfhost_a_key", 8*time.Second, func() bool {
		return hasRelayEvent("selfhost-a", "/events", "selfhost_a_key")
	})

	// M3: Same-origin key change (k1 -> k2) must immediately stop sending old k1.
	if code, _, _ := runCLI("config", "set", "sync.relay.access_key", `"selfhost-secret-A2-value"`); code != 0 {
		t.Fatalf("update selfhostA key failed: exit=%d", code)
	}
	waitFor("selfhost-a SSE updates to selfhost_a2_key", 8*time.Second, func() bool {
		return hasRelayEvent("selfhost-a", "/events", "selfhost_a2_key")
	})
	afterUpdateMark := recordCount()
	closeAllSSE()
	waitFor("selfhost-a post-update control/SSE use selfhost_a2_key", 8*time.Second, func() bool {
		evs := relayEventsSince(afterUpdateMark, "selfhost-a")
		return len(evs) > 0 && hasRelayEvent("selfhost-a", "/events", "selfhost_a2_key")
	})
	for _, ev := range relayEventsSince(afterUpdateMark, "selfhost-a") {
		if ev.KeyLabel == "selfhost_a_key" {
			t.Fatalf("stale selfhost_a_key observed after update to selfhost_a2_key: %+v", ev)
		}
	}

	// M3: Clearing access_key on self-hosted origin switches to key-free mode (key_label=none) with zero stale key.
	if code, _, _ := runCLI("config", "set", "sync.relay.access_key", `""`); code != 0 {
		t.Fatalf("clear selfhostA key failed: exit=%d", code)
	}
	keyClearMark := recordCount()
	closeAllSSE()
	waitFor("selfhost-a SSE reconnects key-free (key_label=none)", 8*time.Second, func() bool {
		for _, ev := range relayEventsSince(keyClearMark, "selfhost-a") {
			if ev.Path == "/events" && ev.KeyLabel == "none" {
				return true
			}
		}
		return false
	})
	for _, ev := range relayEventsSince(keyClearMark, "selfhost-a") {
		if ev.KeyLabel == "selfhost_a_key" || ev.KeyLabel == "selfhost_a2_key" {
			t.Fatalf("stale selfhost key leaked after clearing key on selfhost-a: %+v", ev)
		}
	}

	// M3: Turning off sync.relay.enabled or sync.enabled stops control-plane and SSE sends cleanly.
	if code, _, _ := runCLI("config", "set", "sync.relay.enabled", "false"); code != 0 {
		t.Fatalf("disable sync.relay.enabled failed: exit=%d", code)
	}
	closeAllSSE()
	time.Sleep(250 * time.Millisecond)
	relayDisabledMark := recordCount()
	time.Sleep(600 * time.Millisecond)
	if evs := relayEventsSince(relayDisabledMark, "selfhost-a"); len(evs) != 0 {
		t.Fatalf("expected 0 selfhost-a requests while sync.relay.enabled=false, got %+v", evs)
	}
	if code, _, _ := runCLI("config", "set", "sync.relay.enabled", "true"); code != 0 {
		t.Fatalf("re-enable sync.relay.enabled failed: exit=%d", code)
	}
	waitFor("selfhost-a SSE resumes after re-enabling sync.relay.enabled", 8*time.Second, func() bool {
		for _, ev := range relayEventsSince(relayDisabledMark, "selfhost-a") {
			if ev.Path == "/events" && ev.KeyLabel == "none" {
				return true
			}
		}
		return false
	})

	if code, _, _ := runCLI("config", "set", "sync.enabled", "false"); code != 0 {
		t.Fatalf("disable sync.enabled failed: exit=%d", code)
	}
	closeAllSSE()
	time.Sleep(250 * time.Millisecond)
	syncDisabledMark := recordCount()
	time.Sleep(600 * time.Millisecond)
	if evs := relayEventsSince(syncDisabledMark, "selfhost-a"); len(evs) != 0 {
		t.Fatalf("expected 0 selfhost-a requests while sync.enabled=false, got %+v", evs)
	}
	if code, _, _ := runCLI("config", "set", "sync.enabled", "true"); code != 0 {
		t.Fatalf("re-enable sync.enabled failed: exit=%d", code)
	}
	if code, _, _ := runCLI("config", "set", "sync.relay.access_key", `"selfhost-secret-A-value"`); code != 0 {
		t.Fatalf("restore selfhostA key before origin switch failed: exit=%d", code)
	}
	waitFor("selfhost-a SSE restores selfhost_a_key", 8*time.Second, func() bool {
		return hasRelayEvent("selfhost-a", "/events", "selfhost_a_key")
	})

	// Switching origin without specifying access_key must clear selfhost_a_key.
	if code, _, _ := runCLI("config", "set", "sync.relay.url", fmt.Sprintf("%q", selfHostB.URL)); code != 0 {
		t.Fatalf("set selfhostB url failed: exit=%d", code)
	}
	waitFor("selfhost-b SSE receives no old key (key_label=none)", 8*time.Second, func() bool {
		return hasRelayEvent("selfhost-b", "/events", "none")
	})

	// ---- Step 6c: Signed-in return SelfHost-B -> Official while Cloud 503 holds relay cleanly across restart ----
	mu.Lock()
	transientFailCount = 100
	mu.Unlock()
	hold503Mark := recordCount()
	codeHold, cfgHoldOut, _ := runCLI("--json", "config", "set", "sync.relay.url", `""`)
	if codeHold != 0 || !strings.Contains(cfgHoldOut, `"enabled":true`) {
		t.Fatalf("expected signed-in 503 hold to preserve sync.relay.enabled=true: exit=%d out_has_enabled=%v",
			codeHold, strings.Contains(cfgHoldOut, `"enabled":true`))
	}
	time.Sleep(700 * time.Millisecond)
	if evs := relayEventsSince(hold503Mark, "official"); len(evs) != 0 {
		t.Fatalf("expected 0 official requests during persistent 503 hold, got %+v", evs)
	}
	if code, statusOut, _ := runCLI("cloud", "status", "--json"); code != 0 || !strings.Contains(statusOut, `"state":"active"`) {
		t.Fatalf("expected session state to stay active during 503 hold: exit=%d", code)
	}

	// Restart unicd while Cloud 503 persists -> still 0 official requests, session stays active, config stays enabled=true.
	if code, _, _ := runCLI("daemon", "restart", "--json"); code != 0 {
		t.Fatalf("daemon restart during 503 hold failed: exit=%d", code)
	}
	time.Sleep(700 * time.Millisecond)
	if evs := relayEventsSince(hold503Mark, "official"); len(evs) != 0 {
		t.Fatalf("expected 0 official requests after restart during persistent 503, got %+v", evs)
	}
	if code, cfgHoldAfterRestart, _ := runCLI("config", "--json"); code != 0 || !strings.Contains(cfgHoldAfterRestart, `"enabled":true`) {
		t.Fatalf("expected sync.relay.enabled=true preserved across restart during 503: exit=%d", code)
	}
	if code, statusOut, _ := runCLI("cloud", "status", "--json"); code != 0 || !strings.Contains(statusOut, `"state":"active"`) {
		t.Fatalf("expected session state=active preserved across restart during 503: exit=%d", code)
	}

	// ---- Step 7: Cloud recovers -> first official request uses fresh dc_4, ahead of manual key ----
	mu.Lock()
	transientFailCount = 0
	mu.Unlock()
	waitFor("first official request after 503 recovery uses fresh dc_4", 8*time.Second, func() bool {
		evs := relayEventsSince(hold503Mark, "official")
		return len(evs) > 0 && evs[0].KeyLabel == "dc_4"
	})
	firstOfficial := relayEventsSince(hold503Mark, "official")
	if len(firstOfficial) == 0 || firstOfficial[0].KeyLabel != "dc_4" {
		t.Fatalf("expected very first post-recovery official request to use fresh dc_4, got %+v", firstOfficial)
	}
	if code, _, _ := runCLI("config", "set", "sync.relay.access_key", `"uc_ak_manual_fallback_secret"`); code != 0 {
		t.Fatalf("set manual key failed: exit=%d", code)
	}

	restartMark := recordCount()
	if code, _, _ := runCLI("daemon", "restart", "--json"); code != 0 {
		t.Fatalf("daemon restart failed: exit=%d", code)
	}
	waitFor("after restart official relay keeps managed dc_4 ahead of manual key", 8*time.Second, func() bool {
		evs := relayEventsSince(restartMark, "official")
		return len(evs) > 0 && evs[0].KeyLabel == "dc_4"
	})

	if code, _, _ := runCLI("cloud", "logout", "--json"); code != 0 {
		t.Fatalf("logout failed: exit=%d", code)
	}
	closeAllSSE()
	waitFor("official relay falls back to manual_key after logout", 8*time.Second, func() bool {
		return hasRelayEvent("official", "/events", "manual_key")
	})

	// ---- Step 7b: Signed-out official clearing manual key + SelfHost -> Official without fallback disable relay ----
	if code, _, _ := runCLI("config", "set", "sync.relay.access_key", `""`); code != 0 {
		t.Fatalf("clear manual key failed: exit=%d", code)
	}
	closeAllSSE()
	time.Sleep(250 * time.Millisecond)
	officialClearMark := recordCount()
	time.Sleep(500 * time.Millisecond)
	if evs := relayEventsSince(officialClearMark, "official"); len(evs) != 0 {
		t.Fatalf("expected 0 official requests after clearing signed-out manual key without env fallback, got %+v", evs)
	}
	if code, _, _ := runCLI("config", "set", "sync.relay.url", fmt.Sprintf("%q", selfHostB.URL)); code != 0 {
		t.Fatalf("set selfhostB before signed-out return failed: exit=%d", code)
	}
	if code, _, _ := runCLI("config", "set", "sync.relay.enabled", "true"); code != 0 {
		t.Fatalf("enable selfhostB failed: exit=%d", code)
	}
	signedOutMark := recordCount()
	codeRet, cfgRetOut, _ := runCLI("--json", "config", "set", "sync.relay.url", `""`)
	if codeRet != 0 || !strings.Contains(cfgRetOut, `"enabled":false`) {
		t.Fatalf("expected signed-out return to official without fallback to immediately disable relay: exit=%d has_disabled=%v",
			codeRet, strings.Contains(cfgRetOut, `"enabled":false`))
	}
	if code, _, _ := runCLI("daemon", "restart", "--json"); code != 0 {
		t.Fatalf("daemon restart after signed-out disable failed: exit=%d", code)
	}
	time.Sleep(400 * time.Millisecond)
	if evs := relayEventsSince(signedOutMark, "official"); len(evs) != 0 {
		t.Fatalf("expected 0 unauthenticated official requests after signed-out return, got %+v", evs)
	}

	// ---- Step 8a: Running managed credential crosses expires_at_ms during persistent Cloud 503 ----
	mu.Lock()
	rotateDelay = 300 * time.Millisecond
	credTTL = 1200 * time.Millisecond
	mu.Unlock()
	_ = loginViaCLI()
	waitFor("official relay SSE sees dc_5", 8*time.Second, func() bool {
		return hasRelayEvent("official", "/events", "dc_5")
	})

	mu.Lock()
	transientFailCount = 100
	callsBeforeExpiry := credIssueCalls
	mu.Unlock()
	// Wait past dc_5 expires_at_ms while Cloud returns 503 on rotation attempts.
	time.Sleep(1500 * time.Millisecond)
	closeAllSSE()
	expiryMark := recordCount()
	time.Sleep(4200 * time.Millisecond)

	if evs := relayEventsSince(expiryMark, "official"); len(evs) != 0 {
		t.Fatalf("expected 0 official requests after running credential expired during 503, got %+v", evs)
	}
	mu.Lock()
	expiryIssueRetries := credIssueCalls - callsBeforeExpiry
	transientFailCount = 0
	mu.Unlock()
	if expiryIssueRetries == 0 {
		t.Fatalf("expected background rotation retries during 503 expiry outage, got 0")
	}
	if code, cfgOut, _ := runCLI("config", "--json"); code != 0 || !strings.Contains(cfgOut, `"enabled":true`) {
		t.Fatalf("expected user sync.relay.enabled=true preserved across expired credential 503 hold: exit=%d", code)
	}
	if code, statusOut, _ := runCLI("cloud", "status", "--json"); code != 0 || !strings.Contains(statusOut, `"state":"active"`) {
		t.Fatalf("expected session state=active preserved across expired credential 503 hold: exit=%d", code)
	}

	waitFor("first official request after expiry outage recovery uses fresh dc_6", 8*time.Second, func() bool {
		evs := relayEventsSince(expiryMark, "official")
		return len(evs) > 0 && evs[0].KeyLabel == "dc_6"
	})
	firstPostExpiry := relayEventsSince(expiryMark, "official")
	if len(firstPostExpiry) == 0 || firstPostExpiry[0].KeyLabel != "dc_6" {
		t.Fatalf("expected first post-expiry recovery request to carry dc_6, got %+v", firstPostExpiry)
	}

	// ---- Step 8b: Authoritative revocation -> reauth_required & official relay disabled ----

	mu.Lock()
	revokeMode = true
	rejectKeys[curDeviceCred] = true
	mu.Unlock()
	closeAllSSE()

	waitFor("authoritative revocation flips state to reauth_required and disables relay", 8*time.Second, func() bool {
		code, statusOut, _ := runCLI("cloud", "status", "--json")
		if code != 0 || !strings.Contains(statusOut, `"state":"reauth_required"`) || !strings.Contains(statusOut, `"reauth_required":true`) || !strings.Contains(statusOut, `e2e-user@example.com`) {
			return false
		}
		assertNoSecretLeak("reauth_required status", statusOut)
		_, cfgOut, _ := runCLI("config", "--json")
		return strings.Contains(cfgOut, `"enabled":false`)
	})

	if fi, serr := os.Stat(sessionPath); serr != nil {
		t.Fatalf("stat reauth_required session failed: %v", serr)
	} else if fi.Mode().Perm() != 0o600 {
		t.Fatalf("cloud-session.json mode = %o, want 0600", fi.Mode().Perm())
	}
	if sessRaw, rerr := os.ReadFile(sessionPath); rerr != nil {
		t.Fatalf("read reauth_required session failed: %v", rerr)
	} else {
		assertNoSecretLeak("cloud-session.json after revocation", string(sessRaw))
	}
	if code, daemonLogs, _ := runCLI("daemon", "logs"); code != 0 {
		t.Fatalf("daemon logs failed: exit=%d", code)
	} else {
		assertNoSecretLeak("unicd logs", daemonLogs)
	}

	// ---- Step 9: Token exchange transport failure emits fixed safe diagnostic without underlying transport details ----
	brokenCloud := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/cli/authorize":
			q := r.URL.Query()
			http.Redirect(w, r, q.Get("redirect_uri")+"?code=bad-code&state="+q.Get("state"), http.StatusSeeOther)
		case "/api/auth/token":
			if hj, ok := w.(http.Hijacker); ok {
				conn, _, herr := hj.Hijack()
				if herr == nil {
					_ = conn.Close()
					return
				}
			}
		}
	}))
	defer brokenCloud.Close()
	{
		args := []string{"cloud", "login", "--cloud-url", brokenCloud.URL, "--no-browser", "--json"}
		cmd := exec.Command(filepath.Join(binDir, "unic"), args...)
		cmd.Env = baseEnv
		stderrBuf := &syncBuffer{}
		var stdout bytes.Buffer
		cmd.Stdout, cmd.Stderr = &stdout, stderrBuf
		if err := cmd.Start(); err != nil {
			t.Fatalf("start broken cloud login failed: %v", redactSecrets(err.Error()))
		}
		reURL := regexp.MustCompile(`https?://[^\s]+/cli/authorize\?[^\s]+`)
		var authURL string
		waitFor("broken cloud authorize URL printed", 5*time.Second, func() bool {
			m := reURL.FindString(stderrBuf.String())
			if m != "" {
				authURL = m
				return true
			}
			return false
		})
		resp, gerr := http.Get(authURL)
		if gerr == nil {
			_ = resp.Body.Close()
		}
		err := cmd.Wait()
		if err == nil {
			t.Fatalf("expected token exchange transport failure to fail CLI")
		}
		errOut := redactSecrets(stderrBuf.String() + "\n" + stdout.String())
		if !strings.Contains(errOut, "cloud token exchange request failed") {
			t.Fatalf("expected fixed safe diagnostic \"cloud token exchange request failed\", got redacted=%q", errOut)
		}
		for _, leaked := range []string{"EOF", "dial tcp", "connection refused", "Post \"", "127.0.0.1"} {
			// Skip authorize prompt line itself; check the final error line specifically.
			lines := strings.Split(strings.TrimSpace(errOut), "\n")
			lastLine := lines[len(lines)-1]
			if strings.Contains(lastLine, leaked) {
				t.Fatalf("token exchange error leaked underlying transport detail %q in %q", leaked, lastLine)
			}
		}
		transcript = append(transcript, "$ unic cloud login --cloud-url <broken> --no-browser --json -> safe_diagnostic=cloud token exchange request failed")
	}

	// ---- Step 9b: Real unic CLI rejects Cloud API cross-origin redirect without forwarding secrets ----
	{
		var evilCloudHits int
		evilTarget := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			mu.Lock()
			evilCloudHits++
			mu.Unlock()
			w.WriteHeader(http.StatusCreated)
		}))
		defer evilTarget.Close()
		redirCloud := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			switch r.URL.Path {
			case "/cli/authorize":
				q := r.URL.Query()
				http.Redirect(w, r, q.Get("redirect_uri")+"?code=redir-code&state="+q.Get("state"), http.StatusSeeOther)
			case "/api/auth/token":
				http.Redirect(w, r, evilTarget.URL+"/api/auth/token", http.StatusFound)
			}
		}))
		defer redirCloud.Close()

		args := []string{"cloud", "login", "--cloud-url", redirCloud.URL, "--no-browser", "--json"}
		cmd := exec.Command(filepath.Join(binDir, "unic"), args...)
		cmd.Env = baseEnv
		stderrBuf := &syncBuffer{}
		var stdout bytes.Buffer
		cmd.Stdout, cmd.Stderr = &stdout, stderrBuf
		if err := cmd.Start(); err != nil {
			t.Fatalf("start redirect cloud login failed: %v", redactSecrets(err.Error()))
		}
		reURL := regexp.MustCompile(`https?://[^\s]+/cli/authorize\?[^\s]+`)
		var authURL string
		waitFor("redirect cloud authorize URL printed", 5*time.Second, func() bool {
			m := reURL.FindString(stderrBuf.String())
			if m != "" {
				authURL = m
				return true
			}
			return false
		})
		resp, gerr := http.Get(authURL)
		if gerr == nil {
			_ = resp.Body.Close()
		}
		if err := cmd.Wait(); err == nil {
			t.Fatalf("expected redirected token exchange to fail CLI")
		}
		mu.Lock()
		hits := evilCloudHits
		mu.Unlock()
		if hits != 0 {
			t.Fatalf("expected 0 requests forwarded to redirected Cloud origin, got %d", hits)
		}
		transcript = append(transcript, "$ unic cloud login --cloud-url <redirect> --no-browser --json -> rejected off-origin redirect")
	}

	mu.Lock()
	for _, rec := range relayRecords {
		if strings.HasPrefix(rec.KeyLabel, "LEAKED:") {
			t.Fatalf("managed credential leaked to non-official target: %+v", rec)
		}
		if strings.HasPrefix(rec.KeyLabel, "REDIRECTED:") {
			t.Fatalf("credential reached cross-origin redirect target: %+v", rec)
		}
		if rec.Target == "selfhost-b" && rec.KeyLabel == "selfhost_a_key" {
			t.Fatalf("selfhost-a access_key leaked to selfhost-b: %+v", rec)
		}
	}
	recSnapshot := append([]relayRequestRecord(nil), relayRecords...)
	mu.Unlock()

	countClasses := func(evs []relayRequestRecord, wantKey string) (ctrl, sse int) {
		for _, ev := range evs {
			if ev.KeyLabel == wantKey && ev.Status == http.StatusOK {
				if ev.Path == "/events" {
					sse++
				} else {
					ctrl++
				}
			}
		}
		return ctrl, sse
	}
	duringCtrl, duringSSE := countClasses(duringSlowEvents, "dc_1")
	afterCtrl, afterSSE := countClasses(afterSlowEvents, "dc_2")

	if dest := os.Getenv("UNIC_E2E_ARTIFACTS"); dest != "" {
		if err := os.MkdirAll(dest, 0o700); err != nil {
			t.Fatal(err)
		}
		receipt := map[string]any{
			"relay_events":       recSnapshot,
			"revoked_cred_calls": revokedCredCount,
			"revoked_sess_calls": revokedSessCount,
			"slow_rotation_probe": map[string]any{
				"during_slow_rotate": map[string]any{
					"control_dc_1": duringCtrl,
					"sse_dc_1":     duringSSE,
					"events":       duringSlowEvents,
				},
				"after_release": map[string]any{
					"control_dc_2": afterCtrl,
					"sse_dc_2":     afterSSE,
					"events":       afterSlowEvents,
				},
			},
			"commands": transcript,
			"replay":   "TMPDIR=/private/tmp UNIC_E2E=1 go test -race ./e2e/cli -run '^TestCloudRelayCredentialLifecycle$' -count=1 -v",
		}
		raw, _ := json.MarshalIndent(receipt, "", "  ")
		assertNoSecretLeak("artifacts receipt", string(raw))
		if err := os.WriteFile(filepath.Join(dest, "cloud-relay-receipt.json"), append(raw, '\n'), 0o600); err != nil {
			t.Fatal(err)
		}
	}
}
