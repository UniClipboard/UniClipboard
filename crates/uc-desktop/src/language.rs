//! Which of the shipped interface languages a language tag selects.
//!
//! Shared by every desktop surface that localizes on its own (the tray, the native quick panel),
//! so that they all agree with the main window about which language a setting means.

/// Normalize a language string to a supported locale.
///
/// Matches case-insensitively on the primary subtag. Traditional Chinese tags
/// (`zh-Hant`, `zh-TW`, `zh-HK`, and `zh-MO`) select `"zh-TW"`; other Chinese
/// tags select `"zh-CN"`. Japanese, Russian, and Portuguese region variants
/// collapse to their respective bundles. Anything without a bundle is `"en-US"`.
///
/// Keep the supported set in sync with `SUPPORTED_LANGUAGES` in `apps/gui/src/i18n/index.ts`,
/// including the frontend's subtag fallbacks in `normalizeLanguage()`.
pub fn normalize_language(language: &str) -> &'static str {
    // Accept both separators: BCP-47 hands us "pt-BR", POSIX locale envs "pt_BR".
    let mut subtags = language.split(['-', '_']);
    let primary = subtags.next().unwrap_or_default();
    if primary.eq_ignore_ascii_case("zh") {
        return if subtags.any(|subtag| {
            matches!(
                subtag.to_ascii_lowercase().as_str(),
                "hant" | "tw" | "hk" | "mo"
            )
        }) {
            "zh-TW"
        } else {
            "zh-CN"
        };
    }

    match primary.to_ascii_lowercase().as_str() {
        "ja" => "ja-JP",
        "ru" => "ru-RU",
        "pt" => "pt-BR",
        _ => "en-US",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_region_variants_onto_their_bundle() {
        assert_eq!(normalize_language("zh-CN"), "zh-CN");
        assert_eq!(normalize_language("zh-TW"), "zh-TW");
        assert_eq!(normalize_language("zh-Hant"), "zh-TW");
        assert_eq!(normalize_language("zh-HK"), "zh-TW");
        assert_eq!(normalize_language("zh-MO"), "zh-TW");
        assert_eq!(normalize_language("zh-SG"), "zh-CN");
        assert_eq!(normalize_language("ja-JP"), "ja-JP");
        assert_eq!(normalize_language("ru-BY"), "ru-RU");
        assert_eq!(normalize_language("pt-BR"), "pt-BR");
        // European Portuguese has no bundle; Brazilian copy beats falling back to English.
        assert_eq!(normalize_language("pt-PT"), "pt-BR");
    }

    #[test]
    fn falls_back_to_english_without_a_bundle() {
        assert_eq!(normalize_language("fr-FR"), "en-US");
        assert_eq!(normalize_language("en-US"), "en-US");
        assert_eq!(normalize_language(""), "en-US");
    }

    #[test]
    fn ignores_case_and_accepts_posix_separators() {
        assert_eq!(normalize_language("JA_jp"), "ja-JP");
        assert_eq!(normalize_language("RU-ru"), "ru-RU");
        assert_eq!(normalize_language("PT"), "pt-BR");
        assert_eq!(normalize_language("pt_BR"), "pt-BR");
        assert_eq!(normalize_language("zh_CN"), "zh-CN");
        assert_eq!(normalize_language("ZH_tw"), "zh-TW");
    }

    #[test]
    fn matches_the_primary_subtag_not_a_bare_prefix() {
        // A starts_with() check would have claimed these as Portuguese/Chinese.
        assert_eq!(normalize_language("ptx"), "en-US");
        assert_eq!(normalize_language("zhx-Hant"), "en-US");
    }
}
