import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import i18n, { SUPPORTED_LANGUAGES, normalizeLanguage } from '@/i18n'
import enUS from '@/i18n/locales/en-US.json'
import ptBR from '@/i18n/locales/pt-BR.json'
import ruRU from '@/i18n/locales/ru-RU.json'
import zhCN from '@/i18n/locales/zh-CN.json'

/** Flatten a nested resource bundle into dotted key paths. */
function flatten(node: unknown, prefix = '', out: Record<string, string> = {}) {
  for (const [key, value] of Object.entries(node as Record<string, unknown>)) {
    const path = prefix ? `${prefix}.${key}` : key
    if (value && typeof value === 'object') flatten(value, path, out)
    else out[path] = value as string
  }
  return out
}

/** i18next plural suffixes; `_zero` is an i18next extension, not an Intl category. */
const PLURAL_SUFFIX = /_(zero|one|two|few|many|other)$/

describe('locale bundle parity', () => {
  const bundles: Record<string, Record<string, string>> = {
    'zh-CN': flatten(zhCN),
    'en-US': flatten(enUS),
    'ru-RU': flatten(ruRU),
    'pt-BR': flatten(ptBR),
  }

  it('covers every non-plural en-US key in each locale', () => {
    const expected = Object.keys(bundles['en-US']).filter(k => !PLURAL_SUFFIX.test(k))
    for (const [locale, bundle] of Object.entries(bundles)) {
      const missing = expected.filter(k => !(k in bundle))
      expect(missing, `${locale} is missing keys`).toEqual([])
    }
  })

  it('has no keys beyond what en-US declares', () => {
    const known = new Set(Object.keys(bundles['en-US']).map(k => k.replace(PLURAL_SUFFIX, '')))
    for (const [locale, bundle] of Object.entries(bundles)) {
      const extra = Object.keys(bundle).filter(k => !known.has(k.replace(PLURAL_SUFFIX, '')))
      expect(extra, `${locale} declares unknown keys`).toEqual([])
    }
  })

  it('supplies every plural form each locale grammatically requires', () => {
    // Russian needs one/few/many/other where English only needs one/other, so a
    // bundle copied key-for-key from en-US would silently fall back for 2-4 items.
    // Portuguese likewise adds `many` on top of one/other.
    const groups = new Set(
      Object.keys(bundles['en-US'])
        .filter(k => PLURAL_SUFFIX.test(k))
        .map(k => k.replace(PLURAL_SUFFIX, ''))
    )
    for (const [locale, bundle] of Object.entries(bundles)) {
      const categories = new Intl.PluralRules(locale).resolvedOptions().pluralCategories
      for (const group of groups) {
        const missing = categories.filter(c => !(`${group}_${c}` in bundle))
        expect(missing, `${locale} ${group} is missing plural forms`).toEqual([])
      }
    }
  })

  it('keeps interpolation placeholders identical to the en-US source', () => {
    const placeholders = (value: string): string[] => [...(value.match(/{{[^}]+}}/g) ?? [])].sort()
    for (const [locale, bundle] of Object.entries(bundles)) {
      if (locale === 'en-US') continue
      for (const [key, value] of Object.entries(bundle)) {
        // A plural form inherits its placeholders from the en-US `_other` form,
        // which is the one English uses whenever a count is rendered.
        const source = PLURAL_SUFFIX.test(key)
          ? bundles['en-US'][`${key.replace(PLURAL_SUFFIX, '')}_other`]
          : bundles['en-US'][key]
        if (typeof source !== 'string') continue
        const extra = placeholders(value).filter(p => !placeholders(source).includes(p))
        expect(extra, `${locale} ${key} uses placeholders absent from en-US`).toEqual([])
      }
    }
  })
})

describe('setup i18n keys', () => {
  let initialLanguage: string

  const ensureI18nInitialized = async () => {
    if (i18n.isInitialized) return

    await new Promise<void>(resolve => {
      const handler = () => {
        i18n.off('initialized', handler)
        resolve()
      }
      i18n.on('initialized', handler)
    })
  }

  beforeAll(async () => {
    await ensureI18nInitialized()
    initialLanguage = i18n.language
  })

  afterEach(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  it('resolves zh-CN setup.welcome.title', async () => {
    await i18n.changeLanguage('zh-CN')
    expect(i18n.t('setup.welcome.title')).toBe('开始使用')
    expect(i18n.t('setup.page.loadingSetupState')).toBe('正在加载初始化状态...')
  })

  it('resolves en-US setup.welcome.title', async () => {
    await i18n.changeLanguage('en-US')
    expect(i18n.t('setup.welcome.title')).toBe('Get started')
    expect(i18n.t('setup.page.loadingSetupState')).toBe('Loading setup state...')
  })

  it('resolves ru-RU setup.welcome.title', async () => {
    await i18n.changeLanguage('ru-RU')
    expect(i18n.t('setup.welcome.title')).toBe('Начало работы')
    expect(i18n.t('setup.page.loadingSetupState')).toBe('Загрузка состояния настройки...')
  })

  it('resolves pt-BR setup.welcome.title', async () => {
    await i18n.changeLanguage('pt-BR')
    expect(i18n.t('setup.welcome.title')).toBe('Vamos começar')
    expect(i18n.t('setup.page.loadingSetupState')).toBe('Carregando o estado da configuração...')
  })

  it('contains pairing failure copy in both locales', async () => {
    await i18n.changeLanguage('zh-CN')
    expect(i18n.t('pairing.failed.errors.activeSession')).toBe('已有正在进行的配对，请稍后再试')
    expect(i18n.t('pairing.failed.errors.noParticipant')).toBe('本地没有可确认配对的设备参与者')
    expect(i18n.t('pairing.failed.errors.sessionExpired')).toBe('配对会话已过期或已关闭')
    expect(i18n.t('pairing.failed.errors.daemonUnavailable')).toBe(
      '配对 daemon 不可用，请启动桌面服务后重试'
    )

    await i18n.changeLanguage('en-US')
    expect(i18n.t('pairing.failed.errors.activeSession')).toBe(
      'Another pairing session is already in progress'
    )
    expect(i18n.t('pairing.failed.errors.noParticipant')).toBe(
      'No local device is ready to confirm pairing'
    )
    expect(i18n.t('pairing.failed.errors.sessionExpired')).toBe(
      'The pairing session expired or was already closed'
    )
    expect(i18n.t('pairing.failed.errors.daemonUnavailable')).toBe(
      'The pairing daemon is unavailable. Start the desktop service and try again'
    )
  })

  it('normalizes language tags to the nearest supported locale', () => {
    expect(normalizeLanguage('zh')).toBe('zh-CN')
    expect(normalizeLanguage('zh-TW')).toBe('zh-CN')
    expect(normalizeLanguage('ru')).toBe('ru-RU')
    expect(normalizeLanguage('ru-BY')).toBe('ru-RU')
    expect(normalizeLanguage('RU-ru')).toBe('ru-RU')
    expect(normalizeLanguage('pt')).toBe('pt-BR')
    expect(normalizeLanguage('pt-BR')).toBe('pt-BR')
    // pt-BR is the only Portuguese bundle, so European Portuguese lands here too.
    expect(normalizeLanguage('pt-PT')).toBe('pt-BR')
    expect(normalizeLanguage('fr-FR')).toBe('en-US')
    // POSIX locale envs use an underscore; BCP-47 uses a hyphen.
    expect(normalizeLanguage('pt_BR')).toBe('pt-BR')
    expect(normalizeLanguage('zh_CN')).toBe('zh-CN')
    // The primary subtag decides, not a bare prefix: "ptx" is not Portuguese.
    expect(normalizeLanguage('ptx')).toBe('en-US')
    expect(normalizeLanguage('zhx-Hant')).toBe('en-US')
  })

  it('offers a self-named picker label for every supported language', async () => {
    // Each locale must name every language in that language's own words, so the
    // picker stays readable no matter which locale is active.
    const autonyms: Record<string, string> = {
      'zh-CN': '简体中文',
      'en-US': 'English',
      'ru-RU': 'Русский',
      'pt-BR': 'Português (Brasil)',
    }
    for (const active of SUPPORTED_LANGUAGES) {
      await i18n.changeLanguage(active)
      for (const listed of SUPPORTED_LANGUAGES) {
        expect(i18n.t(`language.${listed}`)).toBe(autonyms[listed])
      }
    }
  })

  it('contains display labels for every built-in history tag', async () => {
    await i18n.changeLanguage('zh-CN')
    expect(i18n.t('history.type.richtext')).toBe('富文本')
    expect(i18n.t('history.type.link')).toBe('链接')
    expect(i18n.t('history.type.code')).toBe('代码')
    expect(i18n.t('history.type.favorited')).toBe('收藏')

    await i18n.changeLanguage('en-US')
    expect(i18n.t('history.type.richtext')).toBe('Rich Text')
    expect(i18n.t('history.type.link')).toBe('Link')
    expect(i18n.t('history.type.code')).toBe('Code')
    expect(i18n.t('history.type.favorited')).toBe('Favorites')
  })
})
