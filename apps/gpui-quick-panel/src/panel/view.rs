use super::*;
use gpui::{div, img, AnyElement, IntoElement, MouseButton, ObjectFit, Render};
use gpui_component::{text::TextView, ActiveTheme};

pub(super) fn units(value: f32) -> gpui::Rems {
    gpui::rems(value / 16.)
}

impl Panel {
    pub(super) fn image_wall(&self, cx: &Context<Self>) -> AnyElement {
        let mut columns: [Vec<usize>; 3] = Default::default();
        let mut heights = [0_f32; 3];
        for (ix, item) in self.items.iter().enumerate() {
            let ratio = self
                .images
                .get(&item.entry_id)
                .map(|i| i.width as f32 / i.height.max(1) as f32)
                .unwrap_or(1.)
                .clamp(0.45, 2.2);
            let column = (0..3)
                .min_by(|a, b| heights[*a].total_cmp(&heights[*b]))
                .unwrap_or(0);
            columns[column].push(ix);
            heights[column] += 1. / ratio;
        }
        div()
            .flex()
            .w_full()
            .gap(units(4.))
            .items_start()
            .children(columns.into_iter().map(|indices| {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(units(4.))
                    .children(indices.into_iter().map(|ix| {
                        let item = &self.items[ix];
                        let image = self.images.get(&item.entry_id);
                        let selected = self.selection.selected() == Some(ix);
                        let ratio = image
                            .map(|i| i.width as f32 / i.height.max(1) as f32)
                            .unwrap_or(1.)
                            .clamp(0.45, 2.2);
                        let tile = div()
                            .id(("image", ix))
                            .w_full()
                            .h(units(109. / ratio))
                            .relative()
                            .rounded_md()
                            .overflow_hidden()
                            .border_2()
                            .border_color(if selected {
                                cx.theme().primary
                            } else {
                                cx.theme().border.opacity(0.3)
                            })
                            .bg(cx.theme().muted)
                            .cursor_pointer()
                            .when_some(image, |tile, image| {
                                tile.child(
                                    img(image.image.clone())
                                        .size_full()
                                        .object_fit(ObjectFit::Cover),
                                )
                            })
                            .when(ix < 10, |tile| {
                                tile.child(
                                    div()
                                        .absolute()
                                        .bottom(units(4.))
                                        .right(units(4.))
                                        .rounded_sm()
                                        .bg(gpui::black().opacity(0.6))
                                        .text_color(gpui::white())
                                        .px_1()
                                        .text_size(units(10.))
                                        .child(format!("⌘{}", if ix == 9 { 0 } else { ix + 1 })),
                                )
                            })
                            .on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    this.select(ix, window, cx);
                                    this.restore(true, event.modifiers().shift, false, window, cx);
                                },
                            ))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, _, window, cx| {
                                    this.select(ix, window, cx);
                                    this.open_actions(window, cx);
                                }),
                            )
                            .on_hover(cx.listener(move |this, hovered, window, cx| {
                                if *hovered && !this.keyboard && this.pointer_moved {
                                    this.hovered = Some(ix);
                                    this.schedule_preview(window, cx);
                                    cx.notify();
                                }
                            }));
                        let bounds_id = item.entry_id.clone();
                        let bounds_tracker = cx.entity().downgrade();
                        div()
                            .on_children_prepainted(move |bounds, window, cx| {
                                if let Some(bounds) = bounds.first().copied() {
                                    let tracker = bounds_tracker.clone();
                                    let id = bounds_id.clone();
                                    window.defer(cx, move |window, cx| {
                                        let _ = tracker.update(cx, |this, cx| {
                                            this.record_image_bounds(id, bounds, window, cx)
                                        });
                                    });
                                }
                            })
                            .w_full()
                            .child(tile)
                            .into_any_element()
                    }))
            }))
            .into_any_element()
    }
}

impl PreviewSnapshot {
    /// The rows of the action list. A click runs the row through the panel, which owns the
    /// selected entry and the history window that keeps the keyboard focus.
    fn actions_list(
        &self,
        actions: &ActionsView,
        cx: &mut Context<preview_window::PreviewWindow>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (accent, muted) = (theme.primary, theme.muted_foreground);
        div()
            .p(units(6.))
            .flex()
            .flex_col()
            .gap(units(2.))
            .children(actions.rows.iter().enumerate().map(|(index, row)| {
                let selected = index == actions.cursor;
                let action = row.action.clone();
                let enabled = row.enabled;
                div()
                    .id(("action", index))
                    .h(units(30.))
                    .px(units(10.))
                    .rounded(units(7.))
                    .flex()
                    .items_center()
                    .gap(units(8.))
                    .text_size(units(13.))
                    .when(selected, |line| line.bg(accent.opacity(0.12)))
                    .when(!enabled, |line| line.opacity(0.4))
                    .when(enabled, |line| line.cursor_pointer())
                    .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
                    .children(
                        row.shortcut
                            .map(|keys| div().text_size(units(11.)).text_color(muted).child(keys)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if enabled {
                            this.run_action(action.clone(), cx);
                        }
                    }))
            }))
            .into_any_element()
    }

    pub(super) fn preview_view(
        &self,
        window: &mut Window,
        cx: &mut Context<preview_window::PreviewWindow>,
        generation: u64,
    ) -> AnyElement {
        let chrome_tracker = cx.weak_entity();
        let content_tracker = cx.weak_entity();
        let scale = self.scale;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.border.opacity(0.5);
        let mut card = div()
            .on_children_prepainted(move |bounds, window, cx| {
                if bounds.len() == 3 {
                    let height =
                        f64::from(bounds[0].size.height + bounds[2].size.height) + 2. * scale;
                    let tracker = chrome_tracker.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = tracker.update(cx, |this, cx| {
                            this.measure(
                                generation,
                                preview_window::Measurement::Chrome,
                                height,
                                window,
                                cx,
                            )
                        });
                    });
                }
            })
            .w_full()
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .rounded(units(crate::window_pair::PREVIEW_CORNER_RADIUS as f32))
            .border_1()
            .border_color(border)
            .bg(cx.global::<crate::appearance::Surfaces>().card)
            .overflow_hidden();
        let Some(item) = self.item.as_ref() else {
            return card.into_any_element();
        };
        let mut metadata = crate::strings::value_label(&item.content_type).to_string();
        if let Some(text) = &self.text {
            metadata.push_str(&format!(" · {} 个字符", text.encode_utf16().count()));
        }
        if let Some(image) = self.image.as_ref() {
            metadata.push_str(&format!(" · {} × {}", image.width, image.height));
        }
        if let Some(actions) = &self.actions {
            metadata = actions.title.clone();
        }
        card = card.child(
            div()
                .p(units(12.))
                .flex_shrink_0()
                .text_size(units(11.))
                .text_color(muted.opacity(0.75))
                .child(metadata),
        );
        let text = self
            .text
            .as_deref()
            .or(item.text_preview.as_deref())
            .unwrap_or("");
        let content = if let Some(actions) = &self.actions {
            self.actions_list(actions, cx)
        } else if self.loading {
            div()
                .p_6()
                .text_size(units(14.))
                .text_color(muted)
                .child("正在加载…")
                .into_any_element()
        } else if item.content_type == "file" {
            div()
                .p_6()
                .flex()
                .flex_col()
                .gap_3()
                .children(item.file_names.iter().map(|name| {
                    div()
                        .p_4()
                        .rounded_lg()
                        .border_1()
                        .border_color(border)
                        .text_size(units(14.))
                        .child(name.clone())
                }))
                .into_any_element()
        } else {
            let escaped = text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            div()
                .p(units(24.))
                .text_size(units(14.))
                .font_family("JetBrains Mono")
                .line_height(units(22.))
                .child(
                    TextView::html("preview-text", format!("<pre>{escaped}</pre>"), window, cx)
                        .selectable(true),
                )
                .into_any_element()
        };
        let measured_content = div()
            .w_full()
            .flex_shrink_0()
            .on_children_prepainted(move |bounds, window, cx| {
                if let Some(bounds) = bounds.first() {
                    let height = f64::from(bounds.size.height);
                    let tracker = content_tracker.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = tracker.update(cx, |this, cx| {
                            this.measure(
                                generation,
                                preview_window::Measurement::Content,
                                height,
                                window,
                                cx,
                            )
                        });
                    });
                }
            })
            .child(content);
        card = card
            .child(
                div()
                    .id("preview-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(measured_content),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(border)
                    .px(units(12.))
                    .py(units(6.))
                    .text_size(units(11.))
                    .text_color(muted)
                    .child(if self.actions.is_some() {
                        crate::strings::ACTIONS_HINT
                    } else if cfg!(target_os = "macos") {
                        "⌘⇧⌫ 删除"
                    } else {
                        "Ctrl+Shift+⌫ 删除"
                    }),
            );
        card.into_any_element()
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let history = self.history_view(cx);
        div()
            .size_full()
            .text_color(cx.theme().foreground)
            .key_context("QuickPanel")
            .on_action(
                cx.listener(|this, _: &NextSuggestion, window, cx| this.tab(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PreviousSuggestion, window, cx| this.tab(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NextCandidate, _, cx| {
                this.next_suggestion_candidate(cx);
            }))
            .capture_action(cx.listener(Self::copy_action))
            // The input binds Command+Backspace (Ctrl+Backspace off macOS) to a deletion; the panel
            // uses the shortcut to clear the search and its filters instead.
            .capture_action(cx.listener(Self::clear_action))
            .capture_key_down(cx.listener(Self::key_down))
            .child(history)
    }
}
