//go:build e2e && linux && gtk3

package main

/*
#cgo pkg-config: gtk+-3.0
#include <gtk/gtk.h>

static int uc_decorated(void *w) { return gtk_window_get_decorated(GTK_WINDOW(w)); }
static int uc_active(void *w) { return gtk_window_is_active(GTK_WINDOW(w)); }
static int uc_visible(void *w) { return gtk_widget_get_visible(GTK_WIDGET(w)); }
static void uc_size(void *w, int *width, int *height) { gtk_window_get_size(GTK_WINDOW(w), width, height); }
*/
import "C"

import (
	"unsafe"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// windowFrameDetail is the e2e probe of a window's frame and focus as GTK reports them: whether the toolkit draws a title
// bar around it (the claim `set_window_decorations` must change), whether it is the active toplevel, and its size.
// It is a claim by the process under test; compositor-side evidence is collected independently by the driver.
func windowFrameDetail(h *HostService, name string) map[string]any {
	w, ok := h.app.Window.GetByName(name)
	if !ok {
		return map[string]any{"window": name, "exists": false}
	}
	detail := map[string]any{"window": name, "exists": true}
	application.InvokeSync(func() {
		handle := unsafe.Pointer(w.NativeWindow())
		var width, height C.int
		C.uc_size(handle, &width, &height)
		detail["decorated"] = C.uc_decorated(handle) != 0
		detail["active"] = C.uc_active(handle) != 0
		detail["visible"] = C.uc_visible(handle) != 0
		detail["width"], detail["height"] = int(width), int(height)
	})
	return detail
}
