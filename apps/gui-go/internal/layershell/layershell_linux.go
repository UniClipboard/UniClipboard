//go:build linux

// Package layershell gives a GTK3 window the wlr-layer-shell role through the mature libgtk-layer-shell, loaded at
// run time with dlopen exactly as the Tauri shell does (crates/uc-tauri/src/quick_panel/layer_shell.rs): the library is
// neither linked nor a hard dependency, so a host without it (or without the protocol, as GNOME) keeps the ordinary
// window. Wails v3.0.0-beta.28 has no Layer Shell support; it only hands out the native GtkWindow* and keeps a hidden
// window unrealized, which is the stage libgtk-layer-shell needs.
//
// Every function must run on the GTK main thread (application.InvokeSync).
package layershell

/*
#cgo pkg-config: gtk+-3.0
#cgo LDFLAGS: -ldl
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
#include <gtk/gtk.h>

typedef gboolean (*uc_bool_fn)(void);
typedef void (*uc_init_fn)(GtkWindow *);
typedef gboolean (*uc_is_layer_fn)(GtkWindow *);
typedef void (*uc_ns_fn)(GtkWindow *, const char *);
typedef void (*uc_int_fn)(GtkWindow *, int);
typedef void (*uc_anchor_fn)(GtkWindow *, int, gboolean);
typedef void (*uc_margin_fn)(GtkWindow *, int, int);
typedef void (*uc_monitor_fn)(GtkWindow *, GdkMonitor *);
typedef int (*uc_get_int_fn)(GtkWindow *);

static struct {
	void *lib;
	int tried;
	char error[160];
	uc_bool_fn supported;
	uc_init_fn init;
	uc_is_layer_fn is_layer;
	uc_ns_fn ns;
	uc_int_fn layer, keyboard, zone;
	uc_anchor_fn anchor;
	uc_margin_fn margin;
	uc_monitor_fn monitor;
	uc_get_int_fn get_keyboard;
} L;

static int uc_ls_load(void) {
	if (L.tried) return L.lib != NULL;
	L.tried = 1;
	L.lib = dlopen("libgtk-layer-shell.so.0", RTLD_NOW | RTLD_GLOBAL);
	if (!L.lib) { snprintf(L.error, sizeof L.error, "GTK3 Layer Shell runtime is not installed"); return 0; }
#define SYM(field, name) \
	*(void **)(&L.field) = dlsym(L.lib, name); \
	if (!L.field) { snprintf(L.error, sizeof L.error, "Missing Layer Shell symbol: %s", name); L.lib = NULL; return 0; }
	SYM(supported, "gtk_layer_is_supported")
	SYM(init, "gtk_layer_init_for_window")
	SYM(is_layer, "gtk_layer_is_layer_window")
	SYM(ns, "gtk_layer_set_namespace")
	SYM(layer, "gtk_layer_set_layer")
	SYM(keyboard, "gtk_layer_set_keyboard_mode")
	SYM(zone, "gtk_layer_set_exclusive_zone")
	SYM(anchor, "gtk_layer_set_anchor")
	SYM(margin, "gtk_layer_set_margin")
	SYM(monitor, "gtk_layer_set_monitor")
#undef SYM
	// Optional (0.6+): only used by the state probe.
	*(void **)(&L.get_keyboard) = dlsym(L.lib, "gtk_layer_get_keyboard_mode");
	return 1;
}

static const char *uc_ls_error(void) { return L.error; }

// 1 when the display is a Wayland one whose compositor offers the layer-shell protocol.
static int uc_ls_supported(void) {
	GdkDisplay *display = gdk_display_get_default();
	if (!display || strcmp(G_OBJECT_TYPE_NAME(display), "GdkWaylandDisplay") != 0) return 0;
	if (!uc_ls_load()) return -1;
	return L.supported() ? 1 : 0;
}

#define UC_BACKDROPS "uniclipboard-layer-backdrops"

static void uc_ls_destroy_backdrops(GtkWindow *panel) {
	GPtrArray *list = g_object_steal_data(G_OBJECT(panel), UC_BACKDROPS);
	if (!list) return;
	for (guint i = 0; i < list->len; i++) gtk_widget_destroy(GTK_WIDGET(g_ptr_array_index(list, i)));
	g_ptr_array_free(list, TRUE);
}

static void uc_ls_on_hide(GtkWidget *panel, gpointer unused) {
	L.keyboard(GTK_WINDOW(panel), 0); // none
	uc_ls_destroy_backdrops(GTK_WINDOW(panel));
}

static void uc_ls_on_destroy(GtkWidget *panel, gpointer unused) {
	uc_ls_destroy_backdrops(GTK_WINDOW(panel));
}

// Initialisation needs an unrealized window; an already realized one is a plain toplevel that can never become a
// layer surface, so it is refused instead of pretending.
static const char *uc_ls_attach(GtkWindow *window) {
	if (uc_ls_supported() != 1) return "Layer Shell is not available";
	if (gtk_widget_get_realized(GTK_WIDGET(window))) return "Layer Shell initialization requires an unrealized window";
	L.init(window);
	L.ns(window, "uniclipboard-quick-panel");
	L.layer(window, 3); // GTK_LAYER_SHELL_LAYER_OVERLAY
	L.zone(window, -1); // neither reserve nor avoid space
	L.anchor(window, 0, TRUE); // left
	L.anchor(window, 2, TRUE); // top
	if (!L.is_layer(window)) return "GTK window did not become a layer surface";
	// A non-resizable GTK window keeps WebKit's natural size; layer surfaces have no resize handles.
	gtk_window_set_resizable(window, TRUE);
	g_signal_connect(window, "hide", G_CALLBACK(uc_ls_on_hide), NULL);
	g_signal_connect(window, "destroy", G_CALLBACK(uc_ls_on_destroy), NULL);
	return NULL;
}

static int uc_ls_monitor_count(void) {
	GdkDisplay *display = gdk_display_get_default();
	return display ? gdk_display_get_n_monitors(display) : 0;
}

// v: x y w h workX workY workW workH scale primary
static int uc_ls_monitor_info(int index, int *v) {
	GdkDisplay *display = gdk_display_get_default();
	GdkMonitor *monitor = display ? gdk_display_get_monitor(display, index) : NULL;
	if (!monitor) return 0;
	GdkRectangle g, w;
	gdk_monitor_get_geometry(monitor, &g);
	gdk_monitor_get_workarea(monitor, &w);
	v[0] = g.x; v[1] = g.y; v[2] = g.width; v[3] = g.height;
	v[4] = w.x; v[5] = w.y; v[6] = w.width; v[7] = w.height;
	v[8] = gdk_monitor_get_scale_factor(monitor);
	v[9] = monitor == gdk_display_get_primary_monitor(display);
	return 1;
}

#define UC_PLACED "uniclipboard-layer-placed"

// The order of surfaces inside one layer is up to the compositor (the protocol does not define it): Hyprland stacks the
// later one on top, sway the earlier one, so with a full-output backdrop a click inside the panel could reach the
// backdrop and dismiss the panel (seen in the e2e on sway: the pointer focus stayed on the backdrop). The backdrop on the
// panel's output therefore gets an input region without the panel's rectangle, which makes the hit test independent of
// the stacking order.
static void uc_ls_apply_input_holes(GtkWindow *panel) {
	GPtrArray *list = g_object_get_data(G_OBJECT(panel), UC_BACKDROPS);
	int *placed = g_object_get_data(G_OBJECT(panel), UC_PLACED);
	if (!list || !placed) return;
	GdkDisplay *display = gtk_widget_get_display(GTK_WIDGET(panel));
	for (guint i = 0; i < list->len; i++) {
		GtkWidget *backdrop = GTK_WIDGET(g_ptr_array_index(list, i));
		GdkMonitor *monitor = g_object_get_data(G_OBJECT(backdrop), "uniclipboard-monitor");
		if (!monitor || !gtk_widget_get_realized(backdrop)) continue;
		GdkRectangle geometry;
		gdk_monitor_get_geometry(monitor, &geometry);
		cairo_rectangle_int_t full = {0, 0, geometry.width, geometry.height};
		cairo_region_t *region = cairo_region_create_rectangle(&full);
		if (monitor == gdk_display_get_monitor(display, placed[0])) {
			cairo_rectangle_int_t hole = {placed[1], placed[2], placed[3], placed[4]};
			cairo_region_subtract_rectangle(region, &hole);
		}
		gtk_widget_input_shape_combine_region(backdrop, region);
		cairo_region_destroy(region);
	}
}

static void uc_ls_place(GtkWindow *window, int monitor_index, int x, int y, int width, int height) {
	int *placed = g_object_get_data(G_OBJECT(window), UC_PLACED);
	if (!placed) {
		placed = g_new0(int, 5);
		g_object_set_data_full(G_OBJECT(window), UC_PLACED, placed, g_free);
	}
	placed[0] = monitor_index; placed[1] = x; placed[2] = y; placed[3] = width; placed[4] = height;
	uc_ls_apply_input_holes(window);
	GdkDisplay *display = gdk_display_get_default();
	GdkMonitor *monitor = display ? gdk_display_get_monitor(display, monitor_index) : NULL;
	if (monitor) L.monitor(window, monitor);
	L.margin(window, 0, x);
	L.margin(window, 2, y);
	// Layer sizing uses the GTK size request, not xdg_toplevel geometry.
	gtk_widget_set_size_request(GTK_WIDGET(window), width, height);
	gtk_window_resize(window, 1, 1);
}

static gboolean uc_ls_backdrop_draw(GtkWidget *widget, cairo_t *cr, gpointer unused) {
	cairo_set_operator(cr, CAIRO_OPERATOR_SOURCE);
	cairo_set_source_rgba(cr, 0, 0, 0, 0);
	cairo_paint(cr);
	return TRUE;
}

// Dismiss on the button RELEASE, not the press: destroying the surface that was just pressed makes the compositor deliver
// the release to nobody, and GDK then keeps an implicit pointer grab on the destroyed window, so the next show's
// backdrops stop receiving presses (seen in the e2e: every show after a click-dismiss lost clicks at some positions).
static gboolean uc_ls_backdrop_press(GtkWidget *widget, GdkEventButton *event, gpointer panel) {
	return TRUE;
}

static gboolean uc_ls_backdrop_release(GtkWidget *widget, GdkEventButton *event, gpointer panel) {
	g_object_set_data(G_OBJECT(panel), "uniclipboard-backdrop-presses", GINT_TO_POINTER(GPOINTER_TO_INT(g_object_get_data(G_OBJECT(panel), "uniclipboard-backdrop-presses")) + 1));
	gtk_widget_hide(GTK_WIDGET(panel));
	gdk_display_flush(gtk_widget_get_display(GTK_WIDGET(panel)));
	return TRUE;
}

// Backdrops first (so the panel is above them in the same overlay layer), then exclusive keyboard. The caller shows
// the panel afterwards.
static const char *uc_ls_show(GtkWindow *panel) {
	if (!L.lib) return "Layer Shell is not loaded";
	uc_ls_destroy_backdrops(panel);
	GdkDisplay *display = gtk_widget_get_display(GTK_WIDGET(panel));
	GPtrArray *list = g_ptr_array_new();
	for (int i = 0; i < gdk_display_get_n_monitors(display); i++) {
		GdkMonitor *monitor = gdk_display_get_monitor(display, i);
		if (!monitor) continue;
		GtkWindow *backdrop = GTK_WINDOW(gtk_window_new(GTK_WINDOW_TOPLEVEL));
		L.init(backdrop);
		L.ns(backdrop, "uniclipboard-quick-panel-dismiss");
		L.layer(backdrop, 3);
		L.zone(backdrop, -1);
		for (int edge = 0; edge < 4; edge++) L.anchor(backdrop, edge, TRUE);
		L.monitor(backdrop, monitor);
		L.keyboard(backdrop, 0);
		gtk_window_set_decorated(backdrop, FALSE);
		gtk_window_set_accept_focus(backdrop, FALSE);
		gtk_window_set_skip_taskbar_hint(backdrop, TRUE);
		gtk_window_set_skip_pager_hint(backdrop, TRUE);
		gtk_widget_set_app_paintable(GTK_WIDGET(backdrop), TRUE);
		GdkScreen *screen = gtk_window_get_screen(backdrop);
		GdkVisual *visual = screen ? gdk_screen_get_rgba_visual(screen) : NULL;
		if (visual) gtk_widget_set_visual(GTK_WIDGET(backdrop), visual);
		g_signal_connect(backdrop, "draw", G_CALLBACK(uc_ls_backdrop_draw), NULL);
		gtk_widget_add_events(GTK_WIDGET(backdrop), GDK_BUTTON_PRESS_MASK | GDK_BUTTON_RELEASE_MASK);
		g_signal_connect_object(backdrop, "button-press-event", G_CALLBACK(uc_ls_backdrop_press), panel, 0);
		g_signal_connect_object(backdrop, "button-release-event", G_CALLBACK(uc_ls_backdrop_release), panel, 0);
		g_object_set_data(G_OBJECT(backdrop), "uniclipboard-monitor", monitor);
		g_ptr_array_add(list, backdrop);
	}
	for (guint i = 0; i < list->len; i++) gtk_widget_show_all(GTK_WIDGET(g_ptr_array_index(list, i)));
	g_object_set_data_full(G_OBJECT(panel), UC_BACKDROPS, list, NULL);
	uc_ls_apply_input_holes(panel);
	L.keyboard(panel, 1); // exclusive
	return NULL;
}

static void uc_ls_release_keyboard(GtkWindow *panel) {
	if (L.lib) L.keyboard(panel, 0);
}

// state: is_layer keyboard backdrops realized visible toplevel-focus; focus widget type name into focus_type
static void uc_ls_state(GtkWindow *panel, int *v, char *focus_type, int focus_type_len) {
	v[6] = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(panel), "uniclipboard-backdrop-presses"));
	v[0] = L.lib ? L.is_layer(panel) : 0;
	v[1] = (L.lib && L.get_keyboard) ? L.get_keyboard(panel) : -1;
	GPtrArray *list = g_object_get_data(G_OBJECT(panel), UC_BACKDROPS);
	v[2] = list ? (int)list->len : 0;
	v[3] = gtk_widget_get_realized(GTK_WIDGET(panel));
	v[4] = gtk_widget_get_visible(GTK_WIDGET(panel));
	v[5] = gtk_window_has_toplevel_focus(panel);
	GtkWidget *focus = gtk_window_get_focus(panel);
	snprintf(focus_type, focus_type_len, "%s", focus ? G_OBJECT_TYPE_NAME(focus) : "");
}

static int uc_ls_realized(GtkWindow *window) { return gtk_widget_get_realized(GTK_WIDGET(window)); }
*/
import "C"

import (
	"errors"
	"unsafe"
)

// Supported reports whether the running GDK display is a Wayland one whose compositor offers layer-shell. A missing
// library is an error distinct from an unsupported compositor, so the caller can log which fallback it took.
func Supported() (bool, error) {
	switch C.uc_ls_supported() {
	case 1:
		return true, nil
	case -1:
		return false, errors.New(C.GoString(C.uc_ls_error()))
	}
	return false, nil
}

func gtk(window unsafe.Pointer) *C.GtkWindow { return (*C.GtkWindow)(window) }

// Attach turns the (not yet realized) window into a layer surface: overlay layer, no exclusive zone, anchored to the
// top-left so margins position it. It fails for a realized window.
func Attach(window unsafe.Pointer) error {
	if message := C.uc_ls_attach(gtk(window)); message != nil {
		return errors.New(C.GoString(message))
	}
	return nil
}

// Realized reports whether GTK already realized the window (Attach is no longer possible).
func Realized(window unsafe.Pointer) bool { return C.uc_ls_realized(gtk(window)) != 0 }

// Monitor is one output in GDK's logical coordinates; Work is the area not covered by panels.
type Monitor struct {
	Index                      int
	X, Y, W, H                 int
	WorkX, WorkY, WorkW, WorkH int
	Scale                      int
	Primary                    bool
}

// Monitors lists the outputs of the default display.
func Monitors() []Monitor {
	var out []Monitor
	for i := 0; i < int(C.uc_ls_monitor_count()); i++ {
		var v [10]C.int
		if C.uc_ls_monitor_info(C.int(i), &v[0]) == 0 {
			continue
		}
		out = append(out, Monitor{Index: i, X: int(v[0]), Y: int(v[1]), W: int(v[2]), H: int(v[3]),
			WorkX: int(v[4]), WorkY: int(v[5]), WorkW: int(v[6]), WorkH: int(v[7]), Scale: int(v[8]), Primary: v[9] != 0})
	}
	return out
}

// Place puts the panel on a monitor with margins relative to that monitor's top-left, and sizes it.
func Place(window unsafe.Pointer, monitor, x, y, width, height int) {
	C.uc_ls_place(gtk(window), C.int(monitor), C.int(x), C.int(y), C.int(width), C.int(height))
}

// Show maps one transparent dismissal surface per output and takes exclusive keyboard focus. The caller shows the
// panel window itself right after; hiding it (any path) releases the keyboard and destroys the surfaces.
func Show(window unsafe.Pointer) error {
	if message := C.uc_ls_show(gtk(window)); message != nil {
		return errors.New(C.GoString(message))
	}
	return nil
}

// ReleaseKeyboard drops the exclusive keyboard mode without hiding.
func ReleaseKeyboard(window unsafe.Pointer) { C.uc_ls_release_keyboard(gtk(window)) }

// State is the panel's layer-shell state, for the e2e probe.
type State struct {
	IsLayer      bool `json:"isLayer"`
	KeyboardMode int  `json:"keyboardMode"` // 0 none, 1 exclusive, 2 on-demand, -1 unknown
	Backdrops    int  `json:"backdrops"`
	Realized     bool `json:"realized"`
	Visible      bool `json:"visible"`
	// Focus is what GTK thinks, to tell "the compositor gave the keyboard" from "the WebView has it".
	ToplevelFocus bool   `json:"toplevelFocus"`
	FocusWidget   string `json:"focusWidget"`
	// BackdropPresses counts button presses GTK delivered to the dismissal surfaces (the e2e compares it with the pointer clicks).
	BackdropPresses int `json:"backdropPresses"` // releases that dismissed (named for the click)
}

func ReadState(window unsafe.Pointer) State {
	var v [7]C.int
	var focus [64]C.char
	C.uc_ls_state(gtk(window), &v[0], &focus[0], C.int(len(focus)))
	return State{IsLayer: v[0] != 0, KeyboardMode: int(v[1]), Backdrops: int(v[2]), Realized: v[3] != 0, Visible: v[4] != 0,
		ToplevelFocus: v[5] != 0, FocusWidget: C.GoString(&focus[0]), BackdropPresses: int(v[6])}
}
