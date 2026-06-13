import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { type ReactElement } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import type { MobileDeviceView, MobileSyncSettingsView } from '@/api/tauri-command/mobile_sync'
import MobileSyncDeviceDialog from '@/components/device/MobileSyncDeviceDialog'
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

describe('MobileSyncDeviceDialog', () => {
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
      <MobileSyncDeviceDialog
        open
        onOpenChange={vi.fn()}
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
      <MobileSyncDeviceDialog
        open
        onOpenChange={vi.fn()}
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
      <MobileSyncDeviceDialog
        open
        onOpenChange={vi.fn()}
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
      <MobileSyncDeviceDialog
        open
        onOpenChange={vi.fn()}
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
      <MobileSyncDeviceDialog
        open
        onOpenChange={vi.fn()}
        device={device}
        settings={settings}
        onRevoke={vi.fn()}
        onRotated={vi.fn()}
      />
    )

    await user.click(screen.getByRole('button', { name: 'Edit device' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))

    // The one-time credential warning surfaces after the daemon echoes a password.
    expect(
      await screen.findByText(
        'These credentials are shown only once. Save them before closing this dialog.'
      )
    ).toBeInTheDocument()

    // Credential rows render the freshly minted username + password (password is
    // masked until toggled, so reveal it before asserting the plaintext row).
    // Scope to the "Credentials" section so the username row is not confused with
    // the dialog's username subtitle.
    const credentialsSection = screen.getByText('Credentials').closest('section')
    expect(credentialsSection).not.toBeNull()
    const credentials = within(credentialsSection as HTMLElement)
    expect(credentials.getByText('mobile_new')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Show password' }))
    expect(credentials.getByText('gen-PW-123456')).toBeInTheDocument()

    // The QR is rebuilt from the new credentials; decode it back to confirm the
    // payload carries the new username + password (mirrors the Rust connect-uri codec).
    const qr = await screen.findByLabelText('QR code that auto-fills the sync credentials')
    const payload = parseConnectUri(qr.getAttribute('data-qr-value') ?? '')
    expect(payload.user).toBe('mobile_new')
    expect(payload.pwd).toBe('gen-PW-123456')
  })

  it('keeps the password omitted when a typed password is cleared before saving', async () => {
    const user = userEvent.setup()

    renderWithI18n(
      <MobileSyncDeviceDialog
        open
        onOpenChange={vi.fn()}
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

  it('does not call the daemon and keeps Save disabled for a whitespace-only label', async () => {
    const user = userEvent.setup()

    renderWithI18n(
      <MobileSyncDeviceDialog
        open
        onOpenChange={vi.fn()}
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
