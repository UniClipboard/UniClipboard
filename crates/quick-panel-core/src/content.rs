//! What kind of thing an entry is, and how the preview describes it. Only fields the daemon
//! already sends are used: content type, tags, link URLs, file names and paths, character count.

use uc_daemon_contract::api::dto::search::SearchResultDto;

use crate::text;

/// How a row and the preview present an entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    RichText,
    Link,
    Code,
    Image,
    File,
}

impl Kind {
    pub fn of(item: &SearchResultDto) -> Self {
        let tagged = |name: &str| item.tags.iter().any(|tag| tag == name);
        match item.content_type.as_str() {
            "image" => Self::Image,
            "file" => Self::File,
            _ if tagged("link") || !item.link_urls.is_empty() => Self::Link,
            _ if tagged("code") => Self::Code,
            // The search index files rich text under the category `html`.
            "html" => Self::RichText,
            _ => Self::Text,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "文本",
            Self::RichText => text::RICH_TEXT,
            Self::Link => "链接",
            Self::Code => "代码",
            Self::Image => "图片",
            Self::File => "文件",
        }
    }
}

/// Splits a link into its host and the rest, so the host can be emphasised.
pub fn split_link(url: &str) -> (&str, &str) {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    match without_scheme.find('/') {
        Some(at) => without_scheme.split_at(at),
        None => (without_scheme, ""),
    }
}

/// A byte count as "512 B", "3.4 KB" or "12.0 MB".
pub fn size_text(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    if bytes < 1024. {
        format!("{} B", bytes as i64)
    } else if bytes < 1024. * 1024. {
        format!("{:.1} KB", bytes / 1024.)
    } else {
        format!("{:.1} MB", bytes / 1024. / 1024.)
    }
}

/// What is known about the loaded entry when the header is written.
pub struct Facts<'a> {
    pub item: &'a SearchResultDto,
    /// The full text once loaded; the preview text of the row until then.
    pub text: Option<&'a str>,
    /// Width, height and size in bytes of a loaded image.
    pub image: Option<(u32, u32, i64)>,
    pub source_name: Option<&'a str>,
    pub now_ms: i64,
}

/// "类型 · 行数或字数或大小 · 来自 <设备> · 时间".
pub fn header(facts: &Facts) -> String {
    let item = facts.item;
    let kind = Kind::of(item);
    let text = facts.text.or(item.text_preview.as_deref());
    let lines = text.map_or(0, |t| t.lines().count());
    let mut parts = vec![kind.label().to_string()];
    match kind {
        Kind::Code => parts.extend((lines > 0).then(|| format!("{lines} 行"))),
        Kind::Text | Kind::RichText => {
            if lines > 1 {
                parts.push(format!("{lines} 行"));
            } else if let Some(count) = item
                .char_count
                .or_else(|| text.map(|t| t.chars().count() as i64))
            {
                parts.push(format!("{count} 个字符"));
            }
        }
        Kind::Link => {
            if let Some(url) = item.link_urls.first() {
                parts.push(split_link(url).0.to_string());
            }
        }
        Kind::Image => {
            if let Some((width, height, bytes)) = facts.image {
                parts.push(format!("{width} × {height}"));
                parts.push(size_text(bytes));
            }
        }
        Kind::File => parts.push(format!("{} 项", item.file_names.len().max(1))),
    }
    if let Some(name) = facts.source_name {
        parts.push(format!("来自 {name}"));
    }
    parts.push(text::relative_time(facts.now_ms - item.active_time_ms));
    parts.join(" · ")
}

/// A file of a file entry: its name and, when known, where it is.
pub fn files(item: &SearchResultDto) -> Vec<(String, Option<String>)> {
    item.file_names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let path = item.file_paths.get(index).filter(|p| !p.is_empty());
            (name.clone(), path.cloned())
        })
        .collect()
}

/// Lines of a code preview, numbered from 1. Very long text stops at a fixed number of lines.
pub const MAX_CODE_LINES: usize = 2000;

pub fn code_lines(text: &str) -> Vec<(usize, &str)> {
    text.lines()
        .take(MAX_CODE_LINES)
        .enumerate()
        .map(|(index, line)| (index + 1, line))
        .collect()
}

/// Byte ranges of `text` that match the words of `query`, for highlighting. Words are matched
/// case-insensitively; words without a letter or digit are not searched, so they are not marked.
/// Matches separated only by punctuation (`tp_zt` for `tp zt`) merge into one range.
pub fn match_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let words: Vec<Vec<char>> = query
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| word.chars().collect())
        .collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let mut found: Vec<(usize, usize)> = Vec::new(); // char index ranges, end exclusive
    for word in &words {
        let mut at = 0;
        while at + word.len() <= chars.len() {
            if word
                .iter()
                .zip(&chars[at..])
                .all(|(n, (_, c))| same(*n, *c))
            {
                found.push((at, at + word.len()));
                at += word.len();
            } else {
                at += 1;
            }
        }
    }
    found.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in found {
        match merged.last_mut() {
            Some(last)
                if start <= last.1
                    || chars[last.1..start]
                        .iter()
                        .all(|(_, c)| !c.is_alphanumeric() && !c.is_whitespace()) =>
            {
                last.1 = last.1.max(end);
            }
            _ => merged.push((start, end)),
        }
    }
    merged
        .into_iter()
        .map(|(start, end)| {
            let from = chars[start].0;
            let to = chars.get(end).map_or(text.len(), |(at, _)| *at);
            from..to
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(content_type: &str) -> SearchResultDto {
        SearchResultDto {
            entry_id: "e".into(),
            content_type: content_type.into(),
            active_time_ms: 0,
            tags: vec![],
            text_preview: None,
            char_count: None,
            mime_type: String::new(),
            file_extensions: vec![],
            file_names: vec![],
            file_paths: vec![],
            link_urls: vec![],
            source_device: None,
            payload_state: None,
        }
    }

    fn facts(item: &SearchResultDto) -> Facts<'_> {
        Facts {
            item,
            text: None,
            image: None,
            source_name: None,
            now_ms: 5 * 60_000,
        }
    }

    #[test]
    fn a_row_is_classified_from_the_existing_fields_only() {
        assert_eq!(Kind::of(&entry("text")), Kind::Text);
        assert_eq!(Kind::of(&entry("html")), Kind::RichText);
        assert_eq!(Kind::of(&entry("image")), Kind::Image);
        assert_eq!(Kind::of(&entry("file")), Kind::File);
        let mut code = entry("text");
        code.tags = vec!["code".into()];
        assert_eq!(Kind::of(&code), Kind::Code);
        let mut link = entry("text");
        link.tags = vec!["link".into()];
        assert_eq!(Kind::of(&link), Kind::Link);
        let mut link = entry("text");
        link.link_urls = vec!["https://example.com".into()];
        assert_eq!(Kind::of(&link), Kind::Link);
        // A link wins over the code tag.
        link.tags = vec!["code".into()];
        assert_eq!(Kind::of(&link), Kind::Link);
    }

    #[test]
    fn a_link_is_split_into_host_and_path() {
        assert_eq!(
            split_link("https://github.com/uniclipboard/desktop/pull/1767"),
            ("github.com", "/uniclipboard/desktop/pull/1767")
        );
        assert_eq!(split_link("example.com"), ("example.com", ""));
        assert_eq!(split_link("http://localhost:8080"), ("localhost:8080", ""));
    }

    #[test]
    fn sizes_use_the_largest_whole_unit() {
        assert_eq!(size_text(0), "0 B");
        assert_eq!(size_text(1023), "1023 B");
        assert_eq!(size_text(3500), "3.4 KB");
        assert_eq!(size_text(12 * 1024 * 1024), "12.0 MB");
        assert_eq!(size_text(-5), "0 B");
    }

    #[test]
    fn text_shows_lines_when_it_has_several_and_characters_otherwise() {
        let mut item = entry("text");
        item.char_count = Some(42);
        item.text_preview = Some("one line".into());
        assert_eq!(header(&facts(&item)), "文本 · 42 个字符 · 5m");
        let mut f = facts(&item);
        f.text = Some("a\nb\nc\n");
        assert_eq!(header(&f), "文本 · 3 行 · 5m");
        item.char_count = None;
        item.text_preview = None;
        assert_eq!(header(&facts(&item)), "文本 · 5m");
    }

    #[test]
    fn code_shows_lines_and_the_device_it_came_from() {
        let mut item = entry("text");
        item.tags = vec!["code".into()];
        let mut f = facts(&item);
        f.text = Some("fn a() {}\nfn b() {}");
        f.source_name = Some("iPhone");
        assert_eq!(header(&f), "代码 · 2 行 · 来自 iPhone · 5m");
    }

    #[test]
    fn link_image_and_file_headers() {
        let mut link = entry("text");
        link.link_urls = vec!["https://github.com/a/b".into()];
        assert_eq!(header(&facts(&link)), "链接 · github.com · 5m");
        let image = entry("image");
        let mut f = facts(&image);
        f.image = Some((2560, 1440, 3500));
        assert_eq!(header(&f), "图片 · 2560 × 1440 · 3.4 KB · 5m");
        assert_eq!(header(&facts(&image)), "图片 · 5m");
        let mut file = entry("file");
        file.file_names = vec!["a.pdf".into(), "b.pdf".into()];
        assert_eq!(header(&facts(&file)), "文件 · 2 项 · 5m");
    }

    #[test]
    fn files_pair_names_with_their_paths() {
        let mut file = entry("file");
        file.file_names = vec!["a.pdf".into(), "b.pdf".into()];
        file.file_paths = vec!["/tmp/a.pdf".into()];
        assert_eq!(
            files(&file),
            [
                ("a.pdf".to_string(), Some("/tmp/a.pdf".to_string())),
                ("b.pdf".to_string(), None)
            ]
        );
    }

    #[test]
    fn code_lines_are_numbered_and_capped() {
        assert_eq!(code_lines("a\n\nb"), [(1, "a"), (2, ""), (3, "b")]);
        let long = "x\n".repeat(MAX_CODE_LINES + 10);
        assert_eq!(code_lines(&long).len(), MAX_CODE_LINES);
    }
    #[test]
    fn matches_ignore_case_and_stay_on_char_boundaries() {
        assert_eq!(match_ranges("Docker compose", "docker"), vec![0..6]);
        assert_eq!(
            match_ranges("周会 Docker 纪要", "docker 纪要"),
            vec![7..13, 14..20]
        );
        assert_eq!(match_ranges("İstanbul", "stan"), vec![2..6]);
        assert_eq!(
            match_ranges("x", "xyz"),
            Vec::<std::ops::Range<usize>>::new()
        );
    }

    #[test]
    fn matches_across_punctuation_merge_and_punctuation_words_are_skipped() {
        assert_eq!(match_ranges("tp_zt_export.csv", "tp zt"), vec![0..5]);
        assert_eq!(match_ranges("a b", "a b"), vec![0..1, 2..3]);
        assert!(match_ranges("#tag", "#").is_empty());
    }
}
