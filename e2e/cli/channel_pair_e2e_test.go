package e2e

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestExternalChannelPairing(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	t.Setenv("UNIC_E2E_SYNC_FOLLOWUP", "1")
	h := newObservabilityRun(t)
	h.artifactName = "external-channel-pairing"
	syncFollowupStart(t, h, "[telemetry]\nenabled=false\n")
	h.run(t, "pair-external-channel", func(t *testing.T) {
		run := func(code string) ([]byte, error) {
			exe := h.cliExecutable
			if exe == "" {
				exe = filepath.Join(binDir, "unic")
			}
			cmd := exec.Command(exe, "channel", "pair", "test.externalfollowup")
			cmd.Env = h.env()
			cmd.Stdin = strings.NewReader(code + "\n")
			return cmd.CombinedOutput()
		}
		if out, err := run("999999"); err == nil || strings.Contains(string(out), "999999") {
			t.Fatal("rejected pairing succeeded or exposed its code")
		}
		var firstID string
		for range 2 {
			out, err := run("012345")
			if err != nil {
				t.Fatalf("pair channel: %v, %s", err, out)
			}
			var peer api.PeerStatus
			if json.Unmarshal(out, &peer) != nil || peer.ID == "" || peer.Kind != "external" || !peer.Enabled || peer.RemoteRevocation || strings.Contains(string(out), "012345") {
				t.Fatal("invalid or sensitive channel projection")
			}
			if firstID != "" && firstID != peer.ID {
				t.Fatal("retry created a second channel")
			}
			firstID = peer.ID
		}
		if code, _ := h.request(t, "POST", "/test/followup/external", ""); code != 200 {
			t.Fatal("paired channel did not accept external content")
		}
		h.assertions = append(h.assertions, map[string]any{"scenario": "external-channel-pairing", "pair_retry_same_channel": true, "secret_redaction": true, "external_receive": true})
	})
}
