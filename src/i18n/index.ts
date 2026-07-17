import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import enUS from './locales/en-US.json'
import ptBR from './locales/pt-BR.json'
import ruRU from './locales/ru-RU.json'
import zhCN from './locales/zh-CN.json'

export const SUPPORTED_LANGUAGES = ['zh-CN', 'en-US', 'ru-RU', 'pt-BR'] as const
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number]

const STORAGE_KEY = 'uniclipboard.language'

export function isSupportedLanguage(language: unknown): language is SupportedLanguage {
  return SUPPORTED_LANGUAGES.includes(language as SupportedLanguage)
}

/**
 * Every region variant collapses onto the one bundle that covers it — including
 * pt-PT, since Brazilian copy serves a Portuguese speaker better than English.
 *
 * Keep in sync with `normalize_language` in `src-tauri/crates/uc-tauri/src/tray.rs`.
 */
const LOCALE_BY_SUBTAG: Partial<Record<string, SupportedLanguage>> = {
  zh: 'zh-CN',
  ru: 'ru-RU',
  pt: 'pt-BR',
}

export function normalizeLanguage(language: string | null | undefined): SupportedLanguage {
  // Fall back to the system language when the caller has no stored preference.
  const tag = language || navigator.language
  // Accept both separators: BCP-47 hands us "pt-BR", POSIX locale envs "pt_BR".
  const primary = tag.split(/[-_]/)[0].toLowerCase()
  return LOCALE_BY_SUBTAG[primary] ?? 'en-US'
}

export function getInitialLanguage(): SupportedLanguage {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (isSupportedLanguage(stored)) return stored
  return normalizeLanguage(navigator.language)
}

export function persistLanguage(language: SupportedLanguage) {
  localStorage.setItem(STORAGE_KEY, language)
}

i18n.use(initReactI18next).init({
  resources: {
    'zh-CN': { translation: zhCN },
    'en-US': { translation: enUS },
    'ru-RU': { translation: ruRU },
    'pt-BR': { translation: ptBR },
  },
  lng: getInitialLanguage(),
  fallbackLng: 'zh-CN',
  interpolation: { escapeValue: false },
})

persistLanguage(i18n.language as SupportedLanguage)

export default i18n
