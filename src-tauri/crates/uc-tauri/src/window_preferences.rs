//! Tauri adaptation of the desktop host's local window preferences.
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{Manager, WebviewWindow, WindowEvent};
use tracing::{warn, Instrument};
use uc_desktop::desktop_preferences::{
    self as preferences, Constraints, Monitor, NormalGeometry, Observation, Placement, Point,
    PreferencesStore, Size, WindowTracker,
};

const SETTLE_DELAY: Duration = Duration::from_millis(300);
const MAIN: &str = crate::main_window::MAIN_WINDOW_LABEL;

pub(crate) struct DesktopWindowPreferences {
    store: Option<PreferencesStore>,
    session: Mutex<Option<Arc<Mutex<Session>>>>,
}
struct Session {
    tracker: WindowTracker,
    epoch: u64,
    alive: bool,
    revealed: bool,
    restoring: bool,
    pending: Option<Observation>,
}
impl DesktopWindowPreferences {
    pub(crate) fn load(path: PathBuf) -> Self {
        let store = match PreferencesStore::load(path) {
            Ok(store) => Some(store),
            Err(error) => {
                warn!(error = %error, error_kind = "desktop_preferences_init", "Desktop preferences writer unavailable");
                None
            }
        };
        Self {
            store,
            session: Mutex::new(None),
        }
    }
    fn session(&self) -> Option<Arc<Mutex<Session>>> {
        self.session
            .lock()
            .unwrap_or_else(|error| {
                warn!("Window preferences session lock poisoned; recovering ownership");
                error.into_inner()
            })
            .clone()
    }
    fn save(&self, value: Option<preferences::WindowPreferences>) {
        if let (Some(store), Some(value)) = (&self.store, value) {
            if let Err(error) = store.update(MAIN, value) {
                warn!(error = %error, error_kind = "desktop_preferences_update", "Failed to update window preferences");
            }
        }
    }
    fn flush(&self) {
        if let Some(store) = &self.store {
            if let Err(error) = store.flush() {
                warn!(error = %error, error_kind = "desktop_preferences_flush", "Failed to flush window preferences");
            }
        }
    }
}
fn locked(session: &Mutex<Session>) -> std::sync::MutexGuard<'_, Session> {
    session.lock().unwrap_or_else(|error| {
        warn!("Window preferences observation lock poisoned; recovering ownership");
        error.into_inner()
    })
}

pub(crate) fn attach(window: &WebviewWindow) {
    let service = window.state::<DesktopWindowPreferences>();
    let preferred = service.store.as_ref().and_then(|store| match store.window(MAIN) {
        Ok(value) => value,
        Err(error) => { warn!(error = %error, error_kind = "desktop_preferences_read", "Failed to read main window preferences"); None }
    });
    let session = Arc::new(Mutex::new(Session {
        tracker: WindowTracker::new(preferred),
        epoch: 0,
        alive: true,
        revealed: false,
        restoring: true,
        pending: None,
    }));
    match service.session.lock() {
        Ok(mut active) => *active = Some(session.clone()),
        Err(error) => {
            warn!("Window preferences session lock poisoned; replacing session");
            *error.into_inner() = Some(session.clone());
        }
    }
    let window_for_event = window.clone();
    window.on_window_event(move |event| {
        let window = &window_for_event;
        match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Err(error) = changed(window, &session, matches!(event, WindowEvent::ScaleFactorChanged { .. })) {
                    warn!(error = %error, error_kind = "window_preferences_observe", "Failed to observe window geometry");
                }
            }
            WindowEvent::CloseRequested { .. } => {
                settle(window, &session, None);
                window.state::<DesktopWindowPreferences>().flush();
            }
            WindowEvent::Destroyed => { let mut state = locked(&session); state.alive = false; state.epoch += 1; }
            _ => {}
        }
    });
}

/// Run once, after frontend frame preferences are applied and before first show.
pub(crate) fn prepare_reveal(window: &WebviewWindow) {
    let service = window.state::<DesktopWindowPreferences>();
    let Some(session) = service.session() else {
        return;
    };
    let preferred = {
        let mut state = locked(&session);
        if state.revealed || !state.alive {
            return;
        }
        state.revealed = true;
        state.tracker.preferred().cloned()
    };
    // Establish the normal default before a first-ever maximize. No disk write.
    match snapshot(window) {
        Ok(observed) => {
            locked(&session).tracker.observe(observed, true);
        }
        Err(error) => {
            warn!(error = %error, error_kind = "window_preferences_snapshot", "Failed to establish default window geometry")
        }
    }
    if let Some(preferred) = preferred {
        if let Err(error) = apply(window, &preferred) {
            warn!(error = %error, error_kind = "window_preferences_restore", "Failed to restore main window preferences");
        }
    }
    if let Err(error) = changed(window, &session, true) {
        warn!(error = %error, error_kind = "window_preferences_observe", "Failed to establish restored window geometry");
    }
}

fn changed(
    window: &WebviewWindow,
    session: &Arc<Mutex<Session>>,
    known_operation: bool,
) -> tauri::Result<()> {
    {
        let state = locked(session);
        if !state.alive || !state.revealed {
            return Ok(());
        }
    }
    let observed = snapshot(window)?;
    let (epoch, preceding) = {
        let mut state = locked(session);
        state.restoring |= known_operation;
        // Preserve a just-resized normal rectangle before a quick maximize or
        // minimize, even when its debounce has not yet expired.
        let preceding = if !state.restoring && (observed.maximized || observed.transient) {
            state
                .pending
                .take()
                .and_then(|pending| state.tracker.observe(pending, false))
        } else {
            None
        };
        state.pending = Some(observed);
        state.epoch += 1;
        (state.epoch, preceding)
    };
    window.state::<DesktopWindowPreferences>().save(preceding);
    let window = window.clone();
    let app = window.app_handle().clone();
    let session = session.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SETTLE_DELAY).await;
        if let Err(error) = app.run_on_main_thread(move || settle(&window, &session, Some(epoch))) {
            warn!(error = %error, error_kind = "window_preferences_dispatch", "Failed to dispatch stable window observation");
        }
    }.in_current_span());
    Ok(())
}
fn settle(window: &WebviewWindow, session: &Mutex<Session>, epoch: Option<u64>) {
    {
        let state = locked(session);
        if !state.alive || !state.revealed || epoch.is_some_and(|epoch| state.epoch != epoch) {
            return;
        }
    }
    let observed = match snapshot(window) {
        Ok(observed) => observed,
        Err(error) => {
            warn!(error = %error, error_kind = "window_preferences_snapshot", "Failed to capture stable window geometry");
            return;
        }
    };
    let accepted = {
        let mut state = locked(session);
        if !state.alive || epoch.is_some_and(|epoch| state.epoch != epoch) {
            return;
        }
        let known = state.restoring;
        let accepted = state.tracker.observe(observed, known);
        state.restoring = false;
        state.pending = None;
        state.epoch += 1;
        accepted
    };
    window.state::<DesktopWindowPreferences>().save(accepted);
}

pub(crate) fn flush(app: &tauri::AppHandle) {
    let service = app.state::<DesktopWindowPreferences>();
    if let (Some(window), Some(session)) = (app.get_webview_window(MAIN), service.session()) {
        settle(&window, &session, None);
    }
    service.flush();
}
fn can_position() -> bool {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        return gtk::gdk::Display::default()
            .is_some_and(|display| display.type_().name() != "GdkWaylandDisplay");
    }
    #[cfg(not(target_os = "linux"))]
    true
}
fn monitor(value: &tauri::Monitor) -> Monitor {
    let area = value.work_area();
    Monitor {
        name: value.name().cloned(),
        origin: Point {
            x: area.position.x as f64,
            y: area.position.y as f64,
        },
        size: Size {
            width: area.size.width as f64,
            height: area.size.height as f64,
        },
        scale_factor: value.scale_factor(),
    }
}
fn snapshot(window: &WebviewWindow) -> tauri::Result<Observation> {
    let scale_factor = window.scale_factor()?;
    let inner = window.inner_size()?;
    let outer = window.outer_size()?;
    let placement = if can_position() {
        if let Some(display) = window.current_monitor()? {
            let display = monitor(&display);
            let position = window.outer_position()?;
            Some(Placement {
                offset: Point {
                    x: (position.x as f64 - display.origin.x) / scale_factor,
                    y: (position.y as f64 - display.origin.y) / scale_factor,
                },
                monitor: display,
            })
        } else {
            None
        }
    } else {
        None
    };
    Ok(Observation {
        normal: NormalGeometry {
            inner_size: Size {
                width: inner.width as f64 / scale_factor,
                height: inner.height as f64 / scale_factor,
            },
            placement,
        },
        maximized: window.is_maximized()?,
        transient: window.is_minimized()? || window.is_fullscreen()?,
        monitors: window.available_monitors()?.iter().map(monitor).collect(),
        scale_factor,
        frame: Size {
            width: outer.width.saturating_sub(inner.width) as f64 / scale_factor,
            height: outer.height.saturating_sub(inner.height) as f64 / scale_factor,
        },
    })
}
fn apply(window: &WebviewWindow, preferred: &preferences::WindowPreferences) -> tauri::Result<()> {
    let app = window.app_handle();
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == MAIN)
        .ok_or_else(|| {
            tauri::Error::Anyhow(anyhow::anyhow!("main window configuration missing"))
        })?;
    let observed = snapshot(window)?;
    let primary = app.primary_monitor()?.as_ref().map(monitor);
    let current = window.current_monitor()?.as_ref().map(monitor);
    let position_supported = can_position();
    let fallback = if position_supported { primary } else { current }
        .and_then(|m| {
            observed
                .monitors
                .iter()
                .position(|candidate| candidate == &m)
        })
        .unwrap_or(0);
    let restored = preferences::restore(
        preferred,
        &observed.monitors,
        fallback,
        Constraints {
            minimum: Size {
                width: config.min_width.unwrap_or(1.0),
                height: config.min_height.unwrap_or(1.0),
            },
            maximum: Size {
                width: config.max_width.unwrap_or(f64::INFINITY),
                height: config.max_height.unwrap_or(f64::INFINITY),
            },
        },
        observed.frame,
        position_supported,
    );
    if window.is_maximized()? {
        window.unmaximize()?;
    }
    // Position before size so the target output establishes the native DPI.
    if let Some(position) = restored.outer_position {
        window.set_position(tauri::PhysicalPosition::new(
            position.x.round() as i32,
            position.y.round() as i32,
        ))?;
    }
    // Explicit target-display pixels avoid relying on when the move's DPI event
    // is delivered relative to these queued native size operations.
    let scale = restored.scale_factor.unwrap_or(observed.scale_factor);
    let physical = |size: Size| {
        tauri::PhysicalSize::new(
            (size.width * scale).round().max(1.0) as u32,
            (size.height * scale).round().max(1.0) as u32,
        )
    };
    window.set_min_size(Some(physical(restored.minimum)))?;
    window.set_size(physical(restored.inner_size))?;
    if restored.maximized {
        window.maximize()?;
    }
    Ok(())
}
