use super::*;
use gpui::{canvas, div, point, px, AnyWindowHandle, IntoElement, PathBuilder, Render};
use gpui_component::ActiveTheme;
use quick_panel_core::geometry::window_pair::{
    preview_capacity, preview_placement, preview_placement_for_size, preview_placement_with_width,
    PreviewPlacement, PreviewSide, ACTIONS_WIDTH, POINTER_HALF_HEIGHT,
};

#[derive(Clone, Copy)]
pub(super) enum Measurement {
    Content,
    Chrome,
}

pub struct PreviewWindow {
    panel: Entity<Panel>,
    history: AnyWindowHandle,
    snapshot: PreviewSnapshot,
    _subscriptions: Vec<Subscription>,
    blur: Option<Task<()>>,
    generation: u64,
    content_height: Option<f64>,
    chrome_height: Option<f64>,
    placement: Option<PreviewPlacement>,
    shown: bool,
    /// Whether the user can see the window. A text preview stays transparent, though drawing,
    /// until its content is loaded, measured and placed, so that no half-laid-out frame shows.
    revealed: bool,
    reveal: Option<Task<()>>,
    image_view: Option<Entity<image_preview::ImagePreview>>,
}

impl PreviewWindow {
    pub fn new(
        panel: Entity<Panel>,
        history: AnyWindowHandle,
        snapshot: PreviewSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        window.set_window_title("UniClipboard Preview");
        if let Err(error) = platform::configure_shaped_preview(window, cx) {
            tracing::warn!(error=%error,"Could not configure shaped preview window");
        }
        let changed = cx.observe_in(&panel, window, |this, panel, window, cx| {
            let next = panel.read(cx).preview_snapshot(cx);
            let image_changed = match (&this.snapshot.image, &next.image) {
                (Some(old), Some(new)) => !Arc::ptr_eq(&old.image, &new.image),
                (None, None) => false,
                _ => true,
            };
            let changed = this.snapshot.item.as_ref().map(|i| &i.entry_id)
                != next.item.as_ref().map(|i| &i.entry_id)
                || this.snapshot.text != next.text
                || this.snapshot.loading != next.loading
                || this.snapshot.scale != next.scale
                || this.snapshot.actions != next.actions
                || image_changed;
            this.snapshot = next;
            if this.snapshot.is_image() {
                if let Some(view) = &this.image_view {
                    view.update(cx, |view, cx| {
                        view.update_source(&this.snapshot, window, cx)
                    });
                } else {
                    this.image_view =
                        Some(cx.new(|cx| {
                            image_preview::ImagePreview::new(&this.snapshot, window, cx)
                        }));
                }
            } else if let Some(view) = this.image_view.take() {
                view.update(cx, |view, cx| view.release(window, cx));
            }
            if changed {
                this.generation += 1;
                if !this.snapshot.is_image() {
                    // The old measurements describe the old content. Stay transparent until the
                    // new content has been measured and placed. The window stays ordered in,
                    // because a hidden native window does not paint and could not be measured.
                    this.content_height = None;
                    this.chrome_height = None;
                    this.conceal(window, cx);
                }
            }
            this.apply_layout(window, cx);
            cx.notify();
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.blur = None;
                return;
            }
            let panel = this.panel.clone();
            let history = this.history;
            this.blur = Some(cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(quick_panel_core::state::timing::BLUR_CHECK_DELAY)
                    .await;
                let _ = history.update(cx, |_, history, cx| {
                    panel.update(cx, |panel, cx| panel.dismiss_if_unfocused(history, cx))
                });
            }));
        });
        let image_view = snapshot
            .is_image()
            .then(|| cx.new(|cx| image_preview::ImagePreview::new(&snapshot, window, cx)));
        Self {
            panel,
            history,
            snapshot,
            _subscriptions: vec![changed, activation],
            blur: None,
            generation: 0,
            content_height: None,
            chrome_height: None,
            placement: None,
            shown: false,
            revealed: false,
            reveal: None,
            image_view,
        }
    }

    /// Runs an action chosen by a click. The history window keeps the keyboard focus and owns
    /// the selected entry, so the panel runs it there.
    pub(super) fn run_action(
        &mut self,
        action: quick_panel_core::actions::Action,
        cx: &mut Context<Self>,
    ) {
        let panel = self.panel.clone();
        let history = self.history;
        cx.defer(move |cx| {
            let _ = history.update(cx, |_, window, cx| {
                panel.update(cx, |panel, cx| panel.run_action(action, window, cx))
            });
        });
    }

    pub(super) fn measure(
        &mut self,
        generation: u64,
        part: Measurement,
        height: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation || !height.is_finite() {
            return;
        }
        let value = match part {
            Measurement::Content => &mut self.content_height,
            Measurement::Chrome => &mut self.chrome_height,
        };
        if value.is_some_and(|old| (old - height).abs() < 0.5) {
            return;
        }
        *value = Some(height);
        self.apply_layout(window, cx);
    }

    fn conceal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal = None;
        if self.revealed {
            self.revealed = false;
            if platform::set_opacity(window, false, cx).is_err() {
                tracing::warn!("Could not hide the preview while it updates");
            }
        }
    }

    /// Shows the window once the frame for the current placement has had time to be drawn.
    fn schedule_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.revealed || self.reveal.is_some() {
            return;
        }
        self.reveal = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(40))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.revealed = true;
                if platform::set_opacity(window, true, cx).is_err() {
                    tracing::warn!("Could not reveal the preview");
                }
            });
        }));
    }

    fn apply_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(anchor) = self.snapshot.anchor else {
            if self.shown {
                if platform::set_visible(window, false, cx).is_err() {
                    tracing::warn!("Could not hide detached preview");
                }
                self.shown = false;
            }
            self.reveal = None;
            self.revealed = false;
            return;
        };
        let placement = if self.snapshot.is_image() {
            use quick_panel_core::geometry::image_geometry::{image_body_size, Size};
            let (width, height) = preview_capacity(anchor);
            let body = if let Some(image) = &self.snapshot.image {
                image_body_size(
                    Size {
                        width: image.width as f64,
                        height: image.height as f64,
                    },
                    f64::from(window.scale_factor()),
                    Size { width, height },
                    self.snapshot.scale,
                )
            } else {
                Size {
                    width: (320. * self.snapshot.scale).min(width),
                    height: (200. * self.snapshot.scale).min(height),
                }
            };
            preview_placement_for_size(anchor, body.width, body.height)
        } else {
            if !self.shown {
                // Painted while transparent so that it can be measured.
                if platform::set_opacity(window, false, cx).is_err()
                    || platform::show_without_focus(window, cx).is_err()
                {
                    tracing::warn!("Could not show item preview");
                    return;
                }
                self.revealed = false;
                self.shown = true;
            }
            let (Some(content), Some(chrome)) = (self.content_height, self.chrome_height) else {
                return;
            };
            // The action list is narrower than a preview.
            let width = if self.snapshot.actions.is_some() {
                ACTIONS_WIDTH
            } else {
                quick_panel_core::geometry::window_pair::PREVIEW_WIDTH
            };
            preview_placement_with_width(anchor, width, content + chrome)
        };
        if self.placement != Some(placement) {
            let frame = placement.frame;
            if let Err(error) =
                platform::set_frame(window, frame.x, frame.y, frame.width, frame.height, cx)
            {
                tracing::warn!(error=%error,"Could not position item preview");
                return;
            }
            self.placement = Some(placement);
            // The native size changes after this update ends; make sure a frame is drawn for it.
            window.refresh();
            tracing::info!(
                history = ?anchor.history,
                work = ?anchor.screen,
                preview = ?frame,
                side = ?placement.side,
                scale = anchor.scale,
                "Quick panel preview placement"
            );
            if let Err(error) =
                platform::clip_preview_shape(window, placement, self.snapshot.scale, cx)
            {
                tracing::warn!(error=%error,"Could not clip preview contents to its contour");
            }
            cx.notify();
        }
        if !self.shown {
            if platform::show_without_focus(window, cx).is_err() {
                tracing::warn!("Could not show item preview");
                return;
            }
            self.shown = true;
        }
        if self.snapshot.is_image() {
            if !self.revealed {
                self.revealed = true;
                let _ = platform::set_opacity(window, true, cx);
            }
        } else if !self.snapshot.loading {
            self.schedule_reveal(window, cx);
        }
    }
}

impl Render for PreviewWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = if let Some(image) = &self.image_view {
            image.clone().into_any_element()
        } else {
            self.snapshot.preview_view(window, cx, self.generation)
        };
        let placement = self.placement.or_else(|| {
            self.snapshot.anchor.map(|a| {
                preview_placement(
                    a,
                    quick_panel_core::geometry::window_pair::MIN_PREVIEW_HEIGHT * a.scale,
                )
            })
        });
        let side = placement.map(|p| p.side).unwrap_or(PreviewSide::Right);
        let depth = px((platform::PREVIEW_POINTER_DEPTH * self.snapshot.scale) as f32);
        let pointer_y = px(placement.map(|p| p.pointer_y).unwrap_or(48.) as f32);
        let half = px((POINTER_HALF_HEIGHT * self.snapshot.scale) as f32);
        let surface = if self.snapshot.is_image() {
            cx.theme().muted
        } else {
            cx.global::<crate::ui::appearance::Surfaces>().card
        };
        let border = cx.theme().border.opacity(0.5);
        let panel = self.panel.clone();
        let history = self.history;
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .when(side == PreviewSide::Right, |body| {
                        body.left(depth).right_0()
                    })
                    .when(side == PreviewSide::Left, |body| body.left_0().right(depth))
                    .child(preview),
            )
            .when(platform::CAPABILITIES.shaped_preview, |view| {
                view.child(
                    canvas(
                        move |_, _, _| {},
                        move |bounds, _, window, _| {
                            let y = bounds.top() + pointer_y;
                            let (base, tip) = if side == PreviewSide::Right {
                                (bounds.left() + depth, bounds.left())
                            } else {
                                (bounds.right() - depth, bounds.right())
                            };
                            let seam = if side == PreviewSide::Right {
                                px(1.)
                            } else {
                                px(-1.)
                            };
                            let mut fill = PathBuilder::fill();
                            fill.move_to(point(base + seam, y - half));
                            fill.line_to(point(tip, y));
                            fill.line_to(point(base + seam, y + half));
                            fill.close();
                            if let Ok(path) = fill.build() {
                                window.paint_path(path, surface);
                            }
                            let mut outline = PathBuilder::stroke(px(1.));
                            outline.move_to(point(base, y - half));
                            outline.line_to(point(tip, y));
                            outline.line_to(point(base, y + half));
                            if let Ok(path) = outline.build() {
                                window.paint_path(path, border);
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .capture_key_down(move |event, _, cx| {
                if event.keystroke.key == "escape" {
                    let panel = panel.clone();
                    cx.defer(move |cx| {
                        let _ = history.update(cx, |_, window, cx| {
                            panel.update(cx, |panel, cx| panel.dismiss(window, cx))
                        });
                    });
                    cx.stop_propagation();
                }
            })
    }
}
