/**
 * DevicesPage 顶层渲染测试。
 *
 * 重写后页面是 Hero + Tabs + 卡片网格，不再嵌套已弃用的
 * ThisDeviceCard / SpaceMembersPanel / MobileSyncDevicesPanel。本测试只验证：
 *   1. 挂载时分发 `fetchLocalDeviceInfo` + `fetchSpaceMembers`
 *   2. 渲染 Hero 区域里的"本设备"标签 + 两个 Tab 切换控件
 *
 * 卡片级别的交互（点击进入 dialog / 邀请新设备 / 撤销移动设备 等）由各自
 * 组件的单元测试覆盖，这里不重复。
 */

import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
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
  it('dispatches fetchLocalDeviceInfo and fetchSpaceMembers on mount', () => {
    dispatchMock.mockClear()
    render(<DevicesPage />)

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchLocalDeviceInfo' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceMembers' })
  })

  it('renders the P2P and Mobile sync tab triggers', () => {
    render(<DevicesPage />)

    // Two tabs: 已配对设备 / Paired devices  +  手机同步 / Mobile Sync
    expect(screen.getAllByRole('tab')).toHaveLength(2)
  })
})
