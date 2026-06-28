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
    expect(i18n.t('setup.welcome.title')).toBe('开始使用')
    expect(i18n.t('setup.page.loadingSetupState')).toBe('正在加载初始化状态...')
  })

  it('resolves en-US setup.welcome.title', async () => {
    await i18n.changeLanguage('en-US')
    expect(i18n.t('setup.welcome.title')).toBe('Get started')
    expect(i18n.t('setup.page.loadingSetupState')).toBe('Loading setup state...')
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

  it('contains display labels for every built-in history tag', async () => {
    await i18n.changeLanguage('zh-CN')
    expect(i18n.t('history.type.link')).toBe('链接')
    expect(i18n.t('history.type.code')).toBe('代码')
    expect(i18n.t('history.type.favorited')).toBe('收藏')

    await i18n.changeLanguage('en-US')
    expect(i18n.t('history.type.link')).toBe('Link')
    expect(i18n.t('history.type.code')).toBe('Code')
    expect(i18n.t('history.type.favorited')).toBe('Favorites')
  })
})
