//! What the list shows when there is nothing to list.

use super::*;

impl Panel {
    /// What the list area shows when there is nothing to list: first use, locked, disconnected, or
    /// no match with ways to loosen the search.
    pub(super) fn empty_page(&self, cx: &Context<Self>) -> AnyElement {
        use quick_panel_core::empty_page::{classify, Empty};
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let searching = !self.state.search.filters.query.trim().is_empty()
            || !self.state.search.filters.chips().is_empty();
        let kind = classify(
            self.state.search.locked,
            self.state.search.disconnected.is_some(),
            searching,
        );
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
        let (icon, title, detail) = match kind {
            Empty::FirstUse => (
                IconName::Inbox,
                text::FIRST_USE_TITLE.to_string(),
                text::FIRST_USE_HINT.to_string(),
            ),
            Empty::Locked => (
                IconName::Asterisk,
                text::LOCKED_TITLE.to_string(),
                text::LOCKED_HINT_GUI.to_string(),
            ),
            Empty::Disconnected => (
                IconName::TriangleAlert,
                text::DISCONNECTED_TITLE.to_string(),
                text::reconnecting(self.state.search.disconnected.unwrap_or(1)),
            ),
            Empty::NoMatch => (
                IconName::Search,
                text::no_match(&self.state.search.filters.query),
                String::new(),
            ),
        };
        let visible = self.state.visible_relaxations();
        let mut page = div()
            .size_full()
            .px(units(24.))
            .flex()
            .flex_col()
            .gap(units(10.))
            .items_center()
            .justify_center()
            .text_color(muted)
            .child(Icon::new(icon).size(units(26.)))
            .child(
                div()
                    .text_size(units(15.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(title),
            );
        if kind == Empty::NoMatch && visible.is_empty() {
            page = page.child(div().text_size(units(12.)).child(text::TRY_OTHER_TERMS));
        } else if kind != Empty::NoMatch {
            page = page.child(div().text_size(units(12.)).text_center().child(detail));
            if kind == Empty::Disconnected {
                page = page.child(
                    div()
                        .text_size(units(12.))
                        .text_center()
                        .child(text::RECONNECT_HINT),
                );
            }
        }
        match kind {
            Empty::FirstUse => {
                let shortcut = cx
                    .global::<crate::app::hotkey::Shortcuts>()
                    .display()
                    .unwrap_or_default();
                page = page
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(units(6.))
                            .text_size(units(12.))
                            .child(text::SUMMON_ANYTIME)
                            .child(keycap(shortcut, cx)),
                    )
                    .child(hint("esc", text::CLOSE));
            }
            Empty::Locked => {
                page = page
                    .child(hint("⏎", text::UNLOCK))
                    .child(hint("esc", text::CLOSE));
            }
            Empty::Disconnected => {
                page = page
                    .child(hint("⏎", text::RECONNECT_NOW))
                    .child(hint(
                        if cfg!(target_os = "macos") {
                            "⌘L"
                        } else {
                            "Ctrl+L"
                        },
                        text::VIEW_LOGS,
                    ))
                    .child(hint("esc", text::CLOSE));
            }
            Empty::NoMatch if !visible.is_empty() => {
                let cursor = self.state.search.relax_cursor.min(visible.len() - 1);
                page = page
                    .child(div().text_size(units(12.)).child(text::TRY_RELAXING))
                    .child(
                        div().w_full().flex().flex_col().gap(units(2.)).children(
                            visible
                                .iter()
                                .enumerate()
                                .map(|(index, (relaxation, count))| {
                                    div()
                                        .h(units(30.))
                                        .px(units(10.))
                                        .rounded(units(7.))
                                        .flex()
                                        .items_center()
                                        .gap(units(8.))
                                        .text_size(units(13.))
                                        .text_color(theme.foreground)
                                        .when(index == cursor, |row| {
                                            row.bg(theme.primary.opacity(0.12))
                                        })
                                        .child(
                                            div()
                                                .w(units(14.))
                                                .text_color(muted)
                                                .child((index + 1).to_string()),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .child(relaxation.label.clone()),
                                        )
                                        .child(div().text_size(units(12.)).text_color(muted).child(
                                            count.map_or("…".to_string(), text::result_count),
                                        ))
                                }),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(units(14.))
                            .child(hint("⏎", text::APPLY_SUGGESTION))
                            .child(hint(
                                if cfg!(target_os = "macos") {
                                    "⌘⌫"
                                } else {
                                    "Ctrl+⌫"
                                },
                                text::CLEAR,
                            )),
                    );
            }
            Empty::NoMatch => {}
        }
        page.into_any_element()
    }
}
