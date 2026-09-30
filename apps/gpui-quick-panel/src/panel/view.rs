use super::*;
use gpui::{div, img, AnyElement, IntoElement, MouseButton, ObjectFit, Render};
use gpui_component::{text::TextView, ActiveTheme, Icon, IconName};

pub(super) fn units(value: f32) -> gpui::Rems {
    gpui::rems(value / 16.)
}

impl Panel {
    /// The image grid: three rows of three equal cells, showing the entries from `grid_top`.
    /// The badge of a cell is the digit that Command plus that digit pastes.
    pub(super) fn image_wall(&self, cx: &Context<Self>) -> AnyElement {
        use crate::grid;
        let visible = grid::visible(self.grid_top, self.items.len());
        let now_ms = chrono::Utc::now().timestamp_millis();
        let cell = |slot: usize| -> AnyElement {
            let ix = visible.start + slot;
            let Some(item) = (ix < visible.end).then(|| &self.items[ix]) else {
                return div().flex_1().min_w_0().into_any_element();
            };
            let image = self.images.get(&item.entry_id);
            let selected = self.selection.selected() == Some(ix);
            let primary = cx.theme().primary;
            let tile = div()
                .id(("image", ix))
                .size_full()
                .relative()
                .rounded(units(7.))
                .overflow_hidden()
                .bg(cx.theme().muted)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(selected, |tile| {
                    tile.border_2()
                        .border_color(primary)
                        .shadow(vec![gpui::BoxShadow {
                            color: primary.opacity(0.14),
                            offset: gpui::point(gpui::px(0.), gpui::px(0.)),
                            blur_radius: gpui::px(0.),
                            spread_radius: gpui::px(3.),
                        }])
                })
                .when(!selected, |tile| {
                    tile.border_1().border_color(cx.theme().border.opacity(0.4))
                })
                .map(|tile| match image {
                    Some(image) => tile.child(
                        img(image.image.clone())
                            .size_full()
                            .object_fit(ObjectFit::Cover),
                    ),
                    None => tile.child(
                        Icon::new(IconName::Frame)
                            .size(units(20.))
                            .text_color(cx.theme().muted_foreground.opacity(0.7)),
                    ),
                })
                .child(
                    div()
                        .absolute()
                        .left(units(5.))
                        .top(units(5.))
                        .min_w(units(16.))
                        .h(units(16.))
                        .rounded(units(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(if selected {
                            primary
                        } else {
                            gpui::black().opacity(0.45)
                        })
                        .text_color(if selected {
                            cx.theme().primary_foreground
                        } else {
                            gpui::white()
                        })
                        .text_size(units(10.))
                        .child((slot + 1).to_string()),
                )
                .child(
                    div()
                        .absolute()
                        .right(units(5.))
                        .bottom(units(4.))
                        .px(units(4.))
                        .rounded(units(4.))
                        .bg(gpui::black().opacity(0.35))
                        .text_color(gpui::white().opacity(0.92))
                        .text_size(units(10.))
                        .child(strings::relative_time(now_ms - item.active_time_ms)),
                )
                .on_click(
                    cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        this.select(ix, window, cx);
                        this.restore(true, event.modifiers().shift, false, window, cx);
                    }),
                )
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
                .flex_1()
                .min_w_0()
                .h_full()
                .p(units(2.))
                .child(tile)
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, _, cx| {
                // One row per notch; a trackpad reports pixels, so 30 px make a row.
                let line = event.delta.pixel_delta(gpui::px(30.)).y;
                let rows = (f32::from(-line) / 30.).round() as isize;
                let next = crate::grid::scrolled(this.grid_top, rows, this.items.len());
                if next != this.grid_top {
                    this.grid_top = next;
                    this.image_bounds.clear();
                    cx.notify();
                }
            }))
            .children((0..grid::ROWS).map(|row| {
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .children((0..grid::COLUMNS).map(|column| cell(row * grid::COLUMNS + column)))
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

    /// The preview body of an entry, by kind.
    fn body(
        &self,
        item: &SearchResultDto,
        text: &str,
        border: gpui::Hsla,
        window: &mut Window,
        cx: &mut Context<preview_window::PreviewWindow>,
    ) -> AnyElement {
        use crate::content::{self, Kind};
        let theme = cx.theme();
        let (muted, mono) = (theme.muted_foreground, theme.mono_font_family.clone());
        match Kind::of(item) {
            Kind::File => div()
                .p(units(16.))
                .flex()
                .flex_col()
                .gap(units(8.))
                .children(content::files(item).into_iter().map(|(name, path)| {
                    div()
                        .p(units(12.))
                        .rounded_lg()
                        .border_1()
                        .border_color(border)
                        .flex()
                        .flex_col()
                        .gap(units(4.))
                        .child(div().text_size(units(14.)).child(name))
                        .children(path.map(|path| {
                            div()
                                .text_size(units(11.))
                                .text_color(muted)
                                .font_family(mono.clone())
                                .child(path)
                        }))
                }))
                .into_any_element(),
            Kind::Link => {
                let urls: Vec<&String> = item.link_urls.iter().collect();
                let (host, rest) = urls
                    .first()
                    .map_or(("", ""), |url| content::split_link(url));
                div()
                    .p(units(20.))
                    .flex()
                    .flex_col()
                    .gap(units(6.))
                    .child(
                        div()
                            .text_size(units(20.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(host.to_string()),
                    )
                    .child(
                        div()
                            .text_size(units(12.))
                            .text_color(muted)
                            .font_family(mono)
                            .child(rest.to_string()),
                    )
                    .children(urls.iter().skip(1).map(|url| {
                        div()
                            .text_size(units(12.))
                            .text_color(muted)
                            .child((*url).clone())
                    }))
                    .when(
                        !text.trim().is_empty()
                            && urls.first().is_none_or(|u| u.as_str() != text.trim()),
                        |view| {
                            view.child(
                                div()
                                    .pt(units(8.))
                                    .text_size(units(13.))
                                    .child(text.to_string()),
                            )
                        },
                    )
                    .into_any_element()
            }
            Kind::Code => div()
                .py(units(12.))
                .text_size(units(13.))
                .line_height(units(20.))
                .font_family(mono)
                .children(content::code_lines(text).into_iter().map(|(number, line)| {
                    div()
                        .flex()
                        .child(
                            div()
                                .w(units(44.))
                                .flex_shrink_0()
                                .pr(units(10.))
                                .text_right()
                                .text_color(muted.opacity(0.7))
                                .child(number.to_string()),
                        )
                        .child(div().flex_1().min_w_0().child(line.to_string()))
                }))
                .into_any_element(),
            Kind::Text | Kind::RichText | Kind::Image => {
                let escaped = text
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                div()
                    .p(units(20.))
                    .text_size(units(14.))
                    .line_height(units(22.))
                    .child(
                        TextView::html("preview-text", format!("<pre>{escaped}</pre>"), window, cx)
                            .selectable(true),
                    )
                    .into_any_element()
            }
        }
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
        let mut metadata = crate::content::header(&crate::content::Facts {
            item,
            text: self.text.as_deref(),
            image: self
                .image
                .as_ref()
                .map(|image| (image.width, image.height, image.size_bytes)),
            source_name: self.source_name.as_deref(),
            now_ms: self.now_ms,
        });
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
        } else {
            self.body(item, text, border, window, cx)
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
            .capture_action(cx.listener(Self::backspace_action))
            .capture_action(cx.listener(Self::move_left_action))
            .capture_action(cx.listener(Self::move_right_action))
            .capture_key_down(cx.listener(Self::key_down))
            .child(history)
    }
}
