use super::*;
fn monitor(name: &str, x: f64, width: f64, scale: f64) -> Monitor {
    Monitor {
        name: Some(name.into()),
        origin: Point { x, y: 40.0 },
        size: Size {
            width,
            height: 1000.0,
        },
        scale_factor: scale,
    }
}
fn preference() -> WindowPreferences {
    WindowPreferences {
        version: 1,
        normal: NormalGeometry {
            inner_size: Size {
                width: 1200.0,
                height: 800.0,
            },
            placement: Some(Placement {
                monitor: monitor("external", -2560.0, 2560.0, 1.0),
                offset: Point { x: 100.0, y: 50.0 },
            }),
        },
        maximized: false,
    }
}
fn constraints() -> Constraints {
    Constraints {
        minimum: Size {
            width: 900.0,
            height: 600.0,
        },
        maximum: Size {
            width: f64::INFINITY,
            height: f64::INFINITY,
        },
    }
}
fn observation(p: &WindowPreferences) -> Observation {
    Observation {
        normal: p.normal.clone(),
        maximized: p.maximized,
        transient: false,
        monitors: vec![monitor("external", -2560.0, 2560.0, 1.0)],
        scale_factor: 1.0,
        frame: Size {
            width: 0.0,
            height: 30.0,
        },
    }
}
#[test]
fn restores_named_monitor_with_new_dpi_and_negative_origin() {
    let p = preference();
    let displays = [
        monitor("primary", 0.0, 1920.0, 1.0),
        monitor("external", -2560.0, 2560.0, 2.0),
    ];
    let result = restore(
        &p,
        &displays,
        0,
        constraints(),
        Size {
            width: 10.0,
            height: 30.0,
        },
        true,
    );
    assert_eq!(
        result.inner_size,
        Size {
            width: 1200.0,
            height: 470.0
        }
    );
    assert_eq!(
        result.outer_position,
        Some(Point {
            x: -2420.0,
            y: 40.0
        })
    );
    assert_eq!(p.normal.inner_size.height, 800.0);
}
#[test]
fn missing_display_falls_back_and_clamps_below_normal_minimum() {
    let p = preference();
    let displays = [monitor("laptop", 0.0, 1600.0, 2.0)];
    let result = restore(
        &p,
        &displays,
        0,
        constraints(),
        Size {
            width: 10.0,
            height: 30.0,
        },
        true,
    );
    assert_eq!(
        result.inner_size,
        Size {
            width: 790.0,
            height: 470.0
        }
    );
    assert_eq!(result.minimum, result.inner_size);
    assert_eq!(result.outer_position, Some(Point { x: 0.0, y: 40.0 }));
}
#[test]
fn unavailable_position_is_not_applied_and_maximums_are_respected() {
    let mut p = preference();
    p.maximized = true;
    let mut limits = constraints();
    limits.maximum = Size {
        width: 1000.0,
        height: 700.0,
    };
    let result = restore(
        &p,
        &[],
        0,
        limits,
        Size {
            width: 0.0,
            height: 0.0,
        },
        false,
    );
    assert_eq!(
        result.inner_size,
        Size {
            width: 1000.0,
            height: 700.0
        }
    );
    assert_eq!(result.outer_position, None);
    assert!(result.maximized);
}
#[test]
fn clamp_and_close_do_not_replace_preferred_layout_but_later_adjustment_does() {
    let p = preference();
    let mut tracker = WindowTracker::new(Some(p.clone()));
    let mut live = observation(&p);
    live.normal.inner_size.width = 790.0;
    assert!(tracker.observe(live.clone(), true).is_none());
    assert!(tracker.observe(live.clone(), false).is_none());
    assert_eq!(tracker.preferred(), Some(&p));
    live.normal.inner_size.width = 760.0;
    assert_eq!(
        tracker
            .observe(live, false)
            .unwrap()
            .normal
            .inner_size
            .width,
        760.0
    );
}
#[test]
fn maximize_minimize_fullscreen_and_unmaximize_preserve_normal_geometry() {
    let p = preference();
    let mut tracker = WindowTracker::new(Some(p.clone()));
    let mut live = observation(&p);
    live.normal.inner_size.width = 790.0;
    tracker.observe(live.clone(), true);
    live.maximized = true;
    live.frame.height = 0.0; // Native borders can change on maximize.
    live.normal.inner_size.width = 800.0;
    let saved = tracker.observe(live.clone(), false).unwrap();
    assert!(saved.maximized);
    assert_eq!(saved.normal, p.normal);
    live.transient = true;
    live.maximized = false;
    live.normal.inner_size.width = 1.0;
    assert!(tracker.observe(live.clone(), false).is_none());
    assert!(tracker.preferred().unwrap().maximized);
    live.transient = false;
    live.frame.height = 30.0;
    live.normal.inner_size.width = 790.0;
    let saved = tracker.observe(live, false).unwrap();
    assert!(!saved.maximized);
    assert_eq!(saved.normal, p.normal);
}
#[test]
fn display_dpi_and_frame_changes_are_not_user_adjustments() {
    let p = preference();
    let mut tracker = WindowTracker::new(Some(p.clone()));
    let mut live = observation(&p);
    tracker.observe(live.clone(), true);
    live.scale_factor = 1.5;
    live.normal.inner_size.width = 1000.0;
    assert!(tracker.observe(live.clone(), false).is_none());
    live.monitors.clear();
    live.normal.inner_size.width = 900.0;
    assert!(tracker.observe(live.clone(), false).is_none());
    live.frame.height = 40.0;
    live.normal.inner_size.height = 700.0;
    assert!(tracker.observe(live, false).is_none());
    assert_eq!(tracker.preferred(), Some(&p));
}
#[test]
fn wayland_resize_preserves_previous_optional_placement() {
    let p = preference();
    let mut tracker = WindowTracker::new(Some(p.clone()));
    let mut live = observation(&p);
    live.normal.placement = None;
    tracker.observe(live.clone(), true);
    live.normal.inner_size.width = 1300.0;
    let saved = tracker.observe(live, false).unwrap();
    assert_eq!(saved.normal.placement, p.normal.placement);
}
#[test]
fn versioned_state_roundtrips_and_rejects_invalid_values() {
    let p = preference();
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<WindowPreferences>(&json).unwrap(), p);
    assert!(!json.contains("minimized"));
    assert!(!json.contains("fullscreen"));
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e20] {
        let mut p = p.clone();
        p.normal.inner_size.width = invalid;
        assert!(!p.valid());
    }
    let mut p = p;
    p.version = 2;
    assert!(!p.valid());
}

#[test]
fn position_only_adjustment_is_saved_without_changing_size() {
    let p = preference();
    let mut tracker = WindowTracker::new(Some(p.clone()));
    let mut live = observation(&p);
    tracker.observe(live.clone(), true);
    live.normal.placement.as_mut().unwrap().offset.x += 100.0;
    let saved = tracker.observe(live.clone(), false).unwrap();
    assert_eq!(saved.normal.inner_size, p.normal.inner_size);
    assert_eq!(saved.normal.placement, live.normal.placement);
}

#[test]
fn clamped_geometry_is_not_written_back_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop-preferences.json");
    let p = preference();
    let store = PreferencesStore::load(path.clone()).unwrap();
    store.update("main", p.clone()).unwrap();
    store.flush().unwrap();
    for _ in 0..3 {
        let reopened = PreferencesStore::load(path.clone()).unwrap();
        let mut tracker = WindowTracker::new(reopened.window("main").unwrap());
        let mut live = observation(&p);
        live.normal.inner_size.width = 790.0;
        assert!(tracker.observe(live.clone(), true).is_none());
        assert!(tracker.observe(live, false).is_none());
        reopened.flush().unwrap();
        assert_eq!(reopened.window("main").unwrap(), Some(p.clone()));
    }
    assert_eq!(
        PreferencesStore::load(path)
            .unwrap()
            .window("main")
            .unwrap(),
        Some(p)
    );
}
