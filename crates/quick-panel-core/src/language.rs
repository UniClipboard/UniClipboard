//! Which language the interface is in, for the features that only exist in Chinese.

/// Whether the interface language is Chinese.
///
/// `configured` is `general.language` from the settings. When it is unset the GUI follows the
/// system language, so this does too; `system_language` reads it. Where the system language cannot
/// be read the answer is yes, because the panel's own text is Chinese only until it is translated.
pub fn is_chinese(
    configured: Option<&str>,
    system_language: impl FnOnce() -> Option<String>,
) -> bool {
    match configured.filter(|tag| !tag.trim().is_empty()) {
        Some(tag) => is_chinese_tag(tag),
        None => system_language().is_none_or(|tag| is_chinese_tag(&tag)),
    }
}

/// `zh`, `zh-CN`, `zh_TW` and `zh-Hant` are Chinese; the primary subtag decides.
fn is_chinese_tag(tag: &str) -> bool {
    tag.trim()
        .split(['-', '_', '.'])
        .next()
        .is_some_and(|primary| primary.eq_ignore_ascii_case("zh"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_primary_subtag_decides() {
        for tag in ["zh", "zh-CN", "zh_TW", "zh-Hant-TW", "ZH", "zh_CN.UTF-8"] {
            assert!(is_chinese(Some(tag), || None), "{tag}");
        }
        for tag in ["en-US", "ja-JP", "pt-BR", "zhuang"] {
            assert!(!is_chinese(Some(tag), || None), "{tag}");
        }
    }
}
