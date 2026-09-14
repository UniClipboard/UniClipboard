//! Tauri-owned GTK Layer Shell panel lifecycle and output-local layout.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use gtk::glib::SignalHandlerId;
use gtk::prelude::*;
use tauri::Manager;
use uc_desktop::hyprland::{Hyprland, WindowTarget};

use super::layer_shell::{KeyboardMode, LayerShell};
use super::QuickPanelPosition;

pub(super) struct CreationHook {
    application: gtk::Application,
    signal: Option<SignalHandlerId>,
    initialized: Rc<RefCell<Option<Result<gtk::Window, String>>>>,
}

impl CreationHook {
    pub(super) fn install() -> Result<Option<Self>, String> {
        let display = gtk::gdk::Display::default().ok_or("GTK display is unavailable")?;
        if display.type_().name() != "GdkWaylandDisplay" {
            return Ok(None);
        }
        let api = LayerShell::get()?;
        if !api.is_supported() {
            return Ok(None);
        }
        let application = gtk::gio::Application::default()
            .and_then(|app| app.downcast::<gtk::Application>().ok())
            .ok_or("Tauri GTK application is unavailable")?;
        let initialized = Rc::new(RefCell::new(None));
        let result = initialized.clone();
        let signal = application.connect_window_added(move |_, window| {
            // The builder is synchronous on the GTK thread. Claim exactly the
            // first window, and later verify it against the returned Tauri window.
            if result.borrow().is_none() {
                *result.borrow_mut() = Some(api.initialize(window).map(|()| window.clone()));
            }
        });
        Ok(Some(Self {
            application,
            signal: Some(signal),
            initialized,
        }))
    }

    pub(super) fn finish(mut self, window: &tauri::WebviewWindow) -> Result<(), String> {
        self.disconnect();
        let gtk = window
            .gtk_window()
            .map_err(|e| e.to_string())?
            .upcast::<gtk::Window>();
        let initialized = self
            .initialized
            .borrow_mut()
            .take()
            .ok_or("GTK creation hook did not run")??;
        if initialized != gtk {
            return Err("GTK creation hook initialized an unexpected window".into());
        }
        // Non-resizable GTK windows retain WebKit's natural size even when a
        // smaller size is requested. Layer surfaces have no compositor resize
        // handles; allow GTK resizing so our work-area limits can take effect.
        gtk.set_resizable(true);
        DismissSurfaces::attach(&gtk);
        window.app_handle().manage(PanelState::default());
        tracing::info!(
            backend = "wayland_layer_shell",
            "Quick panel native backend initialized"
        );
        Ok(())
    }

    fn disconnect(&mut self) {
        if let Some(signal) = self.signal.take() {
            self.application.disconnect(signal);
        }
    }
}

impl Drop for CreationHook {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[derive(Default)]
struct PanelState {
    placement: Mutex<Option<Placement>>,
    previous_window: Mutex<Option<WindowTarget>>,
}

#[derive(Clone, Copy)]
struct Placement {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    cursor: Option<(f64, f64)>,
}

/// Output-local work area, captured once per show. GTK already reports logical
/// coordinates, so neither the output scale nor its physical resolution belongs here.
fn layout(area: Placement, width: f64, height: f64) -> (f64, f64, f64, f64) {
    let width = width.min(area.width * 0.9).floor().max(1.0);
    let height = height.min(area.height * 0.8).floor().max(1.0);
    let (x, y) = match area.cursor {
        Some((x, y)) => (
            super::axis_anchored_position(x, area.x, area.width, width),
            super::axis_anchored_position(y, area.y, area.height, height),
        ),
        None => (
            area.x + (area.width - width) / 2.0,
            area.y + (area.height - height) / 2.0,
        ),
    };
    (x, y, width, height)
}

pub(super) fn active(app: &tauri::AppHandle) -> bool {
    app.try_state::<PanelState>().is_some()
}

fn gtk_window(window: &tauri::WebviewWindow) -> Result<gtk::Window, String> {
    window
        .gtk_window()
        .map(|w| w.upcast())
        .map_err(|e| e.to_string())
}

pub(super) fn prepare_show(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let gtk = gtk_window(window)?;
    let api = LayerShell::get()?;
    let display = gtk.display();
    let hyprland = Hyprland::current();
    let cursor = hyprland.as_ref().and_then(|client| match client.cursor() {
        Ok(cursor) => Some(cursor),
        Err(error) => {
            tracing::warn!(error_kind = "panel_cursor_unavailable", retryable = true, %error, "Using default panel output");
            None
        }
    });
    let monitor = cursor
        .and_then(|cursor| {
            (0..display.n_monitors())
                .filter_map(|index| display.monitor(index))
                .find(|monitor| {
                    let rect = monitor.geometry();
                    cursor.x >= rect.x() as f64
                        && cursor.x < (rect.x() + rect.width()) as f64
                        && cursor.y >= rect.y() as f64
                        && cursor.y < (rect.y() + rect.height()) as f64
                })
        })
        .or_else(|| display.primary_monitor())
        .or_else(|| display.monitor(0))
        .ok_or("No output available for Quick Panel")?;
    let rect = monitor.geometry();
    let work = monitor.workarea();
    let placement = Placement {
        x: (work.x() - rect.x()) as f64,
        y: (work.y() - rect.y()) as f64,
        width: work.width() as f64,
        height: work.height() as f64,
        cursor: match (super::current_position(), cursor) {
            (QuickPanelPosition::FollowCursor, Some(cursor)) => {
                Some((cursor.x - rect.x() as f64, cursor.y - rect.y() as f64))
            }
            _ => None,
        },
    };
    let (x, y, width, height) = layout(placement, width, height);
    let state = window.app_handle().state::<PanelState>();
    let previous = match hyprland {
        Some(client) => match client.active_window() {
            Ok(target) => target,
            Err(error) => {
                tracing::warn!(error_kind = "panel_target_unavailable", retryable = true, %error, "Automatic paste target unavailable");
                None
            }
        },
        None => None,
    };
    *state
        .previous_window
        .lock()
        .map_err(|_| "Panel target lock poisoned")? = previous;
    *state
        .placement
        .lock()
        .map_err(|_| "Panel placement lock poisoned")? = Some(placement);
    api.set_monitor(&gtk, &monitor);
    api.position(&gtk, x, y);
    resize(&gtk, width, height);
    Ok(())
}

fn resize(window: &gtk::Window, width: f64, height: f64) {
    // Layer Shell sizing uses the GTK size request, not xdg_toplevel geometry.
    window.set_size_request(width.ceil() as i32, height.ceil() as i32);
    window.resize(1, 1);
}

pub(super) fn show(window: &tauri::WebviewWindow) -> Result<(), String> {
    let gtk = gtk_window(window)?;
    let api = LayerShell::get()?;
    let surfaces = DismissSurfaces::get(&gtk)?;
    surfaces.show(&gtk)?;
    api.keyboard(&gtk, KeyboardMode::Exclusive);
    if let Err(error) = window.show() {
        api.keyboard(&gtk, KeyboardMode::None);
        surfaces.destroy();
        return Err(error.to_string());
    }
    Ok(())
}

/// Native transparent input surfaces below the panel. Their ownership follows
/// the GTK panel, with weak references back from input handlers to avoid cycles.
#[derive(Default)]
struct DismissSurfaces {
    windows: RefCell<Vec<DismissSurface>>,
}

struct DismissSurface(gtk::Window);

impl std::ops::Deref for DismissSurface {
    type Target = gtk::Window;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for DismissSurface {
    fn drop(&mut self) {
        // SAFETY: this owner controls the native surface lifecycle; input
        // handlers only keep a weak reference to the separate panel window.
        unsafe {
            self.0.destroy();
        }
    }
}

impl DismissSurfaces {
    const DATA_KEY: &'static str = "uniclipboard-layer-dismiss-surfaces";

    fn attach(panel: &gtk::Window) {
        let surfaces = Rc::new(Self::default());
        let hidden = surfaces.clone();
        panel.connect_hide(move |panel| {
            if let Ok(api) = LayerShell::get() {
                api.keyboard(panel, KeyboardMode::None);
            }
            hidden.destroy();
        });
        let destroyed = surfaces.clone();
        panel.connect_destroy(move |_| destroyed.destroy());
        // SAFETY: this module alone owns the key and stores this exact type.
        // The GObject drops the Rc when finalized on the GTK main thread.
        unsafe {
            panel.set_data(Self::DATA_KEY, surfaces);
        }
    }

    fn get(panel: &gtk::Window) -> Result<Rc<Self>, String> {
        // SAFETY: attach stores Rc<Self> under this private key; cloning it
        // before returning avoids borrowing GObject data across GTK callbacks.
        unsafe {
            panel
                .data::<Rc<Self>>(Self::DATA_KEY)
                .map(|data| data.as_ref().clone())
                .ok_or_else(|| "Layer Shell dismissal surfaces are unavailable".into())
        }
    }

    fn destroy(&self) {
        self.windows.take();
    }

    fn show(&self, panel: &gtk::Window) -> Result<(), String> {
        self.destroy();
        let api = LayerShell::get()?;
        let display = panel.display();
        let mut windows = Vec::new();
        for index in 0..display.n_monitors() {
            let Some(monitor) = display.monitor(index) else {
                continue;
            };
            let backdrop = DismissSurface(gtk::Window::new(gtk::WindowType::Toplevel));
            api.initialize(&backdrop)?;
            api.fill_output(&backdrop);
            api.set_monitor(&backdrop, &monitor);
            api.keyboard(&backdrop, KeyboardMode::None);
            backdrop.set_decorated(false);
            backdrop.set_accept_focus(false);
            backdrop.set_skip_taskbar_hint(true);
            backdrop.set_skip_pager_hint(true);
            backdrop.set_app_paintable(true);
            if let Some(screen) = GtkWindowExt::screen(&*backdrop) {
                if let Some(visual) = screen.rgba_visual() {
                    backdrop.set_visual(Some(&visual));
                }
            }
            backdrop.connect_draw(|_, context| {
                context.set_operator(gtk::cairo::Operator::Source);
                context.set_source_rgba(0.0, 0.0, 0.0, 0.0);
                if let Err(error) = context.paint() {
                    tracing::warn!(error_kind = "panel_backdrop_draw_failed", retryable = true, %error, "Panel backdrop draw failed");
                }
                gtk::glib::Propagation::Stop
            });
            backdrop.add_events(gtk::gdk::EventMask::BUTTON_PRESS_MASK);
            let weak_panel = panel.downgrade();
            backdrop.connect_button_press_event(move |_, _| {
                if let Some(panel) = weak_panel.upgrade() {
                    panel.hide();
                    panel.display().flush();
                }
                gtk::glib::Propagation::Stop
            });
            windows.push(backdrop);
        }
        // Map all backdrops before the panel so it is above them in the same
        // overlay layer. Empty GTK surfaces carry no WebView or user content.
        for backdrop in &windows {
            backdrop.show_all();
        }
        *self.windows.borrow_mut() = windows;
        Ok(())
    }
}

pub(super) fn dismiss(window: &tauri::WebviewWindow) -> Result<(), String> {
    let gtk = gtk_window(window)?;
    LayerShell::get()?.keyboard(&gtk, KeyboardMode::None);
    gtk.hide();
    gtk.display().flush();
    Ok(())
}

fn placement(app: &tauri::AppHandle) -> Option<Placement> {
    let state = app.try_state::<PanelState>()?;
    let result = *state.placement.lock().ok()?;
    result
}

pub(super) fn set_layout(
    window: &tauri::WebviewWindow,
    scale: f64,
    expanded: bool,
    window_scale: f64,
) -> Result<(), String> {
    let Some(placement) = placement(window.app_handle()) else {
        return Ok(());
    };
    let (width, height) = super::resized_panel_dimensions(scale, expanded, window_scale);
    let (x, y, width, height) = layout(placement, width, height);
    let gtk = gtk_window(window)?;
    LayerShell::get()?.position(&gtk, x, y);
    resize(&gtk, width, height);
    Ok(())
}

pub(crate) async fn paste(app: &tauri::AppHandle) -> Result<(), String> {
    let target = app
        .try_state::<PanelState>()
        .ok_or("Automatic paste requires the Wayland Layer Shell panel")?
        .previous_window
        .lock()
        .map_err(|_| "Panel target lock poisoned")?
        .take()
        .ok_or("No previous application window is available")?;
    let hyprland = Hyprland::current().ok_or("Automatic Wayland paste requires Hyprland")?;
    let handle = app.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = handle
            .get_webview_window(super::PANEL_LABEL)
            .ok_or_else(|| "Quick Panel window is unavailable".to_string())
            .and_then(|window| dismiss(&window));
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|_| "Panel hide callback was dropped")??;
    // Keep GTK dispatching Wayland events while focus is restored and verified.
    let span = tracing::Span::current();
    tokio::task::spawn_blocking(move || span.in_scope(|| hyprland.paste(&target)))
        .await
        .map_err(|_| "Panel paste worker failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(width: f64, height: f64) -> Placement {
        Placement {
            x: 0.0,
            y: 30.0,
            width,
            height,
            cursor: None,
        }
    }

    #[test]
    fn large_outputs_keep_default_dimensions_and_center_in_work_area() {
        assert_eq!(
            layout(area(1920.0, 1050.0), 800.0, 560.0),
            (560.0, 275.0, 800.0, 560.0)
        );
    }

    #[test]
    fn small_outputs_cap_each_axis_after_ui_scaling() {
        let (x, y, width, height) = layout(area(800.0, 500.0), 1200.0, 840.0);
        assert_eq!((x, y, width, height), (40.0, 80.0, 720.0, 400.0));
    }

    #[test]
    fn cursor_placement_stays_inside_the_offset_work_area() {
        let mut work = area(1000.0, 700.0);
        work.x = 20.0;
        work.cursor = Some((1010.0, 720.0));
        let (x, y, width, height) = layout(work, 800.0, 560.0);
        assert!(x >= work.x && x + width <= work.x + work.width);
        assert!(y >= work.y && y + height <= work.y + work.height);
        assert_eq!(layout(work, 800.0, 560.0), (x, y, width, height));
    }
}
