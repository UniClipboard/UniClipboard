package main

import (
	"context"
	"log"
	"net/http"
	"runtime"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
)

// mainWindowSizeState holds unsaved native state on the Wails main thread.
// The daemon owns the persisted preference; the host never opens its file.
type mainWindowSizeState struct {
	window      *application.WebviewWindow
	normal      settings.WindowSize
	maximized   bool
	provisional bool
	touched     bool
}

func (h *HostService) loadMainWindowSize() (settings.WindowSize, bool, bool) {
	if h.daemon() == nil {
		return settings.WindowSize{Width: 1100, Height: 720}, false, true
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	view, err := settings.Read(ctx, h.daemon())
	if err != nil {
		log.Printf("load main window preference: %v; using defaults", err)
		return settings.WindowSize{Width: 1100, Height: 720}, false, false
	}
	return view.General.MainWindowSize, view.General.MainWindowMaximized, false
}

func (s *mainWindowSizeState) capture() {
	// InvokeSync also runs directly when already on the Wails main thread.
	application.InvokeSync(func() {
		if s.window.IsMinimised() || s.window.IsFullscreen() {
			return
		}
		maximized := s.window.IsMaximised()
		if maximized != s.maximized {
			s.touched = true
		}
		s.maximized = maximized
		if s.maximized {
			return
		}
		width, height := s.window.Size()
		if width > 0 && height > 0 {
			if width != s.normal.Width || height != s.normal.Height {
				s.touched = true
			}
			s.normal = settings.WindowSize{Width: width, Height: height}
		}
	})
}

func (h *HostService) rememberMainWindowSize(w *application.WebviewWindow, size settings.WindowSize, maximized, provisional bool) {
	state := &mainWindowSizeState{window: w, normal: size, maximized: maximized, provisional: provisional}
	application.InvokeSync(func() { h.windowSize = state })
	w.OnWindowEvent(events.Common.WindowDidResize, func(*application.WindowEvent) { state.capture() })
}

func (h *HostService) restoreProvisionalMainWindowSize() {
	if h.daemon() == nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	view, err := settings.Read(ctx, h.daemon())
	if err != nil {
		log.Printf("restore main window preference: %v", err)
		return
	}
	application.InvokeSync(func() {
		state := h.windowSize
		if state == nil || !state.provisional || state.touched {
			return
		}
		size := view.General.MainWindowSize
		minWidth := 900
		if runtime.GOOS == "linux" {
			minWidth = 640
		}
		state.provisional = false
		state.normal = size
		state.maximized = view.General.MainWindowMaximized
		state.window.SetSize(max(size.Width, minWidth), max(size.Height, 600))
		if state.maximized {
			state.window.Maximise()
		}
	})
}

// persistMainWindowSize flushes native state before the daemon is stopped or
// the GUI exits into lightweight mode. Desktop settings serialize and atomically
// persist this patch just like every other desktop preference, on all platforms.
func (h *HostService) persistMainWindowSize() {
	if h.ui != nil {
		h.ui.saveWindowSize()
		return
	}
	if h.daemon() == nil {
		return
	}
	var size *settings.WindowSize
	var maximized bool
	application.InvokeSync(func() {
		if h.windowSize != nil {
			if h.windowSize.provisional && !h.windowSize.touched {
				return
			}
			h.windowSize.capture()
			value := h.windowSize.normal
			size = &value
			maximized = h.windowSize.maximized
		}
	})
	if size == nil {
		return // A silent launch never created a window or a preference to save.
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	patch := map[string]any{"general": map[string]any{"mainWindowSize": size, "mainWindowMaximized": maximized}}
	client := daemonclient.Follow(&h.client)
	if err := client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, nil); err != nil {
		log.Printf("save main window preference: %v", err)
	}
}
