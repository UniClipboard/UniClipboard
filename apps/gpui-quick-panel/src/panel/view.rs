use super::*;
use gpui::{div, img, AnyElement, IntoElement, MouseButton, ObjectFit, Render};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::Input,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    text::TextView,
    ActiveTheme, Icon, IconName, Sizable,
};

fn units(value: f32) -> gpui::Rems {
    gpui::rems(value / 16.)
}

impl Panel {
    fn row_menu(
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

    fn row(&self, ix: usize, cx: &Context<Self>) -> AnyElement {
        let item = &self.items[ix];
        let selected = self.selection.selected() == Some(ix);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let foreground = theme.foreground;
        let primary = theme.primary;
        let on_primary = theme.primary_foreground;
        let lost = item.payload_state.as_deref() == Some("Lost");
        let text = item
            .file_names
            .first()
            .cloned()
            .or_else(|| item.link_urls.first().cloned())
            .or_else(|| item.text_preview.clone())
            .unwrap_or_else(|| filters::label(&item.content_type).into());
        let icon = if item.content_type == "image" {
            IconName::Frame
        } else {
            IconName::File
        };
        let mut leading = Icon::new(icon)
            .size(units(14.))
            .text_color(if selected {
                on_primary.opacity(0.7)
            } else {
                muted.opacity(0.6)
            })
            .into_any_element();
        if let Some(image) = self.images.get(&item.entry_id) {
            leading = img(image.image.clone())
                .w(units(28.))
                .h(units(16.))
                .object_fit(ObjectFit::Cover)
                .rounded_sm()
                .into_any_element();
        }
        let minutes = ((chrono::Utc::now().timestamp_millis() - item.active_time_ms) as f64
            / 60000.)
            .round() as i64;
        let time = if minutes < 1 {
            "just now".into()
        } else if minutes < 60 {
            format!("{minutes}m")
        } else if minutes < 1440 {
            format!("{}h", minutes / 60)
        } else {
            format!("{}d", minutes / 1440)
        };
        let entity = cx.entity();
        let menu_item = item.clone();
        let menu_members = self.members.clone();
        let row = div()
            .id(gpui::SharedString::from(item.entry_id.clone()))
            .w_full()
            .h(units(32.25))
            .px(units(16.))
            .py(units(8.))
            .rounded(units(6.))
            .flex()
            .items_center()
            .gap(units(10.))
            .cursor_pointer()
            .text_size(units(13.))
            .line_height(units(16.25))
            .text_color(if selected { on_primary } else { foreground })
            .when(selected, |row| row.bg(primary))
            .when(!selected && !self.keyboard, |row| {
                row.hover(|row| row.bg(theme.muted.opacity(0.5)))
            })
            .child(leading)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(lost, |text| text.opacity(0.5).line_through())
                    .child(text.replace(['\n', '\r'], " ")),
            )
            .when(item.tags.iter().any(|tag| tag == "favorited"), |row| {
                row.child(
                    Icon::new(IconName::Star)
                        .size(units(12.))
                        .text_color(gpui::rgb(0xfbbf24)),
                )
            })
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(units(11.))
                    .text_color(if selected {
                        on_primary.opacity(0.6)
                    } else {
                        muted.opacity(0.5)
                    })
                    .child(time),
            )
            .when(ix < 10, |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .rounded(units(3.))
                        .border_1()
                        .border_color(if selected {
                            on_primary.opacity(0.3)
                        } else {
                            theme.border
                        })
                        .px(units(4.))
                        .py(units(2.))
                        .text_size(units(10.))
                        .line_height(units(10.))
                        .text_color(if selected {
                            on_primary.opacity(0.7)
                        } else {
                            muted.opacity(0.5)
                        })
                        .child(format!("⌘{}", if ix == 9 { 0 } else { ix + 1 })),
                )
            })
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.select(ix, window, cx);
                    this.restore(true, event.modifiers().alt, window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| this.select(ix, window, cx)),
            )
            .on_hover(cx.listener(move |this, hovered, window, cx| {
                if *hovered && !this.keyboard && this.pointer_moved && !this.loading {
                    this.hovered = Some(ix);
                    this.schedule_preview(window, cx);
                    cx.notify();
                }
            }))
            .context_menu(move |menu, window, cx| {
                Self::row_menu(
                    menu_item.clone(),
                    menu_members.clone(),
                    ix,
                    menu,
                    entity.clone(),
                    window,
                    cx,
                )
            });
        // ContextMenuExt owns a fixed element ID. Scope the whole wrapper by
        // entry, not just its child row, so sibling menus do not share state.
        div()
            .id(gpui::SharedString::from(format!(
                "entry-menu-{}",
                item.entry_id
            )))
            .w_full()
            .child(row)
            .into_any_element()
    }

    fn history_view(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let border = theme.border.opacity(0.5);
        let muted = theme.muted_foreground;
        let filters = self.filters.chips();
        let has_content = !filters.is_empty() || !self.input.read(cx).value().is_empty();
        let search = div().px(units(12.)).py(units(8.)).child(
            div()
                .min_h(units(28.))
                .w_full()
                .rounded(units(16.))
                .border_1()
                .border_color(theme.border.opacity(0.6))
                .bg(theme.muted.opacity(0.7))
                .flex()
                .items_center()
                .gap(units(6.))
                .px(units(10.))
                .children(
                    filters
                        .iter()
                        .take(2)
                        .enumerate()
                        .map(|(ix, (dimension, value))| {
                            let dimension = *dimension;
                            div()
                                .id(("chip", ix))
                                .rounded_full()
                                .px(units(5.))
                                .py(units(2.))
                                .bg(theme.accent)
                                .text_size(units(11.))
                                .cursor_pointer()
                                .child(format!("{} ×", filters::label(value)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.change_filter(dimension, None, window, cx)
                                }))
                        }),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&self.input)
                            .appearance(false)
                            .small()
                            .disabled(self.busy)
                            .prefix(
                                Icon::new(IconName::Search)
                                    .size(units(14.))
                                    .text_color(muted.opacity(0.5)),
                            )
                            .text_size(units(12.)),
                    ),
                )
                .when(!has_content && self.total > 0, |view| {
                    view.child(
                        div()
                            .text_size(units(11.))
                            .text_color(muted.opacity(0.4))
                            .child(format!("{} 项", self.total)),
                    )
                })
                .when(has_content, |view| {
                    view.child(
                        Button::new("clear")
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                    )
                }),
        );
        let type_bar = div()
            .flex()
            .gap(units(4.))
            .px(units(12.))
            .pb(units(8.))
            .children(filters::TYPES.iter().enumerate().map(|(ix, value)| {
                let active = self.filters.content_type == ix;
                let value = (*value).to_string();
                div()
                    .id(("type", ix))
                    .rounded(units(6.))
                    .px(units(8.))
                    .py(units(4.))
                    .text_size(units(11.))
                    .line_height(units(17.6))
                    .bg(if active {
                        theme.primary
                    } else {
                        theme.muted.opacity(0.6)
                    })
                    .text_color(if active {
                        theme.primary_foreground
                    } else {
                        muted
                    })
                    .cursor_pointer()
                    .child(filters::label(&value).to_string())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.change_filter(Dimension::Type, Some(value.clone()), window, cx)
                    }))
            }));
        let anchor_tracker = cx.entity().downgrade();
        let list = div()
            .on_children_prepainted(move |_, window, cx| {
                let tracker = anchor_tracker.clone();
                window.defer(cx, move |window, cx| {
                    let _ = tracker.update(cx, |this, cx| this.update_preview_anchor(window, cx));
                });
            })
            .id("history-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(units(6.))
            .py(units(4.))
            .track_scroll(&self.scroll)
            .on_mouse_move(cx.listener(|this, _, _, _| {
                this.pointer_moved = true;
                this.keyboard = false;
            }))
            .when(self.loading, |list| {
                list.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(units(13.))
                        .text_color(muted)
                        .child("正在搜索…"),
                )
            })
            .when(!self.loading && self.items.is_empty(), |list| {
                list.child(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .items_center()
                        .justify_center()
                        .text_color(muted)
                        .child(Icon::new(IconName::Search).size(units(24.)))
                        .child("暂无匹配的记录")
                        .child(
                            div()
                                .text_size(units(11.))
                                .child("试试其他关键词或筛选条件"),
                        ),
                )
            })
            .when(!self.loading && self.filters.content_type != 3, |list| {
                list.children((0..self.items.len()).map(|ix| self.row(ix, cx)))
            })
            .when(!self.loading && self.filters.content_type == 3, |list| {
                list.child(self.image_wall(cx))
            });
        let tag_bar = div()
            .border_t_1()
            .border_color(border)
            .px(units(12.))
            .py(units(6.))
            .flex()
            .items_center()
            .gap(units(8.))
            .text_size(units(11.))
            .text_color(muted)
            .child("标签")
            .child(
                div()
                    .id("tag-scroll")
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .flex()
                    .gap(units(4.))
                    .children(self.tags.iter().enumerate().map(|(ix, tag)| {
                        let active = self.filters.tag.as_ref() == Some(tag);
                        let tag = tag.clone();
                        div()
                            .id(("tag", ix))
                            .flex_shrink_0()
                            .rounded(units(6.))
                            .px(units(8.))
                            .py(units(4.))
                            .cursor_pointer()
                            .bg(if active {
                                theme.primary
                            } else {
                                theme.muted.opacity(0.6)
                            })
                            .text_color(if active {
                                theme.primary_foreground
                            } else {
                                muted
                            })
                            .child(format!("# {}", filters::label(&tag)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.change_filter(
                                    Dimension::Tag,
                                    if active { None } else { Some(tag.clone()) },
                                    window,
                                    cx,
                                )
                            }))
                    })),
            );
        let mut card = div()
            .relative()
            .w(units(360.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .rounded(units(12.))
            .border_1()
            .border_color(border)
            .bg(cx.global::<crate::appearance::Surfaces>().background)
            .text_color(theme.foreground)
            .overflow_hidden()
            .child(search)
            .child(type_bar)
            .child(list)
            .child(tag_bar);
        if let Some(message) = &self.message {
            card = card.child(
                div()
                    .px_3()
                    .py_2()
                    .text_size(units(11.))
                    .text_color(theme.danger)
                    .child(message.clone())
                    .child(
                        Button::new("retry")
                            .label(if self.locked { "解锁" } else { "重试" })
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.locked {
                                    this.action(String::new(), EntryAction::Unlock, window, cx)
                                } else {
                                    this.search(window, cx)
                                }
                            })),
                    ),
            );
        }
        if self.suggestions_open {
            let candidates = self.candidates(cx);
            if !candidates.is_empty() {
                card = card.child(
                    div()
                        .absolute()
                        .top(units(42.))
                        .left(units(12.))
                        .right(units(12.))
                        .max_h(units(260.))
                        .id("suggestions")
                        .overflow_y_scroll()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .shadow_lg()
                        .p_1()
                        .children(candidates.into_iter().enumerate().map(|(ix, (_, label))| {
                            div()
                                .id(("suggestion", ix))
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .text_size(units(12.))
                                .cursor_pointer()
                                .when(ix == self.suggestion_index, |row| row.bg(theme.accent))
                                .child(label)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.apply_candidate(ix, window, cx)
                                }))
                        })),
                );
            }
        }
        card.into_any_element()
    }

    fn image_wall(&self, cx: &Context<Self>) -> AnyElement {
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
            .rounded(units(12.))
            .border_1()
            .border_color(border)
            .bg(cx.global::<crate::appearance::Surfaces>().card)
            .overflow_hidden();
        let Some(item) = self.item.as_ref() else {
            return card.into_any_element();
        };
        let mut metadata = filters::label(&item.content_type).to_string();
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
        } else if let Some(image) = self.image.as_ref() {
            let image_height =
                (328. * image.height as f32 / image.width.max(1) as f32).clamp(48., 360.);
            div()
                .w_full()
                .p_4()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(image.image.clone())
                        .w_full()
                        .h(units(image_height))
                        .object_fit(ObjectFit::Contain),
                )
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
                    .child("⌥⌫ 删除"),
            );
        card.into_any_element()
    }
}

impl Render for Panel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let history = self.history_view(cx);
        div()
            .size_full()
            .text_color(cx.theme().foreground)
            .capture_key_down(cx.listener(Self::key_down))
            .child(history)
    }
}
