package main

import (
	"context"
	"encoding/json"
	"path/filepath"
	"testing"

	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestTrayRuntimeUpdateCommands(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	cfgPath := filepath.Join(t.TempDir(), "config.toml")
	mgr, err := config.Open(cfgPath, nil)
	if err != nil {
		t.Fatal(err)
	}

	checked := 0
	current := updateStatus{
		Supported:          true,
		Owner:              "direct",
		AutoCheckUpdate:    true,
		AutoDownloadUpdate: false,
	}
	r := &trayRuntime{
		ctx:         ctx,
		cancel:      cancel,
		dir:         t.TempDir(),
		socket:      filepath.Join(t.TempDir(), "unic.sock"),
		settings:    mgr,
		nativeReady: make(chan struct{}),
		state:       roleStatus{Updates: current},
		checkUpdates: func() {
			checked++
		},
		updatePrefs: func(req updatePrefsRequest) updateStatus {
			if req.AutoCheckUpdate != nil {
				current.AutoCheckUpdate = *req.AutoCheckUpdate
			}
			if req.AutoDownloadUpdate != nil {
				current.AutoDownloadUpdate = *req.AutoDownloadUpdate
			}
			return current
		},
	}
	close(r.nativeReady)

	if _, err := r.handler(ctx, "check-updates", nil); err != nil {
		t.Fatalf("check-updates failed: %v", err)
	}
	if checked != 1 {
		t.Fatalf("checked = %d, want 1", checked)
	}

	off := false
	on := true
	raw, err := json.Marshal(updatePrefsRequest{AutoCheckUpdate: &off, AutoDownloadUpdate: &on})
	if err != nil {
		t.Fatal(err)
	}
	reply, err := r.handler(ctx, "set-update-prefs", raw)
	if err != nil {
		t.Fatalf("set-update-prefs failed: %v", err)
	}
	got, ok := reply.(updateStatus)
	if !ok {
		t.Fatalf("unexpected reply type %T", reply)
	}
	if got.AutoCheckUpdate || !got.AutoDownloadUpdate {
		t.Fatalf("unexpected reply status: %+v", got)
	}
	st := r.status()
	if st.Updates.AutoCheckUpdate || !st.Updates.AutoDownloadUpdate || !st.Updates.Supported || st.Updates.Owner != "direct" {
		t.Fatalf("tray status did not reflect updated prefs: %+v", st.Updates)
	}
}
