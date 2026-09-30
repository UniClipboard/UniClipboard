//! Filter suggestions drawn above the results.

use super::*;

impl Panel {
    /// Candidate filters for the words typed, then the heading of the text matches below them.
    /// Tab accepts the highlighted suggestion. One child of the result list.
    pub(super) fn suggestion_block(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let options = self.suggestion_options(cx);
        if options.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let quiet = theme.muted_foreground;
        let accent = theme.primary;
        let surface = cx.global::<crate::ui::appearance::Surfaces>().background;
        let query = self.input.read(cx).value().to_string();
        let cursor = if self.state.suggest.focused {
            self.state.suggest.cursor.min(options.len() - 1)
        } else {
            0
        };
        let start = filters::suggestion_window_start(cursor, MAX_SUGGESTIONS);
        let hidden = options.len().saturating_sub(start + MAX_SUGGESTIONS);
        let heading = |label: String, note: Option<&'static str>| {
            div()
                .h(units(22.))
                .flex_shrink_0()
                .px(units(8.))
                .flex()
                .items_center()
                .text_size(units(11.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(quiet)
                .child(label)
                .child(div().flex_1())
                .when_some(note, |row, note| {
                    row.child(div().font_weight(gpui::FontWeight::NORMAL).child(note))
                })
        };
        let rows = options
            .iter()
            .enumerate()
            .skip(start)
            .take(MAX_SUGGESTIONS)
            .map(|(ix, option)| {
                let word = query
                    .get(option.matched.clone())
                    .unwrap_or_default()
                    .to_string();
                let value = self.chip_label(option.dimension, &option.value);
                let highlighted = ix == cursor;
                div()
                    .h(units(30.))
                    .flex_shrink_0()
                    .pl(units(6.))
                    .pr(units(8.))
                    .rounded(units(7.))
                    .flex()
                    .items_center()
                    .gap(units(8.))
                    .text_size(units(12.5))
                    .when(highlighted, |row| row.bg(accent.opacity(0.12)))
                    .child(div().w(units(16.)).flex_shrink_0())
                    .child(
                        div()
                            .w(units(24.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(dimension_icon(option.dimension))
                                    .size(units(14.))
                                    .text_color(quiet),
                            ),
                    )
                    .child(
                        div()
                            .px(units(5.))
                            .rounded(units(4.))
                            .bg(theme.foreground.opacity(0.06))
                            .font_family(theme.mono_font_family.clone())
                            .text_size(units(11.5))
                            .line_height(units(18.))
                            .child(word),
                    )
                    .child(div().text_color(quiet).child("→"))
                    .child(
                        div()
                            .text_color(quiet)
                            .child(text::dimension_label(option.dimension)),
                    )
                    .child(
                        div().flex_1().min_w_0().flex().child(
                            div()
                                .px(units(7.))
                                .py(units(1.))
                                .rounded(units(5.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(accent)
                                .bg(if highlighted {
                                    surface
                                } else {
                                    theme.foreground.opacity(0.06)
                                })
                                .truncate()
                                .child(value),
                        ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(units(4.))
                            .text_size(units(11.))
                            .text_color(quiet)
                            .when(!highlighted, |hint| hint.child(text::PRESS_AGAIN))
                            .child(keycap("⇥", cx)),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let unmatched = filters::unmatched_words(&query, &options);
        Some(
            div()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .child(heading(
                    text::SUGGESTIONS.to_string(),
                    (options.len() > 1).then_some(text::ACCEPT_IN_ORDER),
                ))
                .children(rows)
                .when(hidden > 0, |block| {
                    block.child(
                        div()
                            .h(units(20.))
                            .px(units(8.))
                            .flex()
                            .items_center()
                            .text_size(units(11.))
                            .text_color(quiet)
                            .child(text::hidden_count(hidden)),
                    )
                })
                .child(
                    div()
                        .h(units(1.))
                        .mx(units(6.))
                        .my(units(6.))
                        .flex_shrink_0()
                        .bg(theme.border.opacity(0.6)),
                )
                .child(heading(text::text_matches_heading(&unmatched), None))
                .into_any_element(),
        )
    }
}
