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
    pub tag: Option<String>,
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
    pub fn cycle(&mut self, reverse: bool) {
        self.content_type =
            (self.content_type + if reverse { TYPES.len() - 1 } else { 1 }) % TYPES.len();
    }
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
            tags: self.tag.iter().cloned().collect(),
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
            Dimension::Tag => self.tag = value,
            Dimension::Source => self.source = value,
            Dimension::Time => self.time = value,
            Dimension::Extension => self.extension = value,
        }
    }

    pub fn chips(&self) -> Vec<(Dimension, String)> {
        let mut chips = vec![];
        if self.content_type > 0 {
            chips.push((Dimension::Type, TYPES[self.content_type].into()));
        }
        for (dimension, value) in [
            (Dimension::Tag, &self.tag),
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
        Dimension::Tag => tags.iter().map(|v| (v.clone(), label(v).into())).collect(),
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
    fn type_cycle_matches_reference_order_in_both_directions() {
        let mut filters = Filters::default();
        for index in [1, 2, 3, 4, 0] {
            filters.cycle(false);
            assert_eq!(filters.content_type, index);
        }
        filters.cycle(true);
        assert_eq!(filters.content_type, 4);
    }

    #[test]
    fn all_dimensions_reach_the_daemon_and_limit_matches_existing_panel() {
        let filters = Filters {
            query: "  hello  ".into(),
            content_type: 2,
            tag: Some("code".into()),
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
