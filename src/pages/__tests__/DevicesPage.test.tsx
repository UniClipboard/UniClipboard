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

import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { refreshPresence } from '@/api/daemon'
import type { MemberRemoval, SharedDeviceRefreshSnapshot } from '@/api/daemon/member'
import {
  continueMemberRemoval,
  getCurrentMemberRemoval,
  getSharedDeviceRefresh,
  isLegacyBootstrapRequired,
  isMemberRemovalBlocked,
  isSharedDeviceRefreshNotFound,
  secureRemoveLegacyMember,
  startSharedDeviceRefresh,
} from '@/api/daemon/member'
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

const memberRemoval: MemberRemoval = {
  revocationId: 'removal-1',
  outcome: 'applied',
  pendingRecipients: 1,
  removedDeviceIds: ['removed-device'],
  pendingRecipientDeviceIds: ['retained-device'],
  updatedAtMs: 1_700_000_000_000,
}

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
type DaemonEventHandler = (event: { topic: string; eventType: string; payload: unknown }) => void
const daemonWsMocks = vi.hoisted(() => ({
  subscribe: vi.fn((_topics: string[], _handler: DaemonEventHandler) => () => undefined),
  onReconnect: vi.fn(() => () => undefined),
}))
const devicesState = {
  localDevice: null as null | Record<string, unknown>,
  localDeviceLoading: false,
  localDeviceError: null as string | null,
  spaceMembers: [] as Array<Record<string, unknown>>,
  spaceMembersError: null as string | null,
  spaceProtection: null as null | Record<string, unknown>,
  spaceProtectionError: null as string | null,
  membershipConvergence: null as null | { state: string },
  membershipConvergenceError: null as string | null,
  networkRecovery: null as null | { phase: string; retryable: boolean },
  networkRecoveryError: null as string | null,
  networkRecoveryRequestId: null as string | null,
  memberRemoval: null as MemberRemoval | null,
  memberRemovalError: null as string | null,
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
  fetchNetworkRecoveryStatus: vi.fn(() => ({ type: 'devices/fetchNetworkRecoveryStatus' })),
  fetchCurrentMemberRemoval: vi.fn(() => ({ type: 'devices/fetchCurrentMemberRemoval' })),
  requestNetworkRecovery: vi.fn(() => ({ type: 'devices/requestNetworkRecovery' })),
  setSpaceMembers: vi.fn((members: unknown[]) => ({ type: 'devices/setSpaceMembers', members })),
  setMemberRemoval: vi.fn((removal: unknown) => ({ type: 'devices/setMemberRemoval', removal })),
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
  continueMemberRemoval: vi.fn(),
  getCurrentMemberRemoval: vi.fn(),
  getSharedDeviceRefresh: vi.fn(),
  isLegacyBootstrapRequired: vi.fn(),
  isMemberRemovalBlocked: vi.fn(),
  isSharedDeviceRefreshNotFound: vi.fn(() => false),
  secureRemoveLegacyMember: vi.fn(),
  startSharedDeviceRefresh: vi.fn(),
}))

vi.mock('@/api/daemon/network-recovery', () => ({
  recoverNetwork: vi.fn(),
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
  daemonWs: { subscribe: daemonWsMocks.subscribe, onReconnect: daemonWsMocks.onReconnect },
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
    devicesState.membershipConvergenceError = null
    devicesState.networkRecovery = null
    devicesState.networkRecoveryError = null
    devicesState.networkRecoveryRequestId = null
    devicesState.memberRemoval = null
    devicesState.memberRemovalError = null
    daemonWsMocks.subscribe.mockClear()
    daemonWsMocks.onReconnect.mockClear()
    vi.mocked(listMobileDevices).mockReset()
    vi.mocked(listMobileDevices).mockResolvedValue([])
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => 'visible',
    })
    vi.mocked(unpairDevice).mockReset()
    vi.mocked(continueMemberRemoval).mockReset()
    vi.mocked(getCurrentMemberRemoval).mockReset()
    vi.mocked(getSharedDeviceRefresh).mockReset()
    vi.mocked(isLegacyBootstrapRequired).mockReset()
    vi.mocked(isMemberRemovalBlocked).mockReset()
    vi.mocked(secureRemoveLegacyMember).mockReset()
    vi.mocked(startSharedDeviceRefresh).mockReset()
    vi.mocked(toast.error).mockClear()
  })

  it('dispatches initial device and member-removal lookups on mount', () => {
    dispatchMock.mockClear()
    render(<DevicesPage />)

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchLocalDeviceInfo' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceMembers' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchCurrentMemberRemoval' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceProtection' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchMembershipConvergence' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchNetworkRecoveryStatus' })
  })

  it('translates a space protection status failure', () => {
    devicesState.spaceProtectionError = 'devices.protection.errors.statusFailed'

    render(<DevicesPage />)

    expect(screen.getByText(i18n.t('devices.protection.errors.statusFailed'))).toBeInTheDocument()
    expect(screen.queryByText('devices.protection.errors.statusFailed')).not.toBeInTheDocument()
  })

  it('shows the member-removal status failure and retries its lookup', async () => {
    devicesState.memberRemovalError = 'devices.memberRemoval.errors.statusFailed'
    const user = userEvent.setup()
    render(<DevicesPage />)

    expect(
      screen.getByText(i18n.t('devices.memberRemoval.errors.statusFailed'))
    ).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: i18n.t('devices.list.actions.retry') }))

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchCurrentMemberRemoval' })
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

  it('does not refresh convergence from websocket events while hidden', () => {
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => 'hidden',
    })
    render(<DevicesPage />)
    const handler = daemonWsMocks.subscribe.mock.calls.at(-1)?.[1]
    expect(handler).toBeTypeOf('function')
    dispatchMock.mockClear()

    const peer = {
      peerId: 'peer-1',
      deviceName: 'Remote Mac',
      connected: true,
      pairingState: 'Trusted',
      channel: 'direct' as const,
      connectionAddress: '192.0.2.10:443',
    }

    act(() => {
      handler?.({ topic: 'peers', eventType: 'peers.changed', payload: { peers: [peer] } })
    })

    expect(dispatchMock).toHaveBeenCalledWith({
      type: 'devices/setSpaceMembers',
      members: [
        {
          peerId: peer.peerId,
          deviceName: peer.deviceName,
          pairingState: peer.pairingState,
          lastSeenAtMs: null,
          connected: true,
          channel: peer.channel,
          connectionAddress: peer.connectionAddress,
        },
      ],
    })
    expect(dispatchMock).not.toHaveBeenCalledWith({ type: 'devices/fetchMembershipConvergence' })
  })

  it('refreshes convergence from websocket events while visible', () => {
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => 'visible',
    })
    render(<DevicesPage />)
    const handler = daemonWsMocks.subscribe.mock.calls.at(-1)?.[1]
    expect(handler).toBeTypeOf('function')
    dispatchMock.mockClear()

    act(() => {
      handler?.({ topic: 'peers', eventType: 'peers.changed', payload: { peers: [] } })
    })

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/setSpaceMembers', members: [] })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchMembershipConvergence' })
  })

  it('refreshes member-removal progress and devices after a removal status notification', () => {
    render(<DevicesPage />)
    const handler = daemonWsMocks.subscribe.mock.calls.at(-1)?.[1]
    dispatchMock.mockClear()

    act(() => {
      handler?.({ topic: 'member-removal', eventType: 'member-removal.changed', payload: {} })
    })

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchCurrentMemberRemoval' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceMembers' })
  })

  it('only offers manual recovery after a retryable final failure', async () => {
    devicesState.networkRecovery = { phase: 'failed', retryable: true }
    const user = userEvent.setup()
    render(<DevicesPage />)

    expect(screen.getByText(i18n.t('devices.networkRecovery.failed'))).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: i18n.t('devices.networkRecovery.retry') }))

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/requestNetworkRecovery' })
  })

  it('keeps the manual recovery action hidden while an automatic retry is scheduled', () => {
    devicesState.networkRecovery = { phase: 'retryScheduled', retryable: true }
    render(<DevicesPage />)

    expect(screen.getByText(i18n.t('devices.networkRecovery.retryScheduled'))).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: i18n.t('devices.networkRecovery.retry') })
    ).not.toBeInTheDocument()
  })

  it('keeps routine convergence invisible while it is still connecting', () => {
    devicesState.membershipConvergence = { state: 'converging' }

    render(<DevicesPage />)

    expect(screen.queryByText(i18n.t('devices.convergence.converging'))).not.toBeInTheDocument()
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

  it('keeps a blocked connection visible when the mobile device query fails', async () => {
    devicesState.membershipConvergence = { state: 'blocked' }
    vi.mocked(listMobileDevices).mockRejectedValueOnce(new Error('mobile query failed'))

    render(<DevicesPage />)

    expect(await screen.findByText(i18n.t('devices.convergence.blocked'))).toBeInTheDocument()
  })

  it('hides stale connection status only when its own request fails', () => {
    devicesState.membershipConvergence = { state: 'blocked' }
    devicesState.membershipConvergenceError = 'membership query failed'

    render(<DevicesPage />)

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

  it('stops convergence polling when complete or hidden', () => {
    vi.useFakeTimers()
    devicesState.membershipConvergence = { state: 'converging' }
    dispatchMock.mockClear()

    const { rerender } = render(<DevicesPage />)
    const convergenceDispatches = () =>
      dispatchMock.mock.calls.filter(
        ([action]) => action.type === 'devices/fetchMembershipConvergence'
      )

    act(() => {
      vi.advanceTimersByTime(2_000)
    })
    expect(convergenceDispatches()).toHaveLength(2)

    devicesState.membershipConvergence = { state: 'complete' }
    rerender(<DevicesPage />)
    act(() => {
      vi.advanceTimersByTime(30_000)
    })
    expect(convergenceDispatches()).toHaveLength(2)

    devicesState.membershipConvergence = { state: 'converging' }
    act(() => {
      Object.defineProperty(document, 'visibilityState', {
        configurable: true,
        get: () => 'hidden',
      })
      document.dispatchEvent(new Event('visibilitychange'))
    })
    rerender(<DevicesPage />)
    act(() => {
      vi.advanceTimersByTime(30_000)
    })
    expect(convergenceDispatches()).toHaveLength(2)
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
    vi.mocked(continueMemberRemoval).mockReset()
    vi.mocked(getCurrentMemberRemoval).mockReset()
    vi.mocked(isLegacyBootstrapRequired).mockReset()
    vi.mocked(isMemberRemovalBlocked).mockReset()
    vi.mocked(secureRemoveLegacyMember).mockReset()
    vi.mocked(toast.error).mockClear()
  })

  afterEach(() => {
    devicesState.localDevice = null
    devicesState.spaceMembers = []
    devicesState.spaceProtection = null
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

  it('recovers the active removal when another removal is already in progress', async () => {
    const inProgress = new Error('member removal in progress')
    vi.mocked(unpairDevice).mockRejectedValue(inProgress)
    vi.mocked(isMemberRemovalBlocked).mockReturnValue(true)
    vi.mocked(getCurrentMemberRemoval).mockResolvedValue(memberRemoval)
    render(<DevicesPage />)
    const user = await openUnpairConfirmation()

    await user.click(screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') }))

    await waitFor(() => expect(getCurrentMemberRemoval).toHaveBeenCalled())
    expect(dispatchMock).toHaveBeenCalledWith({
      type: 'devices/setMemberRemoval',
      removal: memberRemoval,
    })
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('shows an error when active-removal recovery cannot load its status', async () => {
    const inProgress = new Error('member removal in progress')
    vi.mocked(unpairDevice).mockRejectedValue(inProgress)
    vi.mocked(isMemberRemovalBlocked).mockReturnValue(true)
    vi.mocked(getCurrentMemberRemoval).mockRejectedValue(new Error('offline'))
    render(<DevicesPage />)
    const user = await openUnpairConfirmation()

    await user.click(screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') }))

    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(i18n.t('devices.memberRemoval.errors.statusFailed'))
    )
  })

  it('loads the blocked removal when it requires security recovery', async () => {
    const recoveryRequired = new Error('member removal requires recovery')
    const blockedRemoval = { ...memberRemoval, outcome: 'recovery_required' as const }
    vi.mocked(unpairDevice).mockRejectedValue(recoveryRequired)
    vi.mocked(isMemberRemovalBlocked).mockReturnValue(true)
    vi.mocked(getCurrentMemberRemoval).mockResolvedValue(blockedRemoval)
    render(<DevicesPage />)
    const user = await openUnpairConfirmation()

    await user.click(screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') }))

    await waitFor(() => expect(getCurrentMemberRemoval).toHaveBeenCalled())
    expect(dispatchMock).toHaveBeenCalledWith({
      type: 'devices/setMemberRemoval',
      removal: blockedRemoval,
    })
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('cancels an offline readmission directly from the security-upgrade status', async () => {
    const requiresReadmissionMember = {
      ...remoteMember,
      peerId: 'requires-readmission-peer',
      deviceName: 'New Desktop',
    }
    devicesState.spaceMembers = [localDevice, remoteMember, requiresReadmissionMember]
    devicesState.spaceProtection = {
      mode: 'ready',
      members: [
        { deviceId: remoteMember.peerId, status: 'awaiting_readmission' },
        { deviceId: requiresReadmissionMember.peerId, status: 'requires_readmission' },
      ],
      legacyBootstrap: {
        bootstrapId: 'bootstrap-1',
        outcome: 'awaiting_readmission',
        pendingReadmission: 2,
      },
    }
    const completedRemoval: MemberRemoval = {
      ...memberRemoval,
      revocationId: null,
      outcome: 'complete',
      pendingRecipients: 0,
      pendingRecipientDeviceIds: [],
    }
    vi.mocked(unpairDevice).mockResolvedValue(completedRemoval)
    render(<DevicesPage />)
    const user = userEvent.setup()
    const upgradeAlert = screen.getByRole('alert')

    expect(within(upgradeAlert).getByText(remoteMember.deviceName)).toBeInTheDocument()
    expect(within(upgradeAlert).getByText(requiresReadmissionMember.deviceName)).toBeInTheDocument()
    await user.click(
      within(upgradeAlert).getAllByRole('button', {
        name: i18n.t('devices.list.actions.unpair'),
      })[0]
    )

    expect(
      screen.getByText(
        i18n.t('devices.unpair.confirmDescription', { deviceName: remoteMember.deviceName })
      )
    ).toBeInTheDocument()
    const dialog = screen.getByRole('alertdialog')
    await user.click(
      within(dialog).getByRole('button', { name: i18n.t('devices.list.actions.unpair') })
    )

    await waitFor(() => expect(unpairDevice).toHaveBeenCalledWith(remoteMember.peerId))
    expect(dispatchMock).toHaveBeenCalledWith({
      type: 'devices/setMemberRemoval',
      removal: completedRemoval,
    })
  })

  it('disables the confirmation while removal is in progress', async () => {
    let finishRemoval: (() => void) | undefined
    vi.mocked(unpairDevice).mockImplementation(
      () => new Promise<MemberRemoval>(resolve => (finishRemoval = () => resolve(memberRemoval)))
    )
    render(<DevicesPage />)
    const user = await openUnpairConfirmation()
    const confirm = screen.getByRole('button', { name: i18n.t('devices.list.actions.unpair') })

    await user.click(confirm)

    expect(confirm).toBeDisabled()
    expect(
      screen.getByRole('button', { name: i18n.t('devices.unpair.cancelling') })
    ).toHaveAttribute('aria-busy', 'true')
    expect(document.querySelector('svg.animate-spin')).toBeInTheDocument()
    finishRemoval?.()
    await waitFor(() => expect(confirm).not.toBeInTheDocument())
  })

  it('keeps the confirmation open when dismissal is requested during removal', async () => {
    let finishRemoval: (() => void) | undefined
    vi.mocked(unpairDevice).mockImplementation(
      () => new Promise<MemberRemoval>(resolve => (finishRemoval = () => resolve(memberRemoval)))
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
      () => new Promise<MemberRemoval>(resolve => (finishRemoval = () => resolve(memberRemoval)))
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

  it('keeps permanent-loss confirmation open while continuing removal', async () => {
    let finishContinuation: ((value: MemberRemoval) => void) | undefined
    devicesState.memberRemoval = {
      ...memberRemoval,
      pendingRecipientDeviceIds: ['remote-peer'],
    }
    vi.mocked(continueMemberRemoval).mockImplementation(
      () => new Promise<MemberRemoval>(resolve => (finishContinuation = resolve))
    )
    render(<DevicesPage />)
    const user = userEvent.setup()

    await user.click(screen.getByRole('button', { name: remoteMember.deviceName }))
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.markLost') })
    )
    const confirm = screen.getByRole('button', {
      name: i18n.t('devices.memberRemoval.permanentLoss.confirm'),
    })
    await user.click(confirm)

    await waitFor(() =>
      expect(continueMemberRemoval).toHaveBeenCalledWith('removal-1', ['remote-peer'])
    )
    expect(
      screen.getByText(i18n.t('devices.memberRemoval.permanentLoss.title'))
    ).toBeInTheDocument()
    expect(confirm).toBeDisabled()
    finishContinuation?.({ ...memberRemoval, outcome: 'complete', pendingRecipients: 0 })
    await waitFor(() =>
      expect(
        screen.queryByText(i18n.t('devices.memberRemoval.permanentLoss.title'))
      ).not.toBeInTheDocument()
    )
  })

  it('shows an error and keeps permanent-loss confirmation open when continuing fails', async () => {
    devicesState.memberRemoval = {
      ...memberRemoval,
      pendingRecipientDeviceIds: ['remote-peer'],
    }
    vi.mocked(continueMemberRemoval).mockRejectedValue(new Error('offline'))
    render(<DevicesPage />)
    const user = userEvent.setup()

    await user.click(screen.getByRole('button', { name: remoteMember.deviceName }))
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.markLost') })
    )
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.confirm') })
    )

    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        i18n.t('devices.memberRemoval.errors.continueFailed')
      )
    )
    expect(
      screen.getByText(i18n.t('devices.memberRemoval.permanentLoss.title'))
    ).toBeInTheDocument()
  })

  it('closes the permanent-loss dialog when the store reports the removal complete', async () => {
    devicesState.memberRemoval = {
      ...memberRemoval,
      pendingRecipientDeviceIds: ['remote-peer'],
    }
    let finishContinuation: ((value: MemberRemoval) => void) | undefined
    vi.mocked(continueMemberRemoval).mockImplementation(
      () => new Promise<MemberRemoval>(resolve => (finishContinuation = resolve))
    )
    const { rerender } = render(<DevicesPage />)
    const user = userEvent.setup()

    await user.click(screen.getByRole('button', { name: remoteMember.deviceName }))
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.markLost') })
    )
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()

    // The Engine completes the revocation and the ws-driven store update lands
    // before the continue request returns — the dialog must close immediately.
    devicesState.memberRemoval = {
      ...memberRemoval,
      outcome: 'complete',
      pendingRecipients: 0,
      pendingRecipientDeviceIds: [],
    }
    rerender(<DevicesPage />)

    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(finishContinuation).toBeUndefined()
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

describe('DevicesPage shared-device refresh', () => {
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

  const refreshSnapshot = (
    overrides: Partial<SharedDeviceRefreshSnapshot> = {}
  ): SharedDeviceRefreshSnapshot => ({
    requestId: 'refresh-1',
    phase: 'connecting',
    devices: [],
    totalCount: 0,
    discoveredCount: 0,
    connectingCount: 0,
    connectedCount: 0,
    alreadyPresentCount: 0,
    waitingForPeerCount: 0,
    waitingForUpdateCount: 0,
    versionIncompatibleCount: 0,
    rejectedCount: 0,
    unavailableSourceCount: 0,
    ...overrides,
  })

  beforeEach(() => {
    vi.mocked(startSharedDeviceRefresh).mockReset()
    vi.mocked(getSharedDeviceRefresh).mockReset()
    devicesState.localDevice = localDevice
    devicesState.spaceMembers = [localDevice, remoteMember]
    vi.mocked(startSharedDeviceRefresh).mockResolvedValue('refresh-1')
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(refreshSnapshot())
    vi.mocked(isSharedDeviceRefreshNotFound).mockReturnValue(false)
    dispatchMock.mockClear()
  })

  afterEach(() => {
    devicesState.localDevice = null
    devicesState.spaceMembers = []
    vi.mocked(toast.error).mockClear()
    vi.mocked(startSharedDeviceRefresh).mockReset()
    vi.mocked(getSharedDeviceRefresh).mockReset()
  })

  it('hides the refresh entry when there are no paired devices', () => {
    devicesState.spaceMembers = [localDevice]
    render(<DevicesPage />)

    expect(
      screen.queryByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    ).not.toBeInTheDocument()
  })

  it('shows the refresh entry when at least one paired device exists', () => {
    render(<DevicesPage />)

    expect(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    ).toBeInTheDocument()
  })

  it('starts the refresh and opens the result window on click', async () => {
    let resolveStart: (() => void) | undefined
    vi.mocked(startSharedDeviceRefresh).mockImplementation(
      () =>
        new Promise(resolve => {
          resolveStart = () => resolve('refresh-1')
        })
    )
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    )

    expect(startSharedDeviceRefresh).toHaveBeenCalledTimes(1)
    expect(
      await screen.findByText(i18n.t('devices.sharedDeviceRefresh.searching'))
    ).toBeInTheDocument()

    act(() => resolveStart?.())
    expect(
      await screen.findByText(i18n.t('devices.sharedDeviceRefresh.connecting'))
    ).toBeInTheDocument()
  })

  it('reuses the same request while the first round is still active', async () => {
    const user = userEvent.setup()
    render(<DevicesPage />)
    const entry = () =>
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })

    await user.click(entry())
    await waitFor(() => expect(startSharedDeviceRefresh).toHaveBeenCalledTimes(1))
    await waitFor(() =>
      expect(screen.getByText(i18n.t('devices.sharedDeviceRefresh.connecting'))).toBeInTheDocument()
    )

    // Close the window; the request stays alive and the next click reopens
    // the same round instead of starting a second one.
    await user.keyboard('{Escape}')
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    await user.click(entry())

    await waitFor(() => expect(startSharedDeviceRefresh).toHaveBeenCalledTimes(1))
    expect(screen.getByRole('dialog')).toBeInTheDocument()
  })

  it('renders every device state with its copy', async () => {
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      refreshSnapshot({
        phase: 'connecting',
        devices: [
          { deviceId: 'd-1', displayName: '', state: 'discovered' },
          { deviceId: 'd-2', displayName: 'Peer A', state: 'connecting' },
          { deviceId: 'd-3', displayName: 'Peer B', state: 'connected' },
          { deviceId: 'd-4', displayName: 'Peer C', state: 'already_present' },
          { deviceId: 'd-5', displayName: 'Peer D', state: 'waiting_for_peer' },
          { deviceId: 'd-6', displayName: 'Peer E', state: 'waiting_for_update' },
          { deviceId: 'd-7', displayName: 'Peer F', state: 'version_incompatible' },
          { deviceId: 'd-8', displayName: 'Peer G', state: 'rejected' },
        ],
        totalCount: 8,
      })
    )
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    )

    await waitFor(() =>
      expect(
        screen.getByText(i18n.t('devices.sharedDeviceRefresh.deviceStates.connected'))
      ).toBeInTheDocument()
    )
    expect(screen.getByText('Peer B')).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.deviceStates.discovered'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.deviceStates.connecting'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.deviceStates.already_present'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.deviceStates.waiting_for_peer'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.deviceStates.waiting_for_update'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.deviceStates.version_incompatible'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.deviceStates.rejected'))
    ).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.unknownDevice'))
    ).toBeInTheDocument()
  })

  it('shows the explicit empty result after a completed round', async () => {
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      refreshSnapshot({ phase: 'round_completed', devices: [] })
    )
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    )

    expect(await screen.findByText(i18n.t('devices.sharedDeviceRefresh.empty'))).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.sharedDeviceRefresh.roundCompleted'))
    ).toBeInTheDocument()
  })

  it('shows only the summary when a query source is unavailable', async () => {
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      refreshSnapshot({ phase: 'connecting', unavailableSourceCount: 1 })
    )
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    )

    expect(
      await screen.findByText(i18n.t('devices.sharedDeviceRefresh.unavailableSources'))
    ).toBeInTheDocument()
  })

  it('re-reads the authoritative device list when a device first connects', async () => {
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      refreshSnapshot({
        phase: 'connecting',
        devices: [{ deviceId: 'new-peer', displayName: 'New Peer', state: 'connected' }],
        connectedCount: 1,
      })
    )
    dispatchMock.mockClear()
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    )

    await waitFor(() =>
      expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceMembers' })
    )
  })

  it('shows a toast and closes the window when starting fails', async () => {
    vi.mocked(startSharedDeviceRefresh).mockRejectedValue(new Error('daemon offline'))
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    )

    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(i18n.t('devices.sharedDeviceRefresh.startFailed'))
    )
    expect(
      screen.queryByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    ).toBeInTheDocument()
  })

  it('keeps querying in the background after the window is closed mid-round', async () => {
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    )
    await waitFor(() => expect(getSharedDeviceRefresh).toHaveBeenCalled())
    const callsAfterOpen = vi.mocked(getSharedDeviceRefresh).mock.calls.length

    await user.keyboard('{Escape}')
    await waitFor(() =>
      expect(
        screen.queryByText(i18n.t('devices.sharedDeviceRefresh.searching'))
      ).not.toBeInTheDocument()
    )

    const handler = daemonWsMocks.subscribe.mock.calls.at(-1)?.[1]
    expect(handler).toBeTypeOf('function')
    act(() => {
      handler?.({
        topic: 'shared-device-refresh',
        eventType: 'shared-device-refresh.changed',
        payload: { requestId: 'refresh-1' },
      })
    })

    await waitFor(() =>
      expect(vi.mocked(getSharedDeviceRefresh).mock.calls.length).toBeGreaterThan(callsAfterOpen)
    )
  })

  it('shows the ended message when the request no longer exists', async () => {
    vi.mocked(getSharedDeviceRefresh).mockRejectedValue(new Error('not found'))
    vi.mocked(isSharedDeviceRefreshNotFound).mockReturnValue(true)
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.sharedDeviceRefresh.refresh') })
    )

    expect(await screen.findByText(i18n.t('devices.sharedDeviceRefresh.ended'))).toBeInTheDocument()
  })
})

describe('DevicesPage pending-removal device marks', () => {
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
  })

  afterEach(() => {
    devicesState.localDevice = null
    devicesState.spaceMembers = []
    devicesState.memberRemoval = null
  })

  it('marks retained devices that must come online with a warning icon', () => {
    devicesState.memberRemoval = {
      ...memberRemoval,
      outcome: 'applied',
      pendingRecipientDeviceIds: ['remote-peer'],
    }

    render(<DevicesPage />)

    expect(
      screen.getByLabelText(i18n.t('devices.memberRemoval.pendingDeviceHint'))
    ).toBeInTheDocument()
  })

  it('shows recovery progress while an interrupted removal is being restored', () => {
    devicesState.memberRemoval = {
      ...memberRemoval,
      outcome: 'recovering',
      pendingRecipientDeviceIds: [],
    }

    render(<DevicesPage />)

    expect(screen.getByText(i18n.t('devices.memberRemoval.recovering.title'))).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.memberRemoval.recovering.description'))
    ).toBeInTheDocument()
  })

  it('explains the warning on hover', async () => {
    devicesState.memberRemoval = {
      ...memberRemoval,
      outcome: 'applied',
      pendingRecipientDeviceIds: ['remote-peer'],
    }
    const user = userEvent.setup()
    render(<DevicesPage />)

    await user.hover(screen.getByLabelText(i18n.t('devices.memberRemoval.pendingDeviceHint')))

    expect(
      await screen.findByText(i18n.t('devices.memberRemoval.pendingDeviceHint'))
    ).toBeInTheDocument()
  })

  it('does not mark devices once the removal completes', () => {
    devicesState.memberRemoval = {
      ...memberRemoval,
      outcome: 'complete',
      pendingRecipientDeviceIds: ['remote-peer'],
    }

    render(<DevicesPage />)

    expect(
      screen.queryByLabelText(i18n.t('devices.memberRemoval.pendingDeviceHint'))
    ).not.toBeInTheDocument()
  })

  it('does not mark unrelated devices', () => {
    devicesState.memberRemoval = {
      ...memberRemoval,
      outcome: 'applied',
      pendingRecipientDeviceIds: ['some-other-device'],
    }

    render(<DevicesPage />)

    expect(
      screen.queryByLabelText(i18n.t('devices.memberRemoval.pendingDeviceHint'))
    ).not.toBeInTheDocument()
  })
})
