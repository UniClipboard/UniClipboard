//! One result row.

use super::*;

impl Panel {
    pub(super) fn row(&self, ix: usize, cx: &Context<Self>) -> AnyElement {
        let item = &self.state.search.items[ix];
        let kind = RowKind::of(item);
        let selected =
            self.state.search.selection.selected() == Some(ix) && !self.state.suggest.focused;
        let theme = cx.theme();
        let ink: Hsla = if selected {
            theme.primary_foreground
        } else {
            theme.foreground
        };
        let quiet: Hsla = if selected {
            theme.primary_foreground.opacity(0.78)
        } else {
            theme.muted_foreground
        };
        let lost = item.payload_state.as_deref() == Some("Lost");
        let text = item
            .file_names
            .first()
            .cloned()
            .or_else(|| item.link_urls.first().cloned())
            .or_else(|| item.text_preview.clone())
            .unwrap_or_else(|| text::value_label(&item.content_type).into())
            .replace(['\n', '\r'], " ");

        let leading = match self.images.get(&item.entry_id) {
            Some(image) if kind == RowKind::Image => div()
                .w(units(24.))
                .h(units(16.))
                .overflow_hidden()
                .rounded(units(3.))
                .border_1()
                .border_color(if selected {
                    theme.primary_foreground.opacity(0.4)
                } else {
                    theme.border
                })
                .child(
                    img(image.image.clone())
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                .into_any_element(),
            _ => Icon::new(row_icon(kind))
                .size(units(14.))
                .text_color(quiet)
                .into_any_element(),
        };
        // Ellipsis only works when the text is a direct child of the truncating element, so plain
        // text and code put the string straight into the row's text cell. A link needs its host
        // emphasised, so it is two parts whose path part truncates.
        let query = self.input.read(cx).value().to_string();
        // The theme's primary colour marks matches; on the selected row that colour is the row
        // background, so its foreground is used instead.
        let tint = if selected {
            theme.primary_foreground.opacity(0.3)
        } else {
            theme.primary.opacity(0.22)
        };
        let link = (kind == RowKind::Link).then(|| {
            let (host, path) = split_link(&text);
            div()
                .flex()
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(marked(host, &query, tint)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(quiet)
                        .child(marked(path, &query, tint)),
                )
        });
        let secondary = match kind {
            RowKind::RichText => Some(text::RICH_TEXT.to_string()),
            RowKind::File => item
                .file_extensions
                .first()
                .map(|extension| extension.to_uppercase()),
            _ => None,
        };
        let elapsed = chrono::Utc::now().timestamp_millis() - item.active_time_ms;
        let row = div()
            .id(gpui::SharedString::from(item.entry_id.clone()))
            .w_full()
            .h(units(30.))
            .flex_shrink_0()
            .pl(units(6.))
            .pr(units(10.))
            .rounded(units(7.))
            .flex()
            .items_center()
            .gap(units(8.))
            .cursor_pointer()
            .text_size(units(13.))
            .text_color(ink)
            .when(selected, |row| row.bg(theme.primary))
            .when(!selected && !self.state.search.keyboard, |row| {
                row.hover(|row| row.bg(theme.muted.opacity(0.5)))
            })
            .child(
                div()
                    .w(units(16.))
                    .flex_shrink_0()
                    .text_center()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(units(11.))
                    .text_color(quiet)
                    .when(ix < VISIBLE_ROWS, |digit| {
                        digit.child(format!("{}", ix + 1))
                    }),
            )
            .child(
                div()
                    .w(units(24.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(leading),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(lost, |cell| cell.opacity(0.5).line_through())
                    .when(kind == RowKind::Code, |cell| {
                        cell.font_family(theme.mono_font_family.clone())
                            .text_size(units(12.))
                    })
                    .when_some(link, |cell, link| cell.child(link))
                    .when(kind != RowKind::Link, |cell| {
                        cell.child(marked(&text, &query, tint))
                    }),
            )
            .when_some(secondary, |row, label| {
                row.child(self.secondary_chip(label, selected, cx))
            })
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
                    .flex()
                    .items_center()
                    .gap(units(5.))
                    .text_size(units(11.))
                    .text_color(quiet)
                    .when(item.source_device.is_some(), |meta| {
                        meta.child(
                            Icon::new(IconName::Inbox)
                                .size(units(12.))
                                .text_color(quiet),
                        )
                    })
                    .child(text::relative_time(elapsed)),
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
                if *hovered {
                    this.hover(ix, false, window, cx);
                }
            }));
        div().w_full().flex_shrink_0().child(row).into_any_element()
    }
}
