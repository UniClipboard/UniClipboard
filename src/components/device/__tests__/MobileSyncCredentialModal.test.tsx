/**
 * MobileSyncCredentialModal —— 关闭路径与 i18n 行为测试。
 */

import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import MobileSyncCredentialModal from '@/components/device/MobileSyncCredentialModal'
import i18n from '@/i18n'

const mockPayload: RegisterMobileDeviceResult = {
  deviceId: 'device-1',
  label: 'My iPhone',
  clientType: 'iphone_shortcut',
  createdAtMs: 1_700_000_000_000,
  baseUrl: 'http://192.168.1.10:42720',
  username: 'user_a',
  password: 'secret-pass',
  installUrl: 'https://www.icloud.com/shortcuts/example',
  qrCodePngBase64: 'iVBORw0KGgo=',
}

const renderWithI18n = (ui: ReactElement) =>
  render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>)

const defaultHandlers = () => ({
  onDiscard: vi.fn(),
  onComplete: vi.fn(),
})

describe('MobileSyncCredentialModal close behavior', () => {
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
    Element.prototype.scrollIntoView = vi.fn()
  })

  afterAll(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  it('renders Done as the footer primary action label', () => {
    const { onDiscard, onComplete } = defaultHandlers()
    renderWithI18n(
      <MobileSyncCredentialModal
        payload={mockPayload}
        onDiscard={onDiscard}
        onComplete={onComplete}
      />
    )

    expect(screen.getByRole('button', { name: 'Done' })).toBeInTheDocument()
  })

  it('blocks Done without acknowledgement and shows closeBlocked hint', async () => {
    const user = userEvent.setup()
    const { onDiscard, onComplete } = defaultHandlers()

    renderWithI18n(
      <MobileSyncCredentialModal
        payload={mockPayload}
        onDiscard={onDiscard}
        onComplete={onComplete}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Done' }))

    expect(onDiscard).not.toHaveBeenCalled()
    expect(onComplete).not.toHaveBeenCalled()
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Confirm "I have saved these credentials" first'
    )
    expect(screen.getByText('Device added')).toBeInTheDocument()
  })

  it('discards via header X without acknowledgement', async () => {
    const user = userEvent.setup()
    const { onDiscard, onComplete } = defaultHandlers()

    renderWithI18n(
      <MobileSyncCredentialModal
        payload={mockPayload}
        onDiscard={onDiscard}
        onComplete={onComplete}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Close' }))

    expect(onDiscard).toHaveBeenCalledTimes(1)
    expect(onDiscard).toHaveBeenCalledWith('device-1')
    expect(onComplete).not.toHaveBeenCalled()
  })

  it('completes via Done after acknowledgement checkbox is checked', async () => {
    const user = userEvent.setup()
    const { onDiscard, onComplete } = defaultHandlers()

    renderWithI18n(
      <MobileSyncCredentialModal
        payload={mockPayload}
        onDiscard={onDiscard}
        onComplete={onComplete}
      />
    )

    await user.click(screen.getByRole('checkbox', { name: /I have saved these credentials/i }))
    await user.click(screen.getByRole('button', { name: 'Done' }))

    expect(onComplete).toHaveBeenCalledTimes(1)
    expect(onDiscard).not.toHaveBeenCalled()
  })

  it('does not render when payload is null', () => {
    const { onDiscard, onComplete } = defaultHandlers()
    renderWithI18n(
      <MobileSyncCredentialModal payload={null} onDiscard={onDiscard} onComplete={onComplete} />
    )

    expect(screen.queryByText('Device added')).not.toBeInTheDocument()
  })
})
