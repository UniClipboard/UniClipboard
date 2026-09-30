//! The history window: search row, suggestions, nine fixed rows and the footer.

use super::view::units;
use super::*;
use crate::strings;
use gpui::{div, img, AnyElement, App, Hsla, IntoElement, MouseButton, ObjectFit};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::Input,
    ActiveTheme, Icon, IconName, Sizable,
};

/// Rows the list shows at once. Digits 1 to 9 on the rows are the Command+digit shortcuts.
pub(super) const VISIBLE_ROWS: usize = 9;
/// Suggestions listed at most; more are folded into "还有 N 条".
const MAX_SUGGESTIONS: usize = 3;

use crate::content::{split_link, Kind as RowKind};

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

impl Panel {
    /// Text of a filter chip: `#tag`, `@device`, or the label of a type or time range.
    fn chip_label(&self, dimension: Dimension, value: &str) -> String {
        match dimension {
            Dimension::Tag => format!("#{}", strings::value_label(value)),
            Dimension::Source => {
                let name = self
                    .members
                    .iter()
                    .find(|member| member.peer_id == value)
                    .map_or(value, |member| member.device_name.as_str());
                format!("@{name}")
            }
            Dimension::Type => strings::value_label(value).to_string(),
            Dimension::Time => value.to_string(),
        }
    }

    pub(super) fn remove_filter(
        &mut self,
        dimension: Dimension,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filters.remove(dimension, value);
        self.suggestions_open = false;
        self.input.update(cx, |input, cx| input.focus(window, cx));
        self.hovered = None;
        self.keyboard = true;
        self.search(window, cx);
    }

    /// Suggestions for the words typed so far. They are candidates only: nothing is filtered
    /// until one is accepted.
    pub(super) fn suggestion_options(&self, cx: &Context<Self>) -> Vec<filters::Suggestion> {
        if !self.suggestions_open {
            return vec![];
        }
        let sources = self
            .members
            .iter()
            .map(|member| (member.peer_id.clone(), member.device_name.clone()))
            .collect::<Vec<_>>();
        let catalog = filters::Catalog {
            tags: &self.tags,
            sources: &sources,
            today: chrono::Local::now().date_naive(),
            initials: crate::language::is_chinese(
                self.general.as_ref().and_then(|g| g.language.as_deref()),
            ),
        };
        filters::suggestions(&self.input.read(cx).value(), &catalog)
            .into_iter()
            .filter(|option| !self.filters.contains(option.dimension, &option.value))
            .collect()
    }

    /// Turns the suggested words into a filter and keeps the other words as the query.
    pub(super) fn accept_suggestion(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.busy {
            return false;
        }
        let options = self.suggestion_options(cx);
        let Some(option) = options.get(self.suggestion_cursor.min(options.len().saturating_sub(1)))
        else {
            return false;
        };
        self.filters.accept(option);
        let remaining = filters::remaining_query(&self.input.read(cx).value(), &option.matched);
        self.filters.query = remaining.clone();
        self.suggestion_cursor = 0;
        self.input.update(cx, |input, cx| {
            input.set_value(remaining, window, cx);
            input.focus(window, cx);
        });
        self.hovered = None;
        self.keyboard = true;
        self.search(window, cx);
        true
    }

    /// Up and Down while a filter word is being typed: move the suggestion that Tab would accept.
    /// Returns whether the key was taken; otherwise the arrows move through the results.
    pub(super) fn arrow_in_suggestions(&mut self, down: bool, cx: &mut Context<Self>) -> bool {
        if !filters::typing_a_filter(&self.input.read(cx).value()) {
            return false;
        }
        let count = self.suggestion_options(cx).len();
        if count < 2 {
            return false;
        }
        let cursor = self.suggestion_cursor.min(count - 1);
        self.suggestion_cursor = if down {
            (cursor + 1) % count
        } else {
            (cursor + count - 1) % count
        };
        cx.notify();
        true
    }

    /// Tab: accept the suggestion at the cursor; without suggestions, cycle the type filter.
    pub(super) fn tab(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        if !reverse && self.accept_suggestion(window, cx) {
            return;
        }
        self.cycle_type(reverse, window, cx);
    }

    fn cycle_type(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.filters.cycle_type(reverse);
        self.suggestions_open = false;
        self.hovered = None;
        self.keyboard = true;
        self.input.update(cx, |input, cx| input.focus(window, cx));
        self.search(window, cx);
    }

    fn search_row(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let accent = theme.primary;
        // One type is shown at the right end of the row, where Tab changes it; several are chips.
        let type_chips = self.filters.types.len() > 1;
        let chips = self
            .filters
            .chips()
            .into_iter()
            .filter(|(dimension, _)| type_chips || *dimension != Dimension::Type)
            .collect::<Vec<_>>();
        let typed = !self.input.read(cx).value().is_empty();
        let active_type = self.filters.single_type().is_some();
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
                        .disabled(self.busy)
                        .px_0()
                        .text_size(units(15.)),
                ),
            )
            .when(typed || !self.filters.chips().is_empty(), |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_size(units(12.))
                        .text_color(muted)
                        .child(strings::result_count(self.total)),
                )
            })
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
                                self.filters
                                    .single_type()
                                    .map_or(strings::ALL_TYPES, strings::value_label)
                                    .to_string(),
                            ),
                    )
                    .child(keycap("⇥", cx)),
            )
            .when(typed || !self.filters.chips().is_empty(), |row| {
                row.child(
                    Button::new("clear")
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                )
            })
            .into_any_element()
    }

    /// Candidate filters for the words typed. Tab accepts the highlighted one.
    fn suggestion_strip(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let options = self.suggestion_options(cx);
        if options.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let accent = theme.primary;
        let query = self.input.read(cx).value().to_string();
        let cursor = self.suggestion_cursor.min(options.len() - 1);
        let start = filters::suggestion_window_start(cursor, MAX_SUGGESTIONS);
        let hidden = options.len().saturating_sub(start + MAX_SUGGESTIONS);
        let mut rows = options
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
                    .h(units(28.))
                    .flex_shrink_0()
                    .px(units(10.))
                    .rounded(units(7.))
                    .flex()
                    .items_center()
                    .gap(units(8.))
                    .text_size(units(13.))
                    .when(highlighted, |row| row.bg(accent.opacity(0.12)))
                    .child(div().child(word).text_color(muted))
                    .child(div().text_color(muted).child("→"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(value),
                    )
                    .when(highlighted, |row| row.child(keycap("⇥", cx)))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        if hidden > 0 {
            rows.push(
                div()
                    .h(units(20.))
                    .px(units(10.))
                    .text_size(units(11.))
                    .text_color(muted)
                    .child(format!("还有 {hidden} 条"))
                    .into_any_element(),
            );
        }
        Some(
            div()
                .flex_shrink_0()
                .px(units(6.))
                .pt(units(6.))
                .pb(units(4.))
                .border_b_1()
                .border_color(theme.border.opacity(0.5))
                .child(
                    div()
                        .px(units(10.))
                        .pb(units(2.))
                        .flex()
                        .items_center()
                        .gap(units(6.))
                        .text_size(units(11.))
                        .text_color(muted)
                        .child(strings::SUGGESTIONS)
                        .child(strings::ACCEPT_IN_ORDER),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    /// What the list area shows when there is nothing to list: first use, locked, disconnected, or
    /// no match with ways to loosen the search.
    fn empty_page(&self, cx: &Context<Self>) -> AnyElement {
        use crate::states::{classify, Empty};
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let searching = !self.filters.query.trim().is_empty() || !self.filters.chips().is_empty();
        let kind = classify(self.locked, self.disconnected.is_some(), searching);
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
                strings::FIRST_USE_TITLE.to_string(),
                strings::FIRST_USE_HINT.to_string(),
            ),
            Empty::Locked => (
                IconName::Asterisk,
                strings::LOCKED_TITLE.to_string(),
                strings::LOCKED_HINT_GUI.to_string(),
            ),
            Empty::Disconnected => (
                IconName::TriangleAlert,
                strings::DISCONNECTED_TITLE.to_string(),
                strings::reconnecting(self.disconnected.unwrap_or(1)),
            ),
            Empty::NoMatch => (
                IconName::Search,
                strings::no_match(&self.filters.query),
                String::new(),
            ),
        };
        let visible = self.visible_relaxations();
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
            page = page.child(div().text_size(units(12.)).child(strings::TRY_OTHER_TERMS));
        } else if kind != Empty::NoMatch {
            page = page.child(div().text_size(units(12.)).text_center().child(detail));
            if kind == Empty::Disconnected {
                page = page.child(
                    div()
                        .text_size(units(12.))
                        .text_center()
                        .child(strings::RECONNECT_HINT),
                );
            }
        }
        match kind {
            Empty::FirstUse => {
                let shortcut = cx
                    .global::<crate::shortcuts::Shortcuts>()
                    .display()
                    .unwrap_or_default();
                page = page
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(units(6.))
                            .text_size(units(12.))
                            .child(strings::SUMMON_ANYTIME)
                            .child(keycap(shortcut, cx)),
                    )
                    .child(hint("esc", strings::CLOSE));
            }
            Empty::Locked => {
                page = page
                    .child(hint("⏎", strings::UNLOCK))
                    .child(hint("esc", strings::CLOSE));
            }
            Empty::Disconnected => {
                page = page
                    .child(hint("⏎", strings::RECONNECT_NOW))
                    .child(hint(
                        if cfg!(target_os = "macos") {
                            "⌘L"
                        } else {
                            "Ctrl+L"
                        },
                        strings::VIEW_LOGS,
                    ))
                    .child(hint("esc", strings::CLOSE));
            }
            Empty::NoMatch if !visible.is_empty() => {
                let cursor = self.relax_cursor.min(visible.len() - 1);
                page = page
                    .child(div().text_size(units(12.)).child(strings::TRY_RELAXING))
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
                                            count.map_or("…".to_string(), strings::result_count),
                                        ))
                                }),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(units(14.))
                            .child(hint("⏎", strings::APPLY_SUGGESTION))
                            .child(hint(
                                if cfg!(target_os = "macos") {
                                    "⌘⌫"
                                } else {
                                    "Ctrl+⌫"
                                },
                                strings::CLEAR,
                            )),
                    );
            }
            Empty::NoMatch => {}
        }
        page.into_any_element()
    }

    fn footer(&self, cx: &Context<Self>) -> AnyElement {
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
                                .text_color(cx.global::<crate::appearance::Surfaces>().background)
                                .text_size(units(9.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(initial),
                        )
                    })
                    .child(
                        div()
                            .text_color(theme.foreground)
                            .child(strings::paste_to(name.as_deref())),
                    )
                    .child(keycap("⏎", cx)),
            )
            .child(div().flex_1())
            .child(div().child(strings::ACTIONS))
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

    fn secondary_chip(&self, label: String, selected: bool, cx: &Context<Self>) -> AnyElement {
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

    fn row(&self, ix: usize, cx: &Context<Self>) -> AnyElement {
        let item = &self.items[ix];
        let kind = RowKind::of(item);
        let selected = self.selection.selected() == Some(ix);
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
            .unwrap_or_else(|| strings::value_label(&item.content_type).into())
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
        let link = (kind == RowKind::Link).then(|| {
            let (host, path) = split_link(&text);
            div()
                .flex()
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(host.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(quiet)
                        .child(path.to_string()),
                )
        });
        let secondary = match kind {
            RowKind::RichText => Some(strings::RICH_TEXT.to_string()),
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
            .when(!selected && !self.keyboard, |row| {
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
                    .when(kind != RowKind::Link, |cell| cell.child(text.clone())),
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
                    .child(strings::relative_time(elapsed)),
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
                if *hovered && !this.keyboard && this.pointer_moved && !this.loading {
                    this.hovered = Some(ix);
                    this.schedule_preview(window, cx);
                    cx.notify();
                }
            }));
        div().w_full().flex_shrink_0().child(row).into_any_element()
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
                        .child(strings::SEARCHING),
                )
            })
            .when(!self.loading && self.items.is_empty(), |list| {
                list.child(self.empty_page(cx))
            })
            .when(!self.loading && !self.filters.images_only(), |list| {
                list.children((0..self.items.len()).map(|ix| self.row(ix, cx)))
            })
            .when(!self.loading && self.filters.images_only(), |list| {
                list.child(self.image_wall(cx))
            });
        let mut card = div()
            .relative()
            .w(units(crate::window_pair::PANEL_WIDTH as f32))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .rounded(units(12.))
            .border_1()
            .border_color(theme.border.opacity(0.6))
            .bg(cx.global::<crate::appearance::Surfaces>().background)
            .text_color(theme.foreground)
            .overflow_hidden()
            .child(self.search_row(cx))
            .when_some(self.suggestion_strip(cx), |card, strip| card.child(strip))
            .child(list);
        if let Some(message) = self.message.as_ref().filter(|_| !self.locked) {
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
                            .label("重试")
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| this.search(window, cx))),
                    ),
            );
        }
        card.child(self.footer(cx)).into_any_element()
    }
}
