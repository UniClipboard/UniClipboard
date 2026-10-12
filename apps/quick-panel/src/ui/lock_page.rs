//! The locked page: the whole window while the content is locked.
//!
//! It has a passphrase box of its own and shares nothing with the search page: no search row,
//! chips, list or footer, and what is typed here is never a query.

use super::history::keycap;
use super::view::units;
use super::*;
use gpui::{div, AnyElement, IntoElement};
use gpui_component::{input::Input, ActiveTheme, Icon, IconName, Sizable};
use quick_panel_core::state::UnlockStatus;

impl Panel {
    pub(super) fn lock_page(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let status = &self.state.session.unlock;
        let (detail, detail_color) = match status {
            UnlockStatus::Idle => (text::t().locked_hint.to_string(), muted),
            UnlockStatus::Checking => (text::t().unlocking.to_string(), muted),
            UnlockStatus::Failed(failure) => (failure.to_string(), theme.danger),
        };
        let hint = |keys: &'static str, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(units(6.))
                .text_size(units(12.))
                .text_color(muted)
                .child(label)
                .child(keycap(keys, cx))
        };
        div()
            .relative()
            .w(units(
                quick_panel_core::geometry::window_pair::PANEL_WIDTH as f32,
            ))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(units(14.))
            .px(units(32.))
            .rounded(units(platform::WINDOW_CORNER_RADIUS as f32))
            .border_1()
            .border_color(theme.border.opacity(0.6))
            .bg(cx.global::<crate::ui::appearance::Surfaces>().background)
            .text_color(theme.foreground)
            .overflow_hidden()
            .child(
                Icon::new(IconName::Asterisk)
                    .size(units(28.))
                    .text_color(muted),
            )
            .child(
                div()
                    .text_size(units(15.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(text::t().locked_title),
            )
            .child(
                div()
                    .w_full()
                    .h(units(36.))
                    .px(units(10.))
                    .flex()
                    .items_center()
                    .rounded(units(8.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.muted.opacity(0.4))
                    .child(
                        Input::new(&self.passphrase)
                            .appearance(false)
                            .small()
                            .disabled(*status == UnlockStatus::Checking)
                            .px_0()
                            .text_size(units(14.)),
                    ),
            )
            .child(
                div()
                    .text_size(units(12.))
                    .text_color(detail_color)
                    .text_center()
                    .child(detail),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(units(14.))
                    .child(hint("⏎", text::t().unlock))
                    .child(hint("esc", text::t().close)),
            )
            .into_any_element()
    }
}
