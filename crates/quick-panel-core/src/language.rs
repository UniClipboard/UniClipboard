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
    /// The language of a locale as `normalize_language` in `crates/uc-desktop/src/language.rs`
    /// returns it. That function already falls back to `en-US`, so does this.
    pub fn for_locale(locale: &str) -> Self {
        match locale {
            "zh-CN" => Self::ZhCn,
            "zh-TW" => Self::ZhTw,
            "ja-JP" => Self::JaJp,
            "ru-RU" => Self::RuRu,
            "pt-BR" => Self::PtBr,
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
