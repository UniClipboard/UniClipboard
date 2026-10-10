//! The language the panel's own text is in.
//!
//! There is one current language for the whole process, like the main window's single i18next
//! instance: error messages are produced by `Display` implementations, which cannot be handed a
//! language. The app sets it from the settings; everything else only reads it.

use std::sync::atomic::{AtomicU8, Ordering};

/// The languages the main window ships, which the panel has text for as well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Language {
    ZhCn,
    ZhTw,
    EnUs,
    JaJp,
    RuRu,
    PtBr,
}

/// Simplified Chinese until the app has resolved the configured or system language at startup.
static CURRENT: AtomicU8 = AtomicU8::new(Language::ZhCn as u8);

const ALL: [Language; 6] = [
    Language::ZhCn,
    Language::ZhTw,
    Language::EnUs,
    Language::JaJp,
    Language::RuRu,
    Language::PtBr,
];

impl Language {
    /// The shipped language a language tag selects.
    ///
    /// Matches case-insensitively on the primary subtag and accepts both BCP-47 (`pt-BR`) and
    /// POSIX (`pt_BR`) separators. Traditional Chinese tags (`zh-Hant`, `zh-TW`, `zh-HK`, `zh-MO`)
    /// select [`Self::ZhTw`]; other Chinese tags select [`Self::ZhCn`]. Japanese, Russian and
    /// Portuguese region variants collapse onto their bundle. Anything without a bundle is
    /// [`Self::EnUs`].
    ///
    /// Keep the supported set in sync with `SUPPORTED_LANGUAGES` in `apps/gui-go/frontend/src/i18n/index.ts`,
    /// including the frontend's subtag fallbacks in `normalizeLanguage()`.
    pub fn for_locale(locale: &str) -> Self {
        let mut subtags = locale.split(['-', '_']);
        let primary = subtags.next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("zh") {
            return if subtags.any(|subtag| {
                matches!(
                    subtag.to_ascii_lowercase().as_str(),
                    "hant" | "tw" | "hk" | "mo"
                )
            }) {
                Self::ZhTw
            } else {
                Self::ZhCn
            };
        }

        match primary.to_ascii_lowercase().as_str() {
            "ja" => Self::JaJp,
            "ru" => Self::RuRu,
            "pt" => Self::PtBr,
            _ => Self::EnUs,
        }
    }

    pub fn current() -> Self {
        let value = CURRENT.load(Ordering::Relaxed);
        ALL.into_iter()
            .find(|language| *language as u8 == value)
            .unwrap_or(Self::ZhCn)
    }

    /// Makes this the language of all text produced from now on.
    pub fn make_current(self) {
        CURRENT.store(self as u8, Ordering::Relaxed);
    }

    /// Whether typed words are also read as pinyin initials (`tp` for 图片).
    pub fn reads_pinyin_initials(self) -> bool {
        matches!(self, Self::ZhCn | Self::ZhTw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_region_variants_onto_their_bundle() {
        assert_eq!(Language::for_locale("zh-CN"), Language::ZhCn);
        assert_eq!(Language::for_locale("zh-TW"), Language::ZhTw);
        assert_eq!(Language::for_locale("zh-Hant"), Language::ZhTw);
        assert_eq!(Language::for_locale("zh-HK"), Language::ZhTw);
        assert_eq!(Language::for_locale("zh-MO"), Language::ZhTw);
        assert_eq!(Language::for_locale("zh-SG"), Language::ZhCn);
        assert_eq!(Language::for_locale("ja-JP"), Language::JaJp);
        assert_eq!(Language::for_locale("ru-BY"), Language::RuRu);
        assert_eq!(Language::for_locale("pt-BR"), Language::PtBr);
        // European Portuguese has no bundle; Brazilian copy beats falling back to English.
        assert_eq!(Language::for_locale("pt-PT"), Language::PtBr);
    }

    #[test]
    fn falls_back_to_english_without_a_bundle() {
        assert_eq!(Language::for_locale("fr-FR"), Language::EnUs);
        assert_eq!(Language::for_locale("en-US"), Language::EnUs);
        assert_eq!(Language::for_locale(""), Language::EnUs);
    }

    #[test]
    fn ignores_case_and_accepts_posix_separators() {
        assert_eq!(Language::for_locale("JA_jp"), Language::JaJp);
        assert_eq!(Language::for_locale("RU-ru"), Language::RuRu);
        assert_eq!(Language::for_locale("PT"), Language::PtBr);
        assert_eq!(Language::for_locale("pt_BR"), Language::PtBr);
        assert_eq!(Language::for_locale("zh_CN"), Language::ZhCn);
        assert_eq!(Language::for_locale("ZH_tw"), Language::ZhTw);
    }

    #[test]
    fn matches_the_primary_subtag_not_a_bare_prefix() {
        // A starts_with() check would have claimed these as Portuguese/Chinese.
        assert_eq!(Language::for_locale("ptx"), Language::EnUs);
        assert_eq!(Language::for_locale("zhx-Hant"), Language::EnUs);
    }
}
