//! The search box, the filter chips and the result count.

use super::*;

impl Panel {
    pub(super) fn search_row(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let accent = theme.primary;
        // One type is shown at the right end of the row, where Tab changes it; several are chips.
        let type_chips = self.state.search.filters.types.len() > 1;
        let chips = self
            .state
            .search
            .filters
            .chips()
            .into_iter()
            .filter(|(dimension, _)| type_chips || *dimension != Dimension::Type)
            .collect::<Vec<_>>();
        let typed = !self.input.read(cx).value().is_empty();
        let active_type = self.state.search.filters.single_type().is_some();
        div()
            .h(units(48.))
            .flex_shrink_0()
            .px(units(14.))
            .flex()
            .items_center()
            .gap(units(10.))
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .child(
                Icon::new(IconName::Search)
                    .size(units(16.))
                    .text_color(theme.foreground),
            )
            .children(chips.into_iter().map(|(dimension, value)| {
                let label = self.chip_label(dimension, &value);
                div()
                    .id(gpui::SharedString::from(format!(
                        "chip-{dimension:?}-{value}"
                    )))
                    .h(units(24.))
                    .flex_shrink_0()
                    .px(units(8.))
                    .rounded(units(6.))
                    .bg(accent.opacity(0.12))
                    .text_color(accent)
                    .text_size(units(12.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.remove_filter(dimension, &value, window, cx)
                    }))
            }))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.input)
                        .appearance(false)
                        .small()
                        .disabled(self.state.session.busy)
                        .px_0()
                        .text_size(units(15.)),
                ),
            )
            .when(
                typed || !self.state.search.filters.chips().is_empty(),
                |row| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .text_size(units(12.))
                            .text_color(muted)
                            .child(if self.suggestion_options(cx).is_empty() {
                                text::result_count(self.state.search.total)
                            } else {
                                text::text_match_count(self.state.search.total)
                            }),
                    )
                },
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(units(6.))
                    .text_size(units(12.))
                    .child(
                        div()
                            .px(units(8.))
                            .py(units(2.))
                            .rounded(units(5.))
                            .when(active_type, |label| {
                                label
                                    .bg(accent.opacity(0.12))
                                    .text_color(accent)
                                    .font_weight(gpui::FontWeight::MEDIUM)
                            })
                            .when(!active_type, |label| label.text_color(muted))
                            .child(
                                self.state
                                    .search
                                    .filters
                                    .single_type()
                                    .map_or(text::ALL_TYPES, text::value_label)
                                    .to_string(),
                            ),
                    )
                    .child(keycap("⇥", cx)),
            )
            .when(
                typed || !self.state.search.filters.chips().is_empty(),
                |row| {
                    row.child(
                        Button::new("clear")
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                    )
                },
            )
            .into_any_element()
    }

    pub(super) fn secondary_chip(
        &self,
        label: String,
        selected: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        div()
            .flex_shrink_0()
            .px(units(5.))
            .py(units(1.))
            .rounded(units(4.))
            .text_size(units(10.5))
            .line_height(units(14.))
            .bg(if selected {
                theme.primary_foreground.opacity(0.2)
            } else {
                theme.muted.opacity(0.8)
            })
            .text_color(if selected {
                theme.primary_foreground.opacity(0.78)
            } else {
                theme.muted_foreground
            })
            .child(label)
            .into_any_element()
    }
}
