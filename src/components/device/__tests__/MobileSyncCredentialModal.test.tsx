/**
 * MobileSyncCredentialModal —— 关闭路径与 i18n 行为测试。
 */

import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState, type ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import MobileSyncCredentialModal from '@/components/device/MobileSyncCredentialModal'
import i18n from '@/i18n'

const mockPayload: RegisterMobileDeviceResult = {
  deviceId: 'device-1',
  label: 'My iPhone',
  clientType: 'ios_shortcut',
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
  const originalScrollIntoView = Element.prototype.scrollIntoView

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
    Element.prototype.scrollIntoView = originalScrollIntoView
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

    await user.click(screen.getByRole('button', { name: 'Discard registration' }))

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

  it('does not discard on Escape without clicking the header dismiss button', async () => {
    const user = userEvent.setup()
    const { onDiscard, onComplete } = defaultHandlers()

    renderWithI18n(
      <MobileSyncCredentialModal
        payload={mockPayload}
        onDiscard={onDiscard}
        onComplete={onComplete}
      />
    )

    await user.keyboard('{Escape}')

    expect(onDiscard).not.toHaveBeenCalled()
    expect(onComplete).not.toHaveBeenCalled()
    expect(screen.getByText('Device added')).toBeInTheDocument()
  })

  it('does not render when payload is null', () => {
    const { onDiscard, onComplete } = defaultHandlers()
    renderWithI18n(
      <MobileSyncCredentialModal payload={null} onDiscard={onDiscard} onComplete={onComplete} />
    )

    expect(screen.queryByText('Device added')).not.toBeInTheDocument()
  })

  // 父组件 DevicesPage 的 discardCredential 进入函数后立刻把 payload 清空
  // (乐观清空),避免连点 ✕ 触发第二次 revoke 拿到 DEVICE_NOT_FOUND。这里
  // 用一个 wrapper 复现该 contract,验证 modal 在 payload 清空后不再响应。
  it('drops the second rapid X click once parent clears payload optimistically', async () => {
    const user = userEvent.setup()
    const onDiscard = vi.fn<(deviceId: string) => void>()
    const onComplete = vi.fn()

    const Wrapper = () => {
      const [payload, setPayload] = useState<RegisterMobileDeviceResult | null>(mockPayload)
      return (
        <MobileSyncCredentialModal
          payload={payload}
          onDiscard={deviceId => {
            setPayload(null)
            onDiscard(deviceId)
          }}
          onComplete={onComplete}
        />
      )
    }

    renderWithI18n(<Wrapper />)

    const xButton = screen.getByRole('button', { name: 'Discard registration' })
    await user.click(xButton)
    await user.click(xButton)

    expect(onDiscard).toHaveBeenCalledTimes(1)
    expect(onDiscard).toHaveBeenCalledWith('device-1')
    expect(screen.queryByRole('button', { name: 'Discard registration' })).not.toBeInTheDocument()
  })
})
