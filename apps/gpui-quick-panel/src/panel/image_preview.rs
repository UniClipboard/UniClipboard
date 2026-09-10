use super::*;
use crate::image_geometry::{ImageViewport, Offset, Size};
use gpui::{
    canvas, div, img, prelude::*, px, Animation, AnimationExt, Bounds, FocusHandle, MouseButton,
    MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels, Render, RenderImage,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    ActiveTheme, Disableable, Icon, IconName, Sizable,
};

pub struct ImagePreview {
    entry_id: Option<String>,
    data: Option<ImageData>,
    loading: bool,
    error: Option<String>,
    viewport: ImageViewport,
    bounds: Bounds<Pixels>,
    display_scale: f64,
    drag: Option<(gpui::Point<Pixels>, Offset)>,
    hovered: bool,
    hover_generation: u64,
    checker: Option<Arc<RenderImage>>,
    checker_key: Option<(u32, u32, u32, u32)>,
    focus: FocusHandle,
}

impl ImagePreview {
    pub fn new(snapshot: &PreviewSnapshot, window: &Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            entry_id: None,
            data: None,
            loading: false,
            error: None,
            viewport: ImageViewport::new(Size::default(), Size::default()),
            bounds: Bounds::default(),
            display_scale: f64::from(window.scale_factor()),
            drag: None,
            hovered: false,
            hover_generation: 0,
            checker: None,
            checker_key: None,
            focus: cx.focus_handle(),
        };
        view.update_source(snapshot, cx);
        view
    }

    pub fn update_source(&mut self, snapshot: &PreviewSnapshot, cx: &mut Context<Self>) {
        let id = snapshot.item.as_ref().map(|item| item.entry_id.clone());
        let reset = self.entry_id != id;
        self.entry_id = id;
        self.data = snapshot.image.clone();
        self.loading = snapshot.loading;
        self.error = if !self.loading && self.data.is_none() {
            Some(
                snapshot
                    .text
                    .clone()
                    .unwrap_or_else(|| "无法读取图片。".into()),
            )
        } else {
            None
        };
        let native = self.native_size();
        if reset {
            self.viewport = ImageViewport::new(native, self.viewport.viewport);
            self.drag = None;
        } else {
            self.viewport.native = native;
            self.viewport.resize(self.viewport.viewport);
        }
        cx.notify();
    }

    fn native_size(&self) -> Size {
        self.data
            .as_ref()
            .map(|image| Size {
                width: image.width as f64 / self.display_scale,
                height: image.height as f64 / self.display_scale,
            })
            .unwrap_or_default()
    }

    fn measure(&mut self, bounds: Bounds<Pixels>, display_scale: f64, cx: &mut Context<Self>) {
        if self.bounds == bounds && self.display_scale == display_scale {
            return;
        }
        self.bounds = bounds;
        self.display_scale = display_scale;
        self.viewport.native = self.native_size();
        self.viewport.resize(Size {
            width: f64::from(bounds.size.width),
            height: f64::from(bounds.size.height),
        });
        cx.notify();
    }

    fn toggle_zoom(&mut self, point: Offset, cx: &mut Context<Self>) {
        if self.data.is_none() {
            return;
        }
        self.viewport.toggle_at(point);
        self.drag = None;
        cx.notify();
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        if event.click_count == 2 {
            let point = event.position - self.bounds.origin;
            self.toggle_zoom(
                Offset {
                    x: f64::from(point.x),
                    y: f64::from(point.y),
                },
                cx,
            );
        } else if self.viewport.actual_size {
            self.drag = Some((event.position, self.viewport.pan));
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((start, pan)) = self.drag {
            if event.pressed_button == Some(MouseButton::Left) {
                let delta = event.position - start;
                self.viewport.pan_to(Offset {
                    x: pan.x + f64::from(delta.x),
                    y: pan.y + f64::from(delta.y),
                });
                cx.notify();
            } else {
                self.drag = None;
                cx.notify();
            }
        }
    }

    fn checkerboard(&mut self, cx: &Context<Self>) -> Option<Arc<RenderImage>> {
        let width = (self.viewport.viewport.width * self.display_scale).ceil() as u32;
        let height = (self.viewport.viewport.height * self.display_scale).ceil() as u32;
        if width == 0 || height == 0 {
            return None;
        }
        let base: gpui::Rgba = cx.theme().muted.into();
        let other: gpui::Rgba = cx
            .theme()
            .muted
            .blend(cx.theme().foreground.opacity(0.055))
            .into();
        let key = (width, height, u32::from(base), u32::from(other));
        if self.checker_key != Some(key) {
            let cell = (12. * self.display_scale).round().max(1.) as u32;
            let pixel = |color: gpui::Rgba| {
                image::Rgba([
                    (color.b * 255.) as u8,
                    (color.g * 255.) as u8,
                    (color.r * 255.) as u8,
                    255,
                ])
            };
            let a = pixel(base);
            let b = pixel(other);
            let raster = image::RgbaImage::from_fn(width, height, |x, y| {
                if (x / cell + y / cell).is_multiple_of(2) {
                    a
                } else {
                    b
                }
            });
            self.checker = Some(Arc::new(RenderImage::new(vec![image::Frame::new(raster)])));
            self.checker_key = Some(key);
        }
        self.checker.clone()
    }
}

impl Render for ImagePreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hovered = window.is_window_hovered() && self.bounds.contains(&window.mouse_position());
        if self.hovered != hovered {
            self.hovered = hovered;
            self.hover_generation += 1;
        }
        let tracker = cx.weak_entity();
        let display_scale = f64::from(window.scale_factor());
        let size = self.viewport.displayed_size();
        let origin = self.viewport.origin();
        let checker = self.checkerboard(cx);
        let image = self.data.as_ref().map(|data| data.image.clone());
        let can_zoom = self.viewport.native.width > self.viewport.viewport.width + 1.
            || self.viewport.native.height > self.viewport.viewport.height + 1.;
        let mut view = div()
            .id("immersive-image")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(gpui::rems(
                crate::window_pair::PREVIEW_CORNER_RADIUS as f32 / 16.,
            ))
            .bg(cx.theme().muted)
            .when(self.viewport.actual_size && can_zoom, |view| {
                view.cursor_grab()
            })
            .when(self.drag.is_some(), |view| view.cursor_grabbing())
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let tracker = tracker.clone();
                        window.defer(cx, move |_, cx| {
                            let _ = tracker
                                .update(cx, |this, cx| this.measure(bounds, display_scale, cx));
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .when_some(checker, |view, checker| {
                view.child(img(checker).absolute().size_full().rounded(gpui::rems(
                    crate::window_pair::PREVIEW_CORNER_RADIUS as f32 / 16.,
                )))
            })
            .when_some(image, |view, image| {
                view.child(
                    img(image)
                        .absolute()
                        .left(px(origin.x as f32))
                        .top(px(origin.y as f32))
                        .w(px(size.width as f32))
                        .h(px(size.height as f32))
                        .object_fit(ObjectFit::Contain),
                )
            })
            .on_hover(cx.listener(|_, _, _, cx| cx.notify()))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            );
        if let Some(data) = &self.data {
            let bytes = data.size_bytes.max(0) as f64;
            let weight = if bytes >= 1024. * 1024. {
                format!("{:.1} MB", bytes / (1024. * 1024.))
            } else {
                format!("{:.0} KB", bytes / 1024.)
            };
            let label = format!("{} × {} · {weight}", data.width, data.height);
            let overlays = div()
                .absolute()
                .inset_0()
                .child(
                    div()
                        .absolute()
                        .top_2()
                        .left_2()
                        .max_w(px((self.viewport.viewport.width - 16.).max(1.) as f32))
                        .rounded_md()
                        .bg(gpui::black().opacity(0.55))
                        .text_color(gpui::white())
                        .text_size(gpui::rems(0.6875))
                        .px_2()
                        .py_1()
                        .child(label),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_2()
                        .right_2()
                        .rounded_lg()
                        .bg(cx.theme().popover.opacity(0.92))
                        .p_1()
                        .child(
                            Button::new("image-zoom")
                                .icon(if self.viewport.actual_size {
                                    IconName::Minimize
                                } else {
                                    IconName::Maximize
                                })
                                .label(if self.viewport.actual_size {
                                    "完整显示"
                                } else {
                                    "原始尺寸"
                                })
                                .ghost()
                                .small()
                                .disabled(!can_zoom)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_zoom(
                                        Offset {
                                            x: this.viewport.viewport.width / 2.,
                                            y: this.viewport.viewport.height / 2.,
                                        },
                                        cx,
                                    )
                                })),
                        ),
                );
            let visible = self.hovered;
            let generation = self.hover_generation;
            let overlays = if generation == 0 {
                overlays.opacity(0.).into_any_element()
            } else {
                overlays
                    .with_animation(
                        ("image-overlay", generation),
                        Animation::new(Duration::from_millis(140)),
                        move |layer, progress| {
                            layer.opacity(if visible { progress } else { 1. - progress })
                        },
                    )
                    .into_any_element()
            };
            view = view.child(overlays);
        } else {
            view = view.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        Icon::new(if self.loading {
                            IconName::LoaderCircle
                        } else {
                            IconName::Frame
                        })
                        .size_6(),
                    )
                    .child(
                        div()
                            .text_size(gpui::rems(0.8125))
                            .child(self.error.clone().unwrap_or_else(|| "正在加载图片…".into())),
                    ),
            );
        }
        view.child(
            div()
                .absolute()
                .inset_0()
                .rounded(gpui::rems(
                    crate::window_pair::PREVIEW_CORNER_RADIUS as f32 / 16.,
                ))
                .border_1()
                .border_color(cx.theme().border.opacity(0.5)),
        )
    }
}
