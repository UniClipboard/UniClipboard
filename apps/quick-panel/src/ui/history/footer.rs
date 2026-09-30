//! The footer: what Enter does and how to reach the actions.

use super::*;

impl Panel {
    pub(super) fn footer(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let name = self.target.name();
        let initial = name
            .as_deref()
            .and_then(|name| name.chars().next())
            .map(|letter| letter.to_uppercase().to_string())
            .unwrap_or_default();
        div()
            .h(units(34.))
            .flex_shrink_0()
            .pl(units(12.))
            .pr(units(10.))
            .border_t_1()
            .border_color(theme.border.opacity(0.6))
            .flex()
            .items_center()
            .gap(units(8.))
            .text_size(units(12.))
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(units(7.))
                    .when(name.is_some(), |left| {
                        left.child(
                            div()
                                .size(units(16.))
                                .rounded(units(4.))
                                .bg(theme.foreground)
                                // The panel's own background token is transparent, so take the
                                // opaque card colour for the letter.
                                .text_color(
                                    cx.global::<crate::ui::appearance::Surfaces>().background,
                                )
                                .text_size(units(9.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(initial),
                        )
                    })
                    .child(div().text_color(theme.foreground).child(
                        if self.ctx_data(cx).ctx().capabilities.auto_paste {
                            text::paste_to(name.as_deref())
                        } else {
                            text::COPY.to_string()
                        },
                    ))
                    .child(keycap("⏎", cx)),
            )
            .child(div().flex_1())
            .child(div().child(text::ACTIONS))
            .child(keycap(
                if cfg!(target_os = "macos") {
                    "⌘K"
                } else {
                    "Ctrl+K"
                },
                cx,
            ))
            .into_any_element()
    }
}
