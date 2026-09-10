use super::view::units;
use super::*;
use gpui::{div, Animation, AnimationExt, AnyElement, MouseButton};
use gpui_component::ActiveTheme;

impl Panel {
    pub(super) fn open_filter_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.suggestions_open = false;
        self.expand_filter_row(window, cx);
    }

    fn expand_filter_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_generation += 1;
        self.filter_picker_open = true;
        self.filter_navigation = true;
        self.filter_picker_index = 0;
        self.filter_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn close_filter_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_generation = if self.filter_picker_open {
            self.filter_generation + 1
        } else {
            0
        };
        self.filter_picker_open = false;
        self.filter_navigation = false;
        self.suggestions_open = false;
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn remove_filter(
        &mut self,
        dimension: Dimension,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filters.remove(dimension, value);
        self.close_filter_picker(window, cx);
        self.hovered = None;
        self.keyboard = true;
        self.search(window, cx);
    }

    fn bar_options(&self, cx: &Context<Self>) -> Vec<filters::Suggestion> {
        let sources = self
            .members
            .iter()
            .map(|m| (m.peer_id.clone(), m.device_name.clone()))
            .collect::<Vec<_>>();
        let matches = filters::suggestions(&self.input.read(cx).value(), &self.tags, &sources);
        let mut values = self.filters.chips();
        if self.suggestions_open && !matches.is_empty() {
            for option in &matches {
                if !values.contains(&(option.dimension, option.value.clone())) {
                    values.push((option.dimension, option.value.clone()));
                }
            }
        } else {
            if values.is_empty() {
                values.push((Dimension::Type, "all".into()));
            }
            let available = ["text", "image", "file"]
                .into_iter()
                .map(|v| (Dimension::Type, v.to_string()))
                .chain([
                    (Dimension::Tag, "favorited".into()),
                    (Dimension::Type, "richtext".into()),
                ])
                .chain(self.tags.iter().cloned().map(|v| (Dimension::Tag, v)));
            for value in available {
                if !values.contains(&value) {
                    values.push(value);
                }
            }
        }
        values
            .into_iter()
            .map(|(dimension, value)| {
                let matched = matches
                    .iter()
                    .find(|m| m.dimension == dimension && m.value == value)
                    .and_then(|m| m.matched.clone());
                filters::Suggestion {
                    dimension,
                    value,
                    matched,
                }
            })
            .collect()
    }

    pub(super) fn filter_options(&self, cx: &Context<Self>) -> Vec<filters::Suggestion> {
        self.bar_options(cx)
            .into_iter()
            .filter(|option| {
                option.value != "all"
                    && (!self.suggestions_open
                        || !self.filters.contains(option.dimension, &option.value))
            })
            .collect()
    }

    fn choose_filter_option(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(option) = self.filter_options(cx).get(index).cloned() else {
            return;
        };
        if let Some(matched) = option.matched {
            self.filters.apply(option.dimension, Some(option.value));
            let remaining = filters::remaining_query(&self.input.read(cx).value(), &matched);
            self.filters.query = remaining.clone();
            self.filter_generation = if self.filter_picker_open {
                self.filter_generation + 1
            } else {
                0
            };
            self.filter_picker_open = false;
            self.filter_navigation = false;
            self.suggestions_open = false;
            self.input.update(cx, |input, cx| {
                input.set_value(remaining, window, cx);
                input.focus(window, cx);
            });
        } else {
            self.filters.toggle(option.dimension, option.value.clone());
            self.filter_picker_index = self
                .filter_options(cx)
                .iter()
                .position(|v| v.dimension == option.dimension && v.value == option.value)
                .unwrap_or(0);
            self.input.update(cx, |input, cx| input.focus(window, cx));
        }
        self.hovered = None;
        self.keyboard = true;
        self.search(window, cx);
    }

    pub(super) fn focus_filter_suggestion(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        let count = self.filter_options(cx).len();
        if (!self.suggestions_open && !self.filter_picker_open) || count == 0 {
            self.open_filter_picker(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.filter_navigation {
            self.filter_picker_index =
                (self.filter_picker_index + if reverse { count - 1 } else { 1 }) % count;
        } else {
            self.filter_navigation = true;
            self.filter_picker_index = if reverse { count - 1 } else { 0 };
        }
        self.reveal_filter_option(window, cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn handle_filter_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let count = if self.suggestions_open || self.filter_picker_open {
            self.filter_options(cx).len()
        } else {
            0
        };
        let key = event.keystroke.key.as_str();
        if self.filter_navigation {
            match key {
                "up" | "down" if count > 0 => {
                    self.filter_picker_index = (self.filter_picker_index
                        + if key == "up" { count - 1 } else { 1 })
                        % count;
                }
                "enter" => self.choose_filter_option(self.filter_picker_index, window, cx),
                _ => {
                    self.filter_navigation = false;
                    return false;
                }
            }
        } else {
            return false;
        }
        self.reveal_filter_option(window, cx);
        cx.stop_propagation();
        cx.notify();
        true
    }

    fn filter_layout(&self, window: &Window, cx: &Context<Self>) -> FilterLayout {
        let options = self.bar_options(cx);
        let scale = f32::from(window.rem_size()) / 16.;
        let measure = |label: &str| {
            let run = gpui::TextRun {
                len: label.len(),
                font: gpui::font(cx.theme().font_family.clone()),
                color: cx.theme().foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            (f32::from(
                window
                    .text_system()
                    .shape_line(
                        label.to_string().into(),
                        gpui::px(11. * scale),
                        &[run],
                        None,
                    )
                    .width,
            ) / scale
                + 16.)
                .ceil()
        };
        let widths: Vec<_> = options
            .iter()
            .map(|v| measure(&chip_label(v.dimension, &v.value)).min(280.))
            .collect();
        let shown = visible_chips(&widths, 334., |n| measure(&format!("+{n}")));
        let rows = wrapped_rows(&widths, shown, 334.);
        let counter_width = measure(&format!("+{}", options.len() - shown));
        FilterLayout {
            options,
            widths,
            rows,
            counter_width,
        }
    }

    fn reveal_filter_option(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(option) = self
            .filter_options(cx)
            .get(self.filter_picker_index)
            .cloned()
        else {
            return;
        };
        let layout = self.filter_layout(window, cx);
        let row = layout
            .rows
            .iter()
            .position(|row| {
                row.iter().any(|ix| {
                    let v = &layout.options[*ix];
                    v.dimension == option.dimension && v.value == option.value
                })
            })
            .unwrap_or(0);
        if row > 0 {
            if !self.filter_picker_open {
                self.filter_picker_open = true;
                self.filter_generation += 1;
            }
            self.filter_scroll.scroll_to_item(row - 1);
        }
    }

    pub(super) fn filter_bar(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let layout = self.filter_layout(window, cx);
        let keyboard_options = self.filter_options(cx);
        let focused = self
            .filter_navigation
            .then(|| keyboard_options.get(self.filter_picker_index))
            .flatten();
        let expanded = self.filter_picker_open;
        let expanded_height = 26. + layout.rows.len().saturating_sub(1).min(2) as f32 * 30.;
        let render_row = |indices: &[usize]| {
            div()
                .flex()
                .gap(units(4.))
                .h(units(26.))
                .flex_shrink_0()
                .children(indices.iter().map(|index| {
                    let option = &layout.options[*index];
                    let dimension = option.dimension;
                    let value = option.value.clone();
                    let active = if value == "all" {
                        self.filters.chips().is_empty()
                    } else {
                        self.filters.contains(dimension, &value)
                    };
                    let suggested = self.suggestions_open && option.matched.is_some() && !active;
                    let focused =
                        focused.is_some_and(|v| v.dimension == dimension && v.value == value);
                    div()
                        .id(("quick-filter", *index))
                        .w(units(layout.widths[*index]))
                        .h(units(26.))
                        .flex_shrink_0()
                        .rounded(units(6.))
                        .px(units(7.))
                        .py(units(3.))
                        .border_1()
                        .border_color(if focused {
                            theme.primary
                        } else if suggested {
                            theme.primary.opacity(0.25)
                        } else {
                            gpui::transparent_black()
                        })
                        .text_size(units(11.))
                        .line_height(units(18.))
                        .cursor_pointer()
                        .bg(if active {
                            theme.primary
                        } else if suggested {
                            theme.accent
                        } else {
                            theme.muted.opacity(0.6)
                        })
                        .text_color(if active {
                            theme.primary_foreground
                        } else {
                            theme.muted_foreground
                        })
                        .child(div().truncate().child(chip_label(dimension, &value)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.busy {
                                return;
                            }
                            if value == "all" {
                                this.filters = Filters {
                                    query: this.filters.query.clone(),
                                    ..Default::default()
                                };
                                this.search(window, cx);
                            } else if let Some(index) = this
                                .filter_options(cx)
                                .iter()
                                .position(|v| v.dimension == dimension && v.value == value)
                            {
                                this.choose_filter_option(index, window, cx);
                            } else {
                                this.remove_filter(dimension, &value, window, cx);
                            }
                            this.input.update(cx, |input, cx| input.focus(window, cx));
                        }))
                }))
        };
        let mut surface = div()
            .id("filter-surface")
            .occlude()
            .absolute()
            .top_0()
            .left(units(12.))
            .right(units(12.))
            .flex()
            .flex_col()
            .gap(units(4.))
            .rounded(units(6.))
            .overflow_hidden()
            .bg(cx.global::<crate::appearance::Surfaces>().background)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(expanded, |v| {
                v.on_mouse_down_out(
                    cx.listener(|this, _, window, cx| this.close_filter_picker(window, cx)),
                )
            })
            .child(render_row(&layout.rows[0]));
        if layout.rows.len() > 1 {
            surface = surface
                .child(
                    div()
                        .id("filter-options")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.filter_scroll)
                        .flex()
                        .flex_col()
                        .gap(units(4.))
                        .children(layout.rows[1..].iter().map(|row| render_row(row))),
                )
                .child(
                    div()
                        .id("filter-overflow")
                        .absolute()
                        .top_0()
                        .right_0()
                        .w(units(layout.counter_width))
                        .h(units(26.))
                        .px(units(8.))
                        .py(units(4.))
                        .text_size(units(11.))
                        .line_height(units(18.))
                        .cursor_pointer()
                        .text_color(theme.muted_foreground)
                        .bg(cx.global::<crate::appearance::Surfaces>().background)
                        .child(if expanded {
                            "⌃".to_string()
                        } else {
                            format!("+{}", layout.options.len() - layout.rows[0].len())
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.filter_picker_open {
                                this.close_filter_picker(window, cx);
                            } else {
                                this.expand_filter_row(window, cx);
                            }
                        })),
                );
        }
        let surface = if self.filter_generation == 0 {
            surface.h(units(26.)).into_any_element()
        } else {
            surface
                .with_animation(
                    ("filter-expansion", self.filter_generation),
                    Animation::new(Duration::from_millis(160)),
                    move |surface, progress| {
                        let eased = 1. - (1. - progress).powi(3);
                        let height = if expanded {
                            26. + (expanded_height - 26.) * eased
                        } else {
                            expanded_height - (expanded_height - 26.) * eased
                        };
                        surface.h(units(height))
                    },
                )
                .into_any_element()
        };
        div()
            .relative()
            .w_full()
            .h(units(34.))
            .flex_shrink_0()
            .child(gpui::deferred(surface).with_priority(1))
            .into_any_element()
    }
}

struct FilterLayout {
    options: Vec<filters::Suggestion>,
    widths: Vec<f32>,
    rows: Vec<Vec<usize>>,
    counter_width: f32,
}

fn wrapped_rows(widths: &[f32], first: usize, available: f32) -> Vec<Vec<usize>> {
    let mut rows = vec![(0..first).collect()];
    let mut row = Vec::new();
    let mut used = 0.;
    for (index, width) in widths.iter().enumerate().skip(first) {
        if !row.is_empty() && used + width > available {
            rows.push(std::mem::take(&mut row));
            used = 0.;
        }
        row.push(index);
        used += width + 4.;
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

fn chip_label(dimension: Dimension, value: &str) -> String {
    let label = filters::label(value).replace(['\n', '\r'], " ");
    match dimension {
        Dimension::Tag => format!("#{label}"),
        Dimension::Source => format!("来源:{label}"),
        Dimension::Time => format!("时间:{label}"),
        Dimension::Extension => format!(".{label}"),
        Dimension::Type => label.to_string(),
    }
}

fn visible_chips(widths: &[f32], available: f32, overflow_width: impl Fn(usize) -> f32) -> usize {
    let total = widths.iter().sum::<f32>() + widths.len().saturating_sub(1) as f32 * 4.;
    if total <= available {
        return widths.len();
    }
    let mut used = 0.;
    let mut shown = 0;
    for (index, width) in widths.iter().enumerate() {
        used += width + 4.;
        if used + overflow_width(widths.len() - index - 1) <= available {
            shown = index + 1;
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expansion_keeps_the_first_row_and_wraps_overflow_without_losing_options() {
        let rows = wrapped_rows(&[96.; 12], 3, 334.);
        assert_eq!(
            rows,
            vec![vec![0, 1, 2], vec![3, 4, 5], vec![6, 7, 8], vec![9, 10, 11]]
        );
        assert_eq!(wrapped_rows(&[50.; 2], 2, 334.), vec![vec![0, 1]]);
    }
    #[test]
    fn full_row_does_not_reserve_an_unneeded_picker_button() {
        assert_eq!(visible_chips(&[50.; 6], 334., |_| 30.), 6);
        assert_eq!(visible_chips(&[50., 50.], 104., |_| 30.), 2);
        assert_eq!(visible_chips(&[], 334., |_| 30.), 0);
    }
    #[test]
    fn overflow_reserves_only_the_counter_width_and_counts_hidden_options() {
        assert_eq!(visible_chips(&[50.; 7], 334., |_| 30.), 5);
        assert_eq!(visible_chips(&[96.; 6], 334., |_| 30.), 3);
        assert_eq!(visible_chips(&[50., 50.], 103., |_| 30.), 1);
        assert_eq!(visible_chips(&[280., 50.], 334., |_| 30.), 2);
        assert_eq!(visible_chips(&[280., 60.], 334., |_| 30.), 1);
    }
}
