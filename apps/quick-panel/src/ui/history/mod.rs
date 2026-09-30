//! The history window: search row, suggestions, nine fixed rows and the footer.

use super::view::units;
use super::*;
use gpui::{
    div, img, AnyElement, App, HighlightStyle, Hsla, IntoElement, MouseButton, ObjectFit,
    StyledText,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::Input,
    ActiveTheme, Icon, IconName, Sizable,
};
use quick_panel_core::state::VISIBLE_ROWS;
use quick_panel_core::text;

/// Rows the list shows at once. Digits 1 to 9 on the rows are the Command+digit shortcuts.
/// Suggestions listed at most; more are folded into "还有 N 条".
const MAX_SUGGESTIONS: usize = 3;

use quick_panel_core::content::{match_ranges, split_link, Kind as RowKind};

/// The words of `text` that the search matched, on a `tint` background taken from the theme.
fn marked(text: &str, query: &str, tint: Hsla) -> StyledText {
    let mark = HighlightStyle {
        background_color: Some(tint),
        ..Default::default()
    };
    StyledText::new(text.to_string()).with_highlights(
        match_ranges(text, query)
            .into_iter()
            .map(|range| (range, mark)),
    )
}

fn row_icon(kind: RowKind) -> IconName {
    match kind {
        RowKind::Link => IconName::Globe,
        RowKind::Code => IconName::SquareTerminal,
        RowKind::Image => IconName::Frame,
        RowKind::File => IconName::File,
        RowKind::Text | RowKind::RichText => IconName::Menu,
    }
}

/// Small rounded key label, the panel's only decoration.
fn dimension_icon(dimension: Dimension) -> IconName {
    match dimension {
        Dimension::Type => IconName::File,
        Dimension::Tag => IconName::Asterisk,
        Dimension::Source => IconName::Inbox,
        Dimension::Time => IconName::Calendar,
    }
}

pub(super) fn keycap(label: impl Into<gpui::SharedString>, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    div()
        .min_w(units(18.))
        .h(units(18.))
        .px(units(5.))
        .rounded(units(4.))
        .bg(theme.muted.opacity(0.7))
        .text_color(theme.muted_foreground)
        .font_family(theme.mono_font_family.clone())
        .text_size(units(10.5))
        .flex()
        .items_center()
        .justify_center()
        .child(label.into())
}

mod empty_page;
mod footer;
mod row;
mod search_row;
mod suggestions;

impl Panel {
    /// Text of a filter chip: `#tag`, `@device`, or the label of a type or time range.
    pub(super) fn chip_label(&self, dimension: Dimension, value: &str) -> String {
        match dimension {
            Dimension::Tag => format!("#{}", text::value_label(value)),
            Dimension::Source => {
                let name = self
                    .state
                    .catalog
                    .members
                    .iter()
                    .find(|member| member.peer_id == value)
                    .map_or(value, |member| member.device_name.as_str());
                format!("@{name}")
            }
            Dimension::Type => text::value_label(value).to_string(),
            Dimension::Time => value.to_string(),
        }
    }

    pub(super) fn history_view(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
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
            .p(units(6.))
            .track_scroll(&self.scroll)
            .on_mouse_move(cx.listener(|this, _, _, _| this.state.pointer_moved()))
            .when(self.state.search.loading, |list| {
                list.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(units(13.))
                        .text_color(muted)
                        .child(text::SEARCHING),
                )
            })
            .when_some(self.suggestion_block(cx), |list, block| list.child(block))
            .when(
                !self.state.search.loading && self.state.search.items.is_empty(),
                |list| list.child(self.empty_page(cx)),
            )
            .when(
                !self.state.search.loading && !self.state.search.filters.images_only(),
                |list| list.children((0..self.state.search.items.len()).map(|ix| self.row(ix, cx))),
            )
            .when(
                !self.state.search.loading && self.state.search.filters.images_only(),
                |list| list.child(self.image_wall(cx)),
            );
        let mut card = div()
            .relative()
            .w(units(
                quick_panel_core::geometry::window_pair::PANEL_WIDTH as f32,
            ))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .rounded(units(12.))
            .border_1()
            .border_color(theme.border.opacity(0.6))
            .bg(cx.global::<crate::ui::appearance::Surfaces>().background)
            .text_color(theme.foreground)
            .overflow_hidden()
            .child(self.search_row(cx))
            .child(list);
        if let Some(message) = self
            .state
            .session
            .message
            .as_ref()
            .filter(|_| !self.state.search.locked)
        {
            card = card.child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .text_size(units(11.))
                    .text_color(theme.danger)
                    .child(message.clone())
                    .child(
                        Button::new("retry")
                            .label(text::RETRY)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| this.search(window, cx))),
                    ),
            );
        }
        card.child(self.footer(cx)).into_any_element()
    }
}
