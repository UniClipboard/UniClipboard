package mobile

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/engine"
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
