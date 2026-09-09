//! Manual GTK/Wayland smoke test without the daemon or user clipboard data.
//! Run with a GTK3 Layer Shell runtime available in the dynamic linker path.

#[cfg(target_os = "linux")]
#[allow(
    clippy::expect_used,
    reason = "Assertions in an isolated manual smoke test"
)]
fn main() {
    use gtk::prelude::*;
    use std::borrow::Cow;
    use tauri::utils::assets::{AssetKey, AssetsIter, CspHash};
    use tauri::{Assets, Manager, Runtime};

    struct TestAssets;
    const HTML: &[u8] = br#"<!doctype html><html style="background:transparent"><body style="margin:0;background:transparent"><main style="border-radius:24px;clip-path:inset(0 round 24px);height:100vh;background:#18212f;color:white;font:20px sans-serif;overflow:hidden"><h2>Layer Shell smoke test</h2><input autofocus placeholder="Keyboard / IME test"><p>No clipboard data is loaded.</p></main></body></html>"#;
    impl<R: Runtime> Assets<R> for TestAssets {
        fn get(&self, _: &AssetKey) -> Option<Cow<'_, [u8]>> {
            Some(Cow::Borrowed(HTML))
        }
        fn iter(&self) -> Box<AssetsIter<'_>> {
            Box::new(std::iter::once((
                Cow::Borrowed("quick-panel.html"),
                Cow::Borrowed(HTML),
            )))
        }
        fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
            Box::new(std::iter::empty())
        }
    }

    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    tauri::Builder::default()
        .setup(move |app| {
            uc_tauri::quick_panel::pre_create(app.handle());
            let panel = app
                .get_webview_window("quick-panel")
                .expect("panel creation");
            assert!(!panel.is_visible().expect("visibility"));
            let application = panel
                .gtk_window()
                .expect("GTK panel")
                .application()
                .expect("GTK application");
            let target = gtk::ApplicationWindow::new(&application);
            target.set_title("UniClipboard paste smoke target");
            target.set_default_size(500, 200);
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some(
                "Synthetic paste shortcut receiver; clipboard is not read",
            ));
            target.add(&entry);
            let received = std::rc::Rc::new(std::cell::Cell::new(false));
            let received_key = received.clone();
            target.connect_key_press_event(move |_, event| {
                if event.keyval() == gtk::gdk::keys::constants::v
                    && event.state().contains(gtk::gdk::ModifierType::CONTROL_MASK)
                {
                    received_key.set(true);
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            target.show_all();
            target.present();
            let initial_windows = gtk::Window::list_toplevels();
            let handle = app.handle().clone();
            let mut step = 0;
            let mut initial_size = None;
            gtk::glib::timeout_add_local(std::time::Duration::from_secs(2), move || {
                macro_rules! check {
                    ($condition:expr, $message:literal) => {
                        if !$condition {
                            eprintln!("SMOKE FAILED: {}", $message);
                            // Do not unwind through a GTK C callback. Tauri may
                            // terminate before Rust main resumes after run().
                            std::process::exit(1);
                        }
                    };
                }
                match step {
                    0 | 3 => {
                        check!(
                            target.is_active(),
                            "Only paste into the isolated smoke target"
                        );
                        if step == 3 {
                            check!(received.get(), "Target must receive the paste shortcut");
                        }
                        uc_tauri::quick_panel::show(&handle);
                        uc_tauri::quick_panel::finalize_show(&handle);
                    }
                    1 => {
                        check!(
                            panel.is_visible().expect("visible layer"),
                            "Layer must be visible"
                        );
                        check!(
                            panel.is_focused().expect("focused layer"),
                            "Layer must receive keyboard focus"
                        );
                        let gtk_panel = panel.gtk_window().expect("GTK panel");
                        check!(
                            gtk_panel.is_app_paintable(),
                            "Rounded corners require a transparent GTK surface"
                        );
                        check!(
                            gtk_panel.size() == gtk_panel.size_request(),
                            "Layer must honor requested dimensions, not WebKit natural size"
                        );
                        initial_size = Some(panel.inner_size().expect("initial panel size"));
                        uc_tauri::quick_panel::set_layout(&handle, 1.0, true);
                    }
                    2 => {
                        check!(
                            Some(panel.inner_size().expect("updated panel size")) == initial_size,
                            "Linux preview updates must preserve the complete window size"
                        );
                        let handle = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            uc_tauri::commands::quick_panel::paste_to_previous_app(handle, None)
                                .await
                                .expect("paste into isolated smoke target");
                        });
                    }
                    4 => {
                        check!(
                            panel.is_visible().expect("remapped layer"),
                            "Layer must remap"
                        );
                        check!(
                            panel.gtk_window().expect("GTK window").is_realized(),
                            "Layer must retain its GTK resources across remapping"
                        );
                        let backgrounds: Vec<_> = gtk::Window::list_toplevels()
                            .into_iter()
                            .filter(|window| {
                                !initial_windows.contains(window) && window.is_visible()
                            })
                            .collect();
                        check!(!backgrounds.is_empty(), "Dismissal backgrounds must exist");
                        // Exercise GTK callback ownership and cleanup directly.
                        // This does not verify compositor pointer delivery.
                        let event = gtk::gdk::Event::new(gtk::gdk::EventType::ButtonPress);
                        backgrounds[0].emit_by_name::<bool>("button-press-event", &[&event]);
                        check!(
                            !panel.gtk_window().expect("GTK window").is_visible(),
                            "Outside-click callback must hide the panel"
                        );
                        check!(
                            gtk::Window::list_toplevels()
                                .iter()
                                .all(|window| !backgrounds.contains(window)),
                            "Hiding must destroy every dismissal background"
                        );
                        handle.exit(0);
                        return gtk::glib::ControlFlow::Break;
                    }
                    _ => unreachable!(),
                }
                step += 1;
                gtk::glib::ControlFlow::Continue
            });
            Ok(())
        })
        .run(tauri::test::mock_context(TestAssets))
        .expect("Tauri Layer Shell smoke test");
}

#[cfg(not(target_os = "linux"))]
fn main() {}
