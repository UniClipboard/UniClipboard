/**
 * EnableMobileSyncDialog —— i18n smoke 测试。
 *
 * 验证组件能渲染出翻译过的文案、不暴露 raw i18n key,并能正确接受 props。
 * 深度行为(点 confirm → updateMobileSyncSettings 调用 → onSuccess 触发)
 * 涉及 mock Tauri command + AlertDialog portal, ROI 偏低; 同链路在
 * `uc-application` phase 3 的 facade 测试已用 RecordingLanLifecycle 覆盖。
 */

import { render, screen } from '@testing-library/react'
import type { ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import EnableMobileSyncDialog from '@/components/device/EnableMobileSyncDialog'
import i18n from '@/i18n'

const renderWithI18n = (ui: ReactElement) =>
  render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>)

describe('EnableMobileSyncDialog i18n', () => {
  let initialLanguage = 'en-US'

  beforeAll(async () => {
    if (!i18n.isInitialized) {
      await new Promise<void>(resolve => {
        const handler = () => {
          i18n.off('initialized', handler)
          resolve()
        }
        i18n.on('initialized', handler)
      })
    }
    initialLanguage = i18n.language
    await i18n.changeLanguage('en-US')
  })

  afterAll(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  it('renders translated title + confirm button when open', () => {
    renderWithI18n(
      <EnableMobileSyncDialog open onOpenChange={() => undefined} onSuccess={() => undefined} />
    )

    expect(screen.getByText('Enable mobile sync')).toBeInTheDocument()
    expect(screen.getByText('Enable and continue')).toBeInTheDocument()
    expect(screen.getByText('Cancel')).toBeInTheDocument()

    // 不暴露 raw i18n key
    expect(screen.queryByText('enableConfirm.title')).not.toBeInTheDocument()
    expect(screen.queryByText('enableConfirm.confirm')).not.toBeInTheDocument()
  })

  it('renders the default LAN port (42720) inside the body copy', () => {
    renderWithI18n(
      <EnableMobileSyncDialog open onOpenChange={() => undefined} onSuccess={() => undefined} />
    )

    // body 文案带 {{port}} 插值, 默认是 42720
    expect(screen.getByText(/42720/)).toBeInTheDocument()
  })

  it('does not render dialog content when closed', () => {
    renderWithI18n(
      <EnableMobileSyncDialog
        open={false}
        onOpenChange={() => undefined}
        onSuccess={() => undefined}
      />
    )

    expect(screen.queryByText('Enable mobile sync')).not.toBeInTheDocument()
  })
})
