import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { type ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  MobileDeviceView,
  MobileSyncSettingsView,
  RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import MobileDevicePanel from '@/components/device/MobileDevicePanel'
import i18n from '@/i18n'
import { parseConnectUri } from '@/lib/mobileSyncConnectUri'

const mobileSyncMocks = vi.hoisted(() => ({
  updateMobileDevice: vi.fn(),
}))

vi.mock('@/api/tauri-command/mobile_sync', async () => {
  const actual = await vi.importActual<typeof import('@/api/tauri-command/mobile_sync')>(
    '@/api/tauri-command/mobile_sync'
  )
  return {
    ...actual,
    listMobileLanInterfaces: vi.fn(() => Promise.resolve([])),
    updateMobileDevice: mobileSyncMocks.updateMobileDevice,
  }
})

vi.mock('qrcode.react', () => ({
  QRCodeSVG: ({ value, 'aria-label': ariaLabel }: { value: string; 'aria-label'?: string }) => (
    <div aria-label={ariaLabel} data-qr-value={value} />
  ),
}))

const device: MobileDeviceView = {
  deviceId: 'did_phone',
  label: 'Old phone',
  clientType: 'ios_shortcut',
  username: 'mobile_old',
  createdAtMs: 1_700_000_000_000,
  lastSeenAtMs: null,
  lastSeenIp: null,
  reportedName: null,
  reportedOs: null,
}

const settings: MobileSyncSettingsView = {
  enabled: true,
  lanListenEnabled: true,
  lanAdvertiseIp: '192.168.1.10',
  lanPort: 42720,
  lanAdvertiseBaseUrl: null,
  lanListenerError: null,
  shortcutInstallMethods: [],
}

const renderWithI18n = (ui: ReactElement) =>
  render(<I18nextProvider i18n={i18n}>{ui}</I18nextProvider>)

describe('MobileDevicePanel', () => {
  let initialLanguage = 'en-US'

  beforeAll(async () => {
    initialLanguage = i18n.language
    await i18n.changeLanguage('en-US')
  })

  afterAll(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  beforeEach(() => {
    mobileSyncMocks.updateMobileDevice.mockReset()
    mobileSyncMocks.updateMobileDevice.mockResolvedValue({
      deviceId: 'did_phone',
      label: 'New phone',
      username: 'mobile_new',
      password: null,
    })
  })

  it('submits label and username edits without sending password when password is blank', async () => {
    const user = userEvent.setup()
    const onUpdated = vi.fn()

    renderWithI18n(
      <MobileDevicePanel
        device={device}
        settings={settings}
        onRevoke={vi.fn()}
        onRotated={onUpdated}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Edit device' }))
    await user.clear(screen.getByLabelText('Device label'))
    await user.type(screen.getByLabelText('Device label'), 'New phone')
    await user.clear(screen.getByLabelText('Username'))
    await user.type(screen.getByLabelText('Username'), 'mobile_new')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(mobileSyncMocks.updateMobileDevice).toHaveBeenCalledWith({
        deviceId: 'did_phone',
        label: 'New phone',
        username: 'mobile_new',
      })
    })
    expect(onUpdated).toHaveBeenCalledTimes(1)
    expect(await screen.findByRole('heading', { name: 'New phone' })).toBeInTheDocument()
    expect(screen.getByText('mobile_new')).toBeInTheDocument()
  })

  it('requests a generated password when regenerate is selected', async () => {
    const user = userEvent.setup()

    renderWithI18n(
      <MobileDevicePanel
        device={device}
        settings={settings}
        onRevoke={vi.fn()}
        onRotated={vi.fn()}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Edit device' }))
    await user.click(screen.getByRole('button', { name: 'Regenerate' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(mobileSyncMocks.updateMobileDevice).toHaveBeenCalledWith({
        deviceId: 'did_phone',
        label: 'Old phone',
        username: 'mobile_old',
        password: null,
      })
    })
  })

  it('sends the typed password verbatim when a custom password is entered', async () => {
    const user = userEvent.setup()

    renderWithI18n(
      <MobileDevicePanel
        device={device}
        settings={settings}
        onRevoke={vi.fn()}
        onRotated={vi.fn()}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Edit device' }))
    await user.type(screen.getByLabelText('Password'), 's3cret-pass')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(mobileSyncMocks.updateMobileDevice).toHaveBeenCalledWith({
        deviceId: 'did_phone',
        label: 'Old phone',
        username: 'mobile_old',
        password: 's3cret-pass',
      })
    })
  })

  it('maps a USERNAME_TAKEN daemon error to an inline error on the username field', async () => {
    const user = userEvent.setup()
    const onUpdated = vi.fn()
    mobileSyncMocks.updateMobileDevice.mockRejectedValue({
      code: 'USERNAME_TAKEN',
      username: 'mobile_dup',
    })

    renderWithI18n(
      <MobileDevicePanel
        device={device}
        settings={settings}
        onRevoke={vi.fn()}
        onRotated={onUpdated}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Edit device' }))
    await user.clear(screen.getByLabelText('Username'))
    await user.type(screen.getByLabelText('Username'), 'mobile_dup')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    const fieldError = await screen.findByText('Username "mobile_dup" is already taken')
    expect(fieldError).toHaveAttribute('role', 'alert')
    expect(fieldError).toHaveAttribute('id', 'mobile-device-edit-username-error')
    // The username input is wired to the field-error element for assistive tech.
    expect(screen.getByLabelText('Username')).toHaveAttribute(
      'aria-describedby',
      'mobile-device-edit-username-error'
    )
    // A failed submit must not signal a successful rotation to the parent.
    expect(onUpdated).not.toHaveBeenCalled()
    // Still on the edit view (no credential echo on failure).
    expect(screen.getByRole('button', { name: 'Save' })).toBeInTheDocument()
  })

  it('echoes the new credentials and rebuilds the QR when the daemon returns a password', async () => {
    const user = userEvent.setup()
    mobileSyncMocks.updateMobileDevice.mockResolvedValue({
      deviceId: 'did_phone',
      label: 'New phone',
      username: 'mobile_new',
      password: 'gen-PW-123456',
    })

    renderWithI18n(
      <MobileDevicePanel
        device={device}
        settings={settings}
        onRevoke={vi.fn()}
        onRotated={vi.fn()}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Edit device' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))

    // The QR is the pairing hero and rebuilt from the new credentials; decode it
    // back to confirm the payload carries the new username + password (mirrors
    // the Rust connect-uri codec). It renders without expanding anything.
    const qr = await screen.findByLabelText('QR code that auto-fills the sync credentials')
    const payload = parseConnectUri(qr.getAttribute('data-qr-value') ?? '')
    expect(payload.user).toBe('mobile_new')
    expect(payload.pwd).toBe('gen-PW-123456')

    // Credentials + one-time warning live in the collapsed manual-entry fallback
    // (the scan path never needs them). Expand it, then assert the plaintext.
    await user.click(screen.getByRole('button', { name: /Enter the username and password/ }))
    expect(
      await screen.findByText(
        'These credentials are shown only once. Save them before closing this dialog.'
      )
    ).toBeInTheDocument()
    expect(screen.getByText('mobile_new')).toBeInTheDocument()
    // Password is masked until toggled.
    await user.click(screen.getByRole('button', { name: 'Show password' }))
    expect(screen.getByText('gen-PW-123456')).toBeInTheDocument()
  })

  it('keeps the password omitted when a typed password is cleared before saving', async () => {
    const user = userEvent.setup()

    renderWithI18n(
      <MobileDevicePanel
        device={device}
        settings={settings}
        onRevoke={vi.fn()}
        onRotated={vi.fn()}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Edit device' }))
    await user.type(screen.getByLabelText('Password'), 'temp-pass')
    await user.clear(screen.getByLabelText('Password'))
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(mobileSyncMocks.updateMobileDevice).toHaveBeenCalledTimes(1)
    })
    const call = mobileSyncMocks.updateMobileDevice.mock.calls[0][0]
    expect(call).toEqual({
      deviceId: 'did_phone',
      label: 'Old phone',
      username: 'mobile_old',
    })
    // Cleared password must not be forwarded at all (not even as null).
    expect('password' in call).toBe(false)
  })

  it('mounts straight into the guided pairing view when handed add credentials', async () => {
    const user = userEvent.setup()
    const added: RegisterMobileDeviceResult = {
      deviceId: 'did_new',
      label: 'New iPhone',
      clientType: 'ios_shortcut',
      createdAtMs: 1_700_000_000_000,
      baseUrl: 'http://192.168.1.10:42720',
      username: 'user_new',
      password: 'PW-abc-123',
      installUrl: 'https://www.icloud.com/shortcuts/example',
      installQrCodePngBase64: 'aW5zdGFsbFFy',
      connectUri:
        'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjEwOjQyNzIwIn0',
      qrCodeAscii: '',
      qrCodePngBase64: '',
    }

    renderWithI18n(
      <MobileDevicePanel
        device={device}
        settings={settings}
        initialCredential={added}
        onRevoke={vi.fn()}
        onRotated={vi.fn()}
      />
    )

    // Task-framed header + pairing QR appear immediately — no interaction needed.
    expect(screen.getByRole('heading', { name: 'Connect New iPhone' })).toBeInTheDocument()
    expect(
      await screen.findByLabelText('QR code that auto-fills the sync credentials')
    ).toBeInTheDocument()

    // The install helper is an add-only affordance (carries the install QR).
    expect(screen.getByRole('button', { name: /Haven't installed a client/ })).toBeInTheDocument()

    // Credentials stay collapsed in the manual-entry fallback until expanded.
    expect(screen.queryByText('user_new')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: /Enter the username and password/ }))
    expect(screen.getByText('user_new')).toBeInTheDocument()
  })

  it('does not call the daemon and keeps Save disabled for a whitespace-only label', async () => {
    const user = userEvent.setup()

    renderWithI18n(
      <MobileDevicePanel
        device={device}
        settings={settings}
        onRevoke={vi.fn()}
        onRotated={vi.fn()}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Edit device' }))
    await user.clear(screen.getByLabelText('Device label'))
    await user.type(screen.getByLabelText('Device label'), '   ')

    // A blank/whitespace label must block submission: the Save button is disabled
    // and clicking it is a no-op, so the daemon is never reached (the label stays
    // unchanged — "Keep" semantics for an invalid edit).
    const save = screen.getByRole('button', { name: 'Save' })
    expect(save).toBeDisabled()
    await user.click(save)
    expect(mobileSyncMocks.updateMobileDevice).not.toHaveBeenCalled()
  })
})
