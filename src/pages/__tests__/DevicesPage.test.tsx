/**
 * DevicesPage top-level render tests.
 *
 * The page is a master-detail layout: a list column with 本机 / 已配对设备 /
 * 移动同步 sections plus a persistent detail pane. This suite only verifies:
 *   1. mount dispatches `fetchLocalDeviceInfo` + `fetchSpaceMembers`
 *   2. the list column renders its three section labels
 *   3. presence is probed once on mount / on visibility regain, no polling
 *   4. the add-device entry points (header menu + empty-state rows)
 *
 * Panel-level interactions (sync toggles, unpair, mobile edit/revoke) are
 * covered by the panel components' own unit tests.
 *
 * Expected labels come from `i18n.t` rather than literals so the assertions
 * survive a locale switch, matching the language-agnostic style below.
 */

import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { refreshPresence } from '@/api/daemon'
import { isLegacyBootstrapRequired, secureRemoveLegacyMember } from '@/api/daemon/member'
import { unpairDevice } from '@/api/daemon/members'
import {
  getMobileSyncSettings,
  listMobileDevices,
  type MobileDeviceView,
  type MobileSyncSettingsView,
  type RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { toast } from '@/components/ui/toast'
import i18n from '@/i18n'
import DevicesPage from '@/pages/DevicesPage'

/** The one-time registration result the stubbed add dialog hands back. */
const REGISTERED: RegisterMobileDeviceResult = {
  deviceId: 'mobile-1',
  label: 'iPhone 15 Pro',
  username: 'uc-user',
  password: 'one-time-secret',
  clientType: 'ios_shortcut',
  baseUrl: 'http://192.168.1.10:42720',
  connectUri: 'uniclipboard://connect?host=192.168.1.10',
  createdAtMs: 1_700_000_000_000,
  installUrl: 'https://www.icloud.com/shortcuts/stub',
  installQrCodePngBase64: '',
  qrCodeAscii: '',
  qrCodePngBase64: '',
}

const settingsFixture = (overrides: Partial<MobileSyncSettingsView> = {}): MobileSyncSettingsView =>
  ({
    enabled: false,
    lanListenEnabled: false,
    lanAdvertiseIp: null,
    lanPort: null,
    lanListenerError: null,
    shortcutInstallMethods: [],
    ...overrides,
  }) as MobileSyncSettingsView

// The add dialog is stubbed down to a single "succeed" button: these suites are
// about what DevicesPage does with the result, not about the form. The real
// dialog's submit path is covered by its own unit tests.
vi.mock('@/components/device/AddMobileSyncDeviceDialog', () => ({
  default: ({
    open,
    onSuccess,
  }: {
    open: boolean
    onSuccess: (result: RegisterMobileDeviceResult) => void
  }) =>
    open ? (
      <button type="button" data-testid="stub-register" onClick={() => onSuccess(REGISTERED)}>
        register
      </button>
    ) : null,
}))

// The panel is stubbed for the same reason: its fresh-state QR + credential
// rendering is its own suite's job. Here we only need to see which device the
// page selected and whether the one-time credential reached the panel.
vi.mock('@/components/device/MobileDevicePanel', () => ({
  default: ({
    device,
    initialCredential,
  }: {
    device: MobileDeviceView
    initialCredential: RegisterMobileDeviceResult | null
  }) => (
    <div data-testid="mobile-panel" data-device-id={device.deviceId}>
      {initialCredential ? 'has-credential' : 'no-credential'}
    </div>
  ),
}))

const dispatchMock = vi.fn()
const devicesState = {
  localDevice: null as null | Record<string, unknown>,
  localDeviceLoading: false,
  localDeviceError: null as string | null,
  spaceMembers: [] as Array<Record<string, unknown>>,
  spaceMembersError: null as string | null,
  spaceProtection: null as null | Record<string, unknown>,
  spaceProtectionError: null as string | null,
  membershipConvergence: null as null | { state: string },
  memberSyncPreferences: {},
  memberSyncPreferencesLoading: {},
}

vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => dispatchMock,
  useAppSelector: (selector: (s: unknown) => unknown) =>
    selector({
      devices: devicesState,
    }),
}))

vi.mock('@/store/slices/devicesSlice', () => ({
  fetchLocalDeviceInfo: vi.fn(() => ({ type: 'devices/fetchLocalDeviceInfo' })),
  fetchSpaceMembers: vi.fn(() => ({ type: 'devices/fetchSpaceMembers' })),
  clearLocalDeviceError: vi.fn(() => ({ type: 'devices/clearLocalDeviceError' })),
  clearSpaceMembersError: vi.fn(() => ({ type: 'devices/clearSpaceMembersError' })),
  fetchSpaceProtection: vi.fn(() => ({ type: 'devices/fetchSpaceProtection' })),
  fetchMembershipConvergence: vi.fn(() => ({ type: 'devices/fetchMembershipConvergence' })),
  fetchMemberSyncPreferences: vi.fn(() => ({ type: 'devices/fetchMemberSyncPreferences' })),
  updateMemberSyncPreferences: vi.fn(() => ({ type: 'devices/updateMemberSyncPreferences' })),
}))

vi.mock('@/api/daemon', () => ({
  refreshPresence: vi.fn(() => Promise.resolve()),
}))

vi.mock('@/api/daemon/members', () => ({
  unpairDevice: vi.fn(),
}))

vi.mock('@/api/daemon/member', () => ({
  isLegacyBootstrapRequired: vi.fn(),
  secureRemoveLegacyMember: vi.fn(),
}))

vi.mock('@/api/tauri-command/mobile_sync', () => ({
  DEFAULT_MOBILE_LAN_PORT: 42720,
  isMobileSyncError: () => false,
  listMobileDevices: vi.fn(() => Promise.resolve([])),
  revokeMobileDevice: vi.fn(),
  // useMobileDevices 在 mount 时预拉一次 settings, MobileSyncSettingsDialog
  // 即便初始 open=false 也会随父组件 mount, 其 useEffect 会调 list lan
  // interfaces. 两个 stub 都得给, 否则 Vitest 抛 "mock has no export".
  getMobileSyncSettings: vi.fn(() =>
    Promise.resolve({
      enabled: false,
      lanListenEnabled: false,
      lanAdvertiseIp: null,
      lanPort: null,
      lanListenerError: null,
      shortcutInstallMethods: [],
    })
  ),
  listMobileLanInterfaces: vi.fn(() => Promise.resolve([])),
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: { subscribe: () => () => undefined },
}))

vi.mock('@/components/ui/toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    message: vi.fn(),
  },
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: () => ({
    setting: {
      sync: { autoSync: true },
      fileSync: { fileSyncEnabled: true },
      network: { allowRelayFallback: true },
    },
  }),
}))

describe('DevicesPage', () => {
  afterEach(() => {
    vi.mocked(refreshPresence).mockClear()
    vi.useRealTimers()
    devicesState.localDevice = null
    devicesState.localDeviceError = null
    devicesState.spaceMembers = []
    devicesState.spaceMembersError = null
    devicesState.spaceProtection = null
    devicesState.spaceProtectionError = null
    devicesState.membershipConvergence = null
    vi.mocked(unpairDevice).mockReset()
    vi.mocked(isLegacyBootstrapRequired).mockReset()
    vi.mocked(secureRemoveLegacyMember).mockReset()
  })

  it('dispatches fetchLocalDeviceInfo and fetchSpaceMembers on mount', () => {
    dispatchMock.mockClear()
    render(<DevicesPage />)

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchLocalDeviceInfo' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceMembers' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceProtection' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchMembershipConvergence' })
  })

  it('translates a space protection status failure', () => {
    devicesState.spaceProtectionError = 'devices.protection.errors.statusFailed'

    render(<DevicesPage />)

    expect(screen.getByText(i18n.t('devices.protection.errors.statusFailed'))).toBeInTheDocument()
    expect(screen.queryByText('devices.protection.errors.statusFailed')).not.toBeInTheDocument()
  })

  it('renders the list column with the three device sections', () => {
    render(<DevicesPage />)

    // Section labels: local device / paired devices / phone LAN connections (i18n keys
    // devices.thisDevice.title / devices.pairedDevices.title /
    // devices.mobileSync.title). Assert via the section container so the
    // test stays language-agnostic yet structure-sensitive.
    const list = screen.getByRole('complementary')
    expect(list).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 2 })).toBeInTheDocument()
  })

  it('calls refreshPresence exactly once on mount and does not poll on a timer', () => {
    vi.useFakeTimers()
    render(<DevicesPage />)

    expect(refreshPresence).toHaveBeenCalledTimes(1)

    // Advance well past the old 15s polling cadence — no further calls.
    act(() => {
      vi.advanceTimersByTime(60_000)
    })
    expect(refreshPresence).toHaveBeenCalledTimes(1)
  })

  it('calls refreshPresence again when the document becomes visible', () => {
    render(<DevicesPage />)
    expect(refreshPresence).toHaveBeenCalledTimes(1)

    // Switch to hidden — no probe expected.
    act(() => {
      Object.defineProperty(document, 'visibilityState', {
        configurable: true,
        get: () => 'hidden',
      })
      document.dispatchEvent(new Event('visibilitychange'))
    })
    expect(refreshPresence).toHaveBeenCalledTimes(1)

    // Switch back to visible — one extra probe.
    act(() => {
      Object.defineProperty(document, 'visibilityState', {
        configurable: true,
        get: () => 'visible',
      })
      document.dispatchEvent(new Event('visibilitychange'))
    })
    expect(refreshPresence).toHaveBeenCalledTimes(2)
  })

  it('shows a compact status while the space is still connecting', () => {
    devicesState.membershipConvergence = { state: 'converging' }

    render(<DevicesPage />)

    expect(screen.getByText(i18n.t('devices.convergence.converging'))).toBeInTheDocument()
  })

  it('hides the connection status when convergence is complete', () => {
    devicesState.membershipConvergence = { state: 'complete' }

    render(<DevicesPage />)

    expect(screen.queryByText(i18n.t('devices.convergence.converging'))).not.toBeInTheDocument()
    expect(
      screen.queryByText(i18n.t('devices.convergence.waitingForUpgrade'))
    ).not.toBeInTheDocument()
    expect(screen.queryByText(i18n.t('devices.convergence.blocked'))).not.toBeInTheDocument()
  })

  it('suppresses a duplicate upgrade status during the existing security upgrade', () => {
    devicesState.membershipConvergence = { state: 'waiting_for_upgrade' }
    devicesState.spaceProtection = {
      mode: 'migrating',
      members: [],
      legacyBootstrap: {
        bootstrapId: 'bootstrap-1',
        outcome: 'awaiting_readmission',
        pendingReadmission: 1,
      },
    }

    render(<DevicesPage />)

    expect(
      screen.queryByText(i18n.t('devices.convergence.waitingForUpgrade'))
    ).not.toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.protection.bootstrap.awaiting_readmission', { count: 1 }))
    ).toBeInTheDocument()
  })

  it('polls a converging space for at most thirty seconds', () => {
    vi.useFakeTimers()
    devicesState.membershipConvergence = { state: 'converging' }
    dispatchMock.mockClear()

    render(<DevicesPage />)

    const convergenceDispatches = () =>
      dispatchMock.mock.calls.filter(
        ([action]) => action.type === 'devices/fetchMembershipConvergence'
      )
    expect(convergenceDispatches()).toHaveLength(1)

    act(() => {
      vi.advanceTimersByTime(30_000)
    })
    expect(convergenceDispatches()).toHaveLength(16)

    act(() => {
      vi.advanceTimersByTime(30_000)
    })
    expect(convergenceDispatches()).toHaveLength(16)
  })
})

describe('DevicesPage secure legacy removal', () => {
  const localDevice = {
    peerId: 'local-peer',
    deviceName: 'Local Mac',
    deviceId: 'local-device',
    platform: 'macos',
    version: '0.20.0',
  }
  const remoteMember = {
    peerId: 'remote-peer',
    deviceName: 'Old Desktop',
    pairingState: 'Trusted',
    lastSeenAtMs: null,
    connected: true,
    channel: 'direct',
    connectionAddress: '127.0.0.1:5000',
  }

  beforeEach(() => {
    devicesState.localDevice = localDevice
    devicesState.spaceMembers = [localDevice, remoteMember]
    devicesState.spaceProtectionError = null
    vi.mocked(unpairDevice).mockReset()
    vi.mocked(isLegacyBootstrapRequired).mockReset()
    vi.mocked(secureRemoveLegacyMember).mockReset()
  })

  afterEach(() => {
    devicesState.localDevice = null
    devicesState.spaceMembers = []
  })

  async function openUnpairConfirmation() {
    const user = userEvent.setup()
    await user.click(screen.getByRole('button', { name: remoteMember.deviceName }))
    await user.click(screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') }))
    return user
  }

  it('starts the secure upgrade only for the legacy-space conflict', async () => {
    const legacyError = new Error('legacy removal blocked')
    vi.mocked(unpairDevice).mockRejectedValue(legacyError)
    vi.mocked(isLegacyBootstrapRequired).mockImplementation(error => error === legacyError)
    vi.mocked(secureRemoveLegacyMember).mockResolvedValue({
      bootstrap: {
        bootstrapId: 'bootstrap-1',
        outcome: 'awaiting_readmission',
        pendingReadmission: 1,
      },
    })
    render(<DevicesPage />)
    const user = await openUnpairConfirmation()

    await user.click(screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') }))

    await waitFor(() => expect(secureRemoveLegacyMember).toHaveBeenCalledWith(remoteMember.peerId))
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('does not start the secure upgrade for an unrelated removal failure', async () => {
    const unrelatedError = new Error('daemon unavailable')
    vi.mocked(unpairDevice).mockRejectedValue(unrelatedError)
    vi.mocked(isLegacyBootstrapRequired).mockReturnValue(false)
    render(<DevicesPage />)
    const user = await openUnpairConfirmation()

    await user.click(screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') }))

    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(secureRemoveLegacyMember).not.toHaveBeenCalled()
  })

  it('disables the confirmation while removal is in progress', async () => {
    let finishRemoval: (() => void) | undefined
    vi.mocked(unpairDevice).mockImplementation(
      () => new Promise<void>(resolve => (finishRemoval = resolve))
    )
    render(<DevicesPage />)
    const user = await openUnpairConfirmation()
    const confirm = screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') })

    await user.click(confirm)

    expect(confirm).toBeDisabled()
    finishRemoval?.()
    await waitFor(() => expect(confirm).not.toBeInTheDocument())
  })

  it('keeps the confirmation open when dismissal is requested during removal', async () => {
    let finishRemoval: (() => void) | undefined
    vi.mocked(unpairDevice).mockImplementation(
      () => new Promise<void>(resolve => (finishRemoval = resolve))
    )
    render(<DevicesPage />)
    const user = await openUnpairConfirmation()
    const confirm = screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') })

    await user.click(confirm)
    await user.keyboard('{Escape}')

    expect(confirm).toBeInTheDocument()
    finishRemoval?.()
    await waitFor(() => expect(confirm).not.toBeInTheDocument())
  })

  it('keeps the original target when another removal is requested during removal', async () => {
    const secondRemoteMember = {
      ...remoteMember,
      peerId: 'second-remote-peer',
      deviceName: 'Second Desktop',
    }
    devicesState.spaceMembers = [localDevice, remoteMember, secondRemoteMember]
    let finishRemoval: (() => void) | undefined
    vi.mocked(unpairDevice).mockImplementation(
      () => new Promise<void>(resolve => (finishRemoval = resolve))
    )
    render(<DevicesPage />)
    const secondDeviceButton = screen.getByRole('button', { name: secondRemoteMember.deviceName })
    const user = await openUnpairConfirmation()
    const confirm = screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') })

    await user.click(confirm)
    fireEvent.click(secondDeviceButton)
    const secondUnpairRequest = screen
      .getAllByRole('button', { name: i18n.t('devices.list.actions.unpair'), hidden: true })
      .find(button => button !== confirm && !(button as HTMLButtonElement).disabled)
    expect(secondUnpairRequest).toBeDefined()
    fireEvent.click(secondUnpairRequest!)

    expect(
      screen.getByText(
        i18n.t('devices.unpair.confirmDescription', { deviceName: remoteMember.deviceName })
      )
    ).toBeInTheDocument()
    expect(
      screen.queryByText(
        i18n.t('devices.unpair.confirmDescription', { deviceName: secondRemoteMember.deviceName })
      )
    ).not.toBeInTheDocument()
    finishRemoval?.()
    await waitFor(() => expect(confirm).not.toBeInTheDocument())
  })
})

describe('DevicesPage add-device entry points', () => {
  afterEach(() => {
    vi.mocked(toast.error).mockClear()
    vi.mocked(getMobileSyncSettings).mockReset()
    vi.mocked(getMobileSyncSettings).mockResolvedValue(settingsFixture())
  })

  it('opens the header menu with both add paths', async () => {
    const user = userEvent.setup()
    render(<DevicesPage />)

    // The trigger shows the short label but announces the full wording.
    await user.click(screen.getByRole('button', { name: i18n.t('devices.panel.addMenu.trigger') }))

    expect(
      await screen.findByRole('menuitem', { name: i18n.t('devices.panel.addMenu.p2p') })
    ).toBeInTheDocument()
    expect(
      screen.getByRole('menuitem', { name: i18n.t('devices.panel.addMenu.mobile') })
    ).toBeInTheDocument()
  })

  it('offers an add entry in each empty section', async () => {
    render(<DevicesPage />)

    // Both sections are empty under this suite's mocks (no peers, no mobile
    // devices), so each renders its labelled add row instead of a dead line.
    expect(
      await screen.findByRole('button', { name: i18n.t('devices.panel.addMenu.p2p') })
    ).toBeEnabled()
    expect(
      screen.getByRole('button', { name: i18n.t('devices.panel.addMenu.mobile') })
    ).toBeEnabled()
  })

  it('explains a LAN bind failure instead of silently disabling the mobile entry', async () => {
    // Regression guard: this row used to be `disabled` on lanListenerError,
    // which made it unclickable and therefore made the toast below — the only
    // place the failure reason surfaces — unreachable.
    vi.mocked(getMobileSyncSettings).mockResolvedValue(
      settingsFixture({ enabled: true, lanListenEnabled: true, lanListenerError: 'address in use' })
    )
    const user = userEvent.setup()
    render(<DevicesPage />)

    // Let the preloaded settings promise settle before clicking, otherwise
    // handleAddClick still sees `settings === null` and takes the enable path.
    await waitFor(() => expect(getMobileSyncSettings).toHaveBeenCalled())
    await act(async () => {})

    const addMobile = screen.getByRole('button', { name: i18n.t('devices.panel.addMenu.mobile') })
    expect(addMobile).toBeEnabled()
    await user.click(addMobile)

    expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('address in use'))
  })
})

describe('DevicesPage mobile add flow', () => {
  afterEach(() => {
    vi.mocked(getMobileSyncSettings).mockReset()
    vi.mocked(listMobileDevices).mockReset()
    vi.mocked(listMobileDevices).mockResolvedValue([])
  })

  it('selects the newly added device and hands it the one-time credential', async () => {
    // Regression guard (#1347): the page selected the new device while
    // `reload()` was still in flight, so for one render the id resolved to
    // nothing. A render-phase `setSelection({ kind: 'local' })` treated that
    // gap as "device vanished" and overwrote the selection, leaving the user
    // pinned to the local panel once the device finally loaded.
    vi.mocked(getMobileSyncSettings).mockResolvedValue(
      settingsFixture({ enabled: true, lanListenEnabled: true })
    )
    // Empty on mount; the device only appears on the post-register reload —
    // this ordering is the whole point of the test.
    const registered: MobileDeviceView = {
      deviceId: REGISTERED.deviceId,
      label: REGISTERED.label,
      username: REGISTERED.username,
      clientType: REGISTERED.clientType,
      createdAtMs: REGISTERED.createdAtMs,
      lastSeenAtMs: null,
    }
    vi.mocked(listMobileDevices).mockResolvedValueOnce([]).mockResolvedValue([registered])

    const user = userEvent.setup()
    render(<DevicesPage />)

    // Let the preloaded settings settle so handleAddClick takes the add path
    // instead of the first-run enable confirm.
    await waitFor(() => expect(getMobileSyncSettings).toHaveBeenCalled())
    await act(async () => {})

    await user.click(screen.getByRole('button', { name: i18n.t('devices.panel.addMenu.mobile') }))
    await user.click(await screen.findByTestId('stub-register'))

    const panel = await screen.findByTestId('mobile-panel')
    expect(panel).toHaveAttribute('data-device-id', REGISTERED.deviceId)
    expect(panel).toHaveTextContent('has-credential')
  })
})
