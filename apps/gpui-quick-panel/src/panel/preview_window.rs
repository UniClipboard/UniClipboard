use super::*;
use crate::window_pair::{
    preview_placement, PreviewPlacement, PreviewSide, POINTER_DEPTH, POINTER_HALF_HEIGHT,
};
use gpui::{canvas, div, point, px, AnyWindowHandle, IntoElement, PathBuilder, Render};
use gpui_component::ActiveTheme;

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
            let next = panel.read(cx).preview_snapshot();
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
                || image_changed;
            this.snapshot = next;
            if changed {
                this.generation += 1;
                // Reuse the last measured size for the first frame after reopening.
                // Hidden native windows do not paint, so waiting for a fresh
                // measurement before showing them would deadlock the preview.
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
                    .timer(Duration::from_millis(100))
                    .await;
                let _ = history.update(cx, |_, history, cx| {
                    panel.update(cx, |panel, cx| panel.dismiss_if_unfocused(history, cx))
                });
            }));
        });
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
        }
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

    fn apply_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(anchor) = self.snapshot.anchor else {
            if self.shown {
                if platform::set_visible(window, false).is_err() {
                    tracing::warn!("Could not hide detached preview");
                }
                self.shown = false;
            }
            return;
        };
        let (Some(content), Some(chrome)) = (self.content_height, self.chrome_height) else {
            return;
        };
        let placement = preview_placement(anchor, content + chrome);
        if self.placement != Some(placement) {
            let frame = placement.frame;
            if let Err(error) =
                platform::set_frame(window, frame.x, frame.y, frame.width, frame.height, cx)
            {
                tracing::warn!(error=%error,"Could not position item preview");
                return;
            }
            self.placement = Some(placement);
            cx.notify();
        }
        if !self.shown {
            if platform::show_without_focus(window).is_err() {
                tracing::warn!("Could not show item preview");
                return;
            }
            self.shown = true;
        }
    }
}

impl Render for PreviewWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = self.snapshot.preview_view(window, cx, self.generation);
        let placement = self.placement.or_else(|| {
            self.snapshot
                .anchor
                .map(|a| preview_placement(a, crate::window_pair::MIN_PREVIEW_HEIGHT * a.scale))
        });
        let side = placement.map(|p| p.side).unwrap_or(PreviewSide::Right);
        let depth = px((POINTER_DEPTH * self.snapshot.scale) as f32);
        let pointer_y = px(placement.map(|p| p.pointer_y).unwrap_or(48.) as f32);
        let half = px((POINTER_HALF_HEIGHT * self.snapshot.scale) as f32);
        let surface = cx.global::<crate::appearance::Surfaces>().card;
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
            .child(
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
            .on_key_down(move |event, _, cx| {
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
