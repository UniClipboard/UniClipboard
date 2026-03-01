import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import i18n from '@/i18n'

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
    expect(i18n.t('setup.welcome.title')).toBe('欢迎使用 UniClipboard')
    expect(i18n.t('setup.page.loadingSetupState')).toBe('正在加载初始化状态...')
  })

  it('resolves en-US setup.welcome.title', async () => {
    await i18n.changeLanguage('en-US')
    expect(i18n.t('setup.welcome.title')).toBe('Welcome to UniClipboard')
    expect(i18n.t('setup.page.loadingSetupState')).toBe('Loading setup state...')
  })
})
