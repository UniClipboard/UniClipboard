package desktopui

import (
	"context"
	"encoding/json"
	"net"
	"net/http"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func scenarioWeTypePairing(t *testing.T) {
	const pluginID = "unic.wxime"
	const space = "个人共享空间"
	peer := api.PeerStatus{ID: "fixture-space", Name: space, BackendID: pluginID, Kind: "external", Enabled: true}
	var running, paired atomic.Bool
	var calls atomic.Int32
	requests := make(chan api.ChannelPairRequest, 4)
	finish := make(chan struct{})
	reply := func(value any) http.HandlerFunc {
		return func(w http.ResponseWriter, _ *http.Request) {
			_ = json.NewEncoder(w).Encode(value)
		}
	}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/history", reply(api.HistoryPage{}))
	mux.HandleFunc("POST /v1/history/search", reply(api.HistorySearchPage{}))
	mux.HandleFunc("GET /v1/config", reply(config.State{Revision: "fixture", Config: config.Default()}))
	mux.HandleFunc("GET /v1/plugins/marketplace", reply(api.PluginMarketplace{}))
	mux.HandleFunc("GET /v1/sync-backends", func(w http.ResponseWriter, r *http.Request) {
		var backends []api.SyncBackendInfo
		if running.Load() {
			backends = []api.SyncBackendInfo{{ID: pluginID, ImplementationID: pluginID, ExternalChannels: true, State: "running"}}
		}
		reply(backends)(w, r)
	})
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		status := api.Status{ID: "fixture"}
		if paired.Load() {
			status.Peers = []api.PeerStatus{peer}
		}
		reply(status)(w, r)
	})
	mux.HandleFunc("GET /v1/plugins", func(w http.ResponseWriter, r *http.Request) {
		info := api.PluginInfo{
			ID: pluginID, ImplementationID: pluginID, Name: "微信输入法", Kind: "go",
			Capabilities: []string{"sync_backend"}, ExternalChannels: true, Configured: true,
			Restart: !running.Load(),
		}
		if !info.Restart {
			info.RunningVersion = "0.1.0"
		}
		reply([]api.PluginInfo{info})(w, r)
	})
	mux.HandleFunc("POST /v1/channels/pair", func(w http.ResponseWriter, r *http.Request) {
		var in api.ChannelPairRequest
		if json.NewDecoder(r.Body).Decode(&in) != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		calls.Add(1)
		requests <- in
		if in.Code != "012345" {
			w.WriteHeader(http.StatusBadRequest)
			reply(api.ErrorBody{Reason: "invalid_argument"})(w, r)
			return
		}
		select {
		case <-finish:
			paired.Store(true)
			reply(peer)(w, r)
		case <-r.Context().Done():
		}
	})
	sock := filepath.Join(t.TempDir(), "unic.sock")
	listener, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	server := &http.Server{Handler: mux}
	go func() { _ = server.Serve(listener) }()
	t.Cleanup(func() { _ = server.Close() })

	desktop := NewDesktop(context.Background(), api.NewClient(sock))
	t.Cleanup(desktop.Close)
	d := desktop.d
	d.openPluginSettings(pluginID)
	view := ui.NewTesterWith(desktop.View, 1120, 860, ui.TesterOptions{})
	proof.Capture(t, view)
	wait := func(label string, condition func() bool) {
		t.Helper()
		for deadline := time.Now().Add(5 * time.Second); time.Now().Before(deadline); {
			d.live.poll(d)
			view.Advance(20 * time.Millisecond)
			if condition() {
				return
			}
			time.Sleep(15 * time.Millisecond)
		}
		t.Fatalf("timed out waiting for %s", label)
	}
	click := func(label string) {
		t.Helper()
		if err := view.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	inputVisible := func() bool {
		_, ok := view.AccessByRole(ui.RoleTextField, "6 位配对码")
		return ok
	}
	checkRequest := func(code string) {
		t.Helper()
		select {
		case in := <-requests:
			if in.BackendID != pluginID || in.Code != code || in.Resume {
				t.Fatal("pairing did not use the expected code and default instance")
			}
		case <-time.After(5 * time.Second):
			t.Fatal("pairing did not reach the local service")
		}
	}
	wait("restart guidance", func() bool { return view.HasText("重启后即可输入配对码。") })
	if inputVisible() {
		t.Fatal("pairing accepts a code before restart")
	}
	running.Store(true)
	d.live.nextRefresh = time.Time{}
	wait("pairing code field", inputVisible)
	if _, found := view.AccessByRole(ui.RoleTextField, "实例 ID"); found {
		t.Fatal("default pairing shows an instance ID field")
	}
	click("6 位配对码")
	view.Type("123")
	view.Key(0, ui.KeyEnter)
	button, ok := view.AccessByRole(ui.RoleButton, "加入共享空间")
	if !ok || !button.Disabled || calls.Load() != 0 {
		t.Fatal("an incomplete code allowed a pairing request")
	}
	view.Type("456")
	click("加入共享空间")
	checkRequest("123456")
	wait("invalid-code feedback", func() bool { return view.HasText("配对码无效或已过期。") })
	click("6 位配对码")
	view.Type("012345")
	click("加入共享空间")
	checkRequest("012345")
	wait("pending pairing", func() bool {
		a, ok := view.AccessByRole(ui.RoleButton, "正在连接…")
		return ok && a.Disabled
	})
	view.Key(0, ui.KeyEnter)
	close(finish)
	wait("joined space", func() bool { return view.HasText(space) && view.HasText("已加入共享空间") })
	if inputVisible() || calls.Load() != 2 {
		t.Fatal("joined space still allows another pairing request")
	}
}
