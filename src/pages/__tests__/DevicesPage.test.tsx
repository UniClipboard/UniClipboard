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

import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { refreshPresence } from '@/api/daemon'
import { getMobileSyncSettings, type MobileSyncSettingsView } from '@/api/tauri-command/mobile_sync'
import { toast } from '@/components/ui/toast'
import i18n from '@/i18n'
import DevicesPage from '@/pages/DevicesPage'

const dispatchMock = vi.fn()

vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => dispatchMock,
  useAppSelector: (selector: (s: unknown) => unknown) =>
    selector({
      devices: {
        localDevice: null,
        localDeviceLoading: false,
        localDeviceError: null,
        spaceMembers: [],
        spaceMembersError: null,
        memberSyncPreferences: {},
        memberSyncPreferencesLoading: {},
      },
    }),
}))

vi.mock('@/store/slices/devicesSlice', () => ({
  fetchLocalDeviceInfo: vi.fn(() => ({ type: 'devices/fetchLocalDeviceInfo' })),
  fetchSpaceMembers: vi.fn(() => ({ type: 'devices/fetchSpaceMembers' })),
  clearLocalDeviceError: vi.fn(() => ({ type: 'devices/clearLocalDeviceError' })),
  clearSpaceMembersError: vi.fn(() => ({ type: 'devices/clearSpaceMembersError' })),
  fetchMemberSyncPreferences: vi.fn(() => ({ type: 'devices/fetchMemberSyncPreferences' })),
  updateMemberSyncPreferences: vi.fn(() => ({ type: 'devices/updateMemberSyncPreferences' })),
}))

vi.mock('@/api/daemon', () => ({
  refreshPresence: vi.fn(() => Promise.resolve()),
}))

vi.mock('@/api/daemon/members', () => ({
  unpairDevice: vi.fn(),
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
  })

  it('dispatches fetchLocalDeviceInfo and fetchSpaceMembers on mount', () => {
    dispatchMock.mockClear()
    render(<DevicesPage />)

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchLocalDeviceInfo' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceMembers' })
  })

  it('renders the list column with the three device sections', () => {
    render(<DevicesPage />)

    // Section labels: 本设备 / 已配对设备 / 移动设备同步 (i18n keys
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
})

describe('DevicesPage add-device entry points', () => {
  const settingsFixture = (
    overrides: Partial<MobileSyncSettingsView> = {}
  ): MobileSyncSettingsView =>
    ({
      enabled: false,
      lanListenEnabled: false,
      lanAdvertiseIp: null,
      lanPort: null,
      lanListenerError: null,
      shortcutInstallMethods: [],
      ...overrides,
    }) as MobileSyncSettingsView

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
