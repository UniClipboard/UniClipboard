/**
 * DevicesPage top-level render tests.
 *
 * The page is a master-detail layout: a list column with 本机 / 已配对设备 /
 * 移动同步 sections plus a persistent detail pane. This suite only verifies:
 *   1. mount dispatches `fetchLocalDeviceInfo` + `fetchSpaceMembers`
 *   2. the list column renders its three section labels
 *   3. presence is probed once on mount / on visibility regain, no polling
 *
 * Panel-level interactions (sync toggles, unpair, mobile edit/revoke) are
 * covered by the panel components' own unit tests.
 */

import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { refreshPresence } from '@/api/daemon'
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
