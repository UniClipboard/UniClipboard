//! Placement of the panel window and of its satellite preview window.

use super::*;

impl Panel {
    pub(super) fn hide_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.preview_window {
            if handle
                .update(cx, |_, window, cx| platform::set_visible(window, false, cx))
                .is_err()
            {
                tracing::warn!("Preview window is unavailable");
            }
        }
    }

    pub(super) fn layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        gpui_component::Theme::global_mut(cx).font_size = gpui::px(16. * self.scale as f32);
        window.set_rem_size(gpui::px(16. * self.scale as f32));
        let width = quick_panel_core::geometry::window_pair::PANEL_WIDTH * self.scale;
        let x = self.anchor.0;
        if let Err(error) = platform::set_frame(
            window,
            x,
            self.anchor.1,
            width,
            quick_panel_core::geometry::window_pair::PANEL_HEIGHT * self.scale,
            cx,
        ) {
            self.state.set_message(error.to_string());
        }
        if !self.state.preview.expanded {
            self.hide_preview(cx);
        }
    }

    /// Chooses the display for this show session and places the history window on it. The
    /// preview window is laid out inside the same work area (see `update_preview_anchor`), so the
    /// pair stays on one display for the whole session.
    pub(super) fn position(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use quick_panel_core::geometry::{placement, window_pair};
        let panel = (
            window_pair::PANEL_WIDTH * self.scale,
            window_pair::PANEL_HEIGHT * self.scale,
        );
        let preview_reserve = (window_pair::WINDOW_GAP
            + platform::PREVIEW_POINTER_DEPTH
            + window_pair::PREVIEW_WIDTH)
            * self.scale;
        match platform::session_screen() {
            Ok(screen) => {
                self.anchor = placement::panel_origin(
                    screen,
                    self.state.catalog.cursor_anchored,
                    panel,
                    preview_reserve,
                );
                tracing::info!(
                    pointer = ?screen.pointer,
                    work = ?screen.work,
                    origin = ?self.anchor,
                    size = ?panel,
                    scale = self.scale,
                    "Quick panel placement"
                );
                self.session_screen = Some(screen);
            }
            Err(error) => {
                self.session_screen = None;
                self.state.set_message(error.to_string());
            }
        }
        self.layout(window, cx);
    }

    pub(super) fn show_preview(&mut self, history: &mut Window, cx: &mut Context<Self>) {
        self.update_preview_anchor(cx);
        let width = (quick_panel_core::geometry::window_pair::PREVIEW_WIDTH
            + platform::PREVIEW_POINTER_DEPTH)
            * self.scale;
        let height = quick_panel_core::geometry::window_pair::MIN_PREVIEW_HEIGHT * self.scale;
        if self.preview_window.is_none() {
            let snapshot = self.preview_snapshot(cx);
            let panel = cx.entity();
            let history = history.window_handle();
            match cx.open_window(
                gpui::WindowOptions {
                    titlebar: None,
                    kind: gpui::WindowKind::PopUp,
                    focus: false,
                    show: false,
                    is_resizable: false,
                    window_background: gpui::WindowBackgroundAppearance::Transparent,
                    window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                        gpui::point(
                            gpui::px(self.anchor.0 as f32),
                            gpui::px(self.anchor.1 as f32),
                        ),
                        gpui::size(gpui::px(width as f32), gpui::px(height as f32)),
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| {
                        preview_window::PreviewWindow::new(panel, history, snapshot, window, cx)
                    });
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            ) {
                Ok(handle) => self.preview_window = Some(handle.into()),
                Err(_) => {
                    self.state.set_message(text::t().preview_window_failed);
                }
            }
        }
    }

    pub(super) fn sync_visible_thumbnails(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = &self.state.search.items;
        let visible = if !self.state.session.visible || self.state.search.loading {
            Vec::new()
        } else if self.state.search.filters.images_only() {
            quick_panel_core::grid::visible(self.state.search.grid_top, items.len())
                .filter_map(|index| items.get(index))
                .filter(|item| quick_panel_core::content::has_image_preview(item))
                .filter(|item| {
                    let Some(mut bounds) = self.image_bounds.get(&item.entry_id).copied() else {
                        return false;
                    };
                    bounds.origin += self.scroll.offset();
                    let intersection = bounds.intersect(&self.scroll.bounds());
                    intersection.size.width > gpui::px(0.)
                        && intersection.size.height > gpui::px(0.)
                })
                .map(|item| item.entry_id.clone())
                .collect()
        } else {
            let viewport = self.scroll.bounds();
            let lead = self.list_lead(cx);
            items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    if !quick_panel_core::content::has_image_preview(item) {
                        return None;
                    }
                    let mut bounds = self.scroll.bounds_for_item(index + lead)?;
                    bounds.origin += self.scroll.offset();
                    let intersection = bounds.intersect(&viewport);
                    (intersection.size.width > gpui::px(0.)
                        && intersection.size.height > gpui::px(0.))
                    .then(|| item.entry_id.clone())
                })
                .collect()
        };
        if self.visible_thumbnails != visible {
            self.visible_thumbnails = visible;
            self.load_thumbnails(window, cx);
            cx.notify();
        }
    }

    pub(super) fn update_preview_anchor(&mut self, cx: &mut Context<Self>) {
        let next = (|| {
            if !self.state.session.visible
                || !self.state.preview.expanded
                || self.state.search.loading
            {
                return None;
            }
            let id = self.state.preview.entry.as_ref()?;
            let index = self
                .state
                .search
                .items
                .iter()
                .position(|item| &item.entry_id == id)?;
            let mut item = if self.state.search.filters.images_only() {
                *self.image_bounds.get(id)?
            } else {
                self.scroll.bounds_for_item(index + self.list_lead(cx))?
            };
            item.origin += self.scroll.offset();
            let viewport = self.scroll.bounds();
            let item = item.intersect(&viewport);
            if item.size.width <= gpui::px(0.) || item.size.height <= gpui::px(0.) {
                return None;
            }
            use quick_panel_core::geometry::window_pair::{PreviewAnchor, Rect};
            // Everything is in the global space of this session's work area, and the history
            // window is where `position` put it. GPUI's own window and display bounds are not
            // used: on macOS they are relative to the display the window is on.
            let work = self.session_screen?.work;
            let panel = (
                quick_panel_core::geometry::window_pair::PANEL_WIDTH * self.scale,
                quick_panel_core::geometry::window_pair::PANEL_HEIGHT * self.scale,
            );
            Some(PreviewAnchor::for_session(
                self.anchor,
                panel,
                Rect {
                    x: f64::from(item.origin.x),
                    y: f64::from(item.origin.y),
                    width: f64::from(item.size.width),
                    height: f64::from(item.size.height),
                },
                work,
                self.scale,
                platform::PREVIEW_POINTER_DEPTH,
            ))
        })();
        if self.preview_anchor != next {
            self.preview_anchor = next;
            cx.notify();
        }
    }

    pub(super) fn record_image_bounds(
        &mut self,
        id: String,
        mut bounds: gpui::Bounds<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        bounds.origin -= self.scroll.offset();
        self.image_bounds.insert(id, bounds);
        self.update_preview_anchor(cx);
        self.sync_visible_thumbnails(window, cx);
    }

    pub(super) fn preview_snapshot(&self, cx: &App) -> PreviewSnapshot {
        let state = &self.state;
        let item = state
            .search
            .items
            .iter()
            .find(|item| Some(&item.entry_id) == state.preview.entry.as_ref())
            .cloned();
        let image = item
            .as_ref()
            .and_then(|item| {
                self.preview_image
                    .as_ref()
                    .filter(|(id, _)| id == &item.entry_id)
            })
            .map(|(_, data)| data.clone());
        let source_name = item.as_ref().and_then(|item| {
            let id = item.source_device.as_deref()?;
            state
                .catalog
                .members
                .iter()
                .find(|member| member.peer_id == id)
                .map(|member| member.device_name.clone())
        });
        let data = self.ctx_data(cx);
        PreviewSnapshot {
            item,
            image,
            text: state.preview.text.clone(),
            loading: state.preview.loading
                || (self.preview_decoding.is_some()
                    && self.preview_decoding.as_deref() == state.preview.entry.as_deref()),
            anchor: self.preview_anchor,
            scale: self.scale,
            actions: state.actions_view(&data.ctx()),
            source_name,
            now_ms: chrono::Utc::now().timestamp_millis(),
            open: state.session.visible,
        }
    }
}
