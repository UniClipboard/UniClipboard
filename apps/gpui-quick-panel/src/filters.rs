use uc_daemon_client::SearchQueryRequest;

pub const TYPES: [&str; 5] = ["all", "text", "richtext", "image", "file"];
pub const BUILTIN_TAGS: [&str; 5] = ["link", "code", "favorited", "image", "directory"];
pub const TIMES: [&str; 6] = [
    "today",
    "yesterday",
    "last_7d",
    "last_30d",
    "this_week",
    "this_month",
];
pub const EXTENSIONS: [&str; 10] = [
    "txt", "md", "jpg", "png", "pdf", "ts", "js", "json", "rs", "go",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    Type,
    Tag,
    Source,
    Time,
    Extension,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Filters {
    pub query: String,
    pub content_type: usize,
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub time: Option<String>,
    pub extension: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Token {
    pub dimension: Dimension,
    pub partial: String,
    pub committed: bool,
}

pub fn parse_token(value: &str) -> Option<Token> {
    let value = value.trim_start();
    let (dimension, rest) = if let Some(rest) = value.strip_prefix('#') {
        (Dimension::Tag, rest)
    } else {
        let (key, rest) = value.split_once(':')?;
        let dimension = match key.to_ascii_lowercase().as_str() {
            "type" => Dimension::Type,
            "from" => Dimension::Source,
            "time" => Dimension::Time,
            "ext" => Dimension::Extension,
            _ => return None,
        };
        (dimension, rest)
    };
    Some(Token {
        dimension,
        partial: rest.trim().into(),
        committed: rest.ends_with(char::is_whitespace) && !rest.trim().is_empty(),
    })
}

impl Filters {
    pub fn request(&self) -> SearchQueryRequest {
        SearchQueryRequest {
            query: self.query.trim().into(),
            operator: None,
            time_preset: self.time.clone(),
            from_ms: None,
            to_ms: None,
            content_types: TYPES
                .get(self.content_type)
                .filter(|v| **v != "all")
                .map(|v| vec![(*v).into()])
                .unwrap_or_default(),
            tags: self.tags.clone(),
            extensions: self.extension.iter().cloned().collect(),
            source_devices: self.source.iter().cloned().collect(),
            limit: 50,
            offset: 0,
        }
    }

    pub fn apply(&mut self, dimension: Dimension, value: Option<String>) {
        match dimension {
            Dimension::Type => {
                self.content_type = value
                    .and_then(|v| TYPES.iter().position(|t| *t == v))
                    .unwrap_or(0)
            }
            Dimension::Tag => match value {
                Some(value) if !self.tags.contains(&value) => self.tags.push(value),
                Some(_) => {}
                None => self.tags.clear(),
            },
            Dimension::Source => self.source = value,
            Dimension::Time => self.time = value,
            Dimension::Extension => self.extension = value,
        }
    }

    pub fn toggle(&mut self, dimension: Dimension, value: String) {
        if self.contains(dimension, &value) {
            self.remove(dimension, &value);
        } else {
            self.apply(dimension, Some(value));
        }
    }

    pub fn remove(&mut self, dimension: Dimension, value: &str) {
        if dimension == Dimension::Tag {
            self.tags.retain(|tag| tag != value);
        } else {
            self.apply(dimension, None);
        }
    }

    pub fn contains(&self, dimension: Dimension, value: &str) -> bool {
        self.chips()
            .iter()
            .any(|(d, v)| *d == dimension && v == value)
    }

    pub fn chips(&self) -> Vec<(Dimension, String)> {
        let mut chips = vec![];
        if self.content_type > 0 {
            chips.push((Dimension::Type, TYPES[self.content_type].into()));
        }
        chips.extend(self.tags.iter().cloned().map(|tag| (Dimension::Tag, tag)));
        for (dimension, value) in [
            (Dimension::Source, &self.source),
            (Dimension::Time, &self.time),
            (Dimension::Extension, &self.extension),
        ] {
            if let Some(value) = value {
                chips.push((dimension, value.clone()));
            }
        }
        chips
    }
}

pub fn label(value: &str) -> &str {
    match value {
        "all" => "全部",
        "text" => "文本",
        "richtext" => "富文本",
        "image" => "图片",
        "file" => "文件",
        "link" => "链接",
        "code" => "代码",
        "favorited" => "收藏",
        "directory" => "文件夹",
        "today" => "今天",
        "yesterday" => "昨天",
        "last_7d" => "最近 7 天",
        "last_30d" => "最近 30 天",
        "this_week" => "本周",
        "this_month" => "本月",
        _ => value,
    }
}

pub fn candidates(
    token: &Token,
    tags: &[String],
    sources: &[(String, String)],
) -> Vec<(String, String)> {
    let values: Vec<(String, String)> = match token.dimension {
        Dimension::Type => TYPES[1..]
            .iter()
            .map(|v| ((*v).into(), label(v).into()))
            .collect(),
        Dimension::Tag => tags
            .iter()
            .map(|v| (v.clone(), format!("#{}", label(v))))
            .collect(),
        Dimension::Source => sources.to_vec(),
        Dimension::Time => TIMES
            .iter()
            .map(|v| ((*v).into(), label(v).into()))
            .collect(),
        Dimension::Extension => EXTENSIONS
            .iter()
            .map(|v| ((*v).into(), format!(".{v}")))
            .collect(),
    };
    let needle = token.partial.to_lowercase();
    values
        .into_iter()
        .filter(|(value, label)| {
            value.to_lowercase().contains(&needle) || label.to_lowercase().contains(&needle)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchored_filter_tokens_do_not_steal_urls_or_plain_queries() {
        assert_eq!(
            parse_token("  TYPE:image "),
            Some(Token {
                dimension: Dimension::Type,
                partial: "image".into(),
                committed: true
            })
        );
        assert_eq!(
            parse_token("#收藏"),
            Some(Token {
                dimension: Dimension::Tag,
                partial: "收藏".into(),
                committed: false
            })
        );
        assert!(parse_token("https://example.com").is_none());
        assert!(parse_token("some type:image").is_none());
        assert!(!parse_token("type: ").unwrap().committed);
    }

    #[test]
    fn adding_tags_preserves_type_and_existing_tags_without_duplicates() {
        let mut filters = Filters::default();
        filters.apply(Dimension::Type, Some("image".into()));
        filters.apply(Dimension::Tag, Some("favorited".into()));
        filters.apply(Dimension::Tag, Some("工作".into()));
        filters.apply(Dimension::Tag, Some("favorited".into()));
        assert_eq!(filters.request().content_types, ["image"]);
        assert_eq!(filters.request().tags, ["favorited", "工作"]);
    }

    #[test]
    fn removing_one_tag_keeps_the_other_conditions_and_order() {
        let mut filters = Filters::default();
        filters.apply(Dimension::Type, Some("image".into()));
        for tag in ["favorited", "工作", "code"] {
            filters.apply(Dimension::Tag, Some(tag.into()));
        }
        filters.remove(Dimension::Tag, "工作");
        assert_eq!(filters.request().tags, ["favorited", "code"]);
        filters.remove(Dimension::Type, "image");
        assert!(filters.request().content_types.is_empty());
        assert_eq!(filters.request().tags, ["favorited", "code"]);
    }

    #[test]
    fn toggling_replaces_types_but_preserves_other_selected_tags() {
        let mut filters = Filters::default();
        filters.toggle(Dimension::Type, "image".into());
        filters.toggle(Dimension::Tag, "favorited".into());
        filters.toggle(Dimension::Tag, "工作".into());
        filters.toggle(Dimension::Type, "text".into());
        assert_eq!(filters.request().content_types, ["text"]);
        assert_eq!(filters.request().tags, ["favorited", "工作"]);
        filters.toggle(Dimension::Tag, "favorited".into());
        filters.toggle(Dimension::Type, "text".into());
        assert!(filters.request().content_types.is_empty());
        assert_eq!(filters.request().tags, ["工作"]);
    }

    #[test]
    fn all_dimensions_reach_the_daemon_and_limit_matches_existing_panel() {
        let filters = Filters {
            query: "  hello  ".into(),
            content_type: 2,
            tags: vec!["code".into()],
            source: Some("phone".into()),
            time: Some("last_7d".into()),
            extension: Some("rs".into()),
        };
        let request = filters.request();
        assert_eq!(request.query, "hello");
        assert_eq!(request.content_types, ["richtext"]);
        assert_eq!(request.tags, ["code"]);
        assert_eq!(request.source_devices, ["phone"]);
        assert_eq!(request.extensions, ["rs"]);
        assert_eq!(request.time_preset.as_deref(), Some("last_7d"));
        assert_eq!(request.limit, 50);
    }
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub dimension: Dimension,
    pub value: String,
    pub matched: Option<std::ops::Range<usize>>,
}

pub fn suggestions(query: &str, tags: &[String], sources: &[(String, String)]) -> Vec<Suggestion> {
    let mut result = Vec::new();
    for dimension in [
        Dimension::Type,
        Dimension::Tag,
        Dimension::Source,
        Dimension::Time,
        Dimension::Extension,
    ] {
        let token = Token {
            dimension,
            partial: String::new(),
            committed: false,
        };
        for (value, display) in candidates(&token, tags, sources) {
            let mut matched = if matches!(dimension, Dimension::Type | Dimension::Tag) {
                [label(&value), value.as_str()].iter().find_map(|alias| {
                    let lowered = query.to_ascii_lowercase();
                    lowered
                        .match_indices(&alias.to_ascii_lowercase())
                        .find_map(|(start, text)| {
                            let end = start + text.len();
                            let before = query[..start].chars().next_back();
                            let after = query[end..].chars().next();
                            let boundary = |c: char| {
                                !c.is_ascii_alphanumeric() && c != '_' && c != '#' && c != ':'
                            };
                            (before.is_none_or(boundary) && after.is_none_or(boundary))
                                .then_some(start..end)
                        })
                })
            } else {
                None
            };
            let mut offset = 0;
            for word in query.split_inclusive(char::is_whitespace) {
                if matched.is_some() {
                    break;
                }
                let text = word.trim_end();
                let range = offset..offset + text.len();
                offset += word.len();
                if text.is_empty() {
                    continue;
                }
                if let Some(token) = parse_token(text) {
                    if token.dimension == dimension
                        && (value.to_lowercase().contains(&token.partial.to_lowercase())
                            || display
                                .to_lowercase()
                                .contains(&token.partial.to_lowercase()))
                    {
                        matched = Some(range);
                        break;
                    }
                } else if matches!(dimension, Dimension::Type | Dimension::Tag) {
                    let needle = text.to_lowercase();
                    if value.to_lowercase().contains(&needle)
                        || label(&value).to_lowercase().contains(&needle)
                    {
                        matched = Some(range);
                        break;
                    }
                }
            }
            if let Some(matched) = matched {
                result.push(Suggestion {
                    dimension,
                    value,
                    matched: Some(matched),
                });
            }
        }
    }
    result
}

pub fn remaining_query(query: &str, matched: &std::ops::Range<usize>) -> String {
    let mut remainder = query.to_string();
    remainder.replace_range(matched.clone(), "");
    remainder.trim().to_string()
}

#[cfg(test)]
mod suggestion_tests {
    use super::*;
    #[test]
    fn confirming_a_tag_preserves_the_other_keywords_and_does_not_mutate_on_suggestion() {
        let filters = Filters {
            query: "工作 设计".into(),
            ..Default::default()
        };
        let choices = suggestions(&filters.query, &["工作".into()], &[]);
        assert_eq!(filters.request().query, "工作 设计");
        assert!(filters.request().tags.is_empty());
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].dimension, Dimension::Tag);
        assert_eq!(
            remaining_query(&filters.query, choices[0].matched.as_ref().unwrap()),
            "设计"
        );
    }
    #[test]
    fn plain_text_can_suggest_types_and_tags_without_requiring_prefixes() {
        let choices = suggestions("图片 设计", &["image".into()], &[]);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].dimension, Dimension::Type);
        assert_eq!(choices[1].dimension, Dimension::Tag);
        assert!(suggestions("https://example.com", &["link".into()], &[]).is_empty());
        assert!(suggestions("context", &[], &[]).is_empty());
    }
    #[test]
    fn complete_multiword_tag_is_consumed_as_one_condition() {
        let query = "工作 资料 设计";
        let choices = suggestions(query, &["工作 资料".into()], &[]);
        assert_eq!(
            remaining_query(query, choices[0].matched.as_ref().unwrap()),
            "设计"
        );
    }
    #[test]
    fn explicit_tokens_can_follow_keywords_and_unicode_offsets_remain_valid() {
        let query = "设计 #工作";
        let choices = suggestions(query, &["工作".into()], &[]);
        assert_eq!(choices.len(), 1);
        assert_eq!(
            remaining_query(query, choices[0].matched.as_ref().unwrap()),
            "设计"
        );
        assert_eq!(suggestions("设计 type:image", &[], &[]).len(), 1);
    }
}
