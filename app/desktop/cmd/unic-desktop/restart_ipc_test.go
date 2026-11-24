package main

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func TestTrayRuntimeRestartCommandGuards(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	cfgPath := filepath.Join(t.TempDir(), "config.toml")
	mgr, err := config.Open(cfgPath, nil)
	if err != nil {
		t.Fatal(err)
	}

	newRuntime := func() *trayRuntime {
		r := &trayRuntime{
			ctx:         ctx,
			cancel:      cancel,
			dir:         t.TempDir(),
			socket:      filepath.Join(t.TempDir(), "unic.sock"),
			settings:    mgr,
			nativeReady: make(chan struct{}),
		}
		close(r.nativeReady)
		return r
	}

	t.Run("retiring returns unavailable", func(t *testing.T) {
		r := newRuntime()
		r.retiring = true
		if _, err := r.handler(ctx, "restart", nil); !errors.Is(err, desktopprocess.ErrUnavailable) {
			t.Fatalf("err = %v, want ErrUnavailable", err)
		}
	})

	t.Run("paused returns migration error", func(t *testing.T) {
		r := newRuntime()
		r.paused = true
		if _, err := r.handler(ctx, "restart", nil); err == nil {
			t.Fatal("expected error while paused for migration")
		}
	})

	t.Run("concurrent desktop transition rejects the restart", func(t *testing.T) {
		r := newRuntime()
		r.opening.Lock()
		defer r.opening.Unlock()
		if _, err := r.handler(ctx, "restart", nil); err == nil {
			t.Fatal("expected error while another desktop transition holds the lock")
		}
	})
}
