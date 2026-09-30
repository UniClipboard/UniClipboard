//! Placement of the panel window and of its satellite preview window.

use super::*;

impl Panel {
    pub(super) fn hide_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.preview_window {
            if handle
                .update(cx, |_, window, _| platform::set_visible(window, false))
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

    pub(super) fn position(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match platform::panel_anchor(
            self.state.catalog.cursor_anchored,
            quick_panel_core::geometry::window_pair::PANEL_WIDTH * self.scale,
            quick_panel_core::geometry::window_pair::PANEL_HEIGHT * self.scale,
        ) {
            Ok(anchor) => self.anchor = anchor,
            Err(error) => self.state.set_message(error.to_string()),
        }
        self.layout(window, cx);
    }

    pub(super) fn show_preview(&mut self, history: &mut Window, cx: &mut Context<Self>) {
        self.update_preview_anchor(history, cx);
        let width = (quick_panel_core::geometry::window_pair::PREVIEW_WIDTH
            + quick_panel_core::geometry::window_pair::POINTER_DEPTH)
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
                    self.state.set_message(text::PREVIEW_WINDOW_FAILED);
                }
            }
        }
    }

    pub(super) fn update_preview_anchor(&mut self, history: &Window, cx: &mut Context<Self>) {
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
            let frame = history.bounds();
            let screen = history.display(cx)?.bounds();
            use quick_panel_core::geometry::window_pair::{PreviewAnchor, Rect};
            Some(PreviewAnchor {
                history: Rect {
                    x: f64::from(frame.origin.x),
                    y: f64::from(frame.origin.y),
                    width: f64::from(frame.size.width),
                    height: f64::from(frame.size.height),
                },
                item: Rect {
                    x: f64::from(frame.origin.x + item.origin.x),
                    y: f64::from(frame.origin.y + item.origin.y),
                    width: f64::from(item.size.width),
                    height: f64::from(item.size.height),
                },
                screen: Rect {
                    x: f64::from(screen.origin.x),
                    y: f64::from(screen.origin.y),
                    width: f64::from(screen.size.width),
                    height: f64::from(screen.size.height),
                },
                scale: self.scale,
            })
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
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        bounds.origin -= self.scroll.offset();
        self.image_bounds.insert(id, bounds);
        self.update_preview_anchor(window, cx);
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
            .and_then(|item| self.images.get(&item.entry_id))
            .cloned();
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
            loading: state.preview.loading,
            anchor: self.preview_anchor,
            scale: self.scale,
            actions: state.actions_view(&data.ctx()),
            source_name,
            now_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}
