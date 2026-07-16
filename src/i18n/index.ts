import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import enUS from './locales/en-US.json'
import ruRU from './locales/ru-RU.json'
import zhCN from './locales/zh-CN.json'

export const SUPPORTED_LANGUAGES = ['zh-CN', 'en-US', 'ru-RU'] as const
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number]

const STORAGE_KEY = 'uniclipboard.language'

export function isSupportedLanguage(language: unknown): language is SupportedLanguage {
  return SUPPORTED_LANGUAGES.includes(language as SupportedLanguage)
}

export function normalizeLanguage(language: string | null | undefined): SupportedLanguage {
  if (!language) {
    // Fallback to system language
    language = navigator.language
  }
  const lower = language.toLowerCase()
  if (lower.startsWith('zh')) return 'zh-CN'
  if (lower.startsWith('ru')) return 'ru-RU'
  return 'en-US'
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
  },
  lng: getInitialLanguage(),
  fallbackLng: 'zh-CN',
  interpolation: { escapeValue: false },
})

persistLanguage(i18n.language as SupportedLanguage)

export default i18n
