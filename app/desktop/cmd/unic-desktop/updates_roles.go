package main

import (
	"context"
	"io"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
)

// Verify the complete signed archive before retiring other processes. The
// updater may replace the executable immediately after Open returns, so all
// old-role IPC and owned-service shutdown must finish before that return.
type desktopUpdateSource struct {
	mygo.UpdateSource
	runtime *trayRuntime
}

func (s *desktopUpdateSource) Open(ctx context.Context, url string) (io.ReadCloser, error) {
	archive, err := s.UpdateSource.Open(ctx, url)
	if err != nil {
		return nil, err
	}
	s.runtime.opening.Lock()
	defer s.runtime.opening.Unlock()
	wasPaused := s.runtime.pauseAdmission()
	if s.runtime.shortcut != nil {
		s.runtime.shortcut(false)
	}
	if err = s.runtime.closeViews(ctx, desktopprocess.QuickPanel, desktopprocess.GUI); err == nil {
		err = s.runtime.pause(false)
	}
	if err != nil {
		_ = archive.Close()
		s.runtime.mu.Lock()
		s.runtime.paused = wasPaused
		s.runtime.mu.Unlock()
		if !wasPaused {
			_ = s.runtime.start(ctx, false)
		}
		if s.runtime.shortcut != nil {
			state := s.runtime.shortcut(false)
			s.runtime.mu.Lock()
			s.runtime.state.Shortcut = state
			s.runtime.mu.Unlock()
		}
		return nil, err
	}
	return archive, nil
}
