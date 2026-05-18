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
  // 阶段 5 起后端多渲染一份 install URL QR, 让"安装快捷指令" tab 能直接扫装。
  // base64 占位字符串与 qrCodePngBase64 故意不同, 测试要断它俩不能串位。
  installQrCodePngBase64: 'aW5zdGFsbFFy',
  // 阶段 2 起 QR 编的是 connectUri (uniclipboard://connect?...) 而非 installUrl。
  // 这里 base64 是占位 — 单测断言 alt 文案而不去解析 PNG 字节。
  connectUri:
    'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjEwOjQyNzIwIn0',
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
  // 阶段 5: tab 按"接入方式"分,「扫码接入」(默认) 主 QR = connect URI,
  // 「安装快捷指令」(次) 主 QR = install URL。这两个 QR 必须图源不串位,
  // 文案也必须与新 i18n keys 对齐。后续误改文案 / 误删 tab / 把两个 QR
  // 接反 (`installQrCodePngBase64` ↔ `qrCodePngBase64`) 都会立刻爆炸。
  it('defaults to the Scan tab with the connect-URI QR', () => {
    const { onDiscard, onComplete } = defaultHandlers()
    renderWithI18n(
      <MobileSyncCredentialModal
        payload={mockPayload}
        onDiscard={onDiscard}
        onComplete={onComplete}
      />
    )

    // Tab triggers visible with new labels.
    expect(screen.getByRole('tab', { name: 'Scan to add' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Install Shortcut' })).toBeInTheDocument()
    // Active tab is "Scan to add" — its panel renders the connect-URI QR.
    const qr = screen.getByAltText('QR code that auto-fills the sync credentials')
    expect(qr.getAttribute('src')).toBe('data:image/png;base64,iVBORw0KGgo=')
    expect(
      screen.getByText('Scan with your phone to auto-fill the credentials')
    ).toBeInTheDocument()
    // Cross-platform note that replaces the old "Android" tab dead end.
    expect(
      screen.getByText(
        'Works with the UniClipboard iOS app and any SyncClipboard-protocol client (including third-party Android apps).'
      )
    ).toBeInTheDocument()
  })

  it('switches to the Install Shortcut tab and shows the install-URL QR + link', async () => {
    const user = userEvent.setup()
    const { onDiscard, onComplete } = defaultHandlers()
    renderWithI18n(
      <MobileSyncCredentialModal
        payload={mockPayload}
        onDiscard={onDiscard}
        onComplete={onComplete}
      />
    )

    await user.click(screen.getByRole('tab', { name: 'Install Shortcut' }))

    // Heading + install-link CredentialField visible.
    expect(screen.getByText('iOS Shortcut — install once on your iPhone')).toBeInTheDocument()
    expect(screen.getByText('Install link')).toBeInTheDocument()
    expect(screen.getByText('https://www.icloud.com/shortcuts/example')).toBeInTheDocument()
    // Install-URL QR comes from the new installQrCodePngBase64 field — must
    // not accidentally fall back to the main connect-URI QR.
    const installQr = screen.getByAltText('QR code for the iCloud shortcut install link')
    expect(installQr.getAttribute('src')).toBe('data:image/png;base64,aW5zdGFsbFFy')
  })

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
