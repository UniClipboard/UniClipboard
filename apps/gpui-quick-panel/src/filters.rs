//! The search filters: what the user asked for, how the search box words suggest them, and how
//! they become a daemon request.
//!
//! Within one dimension the values are alternatives (OR); different dimensions all have to hold
//! (AND). Time has a single value, and a new one replaces it.

use chrono::{Local, NaiveDate};
use uc_daemon_client::SearchQueryRequest;

use crate::date_range::{self, DateRange};
use crate::strings::value_label;

/// Content types the panel offers, in the order Tab cycles them.
pub const TYPES: [&str; 4] = ["text", "richtext", "image", "file"];
pub const BUILTIN_TAGS: [&str; 5] = ["link", "code", "favorited", "image", "directory"];

/// Words typed as pinyin initials (Chinese interface only) and what they stand for. A time word
/// is read by the date parser.
const INITIALS: [(&str, Dimension, &str); 14] = [
    ("wb", Dimension::Type, "text"),
    ("fwb", Dimension::Type, "richtext"),
    ("tp", Dimension::Type, "image"),
    ("wj", Dimension::Type, "file"),
    ("lj", Dimension::Tag, "link"),
    ("dm", Dimension::Tag, "code"),
    ("sc", Dimension::Tag, "favorited"),
    ("wjj", Dimension::Tag, "directory"),
    ("jt", Dimension::Time, "今天"),
    ("zt", Dimension::Time, "昨天"),
    ("bz", Dimension::Time, "本周"),
    ("sz", Dimension::Time, "上周"),
    ("by", Dimension::Time, "本月"),
    ("sy", Dimension::Time, "上月"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dimension {
    Type,
    Tag,
    Source,
    Time,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    pub query: String,
    pub types: Vec<String>,
    pub tags: Vec<String>,
    /// Ids of the devices the entries came from.
    pub sources: Vec<String>,
    pub time: Option<DateRange>,
}

/// A prefixed word: `#work`, `@phone` or `/image`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Token {
    pub dimension: Dimension,
    pub partial: String,
}

pub fn parse_token(value: &str) -> Option<Token> {
    let value = value.trim_start();
    let mut chars = value.chars();
    let dimension = match chars.next()? {
        '#' => Dimension::Tag,
        '@' => Dimension::Source,
        '/' => Dimension::Type,
        _ => return None,
    };
    Some(Token {
        dimension,
        partial: chars.as_str().trim().into(),
    })
}

/// The name the daemon knows a content type by.
fn daemon_type(value: &str) -> &str {
    match value {
        "richtext" => "html",
        other => other,
    }
}

/// The text to search for. Words made only of punctuation (a lone `#`, `@` or `/` being typed as a
/// filter prefix, say) hold nothing the search index can match, and the daemon answers such a query
/// with an error, so they are sent as an empty text: a filter-only search.
fn searchable_text(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.chars().any(char::is_alphanumeric) {
        trimmed.into()
    } else {
        String::new()
    }
}

impl Filters {
    pub fn request(&self) -> SearchQueryRequest {
        self.request_on(Local::now().date_naive())
    }

    /// The daemon request as of `today`, which an open-ended time range runs up to.
    pub fn request_on(&self, today: NaiveDate) -> SearchQueryRequest {
        // The daemon takes both ends of a time range or neither.
        let (from_ms, to_ms) = self
            .time
            .as_ref()
            .map(|range| range.bounds_ms(today))
            .unzip();
        SearchQueryRequest {
            query: searchable_text(&self.query),
            operator: None,
            time_preset: None,
            from_ms,
            to_ms,
            content_types: self.types.iter().map(|t| daemon_type(t).into()).collect(),
            tags: self.tags.clone(),
            extensions: vec![],
            source_devices: self.sources.clone(),
            limit: 50,
            offset: 0,
        }
    }

    fn values_mut(&mut self, dimension: Dimension) -> Option<&mut Vec<String>> {
        match dimension {
            Dimension::Type => Some(&mut self.types),
            Dimension::Tag => Some(&mut self.tags),
            Dimension::Source => Some(&mut self.sources),
            Dimension::Time => None,
        }
    }

    /// Adds an alternative to the dimension. Time is set with [`Filters::set_time`].
    pub fn add(&mut self, dimension: Dimension, value: String) {
        if let Some(values) = self.values_mut(dimension) {
            if !values.contains(&value) {
                values.push(value);
            }
        }
    }

    /// Makes `value` the only one of its dimension.
    pub fn replace(&mut self, dimension: Dimension, value: String) {
        self.clear(dimension);
        self.add(dimension, value);
    }

    pub fn set_time(&mut self, range: DateRange) {
        self.time = Some(range);
    }

    pub fn clear(&mut self, dimension: Dimension) {
        match self.values_mut(dimension) {
            Some(values) => values.clear(),
            None => self.time = None,
        }
    }

    pub fn remove(&mut self, dimension: Dimension, value: &str) {
        match self.values_mut(dimension) {
            Some(values) => values.retain(|v| v != value),
            None => self.time = None,
        }
    }

    /// Applies an accepted suggestion. Words recognised without a prefix replace the dimension's
    /// value, so typing `jt` after `zt` gives today rather than both.
    pub fn accept(&mut self, suggestion: &Suggestion) {
        match &suggestion.time {
            Some(range) => self.set_time(range.clone()),
            None if suggestion.replace => {
                self.replace(suggestion.dimension, suggestion.value.clone())
            }
            None => self.add(suggestion.dimension, suggestion.value.clone()),
        }
    }

    pub fn contains(&self, dimension: Dimension, value: &str) -> bool {
        self.chips()
            .iter()
            .any(|(d, v)| *d == dimension && v == value)
    }

    /// Every condition as (dimension, value). A time range's value is its label.
    pub fn chips(&self) -> Vec<(Dimension, String)> {
        let mut chips = vec![];
        for (dimension, values) in [
            (Dimension::Type, &self.types),
            (Dimension::Tag, &self.tags),
            (Dimension::Source, &self.sources),
        ] {
            chips.extend(values.iter().cloned().map(|value| (dimension, value)));
        }
        chips.extend(
            self.time
                .iter()
                .map(|range| (Dimension::Time, range.label.clone())),
        );
        chips
    }

    /// The type when exactly one is chosen.
    pub fn single_type(&self) -> Option<&str> {
        match self.types.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }

    /// The image wall replaces the list when images are the only type.
    pub fn images_only(&self) -> bool {
        self.single_type() == Some("image")
    }

    /// Tab: from no type through each type and back. Several types count as none.
    pub fn cycle_type(&mut self, reverse: bool) {
        let count = TYPES.len() + 1;
        let current = self
            .single_type()
            .and_then(|t| TYPES.iter().position(|known| *known == t))
            .map_or(0, |position| position + 1);
        let next = if reverse {
            (current + count - 1) % count
        } else {
            (current + 1) % count
        };
        self.types = next
            .checked_sub(1)
            .map(|position| TYPES[position].into())
            .into_iter()
            .collect();
    }
}

/// What the search box words can be suggested as.
pub struct Catalog<'a> {
    pub tags: &'a [String],
    /// Device (id, name).
    pub sources: &'a [(String, String)],
    pub today: NaiveDate,
    /// Chinese interface: words are also read as pinyin initials.
    pub initials: bool,
}

/// The words of `query` that no suggestion claims: they stay plain text search.
pub fn unmatched_words(query: &str, options: &[Suggestion]) -> String {
    let mut words = Vec::new();
    let mut at = 0;
    for word in query.split_whitespace() {
        let start = at + query[at..].find(word).unwrap_or(0);
        let end = start + word.len();
        at = end;
        let claimed = options
            .iter()
            .any(|option| option.matched.start < end && start < option.matched.end);
        if !claimed {
            words.push(word);
        }
    }
    words.join(" ")
}

/// A filter the typed words could become. Nothing changes until it is accepted.
#[derive(Clone, Debug)]
pub struct Suggestion {
    pub dimension: Dimension,
    /// The tag, device id or type; for a time range, its label.
    pub value: String,
    pub matched: std::ops::Range<usize>,
    /// Set for a time range; `value` is then its label.
    pub time: Option<DateRange>,
    /// The suggestion replaces the dimension's values instead of adding to them.
    pub replace: bool,
}

fn candidates(
    token: &Token,
    tags: &[String],
    sources: &[(String, String)],
) -> Vec<(String, String)> {
    let values: Vec<(String, String)> = match token.dimension {
        Dimension::Type => TYPES
            .iter()
            .map(|v| ((*v).into(), value_label(v).into()))
            .collect(),
        Dimension::Tag => tags
            .iter()
            .map(|v| (v.clone(), format!("#{}", value_label(v))))
            .collect(),
        Dimension::Source => sources.to_vec(),
        Dimension::Time => vec![],
    };
    let needle = token.partial.to_lowercase();
    values
        .into_iter()
        .filter(|(value, label)| {
            value.to_lowercase().contains(&needle) || label.to_lowercase().contains(&needle)
        })
        .collect()
}

/// The span of `alias` in `query` when it stands on its own, not inside a longer word.
fn standalone(query: &str, alias: &str) -> Option<std::ops::Range<usize>> {
    let lowered = query.to_ascii_lowercase();
    lowered
        .match_indices(&alias.to_ascii_lowercase())
        .find_map(|(start, text)| {
            let end = start + text.len();
            let boundary =
                |c: char| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '#' | '@' | '/');
            (query[..start].chars().next_back().is_none_or(boundary)
                && query[end..].chars().next().is_none_or(boundary))
            .then_some(start..end)
        })
}

/// Pinyin initials of a name, `设计素材` -> `sjsc`; Latin letters and digits stand for themselves.
fn initials_of(name: &str) -> String {
    use pinyin::ToPinyin;
    name.chars()
        .filter_map(|ch| match ch.to_pinyin() {
            Some(pinyin) => pinyin.first_letter().chars().next(),
            None => ch.is_ascii_alphanumeric().then(|| ch.to_ascii_lowercase()),
        })
        .collect()
}

/// Whether a typed word (lowercase Latin letters, at least two) abbreviates `name`.
fn abbreviates(word: &str, name: &str) -> bool {
    word.len() >= 2
        && word.chars().all(|c| c.is_ascii_lowercase())
        && (initials_of(name).starts_with(word) || name.to_lowercase().starts_with(word))
}

/// Whether the word being typed (the last one) is a filter word: `#tag`, `@device` or `/type`.
/// Only then do the arrow keys belong to the suggestions; for plain words they move through the
/// results, even when a suggestion happens to be listed.
pub fn typing_a_filter(query: &str) -> bool {
    !query.ends_with(char::is_whitespace)
        && query
            .split_whitespace()
            .next_back()
            .is_some_and(|word| word.starts_with(['#', '@', '/']))
}

/// First suggestion shown, so that the highlighted one is always inside a window of `shown`.
pub fn suggestion_window_start(cursor: usize, shown: usize) -> usize {
    (cursor + 1).saturating_sub(shown.max(1))
}

pub fn suggestions(query: &str, catalog: &Catalog) -> Vec<Suggestion> {
    let mut result = Vec::new();
    let words = date_range::words(query);
    for dimension in [Dimension::Type, Dimension::Tag, Dimension::Source] {
        let all = Token {
            dimension,
            partial: String::new(),
        };
        for (value, display) in candidates(&all, catalog.tags, catalog.sources) {
            let mut matched = if dimension == Dimension::Source {
                None
            } else {
                [value_label(&value), value.as_str()]
                    .iter()
                    .find_map(|alias| standalone(query, alias))
            };
            for word in &words {
                if matched.is_some() {
                    break;
                }
                let text = &query[word.clone()];
                let needle = match parse_token(text) {
                    Some(token) if token.dimension == dimension => token.partial.to_lowercase(),
                    Some(_) => continue,
                    None if dimension != Dimension::Source => text.to_lowercase(),
                    None => continue,
                };
                if value.to_lowercase().contains(&needle)
                    || display.to_lowercase().contains(&needle)
                    || value_label(&value).to_lowercase().contains(&needle)
                {
                    matched = Some(word.clone());
                }
            }
            if let Some(matched) = matched {
                result.push(Suggestion {
                    dimension,
                    value,
                    matched,
                    time: None,
                    replace: false,
                });
            }
        }
    }
    if let Some(found) = date_range::find(query, catalog.today) {
        result.push(Suggestion {
            dimension: Dimension::Time,
            value: found.range.label.clone(),
            matched: found.span,
            time: Some(found.range),
            replace: true,
        });
    }
    if catalog.initials {
        for word in &words {
            let text = query[word.clone()].to_ascii_lowercase();
            for (initials, dimension, value) in INITIALS {
                if text != initials {
                    continue;
                }
                let time = (dimension == Dimension::Time)
                    .then(|| date_range::parse(value, catalog.today))
                    .flatten();
                let known = match dimension {
                    Dimension::Tag => catalog.tags.iter().any(|tag| tag == value),
                    Dimension::Time => time.is_some(),
                    _ => true,
                };
                let value = time.as_ref().map_or(value, |range| range.label.as_str());
                if known
                    && !result
                        .iter()
                        .any(|s| s.dimension == dimension && s.value == value)
                {
                    result.push(Suggestion {
                        dimension,
                        value: value.into(),
                        matched: word.clone(),
                        time,
                        replace: true,
                    });
                }
            }
        }
    }
    if catalog.initials {
        // Custom tags and device names: `sjsc` for a tag named 设计素材, `ip` for iPhone.
        for word in &words {
            let text = &query[word.clone()];
            let named = catalog
                .tags
                .iter()
                .map(|tag| (Dimension::Tag, tag.as_str(), value_label(tag)))
                .chain(
                    catalog
                        .sources
                        .iter()
                        .map(|(id, name)| (Dimension::Source, id.as_str(), name.as_str())),
                );
            for (dimension, value, name) in named {
                if abbreviates(text, name)
                    && !result
                        .iter()
                        .any(|s| s.dimension == dimension && s.value == value)
                {
                    result.push(Suggestion {
                        dimension,
                        value: value.into(),
                        matched: word.clone(),
                        time: None,
                        replace: true,
                    });
                }
            }
        }
    }
    // Keep the dimensions in their order; the sort is stable.
    result.sort_by_key(|suggestion| suggestion.dimension);
    result
}

pub fn remaining_query(query: &str, matched: &std::ops::Range<usize>) -> String {
    let mut remainder = query.to_string();
    remainder.replace_range(matched.clone(), "");
    remainder.trim().to_string()
}

#[cfg(test)]
mod tests {

    #[test]
    fn punctuation_only_text_is_sent_as_an_empty_query() {
        for text in ["@", "#", "/", " @ ", "# @ /", "...", "—"] {
            let filters = Filters {
                query: text.into(),
                ..Default::default()
            };
            assert_eq!(filters.request_on(today()).query, "", "for {text:?}");
        }
        for text in ["a", "#w", "@ hello", "设", "co-de", " 12 "] {
            let filters = Filters {
                query: text.into(),
                ..Default::default()
            };
            assert_eq!(
                filters.request_on(today()).query,
                text.trim(),
                "for {text:?}"
            );
        }
    }

    #[test]
    fn arrows_belong_to_the_suggestions_only_while_a_filter_word_is_typed() {
        assert!(typing_a_filter("#"));
        assert!(typing_a_filter("#de"));
        assert!(typing_a_filter("hello @mac"));
        assert!(typing_a_filter("/im"));
        assert!(!typing_a_filter(""));
        assert!(!typing_a_filter("图片"));
        assert!(!typing_a_filter("#work notes"));
        assert!(!typing_a_filter("#work "));
    }

    #[test]
    fn the_suggestion_window_follows_the_highlight() {
        assert_eq!(suggestion_window_start(0, 3), 0);
        assert_eq!(suggestion_window_start(2, 3), 0);
        assert_eq!(suggestion_window_start(3, 3), 1);
        assert_eq!(suggestion_window_start(7, 3), 5);
        assert_eq!(suggestion_window_start(0, 0), 0);
    }
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()
    }

    fn catalog<'a>(tags: &'a [String], sources: &'a [(String, String)]) -> Catalog<'a> {
        Catalog {
            tags,
            sources,
            today: today(),
            initials: false,
        }
    }

    fn chinese<'a>(tags: &'a [String], sources: &'a [(String, String)]) -> Catalog<'a> {
        Catalog {
            initials: true,
            ..catalog(tags, sources)
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).into()).collect()
    }

    #[test]
    fn prefixes_pick_the_dimension_and_leave_urls_and_plain_words_alone() {
        let token = |value| parse_token(value).map(|t| (t.dimension, t.partial));
        assert_eq!(token("  #收藏"), Some((Dimension::Tag, "收藏".into())));
        assert_eq!(token("@iPhone"), Some((Dimension::Source, "iPhone".into())));
        assert_eq!(token("/image"), Some((Dimension::Type, "image".into())));
        assert_eq!(token("#"), Some((Dimension::Tag, String::new())));
        assert!(parse_token("https://example.com").is_none());
        assert!(parse_token("type:image").is_none());
        assert!(parse_token("from:phone").is_none());
        assert!(parse_token("time:today").is_none());
        assert!(parse_token("ext:rs").is_none());
        assert!(parse_token("a@b.com").is_none());
    }

    #[test]
    fn values_of_one_dimension_are_alternatives_without_duplicates() {
        let mut filters = Filters::default();
        filters.add(Dimension::Type, "image".into());
        filters.add(Dimension::Type, "file".into());
        filters.add(Dimension::Type, "image".into());
        for tag in ["favorited", "工作", "favorited"] {
            filters.add(Dimension::Tag, tag.into());
        }
        filters.add(Dimension::Source, "phone".into());
        filters.add(Dimension::Source, "laptop".into());
        let request = filters.request_on(today());
        assert_eq!(request.content_types, ["image", "file"]);
        assert_eq!(request.tags, ["favorited", "工作"]);
        assert_eq!(request.source_devices, ["phone", "laptop"]);
        assert!(request.extensions.is_empty());
    }

    #[test]
    fn rich_text_is_asked_for_as_html() {
        let mut filters = Filters::default();
        filters.add(Dimension::Type, "richtext".into());
        assert_eq!(filters.request_on(today()).content_types, ["html"]);
    }

    #[test]
    fn removing_one_value_keeps_the_other_conditions_and_order() {
        let mut filters = Filters::default();
        filters.add(Dimension::Type, "image".into());
        for tag in ["favorited", "工作", "code"] {
            filters.add(Dimension::Tag, tag.into());
        }
        filters.remove(Dimension::Tag, "工作");
        assert_eq!(filters.request_on(today()).tags, ["favorited", "code"]);
        filters.remove(Dimension::Type, "image");
        assert!(filters.request_on(today()).content_types.is_empty());
        assert_eq!(filters.request_on(today()).tags, ["favorited", "code"]);
    }

    #[test]
    fn a_time_range_reaches_the_daemon_as_both_ends_and_replaces_the_previous_one() {
        let mut filters = Filters::default();
        assert_eq!(filters.request_on(today()).from_ms, None);
        assert_eq!(filters.request_on(today()).to_ms, None);
        filters.set_time(date_range::parse("9.1-9.15", today()).unwrap());
        filters.set_time(date_range::parse("上周", today()).unwrap());
        let request = filters.request_on(today());
        let (from, to) = date_range::parse("上周", today())
            .unwrap()
            .bounds_ms(today());
        assert_eq!((request.from_ms, request.to_ms), (Some(from), Some(to)));
        assert!(request.time_preset.is_none());
        assert_eq!(filters.chips(), [(Dimension::Time, "上周".into())]);
        filters.remove(Dimension::Time, "上周");
        assert!(filters.time.is_none());
    }

    #[test]
    fn the_request_carries_the_trimmed_query_and_the_panel_page_size() {
        let filters = Filters {
            query: "  hello  ".into(),
            ..Default::default()
        };
        let request = filters.request_on(today());
        assert_eq!(request.query, "hello");
        assert_eq!(request.limit, 50);
    }

    #[test]
    fn tab_cycles_through_the_types_and_back_to_none() {
        let mut filters = Filters::default();
        let mut seen = vec![];
        for _ in 0..TYPES.len() + 1 {
            filters.cycle_type(false);
            seen.push(filters.single_type().map(String::from));
        }
        assert_eq!(
            seen,
            [
                Some("text"),
                Some("richtext"),
                Some("image"),
                Some("file"),
                None
            ]
            .map(|t| t.map(String::from))
        );
        filters.cycle_type(true);
        assert_eq!(filters.single_type(), Some("file"));
        // Several types count as none, so Tab starts from the first.
        filters.add(Dimension::Type, "image".into());
        filters.cycle_type(false);
        assert_eq!(filters.types, ["text"]);
    }

    #[test]
    fn the_image_wall_needs_images_to_be_the_only_type() {
        let mut filters = Filters::default();
        assert!(!filters.images_only());
        filters.add(Dimension::Type, "image".into());
        assert!(filters.images_only());
        filters.add(Dimension::Type, "file".into());
        assert!(!filters.images_only());
    }

    #[test]
    fn confirming_a_tag_preserves_the_other_keywords_and_does_not_mutate_on_suggestion() {
        let filters = Filters {
            query: "工作 设计".into(),
            ..Default::default()
        };
        let tags = strings(&["工作"]);
        let choices = suggestions(&filters.query, &catalog(&tags, &[]));
        assert_eq!(filters.request_on(today()).query, "工作 设计");
        assert!(filters.request_on(today()).tags.is_empty());
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].dimension, Dimension::Tag);
        assert_eq!(remaining_query(&filters.query, &choices[0].matched), "设计");
    }

    #[test]
    fn plain_text_can_suggest_types_and_tags_without_requiring_prefixes() {
        let tags = strings(&["image"]);
        let choices = suggestions("图片 设计", &catalog(&tags, &[]));
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].dimension, Dimension::Type);
        assert_eq!(choices[1].dimension, Dimension::Tag);
        let link = strings(&["link"]);
        assert!(suggestions("https://example.com", &catalog(&link, &[])).is_empty());
        assert!(suggestions("context", &catalog(&[], &[])).is_empty());
    }

    #[test]
    fn complete_multiword_tag_is_consumed_as_one_condition() {
        let query = "工作 资料 设计";
        let tags = strings(&["工作 资料"]);
        let choices = suggestions(query, &catalog(&tags, &[]));
        assert_eq!(remaining_query(query, &choices[0].matched), "设计");
    }

    #[test]
    fn prefixed_words_can_follow_keywords_and_unicode_offsets_remain_valid() {
        let query = "设计 #工作";
        let tags = strings(&["工作"]);
        let choices = suggestions(query, &catalog(&tags, &[]));
        assert_eq!(choices.len(), 1);
        assert_eq!(remaining_query(query, &choices[0].matched), "设计");
        let choices = suggestions("设计 /image", &catalog(&[], &[]));
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].dimension, Dimension::Type);
        assert_eq!(choices[0].value, "image");
        assert!(!choices[0].replace);
    }

    #[test]
    fn at_suggests_devices_by_name_or_id() {
        let devices = vec![("peer-1".to_string(), "iPhone".to_string())];
        let by_name = suggestions("@iph", &catalog(&[], &devices));
        assert_eq!(by_name.len(), 1);
        assert_eq!(
            (by_name[0].dimension, by_name[0].value.as_str()),
            (Dimension::Source, "peer-1")
        );
        assert_eq!(suggestions("@peer", &catalog(&[], &devices)).len(), 1);
        assert_eq!(suggestions("@", &catalog(&[], &devices)).len(), 1);
        // Without the prefix a device name is just a word.
        assert!(suggestions("iPhone", &catalog(&[], &devices)).is_empty());
        assert!(suggestions("@android", &catalog(&[], &devices)).is_empty());
    }

    #[test]
    fn a_date_in_the_words_suggests_a_time_range_that_replaces_the_old_one() {
        let query = "报告 9.1-9.15 草稿";
        let choices = suggestions(query, &catalog(&[], &[]));
        assert_eq!(choices.len(), 1);
        let choice = &choices[0];
        assert_eq!(
            (choice.dimension, choice.value.as_str()),
            (Dimension::Time, "9月1日 – 15日")
        );
        assert_eq!(remaining_query(query, &choice.matched), "报告  草稿");
        let mut filters = Filters::default();
        filters.set_time(date_range::parse("上周", today()).unwrap());
        filters.accept(choice);
        assert_eq!(filters.chips(), [(Dimension::Time, "9月1日 – 15日".into())]);
    }

    #[test]
    fn pinyin_initials_are_read_only_when_enabled() {
        let tags = strings(&["link", "favorited"]);
        assert!(suggestions("tp", &catalog(&tags, &[])).is_empty());
        let choices = suggestions("tp zt", &chinese(&tags, &[]));
        let summary: Vec<_> = choices
            .iter()
            .map(|s| (s.dimension, s.value.as_str()))
            .collect();
        assert_eq!(
            summary,
            [(Dimension::Type, "image"), (Dimension::Time, "昨天")]
        );
        assert!(choices.iter().all(|s| s.replace));
        // Only whole words count, and a tag must exist.
        assert!(suggestions("tpx", &chinese(&tags, &[])).is_empty());
        assert!(suggestions("dm", &chinese(&tags, &[])).is_empty());
        assert_eq!(suggestions("lj", &chinese(&tags, &[]))[0].value, "link");
    }

    #[test]
    fn pinyin_initials_reach_custom_tags_and_device_names() {
        let tags = strings(&["设计素材与灵感收集", "工作"]);
        let devices = vec![
            ("p1".to_string(), "iPhone".to_string()),
            ("p2".to_string(), "iPad".to_string()),
            ("p3".to_string(), "小米手机".to_string()),
        ];
        let pick = |query: &str, catalog: &Catalog| -> Vec<(Dimension, String)> {
            suggestions(query, catalog)
                .into_iter()
                .map(|s| (s.dimension, s.value))
                .collect()
        };
        let on = chinese(&tags, &devices);
        assert_eq!(
            pick("sjsc", &on),
            [(Dimension::Tag, "设计素材与灵感收集".to_string())]
        );
        assert_eq!(pick("gz", &on), [(Dimension::Tag, "工作".to_string())]);
        assert_eq!(
            pick("ip", &on),
            [
                (Dimension::Source, "p1".to_string()),
                (Dimension::Source, "p2".to_string())
            ]
        );
        assert_eq!(pick("xmsj", &on), [(Dimension::Source, "p3".to_string())]);
        // One letter, digits, capitals and a disabled gate suggest nothing.
        assert!(pick("s", &on).is_empty());
        assert!(pick("IP", &on).is_empty());
        assert!(pick("ip", &catalog(&tags, &devices)).is_empty());
        // A replacing suggestion, like the other pinyin ones.
        let mut filters = Filters::default();
        filters.add(Dimension::Source, "p3".into());
        filters.accept(&suggestions("ip", &on)[0]);
        assert_eq!(filters.sources, ["p1"]);
    }

    #[test]
    fn a_recognised_word_replaces_the_value_and_leaves_the_other_words() {
        let query = "docker jt";
        let choices = suggestions(query, &chinese(&[], &[]));
        assert_eq!(choices.len(), 1);
        let mut filters = Filters::default();
        filters.set_time(date_range::parse("昨天", today()).unwrap());
        filters.accept(&choices[0]);
        assert_eq!(filters.chips(), [(Dimension::Time, "今天".into())]);
        assert_eq!(remaining_query(query, &choices[0].matched), "docker");

        let mut filters = Filters::default();
        filters.add(Dimension::Type, "text".into());
        filters.add(Dimension::Type, "file".into());
        filters.accept(&suggestions("tp", &chinese(&[], &[]))[0]);
        assert_eq!(filters.types, ["image"]);
    }

    #[test]
    fn words_no_suggestion_claims_stay_text() {
        let claim = |range: std::ops::Range<usize>| Suggestion {
            dimension: Dimension::Type,
            value: "image".into(),
            matched: range,
            time: None,
            replace: false,
        };
        assert_eq!(
            unmatched_words("ip jt docker", &[claim(0..2), claim(3..5)]),
            "docker"
        );
        assert_eq!(unmatched_words("tp zt", &[claim(0..2), claim(3..5)]), "");
        assert_eq!(unmatched_words("docker", &[]), "docker");
    }
}
