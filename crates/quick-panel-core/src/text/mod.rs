//! User-visible text of the panel, in every language the main window ships.
//!
//! Each language is one [`Text`] value in its own file, so a missing string is a compile error
//! rather than a silent fallback. [`t`] returns the text of the current language (see
//! `crate::language`); templates name their arguments in braces, such as `{n}`.

use crate::language::Language;

mod en_us;
mod ja_jp;
mod pt_br;
mod ru_ru;
mod zh_cn;
mod zh_tw;

/// A phrase with a count in it, in the CLDR plural forms the shipped languages use.
pub struct Count {
    /// English and Portuguese singular, Russian `one` (1, 21, 31…).
    pub one: &'static str,
    /// Russian `few` (2–4, 22–24…). Other languages repeat `other`.
    pub few: &'static str,
    /// Everything else, and the only form Chinese and Japanese have.
    pub other: &'static str,
}

/// Words and patterns for the time-range filter chips.
pub struct Dates {
    pub today: &'static str,
    pub yesterday: &'static str,
    pub day_before_yesterday: &'static str,
    pub this_week: &'static str,
    pub last_week: &'static str,
    pub this_month: &'static str,
    pub last_month: &'static str,
    pub last_days: Count,
    /// Month names for `{month}`, January first.
    pub months: [&'static str; 12],
    /// A day of the current year. Patterns may use `{y}`, `{m}`, `{mm}`, `{d}`, `{dd}`, `{month}`.
    pub day: &'static str,
    /// A day of another year.
    pub day_with_year: &'static str,
    /// The last day of a range that starts in the same month.
    pub end_day_same_month: &'static str,
    /// A whole month of the current year.
    pub month: &'static str,
    /// A whole month of another year.
    pub month_with_year: &'static str,
    /// From `{date}` up to today.
    pub since: &'static str,
    /// From the earliest entry up to and including `{date}`.
    pub until: &'static str,
}

/// Messages for failures of platform operations, see `ports::PlatformError`.
pub struct Platform {
    pub unsupported: &'static str,
    pub panel_window_inaccessible: &'static str,
    pub preview_window_inaccessible: &'static str,
    pub unsupported_window_kind: &'static str,
    pub panel_window_closed: &'static str,
    pub preview_window_closed: &'static str,
    pub preview_layer_not_ready: &'static str,
    pub main_thread_required: &'static str,
    pub no_display: &'static str,
    pub invalid_link: &'static str,
    pub cannot_open: &'static str,
    pub no_paste_permission: &'static str,
    pub no_paste_target: &'static str,
    pub paste_target_quit: &'static str,
    pub focus_moved: &'static str,
    pub cannot_return_to_target: &'static str,
    pub paste_target_missing: &'static str,
    pub cannot_create_typing_event: &'static str,
    pub cannot_create_paste_event: &'static str,
}

/// Messages for failures of history operations, see `ports::history`.
pub struct Service {
    pub locked: &'static str,
    pub search_failed: &'static str,
    pub disconnected: &'static str,
    pub interrupted: &'static str,
    pub search_timeout: &'static str,
    pub not_running: &'static str,
    pub cannot_connect: &'static str,
    pub restore_failed: &'static str,
    pub restore_timeout: &'static str,
    pub action_failed: &'static str,
    pub tags_unavailable: &'static str,
    pub devices_unavailable: &'static str,
    pub preview_timeout: &'static str,
    pub preview_unreadable: &'static str,
    pub entry_gone: &'static str,
    pub image_unreadable: &'static str,
    pub image_gone: &'static str,
    pub image_bad_format: &'static str,
    pub image_unsupported: &'static str,
    pub image_undecodable: &'static str,
    pub copy_failed: &'static str,
    pub no_paths: &'static str,
    pub preview_unreadable_retry: &'static str,
}

/// All text of the panel in one language.
pub struct Text {
    pub search_placeholder: &'static str,
    pub actions: &'static str,
    pub send_to: &'static str,
    pub actions_hint: &'static str,
    pub nothing_to_open: &'static str,
    pub no_log_dir: &'static str,
    pub first_use_title: &'static str,
    pub first_use_hint: &'static str,
    pub summon_anytime: &'static str,
    pub locked_title: &'static str,
    pub unlock: &'static str,
    pub disconnected_title: &'static str,
    pub reconnect_hint: &'static str,
    pub reconnect_now: &'static str,
    pub view_logs: &'static str,
    pub close: &'static str,
    pub try_relaxing: &'static str,
    pub apply_suggestion: &'static str,
    pub clear: &'static str,
    pub needs_app: &'static str,
    pub host_gone: &'static str,
    pub locked_hint_gui: &'static str,
    pub open_main_window: &'static str,
    pub settings: &'static str,
    pub suggestions: &'static str,
    pub accept_in_order: &'static str,
    pub press_again: &'static str,
    pub all_types: &'static str,
    pub try_other_terms: &'static str,
    pub searching: &'static str,
    pub just_now: &'static str,
    pub loading: &'static str,
    pub loading_image: &'static str,
    pub retry: &'static str,
    pub fit_to_window: &'static str,
    pub actual_size: &'static str,
    pub delete_hint_mac: &'static str,
    pub delete_hint_other: &'static str,
    pub shortcut_taken: &'static str,
    pub preview_window_failed: &'static str,
    /// What Enter does on a platform that cannot paste by itself.
    pub copy: &'static str,
    /// `{n}` filter chips that do not fit.
    pub hidden_count: &'static str,
    /// `{n}` is the number of the attempt.
    pub reconnecting: &'static str,
    pub no_match: &'static str,
    /// `{query}` is what was typed.
    pub no_match_query: &'static str,
    pub paste: &'static str,
    /// `{name}` is the application.
    pub paste_to: &'static str,
    pub result_count: Count,
    pub text_match_count: Count,
    pub text_matches: &'static str,
    // Names of the filter dimensions.
    pub dimension_type: &'static str,
    pub dimension_tag: &'static str,
    pub dimension_source: &'static str,
    pub dimension_time: &'static str,
    // Content types and built-in tags.
    pub kind_text: &'static str,
    pub kind_rich_text: &'static str,
    pub kind_image: &'static str,
    pub kind_file: &'static str,
    pub kind_link: &'static str,
    pub kind_code: &'static str,
    pub kind_directory: &'static str,
    pub tag_favorited: &'static str,
    // Entry actions.
    pub paste_plain: &'static str,
    pub paste_keep_open: &'static str,
    pub copy_only: &'static str,
    pub paste_paths: &'static str,
    pub open: &'static str,
    pub reveal_file: &'static str,
    pub send_to_device: &'static str,
    pub favorite: &'static str,
    pub unfavorite: &'static str,
    pub delete: &'static str,
    pub all_devices: &'static str,
    // Preview header facts.
    pub lines: Count,
    pub characters: Count,
    pub items: Count,
    /// `{name}` is the device.
    pub from_device: &'static str,
    // Ways to loosen a search that found nothing. `{value}` is a type or tag, `{name}` a device.
    pub remove_type: &'static str,
    pub remove_all_types: &'static str,
    pub remove_tag: &'static str,
    pub remove_all_tags: &'static str,
    pub widen_time: &'static str,
    pub remove_device: &'static str,
    pub include_all_devices: &'static str,
    pub dates: Dates,
    pub platform: Platform,
    pub service: Service,
}

/// The text of the current language.
pub fn t() -> &'static Text {
    of(Language::current())
}

/// The text of a language.
pub fn of(language: Language) -> &'static Text {
    match language {
        Language::ZhCn => &zh_cn::TEXT,
        Language::ZhTw => &zh_tw::TEXT,
        Language::EnUs => &en_us::TEXT,
        Language::JaJp => &ja_jp::TEXT,
        Language::RuRu => &ru_ru::TEXT,
        Language::PtBr => &pt_br::TEXT,
    }
}

/// Puts the arguments into a template, e.g. `fill("{n} more", &[("n", "3")])`.
pub fn fill(template: &str, arguments: &[(&str, &str)]) -> String {
    arguments
        .iter()
        .fold(template.to_string(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

impl Count {
    /// A phrase that does not change with the count, as in Chinese and Japanese.
    pub const fn same(form: &'static str) -> Self {
        Self {
            one: form,
            few: form,
            other: form,
        }
    }

    /// The phrase for `n`, in the plural form the current language uses for it.
    pub fn of(&self, n: i64) -> String {
        let n_abs = n.unsigned_abs();
        let form = match Language::current() {
            Language::ZhCn | Language::ZhTw | Language::JaJp => self.other,
            Language::EnUs => {
                if n_abs == 1 {
                    self.one
                } else {
                    self.other
                }
            }
            // CLDR `pt`: integers 0 and 1 are singular.
            Language::PtBr => {
                if n_abs <= 1 {
                    self.one
                } else {
                    self.other
                }
            }
            Language::RuRu => match (n_abs % 10, n_abs % 100) {
                (1, rest) if rest != 11 => self.one,
                (2..=4, rest) if !(12..=14).contains(&rest) => self.few,
                _ => self.other,
            },
        };
        fill(form, &[("n", &n.to_string())])
    }
}

/// Row under the visible filter chips when some do not fit.
pub fn hidden_count(hidden: usize) -> String {
    fill(t().hidden_count, &[("n", &hidden.to_string())])
}

/// Display name of a content type or a built-in tag; anything else (a custom tag) is shown as is.
pub fn value_label(value: &str) -> &str {
    let t = t();
    match value {
        "text" => t.kind_text,
        "richtext" => t.kind_rich_text,
        "image" => t.kind_image,
        "file" => t.kind_file,
        "link" => t.kind_link,
        "code" => t.kind_code,
        "favorited" => t.tag_favorited,
        "directory" => t.kind_directory,
        _ => value,
    }
}

/// Explains the disconnected page: how many times the panel has tried to reach the daemon.
pub fn reconnecting(attempt: u32) -> String {
    fill(t().reconnecting, &[("n", &attempt.to_string())])
}

/// Says what found nothing, with the typed words when there are any.
pub fn no_match(query: &str) -> String {
    if query.trim().is_empty() {
        t().no_match.to_string()
    } else {
        fill(t().no_match_query, &[("query", query.trim())])
    }
}

/// Footer text naming where the selected entry will be pasted.
pub fn paste_to(application: Option<&str>) -> String {
    match application {
        Some(name) => fill(t().paste_to, &[("name", name)]),
        None => t().paste.to_string(),
    }
}

/// Number of results shown at the right end of the search row.
pub fn result_count(total: u32) -> String {
    t().result_count.of(i64::from(total))
}

/// Count shown while suggestions are pending: the results are text matches only.
pub fn text_match_count(total: u32) -> String {
    t().text_match_count.of(i64::from(total))
}

/// Heading of the text matches under the suggestions; the words that stayed plain text follow it.
pub fn text_matches_heading(words: &str) -> String {
    if words.is_empty() {
        t().text_matches.to_string()
    } else {
        format!("{} · {words}", t().text_matches)
    }
}

/// Name of a filter dimension in a suggestion row.
pub fn dimension_label(dimension: crate::query::filters::Dimension) -> &'static str {
    use crate::query::filters::Dimension;
    let t = t();
    match dimension {
        Dimension::Type => t.dimension_type,
        Dimension::Tag => t.dimension_tag,
        Dimension::Source => t.dimension_source,
        Dimension::Time => t.dimension_time,
    }
}

/// Short age of an entry: "just now" in the current language, "5m", "3h" or "2d".
pub fn relative_time(elapsed_ms: i64) -> String {
    let minutes = (elapsed_ms as f64 / 60_000.).round() as i64;
    if minutes < 1 {
        t().just_now.to_string()
    } else if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 1440 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", minutes / 1440)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_are_shown_in_the_largest_whole_unit() {
        assert_eq!(relative_time(20_000), t().just_now);
        assert_eq!(relative_time(-5_000), t().just_now);
        assert_eq!(relative_time(5 * 60_000), "5m");
        assert_eq!(relative_time(59 * 60_000), "59m");
        assert_eq!(relative_time(3 * 3_600_000), "3h");
        assert_eq!(relative_time(2 * 86_400_000), "2d");
    }

    #[test]
    fn the_footer_names_the_target_or_says_only_paste() {
        assert_eq!(paste_to(Some("Terminal")), "粘贴到 Terminal");
        assert_eq!(paste_to(None), "粘贴");
    }
}
