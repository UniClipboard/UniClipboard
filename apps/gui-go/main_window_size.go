package main

import (
	"context"
	"log"
	"net/http"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
)

// mainWindowSizeState holds unsaved native state on the Wails main thread.
// The daemon owns the persisted preference; the host never opens its file.
type mainWindowSizeState struct {
	window *application.WebviewWindow
	normal settings.WindowSize
}

func (h *HostService) loadMainWindowSize() settings.WindowSize {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	view, err := settings.Read(ctx, h.daemon())
	if err != nil {
		log.Printf("load main window preference: %v; using defaults", err)
		return settings.WindowSize{Width: 1100, Height: 720}
	}
	return view.General.MainWindowSize
}

func (s *mainWindowSizeState) capture() {
	// InvokeSync also runs directly when already on the Wails main thread.
	application.InvokeSync(func() {
		if s.window.IsMinimised() || s.window.IsMaximised() || s.window.IsFullscreen() {
			return
		}
		width, height := s.window.Size()
		if width > 0 && height > 0 {
			s.normal = settings.WindowSize{Width: width, Height: height}
		}
	})
}

func (h *HostService) rememberMainWindowSize(w *application.WebviewWindow, size settings.WindowSize) {
	state := &mainWindowSizeState{window: w, normal: size}
	application.InvokeSync(func() { h.windowSize = state })
	w.OnWindowEvent(events.Common.WindowDidResize, func(*application.WindowEvent) { state.capture() })
}

// persistMainWindowSize flushes native state before the daemon is stopped or
// the GUI exits into lightweight mode. Desktop settings serialize and atomically
// persist this patch just like every other desktop preference, on all platforms.
func (h *HostService) persistMainWindowSize() {
	var size *settings.WindowSize
	application.InvokeSync(func() {
		if h.windowSize != nil {
			h.windowSize.capture()
			value := h.windowSize.normal
			size = &value
		}
	})
	if size == nil {
		return // A silent launch never created a window or a preference to save.
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	patch := map[string]any{"general": map[string]any{"mainWindowSize": size}}
	client := daemonclient.Follow(&h.client)
	if err := client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, nil); err != nil {
		log.Printf("save main window preference: %v", err)
	}
}
