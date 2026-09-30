use super::*;
use gpui::{div, img, AnyElement, IntoElement, MouseButton, ObjectFit, Render};
use gpui_component::{
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    text::TextView,
    ActiveTheme, IconName,
};

pub(super) fn units(value: f32) -> gpui::Rems {
    gpui::rems(value / 16.)
}

impl Panel {
    pub(super) fn row_menu(
        item: SearchResultDto,
        members: Vec<SpaceMemberDto>,
        ix: usize,
        menu: PopupMenu,
        entity: Entity<Self>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let unavailable = item.payload_state.as_deref() == Some("Lost");
        let copy = entity.clone();
        let favorite = entity.clone();
        let delete = entity.clone();
        let id = item.entry_id.clone();
        let favorite_id = id.clone();
        let delete_id = id.clone();
        let is_favorite = item.tags.iter().any(|tag| tag == "favorited");
        let send_entity = entity.clone();
        let paths = item.file_paths.clone();
        let path_entity = entity.clone();
        let reveal = paths.first().cloned();
        let menu = menu.item(
            PopupMenuItem::new("复制")
                .icon(IconName::Copy)
                .disabled(unavailable)
                .on_click(move |_, window, cx| {
                    copy.update(cx, |this, cx| {
                        this.select(ix, window, cx);
                        this.restore(false, false, window, cx);
                    });
                }),
        );
        let menu = if item.content_type == "file" {
            menu.item(
                PopupMenuItem::new("粘贴文件路径")
                    .icon(IconName::File)
                    .disabled(unavailable)
                    .on_click(move |_, window, cx| {
                        path_entity
                            .update(cx, |this, cx| this.paste_paths(paths.clone(), window, cx));
                    }),
            )
        } else {
            menu
        };
        let menu = menu
            .item(
                PopupMenuItem::new(if is_favorite {
                    "取消收藏"
                } else {
                    "收藏"
                })
                .icon(IconName::Star)
                .on_click(move |_, window, cx| {
                    favorite.update(cx, |this, cx| {
                        this.action(
                            favorite_id.clone(),
                            EntryAction::Favorite(!is_favorite),
                            window,
                            cx,
                        )
                    });
                }),
            )
            .submenu("发送到设备", window, cx, move |mut menu, _, _| {
                if members.is_empty() {
                    return menu.item(PopupMenuItem::new("没有配对设备").disabled(true));
                }
                let entity = send_entity.clone();
                let id = id.clone();
                let all_id = id.clone();
                menu = menu
                    .item(
                        PopupMenuItem::new("所有设备")
                            .disabled(unavailable)
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.action(all_id.clone(), EntryAction::Send(None), window, cx)
                                });
                            }),
                    )
                    .separator();
                for member in &members {
                    let entity = send_entity.clone();
                    let id = id.clone();
                    let peer = member.peer_id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(member.device_name.clone())
                            .disabled(unavailable || !member.connected)
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.action(
                                        id.clone(),
                                        EntryAction::Send(Some(peer.clone())),
                                        window,
                                        cx,
                                    )
                                });
                            }),
                    );
                }
                menu
            });
        let menu = if let Some(path) = reveal {
            let entity = entity.clone();
            menu.item(
                PopupMenuItem::new("在文件夹中显示")
                    .icon(IconName::FolderOpen)
                    .on_click(move |_, _, cx| {
                        if let Err(message) = platform::reveal_path(&path) {
                            entity.update(cx, |this, cx| {
                                this.message = Some(message);
                                cx.notify();
                            });
                        }
                    }),
            )
        } else {
            menu
        };
        menu.separator()
            .item(PopupMenuItem::new("删除").icon(IconName::Delete).on_click(
                move |_, window, cx| {
                    delete.update(cx, |this, cx| {
                        this.action(delete_id.clone(), EntryAction::Delete, window, cx)
                    });
                },
            ))
    }

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
                                    this.restore(true, event.modifiers().alt, window, cx);
                                },
                            ))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, _, window, cx| this.select(ix, window, cx)),
                            )
                            .on_hover(cx.listener(move |this, hovered, window, cx| {
                                if *hovered && !this.keyboard && this.pointer_moved {
                                    this.hovered = Some(ix);
                                    this.schedule_preview(window, cx);
                                    cx.notify();
                                }
                            }));
                        let item = item.clone();
                        let members = self.members.clone();
                        let entity = cx.entity();
                        let menu_id =
                            gpui::SharedString::from(format!("image-menu-{}", item.entry_id));
                        let bounds_id = item.entry_id.clone();
                        let bounds_tracker = cx.entity().downgrade();
                        let menu = tile.context_menu(move |menu, window, cx| {
                            Self::row_menu(
                                item.clone(),
                                members.clone(),
                                ix,
                                menu,
                                entity.clone(),
                                window,
                                cx,
                            )
                        });
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
                            .id(menu_id)
                            .w_full()
                            .child(menu)
                            .into_any_element()
                    }))
            }))
            .into_any_element()
    }
}

impl PreviewSnapshot {
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
        let content = if self.loading {
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
                    .child(if cfg!(target_os = "macos") {
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
            .capture_key_down(cx.listener(Self::key_down))
            .child(history)
    }
}
