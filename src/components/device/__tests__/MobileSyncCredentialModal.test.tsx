/**
 * MobileSyncCredentialModal —— 关闭路径、骨架结构、备份操作的行为测试。
 *
 * 设计后的 modal 取消了:
 * - "扫码接入 / 安装快捷指令" Tab 切换
 * - "下载 → 配对" stepper
 * - 顶部黄色警告横幅 (融入凭据折叠区)
 * - 「我已保存」勾选门槛
 * - X = "撤销" (X 改为 onComplete, 撤销下沉到 DevicesPage)
 *
 * 维护提醒: 这里断言的文案必须与 src/i18n/locales/en-US.json 里
 * devices.mobileSync.credential.* 的 key 一一对应。删/改 i18n key 时
 * 同步删/改对应断言, 否则 CI 会以 "Unable to find an element with..."
 * 失败。
 */

import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { type ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import MobileSyncCredentialModal from '@/components/device/MobileSyncCredentialModal'
import i18n from '@/i18n'

// Modal mounts a LAN interface lookup so the user can switch which IP the
// connect-URI QR points to. The lookup hits Tauri commands at runtime — stub
// with an empty list to keep these tests focused on close/skeleton behavior
// (empty list → modal falls back to the read-only baseUrl chip, no dropdown).
vi.mock('@/api/tauri-command/mobile_sync', async () => {
  const actual = await vi.importActual<typeof import('@/api/tauri-command/mobile_sync')>(
    '@/api/tauri-command/mobile_sync'
  )
  return {
    ...actual,
    listMobileLanInterfaces: vi.fn(() => Promise.resolve([])),
  }
})

const mockPayload: RegisterMobileDeviceResult = {
  deviceId: 'device-1',
  label: 'My phone',
  clientType: 'ios_shortcut',
  createdAtMs: 1_700_000_000_000,
  baseUrl: 'http://192.168.1.10:42720',
  username: 'user_a',
  password: 'secret-pass',
  installUrl: 'https://www.icloud.com/shortcuts/example',
  installQrCodePngBase64: 'aW5zdGFsbFFy',
  connectUri:
    'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjEwOjQyNzIwIn0',
  qrCodePngBase64: 'iVBORw0KGgo=',
}

const renderWithI18n = (ui: ReactElement) =>
  render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>)

describe('MobileSyncCredentialModal', () => {
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

  // userEvent v14 ships its own in-memory clipboard; the modal calls
  // `navigator.clipboard.writeText` directly (not through userEvent.copy()).
  // Ensure navigator.clipboard exists in jsdom, then spy on its writeText for
  // each test — vi.restoreAllMocks() in afterEach is implicit via vitest's
  // default mockReset setting in this project.
  let writeTextSpy: ReturnType<typeof vi.spyOn>
  beforeEach(() => {
    if (!('clipboard' in navigator)) {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: { writeText: () => Promise.resolve() },
      })
    }
    writeTextSpy = vi
      .spyOn(navigator.clipboard, 'writeText')
      .mockImplementation(() => Promise.resolve())
  })

  it('does not render when payload is null', () => {
    const onComplete = vi.fn()
    renderWithI18n(<MobileSyncCredentialModal payload={null} onComplete={onComplete} />)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('renders title with the device label and the pair QR', () => {
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    // Title interpolates {{label}} from the payload.
    expect(screen.getByText('Scan to connect My phone')).toBeInTheDocument()
    // Default sub-step is gone — modal now lands directly on pair QR.
    expect(
      screen.getByLabelText('QR code that auto-fills the sync credentials')
    ).toBeInTheDocument()
  })

  it('closes via the footer Done button', async () => {
    const user = userEvent.setup()
    const onComplete = vi.fn()
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={onComplete} />)

    await user.click(screen.getByRole('button', { name: 'Done' }))

    expect(onComplete).toHaveBeenCalledTimes(1)
  })

  // X is now wired to onComplete — the discard-and-revoke path was removed
  // (it now lives on the device card's revoke button on DevicesPage). This
  // test guards against accidental regressions where someone re-routes X
  // back to a "discard" callback.
  it('closes via the header X button (no longer discards)', async () => {
    const user = userEvent.setup()
    const onComplete = vi.fn()
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={onComplete} />)

    // X uses a distinct aria-label ("Close") from the footer ("Done") so
    // assistive tech can tell them apart — both close the dialog but the
    // visual roles differ (header icon vs footer primary).
    await user.click(screen.getByRole('button', { name: 'Close' }))

    expect(onComplete).toHaveBeenCalledTimes(1)
  })

  it('closes via Escape (no acknowledgement gate)', async () => {
    const user = userEvent.setup()
    const onComplete = vi.fn()
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={onComplete} />)

    await user.keyboard('{Escape}')

    expect(onComplete).toHaveBeenCalledTimes(1)
  })

  // ── Skeleton: two collapsibles default closed ────────────────────────
  it('keeps both collapsibles closed by default', () => {
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    // Collapsible triggers are visible (headers).
    expect(screen.getByText("Haven't installed a client?")).toBeInTheDocument()
    expect(screen.getByText('Credentials')).toBeInTheDocument()

    // Content inside is not visible until expanded.
    expect(screen.queryByText('Install UniClipboard App (TestFlight)')).not.toBeInTheDocument()
    // The Username label only appears inside the (default-closed) credentials
    // collapsible content — guard against accidental "always show" regressions.
    expect(screen.queryByText('Username')).not.toBeInTheDocument()
  })

  // The "no client" section uses Tabs (iOS / Android) with a scan-to-download
  // QR as the primary action of each tab. The iOS tab additionally exposes
  // a secondary "shortcut fallback" link (Android doesn't need one — uc-android
  // speaks SyncClipboard directly).
  it('expands the "no client" section and shows iOS as the default tab', async () => {
    const user = userEvent.setup()
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    await user.click(screen.getByRole('button', { name: /Haven't installed a client/i }))

    // Both tab triggers visible.
    expect(screen.getByRole('tab', { name: 'iOS' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Android' })).toBeInTheDocument()

    // Default iOS tab — scan caption + browser button + shortcut fallback link.
    expect(screen.getByText('Scan to install UniClipboard App (TestFlight)')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Open TestFlight in browser/i })).toBeInTheDocument()
    expect(screen.getByText(/Don't want the App/)).toBeInTheDocument()

    // Android caption is NOT in the DOM yet (tabs are mutually exclusive).
    expect(screen.queryByText('Scan to download APK (GitHub Releases)')).not.toBeInTheDocument()
  })

  it('switches the no-client section to Android tab on click', async () => {
    const user = userEvent.setup()
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    await user.click(screen.getByRole('button', { name: /Haven't installed a client/i }))
    await user.click(screen.getByRole('tab', { name: 'Android' }))

    expect(screen.getByText('Scan to download APK (GitHub Releases)')).toBeInTheDocument()
    expect(
      screen.getByRole('button', { name: /Open GitHub Releases in browser/i })
    ).toBeInTheDocument()
    // iOS-only "shortcut fallback" must NOT leak into the Android tab.
    expect(screen.queryByText(/Don't want the App/)).not.toBeInTheDocument()
  })

  it('expands the credentials section to reveal username + password fields', async () => {
    const user = userEvent.setup()
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    // The collapsible trigger lives inside the amber warning panel; click the
    // text "Credentials" rather than the panel itself.
    await user.click(screen.getByRole('button', { name: /Credentials/i }))

    expect(screen.getByText('Username')).toBeInTheDocument()
    expect(screen.getByText('user_a')).toBeInTheDocument()
    expect(screen.getByText('Password')).toBeInTheDocument()
    // Password is masked by default — assert the masked rendering, not plain.
    expect(screen.queryByText('secret-pass')).not.toBeInTheDocument()
  })

  // ── Backup button: copies the three-line human-readable format ──────
  // This is the load-bearing escape hatch for "I closed the modal without
  // saving the password" — the only one-click way to grab all three fields.
  it('copies the three-line backup to clipboard when Backup is clicked', async () => {
    const user = userEvent.setup()
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    // The Backup button is in the collapsible header, visible without expanding.
    await user.click(screen.getByRole('button', { name: 'Backup' }))

    expect(writeTextSpy).toHaveBeenCalledTimes(1)
    expect(writeTextSpy).toHaveBeenCalledWith(
      'Server: http://192.168.1.10:42720\nUsername: user_a\nPassword: secret-pass'
    )
  })

  // The backup button's onClick uses e.stopPropagation() so clicking it does
  // not also toggle the collapsible (would flash the user/pwd fields and lose
  // the masked-password protection for a moment).
  it('clicking Backup does not toggle the credentials collapsible', async () => {
    const user = userEvent.setup()
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    await user.click(screen.getByRole('button', { name: 'Backup' }))

    // Still closed — Username field not in DOM.
    expect(screen.queryByText('Username')).not.toBeInTheDocument()
  })

  // ── BaseUrl dropdown: always show, even when backend list is empty ────
  // Original bug: when listMobileLanInterfaces() returned [] (single-NIC
  // machine where daemon filtered everything, or a permissions/timing fail),
  // the chip silently fell back to a read-only span — user lost the visual
  // affordance to switch. Now dropdownInterfaces always falls back to
  // payloadHost so the dropdown stays visible.
  it('renders the dropdown even when backend returns no LAN interfaces (uses payloadHost fallback)', () => {
    // listMobileLanInterfaces mock returns [] by default in this file.
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    // Radix Select trigger announces itself as a combobox; presence of this
    // role under the modal's i18n aria-label confirms the dropdown rendered.
    expect(screen.getByRole('combobox', { name: 'Select server URL' })).toBeInTheDocument()
  })

  it('shows the dropdown when there is exactly one LAN interface from the backend', async () => {
    const mod = await import('@/api/tauri-command/mobile_sync')
    vi.mocked(mod.listMobileLanInterfaces).mockResolvedValueOnce([
      { name: 'en0', ipv4: '192.168.1.10' },
    ])
    renderWithI18n(<MobileSyncCredentialModal payload={mockPayload} onComplete={vi.fn()} />)

    // Single-IP case: dropdown still rendered (UI consistency over strict
    // "nothing to switch" silence). Threshold was relaxed from > 1 to > 0.
    expect(await screen.findByRole('combobox', { name: 'Select server URL' })).toBeInTheDocument()
  })
})
