package e2e

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/localcontent"
)

func TestMarketplaceLivePluginUpdate(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	work := shortTemp(t)
	const (
		id       = "example.live-update"
		actionID = "example.action-update"
	)
	repository := "https://github.com/example/live-update"
	commits := []string{strings.Repeat("a", 40), strings.Repeat("b", 40), strings.Repeat("c", 40)}
	actionCommits := []string{strings.Repeat("d", 40), strings.Repeat("e", 40)}
	responses := make(map[string][]byte)
	addVersion := func(index int, commit, repo, pluginID, capability, metadata string) {
		t.Helper()
		version := fmt.Sprintf("%d.0.0", index+1)
		binaryPath := filepath.Join(work, "fixture-"+version)
		build := exec.CommandContext(ctx, "go", "build", "-ldflags", "-X main.version="+version+" -X main.capability="+capability, "-o", binaryPath, "./e2e/cli/fixtures/pluginupdate")
		build.Dir = root
		if out, err := build.CombinedOutput(); err != nil {
			t.Fatalf("build fixture %s: %v\n%s", version, err, out)
		}
		binary, err := os.ReadFile(binaryPath)
		if err != nil {
			t.Fatal(err)
		}
		asset := pluginArchive(t, map[string][]byte{"plugin": binary})
		manifest := fmt.Sprintf(`manifest_version = 1
id = %q
version = %q
api_version = 1
kind = "go"
module = "example.com/live-update"
package = "."
capabilities = [%q]
%s
[[artifacts]]
goos = %q
goarch = %q
release = %q
asset = "plugin.tar.gz"
executable = "plugin"
sha256 = "%x"
`, pluginID, version, capability, strings.ReplaceAll(metadata, "<version>", version), runtime.GOOS, runtime.GOARCH, version, sha256.Sum256(asset))
		responses["/api.github.com/repos/example/"+repo+"/commits/"+commit] = fmt.Appendf(nil, `{"sha":%q}`, commit)
		responses["/codeload.github.com/example/"+repo+"/tar.gz/"+commit] = pluginArchive(t, map[string][]byte{"repo/spec.toml": []byte(manifest)})
		responses[path.Join("/github.com/example", repo, "releases/download", version, "plugin.tar.gz")] = asset
	}
	for index, commit := range commits {
		metadata := `external_channels = true
transport_kind = "object_store"`
		if index > 0 {
			metadata += "\nimage_files_as_images = true"
		}
		addVersion(index, commit, "live-update", id, "sync_backend", metadata)
	}
	for index, commit := range actionCommits {
		addVersion(index, commit, "action-update", actionID, "action", `[[actions]]
id = "show"
title = "Version <version>"
accepts = ["text"]
placements = ["history.item_menu"]
post_actions = ["none"]`)
	}
	github := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if data := responses[r.URL.Path]; data != nil {
			_, _ = w.Write(data)
			return
		}
		http.NotFound(w, r)
	}))
	defer github.Close()
	t.Setenv("UNIC_E2E_GITHUB", github.URL)
	n := &node{home: filepath.Join(work, "home"), sock: filepath.Join(work, "u.sock")}
	if err := os.Mkdir(n.home, 0700); err != nil {
		t.Fatal(err)
	}
	if err := initializeFixtureProfile(filepath.Join(binDir, "unic"), n.home, n.env()); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(n.home, "config.toml"), []byte("[telemetry]\nenabled=false\n[plugins.instances.\"builtin.tailcat\"]\nenabled=false\n"), 0600); err != nil {
		t.Fatal(err)
	}
	cmd := exec.CommandContext(ctx, filepath.Join(binDir, "unicd"))
	cmd.Env = slices.DeleteFunc(n.env(), func(value string) bool { return strings.HasPrefix(value, "UNIC_MOCK_CLIPBOARD_DIR=") })
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = cmd.Process.Kill(); _ = cmd.Wait() }()
	for deadline := time.Now().Add(10 * time.Second); !n.listening(); {
		if time.Now().After(deadline) {
			t.Fatal("service did not start")
		}
		time.Sleep(25 * time.Millisecond)
	}
	client := api.NewClient(n.sock)
	var adapterSession string
	if err := client.AdapterCall(ctx, "", "register", api.AdapterRegistration{}, &adapterSession); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = client.AdapterCall(ctx, adapterSession, "close", nil, nil) }()
	adapter := &clipboardintegration.Remote{Client: client, Session: adapterSession}
	checkCapture := func(want bool) {
		t.Helper()
		got, err := adapter.WantsClipboardFileImage(ctx)
		if err != nil || got != want {
			t.Fatalf("capture interest: got %t, want %t: %v", got, want, err)
		}
	}
	checkCapture(false)
	install := func(index int) (api.PluginInfo, error) {
		return client.InstallPlugin(ctx, api.PluginInstallRequest{Repository: repository, Ref: commits[index], Replace: true})
	}
	if info, err := install(1); err != nil || info.Configured || info.RunningVersion != "" {
		t.Fatalf("expected a disabled installation with the capability: %+v: %v", info, err)
	}
	checkCapture(false)
	if _, err := install(0); err != nil {
		t.Fatal(err)
	}
	checkCapture(false)
	if _, err := client.StartPlugin(ctx, id); err != nil {
		t.Fatal(err)
	}
	cfg, err := client.Config(ctx)
	if err != nil {
		t.Fatal(err)
	}
	alias := id + ".alias"
	if _, err := client.CreatePluginInstance(ctx, api.PluginInstanceCreate{ID: alias, ImplementationID: id, Revision: cfg.Revision}); err != nil {
		t.Fatal(err)
	}
	if _, err := client.StartPlugin(ctx, alias); err != nil {
		t.Fatal(err)
	}
	channel, err := client.PairChannel(ctx, api.ChannelPairRequest{BackendID: id, Code: "012345"})
	if err != nil {
		t.Fatal(err)
	}
	aliasChannel, err := client.PairChannel(ctx, api.ChannelPairRequest{BackendID: alias, Code: "012345"})
	if err != nil {
		t.Fatal(err)
	}
	checkCapture(false)
	send := func(requestID string) string {
		t.Helper()
		result, err := client.Submit(ctx, api.SubmitInput{RequestID: requestID,
			Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(requestID)}}},
		})
		if err != nil || result.OperationID == "" {
			t.Fatalf("send %s: %+v: %v", requestID, result, err)
		}
		return string(result.OperationID)
	}
	checkTargets := func(operationID string, want engine.TargetState) {
		t.Helper()
		op, err := client.Operation(ctx, operationID)
		if err != nil || len(op.Targets) != 2 {
			t.Fatalf("expected both targets: %+v: %v", op, err)
		}
		for _, target := range op.Targets {
			if target.State != want {
				t.Fatalf("target state after plugin update: got %s, want %s", target.State, want)
			}
		}
	}
	interrupted := send("send-before-update")
	checkTargets(interrupted, engine.TargetQueued)
	updated, err := install(1)
	if err != nil || updated.Commit != commits[1] || updated.RunningVersion != "2.0.0" || updated.Restart {
		t.Fatalf("expected live version 2.0.0, got %+v: %v", updated, err)
	}
	checkTargets(interrupted, engine.TargetFailed)
	checkTargets(send("send-after-update"), engine.TargetSent)
	checkCapture(true)
	caps := engine.HostCapabilities{Observe: engine.AbilityAvailable, ReadFiles: true}
	if err := client.AdapterCall(ctx, adapterSession, "capabilities", caps, nil); err != nil {
		t.Fatal(err)
	}
	const epoch = "image-file-capture"
	if err := client.AdapterCall(ctx, adapterSession, "observe", engine.Observation{Epoch: epoch, Sequence: 1, Mode: engine.ObserveBaseline}, nil); err != nil {
		t.Fatal(err)
	}
	filePath := filepath.Join(work, "original.png")
	if err := os.WriteFile(filePath, []byte("different backing bytes"), 0600); err != nil {
		t.Fatal(err)
	}
	var pixels bytes.Buffer
	if err := png.Encode(&pixels, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	input, err := localcontent.Input(clip.Snapshot{
		Files: []clip.File{{Name: filepath.Base(filePath), Path: filePath}}, FileImagePNG: pixels.Bytes(),
	})
	if err != nil {
		t.Fatal(err)
	}
	handle, err := adapter.ImportContent(ctx, input)
	if err != nil {
		t.Fatal(err)
	}
	var observed engine.ObservationResult
	if err := client.AdapterCall(ctx, adapterSession, "observe", engine.Observation{
		Epoch: epoch, Sequence: 2, Mode: engine.ObserveChange, LeaseID: handle.LeaseID,
	}, &observed); err != nil || observed.OperationID == "" {
		t.Fatalf("the adapter image did not reach the declared backend: %+v: %v", observed, err)
	}
	checkTargets(string(observed.OperationID), engine.TargetSent)
	checkRunning := func() {
		t.Helper()
		list, err := client.Plugins(ctx)
		if err != nil || len(list) != 2 {
			t.Fatalf("expected both running plugin instances: %+v: %v", list, err)
		}
		for _, info := range list {
			if info.Commit != commits[1] || info.RunningVersion != "2.0.0" || info.Restart || info.PreviousVersion != "1.0.0" {
				t.Fatalf("wrong installation or runtime for %s: %+v", info.ID, info)
			}
		}
		status, err := client.Status(ctx)
		if err != nil {
			t.Fatal(err)
		}
		for _, existing := range []api.PeerStatus{channel, aliasChannel} {
			if !slices.ContainsFunc(status.Peers, func(p api.PeerStatus) bool { return p.ID == existing.ID }) {
				t.Fatal("update lost an existing channel")
			}
		}
	}
	checkRunning()
	if info, err := install(2); err == nil {
		t.Fatalf("candidate that failed startup was accepted: %+v", info)
	}
	checkRunning()
	checkCapture(true)
	for instance, existing := range map[string]api.PeerStatus{id: channel, alias: aliasChannel} {
		restored, err := client.PairChannel(ctx, api.ChannelPairRequest{BackendID: instance, Resume: true})
		if err != nil || restored.ID != existing.ID {
			t.Fatalf("restored process cannot resolve its existing channel: %v", err)
		}
	}
	for index, commit := range actionCommits {
		info, err := client.InstallPlugin(ctx, api.PluginInstallRequest{
			Repository: "https://github.com/example/action-update", Ref: commit, Replace: index > 0,
		})
		if err != nil {
			t.Fatal(err)
		}
		version := fmt.Sprintf("%d.0.0", index+1)
		if index == 0 {
			info, err = client.StartPlugin(ctx, actionID)
		}
		if err != nil || info.RunningVersion != version || info.Restart {
			t.Fatalf("unexpected action runtime: %+v: %v", info, err)
		}
		actions, err := client.Actions(ctx)
		if err != nil {
			t.Fatal(err)
		}
		var titles []string
		for _, item := range actions {
			if item.Key == actionID+"/show" {
				titles = append(titles, item.Title)
			}
		}
		if !slices.Equal(titles, []string{"Version " + version}) {
			t.Fatalf("expected only the active action declaration, got %v", titles)
		}
	}
	if dest := os.Getenv("UNIC_E2E_ARTIFACTS"); dest != "" {
		if err := os.MkdirAll(dest, 0700); err != nil {
			t.Fatal(err)
		}
		data, err := json.MarshalIndent(map[string]any{
			"service_pid": cmd.Process.Pid, "installed_commit": updated.Commit, "instances": 2,
			"running_version": updated.RunningVersion, "channels_preserved": true,
			"failed_candidate_rejected": true, "action_replaced": true, "restart_required": false,
			"interrupted_transfers_resolved":   true,
			"capability_gated_clipboard_image": true,
		}, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dest, "marketplace-live-plugin-update.json"), data, 0600); err != nil {
			t.Fatal(err)
		}
	}
}
