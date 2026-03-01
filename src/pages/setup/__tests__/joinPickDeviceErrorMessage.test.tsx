import { render, screen } from '@testing-library/react'
import type { ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import i18n from '@/i18n'
import JoinPickDeviceStep from '@/pages/setup/JoinPickDeviceStep'

const renderWithI18n = (ui: ReactElement) =>
  render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>)

describe('JoinPickDeviceStep error message mapping', () => {
  let initialLanguage = 'en-US'

  const ensureI18nInitialized = async () => {
    if (i18n.isInitialized) {
      return
    }
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
    await i18n.changeLanguage('zh-CN')
  })

  afterAll(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  it('shows pairing rejected message instead of load peers failure', () => {
    renderWithI18n(
      <JoinPickDeviceStep
        onSelectPeer={() => undefined}
        onBack={() => undefined}
        onRefresh={() => undefined}
        peers={[]}
        error="PairingRejected"
      />
    )

    expect(screen.getByText('对方设备已拒绝此次配对请求。')).toBeInTheDocument()
    expect(screen.queryByText('获取设备列表失败。')).not.toBeInTheDocument()
  })

  it('shows pairing failed message instead of load peers failure', () => {
    renderWithI18n(
      <JoinPickDeviceStep
        onSelectPeer={() => undefined}
        onBack={() => undefined}
        onRefresh={() => undefined}
        peers={[]}
        error="PairingFailed"
      />
    )

    expect(screen.getByText('对方设备未同意或未及时响应配对请求，请重试。')).toBeInTheDocument()
    expect(screen.queryByText('获取设备列表失败。')).not.toBeInTheDocument()
  })
})
