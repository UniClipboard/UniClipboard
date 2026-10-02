//! GTK3 Layer Shell access. All methods run on the GTK main thread.

use std::ffi::{c_char, c_int};
use std::sync::OnceLock;

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use libloading::Library;

type Window = *mut gtk::ffi::GtkWindow;

// The library installs GTK virtual methods. It must remain loaded until process
// exit, even after our last layer window is destroyed.
static API: OnceLock<Result<LayerShell, String>> = OnceLock::new();

pub(super) struct LayerShell {
    _library: Library,
    supported: unsafe extern "C" fn() -> c_int,
    init: unsafe extern "C" fn(Window),
    is_layer: unsafe extern "C" fn(Window) -> c_int,
    namespace: unsafe extern "C" fn(Window, *const c_char),
    layer: unsafe extern "C" fn(Window, c_int),
    keyboard: unsafe extern "C" fn(Window, c_int),
    exclusive_zone: unsafe extern "C" fn(Window, c_int),
    anchor: unsafe extern "C" fn(Window, c_int, c_int),
    margin: unsafe extern "C" fn(Window, c_int, c_int),
    monitor: unsafe extern "C" fn(Window, *mut gtk::gdk::ffi::GdkMonitor),
}

impl LayerShell {
    pub(super) fn get() -> Result<&'static Self, String> {
        API.get_or_init(Self::load).as_ref().map_err(Clone::clone)
    }

    fn load() -> Result<Self, String> {
        // SAFETY: symbols and signatures are from the stable GTK3 Layer Shell
        // ABI. The owning library is retained for the lifetime of every symbol.
        unsafe {
            let library = Library::new("libgtk-layer-shell.so.0")
                .map_err(|_| "GTK3 Layer Shell runtime is not installed".to_string())?;
            macro_rules! symbol {
                ($name:literal) => {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|_| concat!("Missing Layer Shell symbol: ", $name).to_string())?
                };
            }
            Ok(Self {
                supported: symbol!("gtk_layer_is_supported"),
                init: symbol!("gtk_layer_init_for_window"),
                is_layer: symbol!("gtk_layer_is_layer_window"),
                namespace: symbol!("gtk_layer_set_namespace"),
                layer: symbol!("gtk_layer_set_layer"),
                keyboard: symbol!("gtk_layer_set_keyboard_mode"),
                exclusive_zone: symbol!("gtk_layer_set_exclusive_zone"),
                anchor: symbol!("gtk_layer_set_anchor"),
                margin: symbol!("gtk_layer_set_margin"),
                monitor: symbol!("gtk_layer_set_monitor"),
                _library: library,
            })
        }
    }

    pub(super) fn is_supported(&self) -> bool {
        // SAFETY: called on the initialized GTK main thread.
        unsafe { (self.supported)() != 0 }
    }

    pub(super) fn initialize(&self, window: &gtk::Window) -> Result<(), String> {
        if window.is_realized() {
            return Err("Layer Shell initialization requires an unrealized window".into());
        }
        // SAFETY: window is a live unrealized GtkWindow on the GTK main thread.
        unsafe {
            let ptr = window.to_glib_none().0;
            (self.init)(ptr);
            (self.namespace)(ptr, c"uniclipboard-quick-panel".as_ptr());
            (self.layer)(ptr, 3); // GTK_LAYER_SHELL_LAYER_OVERLAY
            (self.exclusive_zone)(ptr, -1); // Do not reserve or avoid work area.
            (self.anchor)(ptr, 0, 1); // Left
            (self.anchor)(ptr, 2, 1); // Top
        }
        if !self.is_layer(window) {
            return Err("GTK window did not become a layer surface".into());
        }
        Ok(())
    }

    pub(super) fn is_layer(&self, window: &gtk::Window) -> bool {
        // SAFETY: live GtkWindow, GTK main thread.
        unsafe { (self.is_layer)(window.to_glib_none().0) != 0 }
    }

    pub(super) fn fill_output(&self, window: &gtk::Window) {
        // SAFETY: live layer window; anchor all edges without reserving space.
        unsafe {
            let ptr = window.to_glib_none().0;
            (self.namespace)(ptr, c"uniclipboard-quick-panel-dismiss".as_ptr());
            (self.anchor)(ptr, 1, 1); // Right
            (self.anchor)(ptr, 3, 1); // Bottom
        }
    }

    pub(super) fn set_monitor(&self, window: &gtk::Window, monitor: &gtk::gdk::Monitor) {
        // SAFETY: both GObjects are live on the GTK main thread.
        unsafe { (self.monitor)(window.to_glib_none().0, monitor.to_glib_none().0) }
    }

    pub(super) fn position(&self, window: &gtk::Window, x: f64, y: f64) {
        // SAFETY: live layer window, GTK main thread; margins are logical pixels.
        unsafe {
            (self.margin)(window.to_glib_none().0, 0, x.round() as c_int);
            (self.margin)(window.to_glib_none().0, 2, y.round() as c_int);
        }
    }

    pub(super) fn keyboard(&self, window: &gtk::Window, mode: KeyboardMode) {
        // SAFETY: live layer window and a valid GtkLayerShellKeyboardMode.
        unsafe { (self.keyboard)(window.to_glib_none().0, mode as c_int) }
    }
}

#[derive(Clone, Copy)]
#[repr(i32)]
pub(super) enum KeyboardMode {
    None = 0,
    Exclusive = 1,
}
