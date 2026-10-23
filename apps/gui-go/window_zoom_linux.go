//go:build linux && gtk4

package main

/*
#cgo pkg-config: gtk4 webkitgtk-6.0
#include <gtk/gtk.h>
#include <webkit/webkit.h>

static WebKitWebView *uc_find_webview(GtkWidget *widget) {
    if (WEBKIT_IS_WEB_VIEW(widget)) return WEBKIT_WEB_VIEW(widget);
    for (GtkWidget *child = gtk_widget_get_first_child(widget); child; child = gtk_widget_get_next_sibling(child)) {
        WebKitWebView *view = uc_find_webview(child);
        if (view) return view;
    }
    return NULL;
}

static int uc_set_webview_zoom(void *window, double scale) {
    WebKitWebView *view = uc_find_webview(GTK_WIDGET(window));
    if (!view) return 0;
    webkit_web_view_set_zoom_level(view, scale);
    return 1;
}
*/
import "C"

import (
	"errors"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// Wails beta.28 clamps Linux SetZoom to >= 1. WebKitGTK supports every positive
// factor, so use its native API until Wails supports the application's 80% floor.
// The caller owns GTK main-thread dispatch and supplies its own window.
func setWindowZoom(w application.Window, scale float64) error {
	handle := w.NativeWindow()
	if handle == nil || C.uc_set_webview_zoom(handle, C.double(scale)) == 0 {
		return errors.New("calling WebView is unavailable")
	}
	return nil
}
