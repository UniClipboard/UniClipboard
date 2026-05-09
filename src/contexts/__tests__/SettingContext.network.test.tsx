import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getSettings, updateSettings } from '@/api/daemon'
import type { Settings } from '@/api/daemon/settings'
import { SettingProvider } from '@/contexts/SettingContext'
import { useSetting } from '@/hooks/useSetting'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { emitSettingsChanged } from '@/lib/settings-events'
import { invokeWithTrace } from '@/lib/tauri-command'

vi.mock('@/api/daemon', () => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
}))

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn(),
}))

vi.mock('@/lib/settings-events', () => ({
  emitSettingsChanged: vi.fn(),
}))

vi.mock('@/lib/tauri-command', () => ({
  invokeWithTrace: vi.fn(),
}))

vi.mock('@/i18n', () => ({
  __esModule: true,
  default: {
    language: 'en-US',
    changeLanguage: vi.fn().mockResolvedValue(undefined),
  },
  normalizeLanguage: vi.fn((language: string | null | undefined) => language ?? 'en-US'),
  persistLanguage: vi.fn(),
}))

const mockGetSettings = vi.mocked(getSettings)
const mockUpdateSettings = vi.mocked(updateSettings)
const mockConnectDaemonWs = vi.mocked(connectDaemonWs)
const mockEmitSettingsChanged = vi.mocked(emitSettingsChanged)
const mockInvokeWithTrace = vi.mocked(invokeWithTrace)

const baseSetting: Settings = {
  schemaVersion: 1,
  general: {
    autoStart: false,
    silentStart: false,
    autoCheckUpdate: true,
    theme: 'light',
    themeColor: 'zinc',
    themeColorLight: null,
    themeColorDark: null,
    themeOverridesLight: {},
    themeOverridesDark: {},
    language: 'en-US',
    deviceName: 'Test Device',
    telemetryEnabled: true,
  },
  sync: {
    autoSync: true,
    syncFrequency: 'realtime',
    contentTypes: {
      text: true,
      image: true,
      link: true,
      file: true,
      codeSnippet: true,
      richText: true,
    },
  },
  retentionPolicy: {
    enabled: false,
    rules: [],
    skipPinned: false,
    evaluation: 'anyMatch',
  },
  security: {
    encryptionEnabled: false,
    passphraseConfigured: false,
    autoUnlockEnabled: false,
  },
  pairing: {
    stepTimeout: 15,
    userVerificationTimeout: 120,
    sessionTimeout: 300,
    maxRetries: 3,
    protocolVersion: '1.0.0',
  },
  keyboardShortcuts: {},
  fileSync: {
    fileSyncEnabled: true,
    smallFileThreshold: 10 * 1024 * 1024,
    maxFileSize: 5 * 1024 * 1024 * 1024,
    fileCacheQuotaPerDevice: 500 * 1024 * 1024,
    fileRetentionHours: 24,
    fileAutoCleanup: true,
  },
  network: {
    allowRelayFallback: true,
    allowOverlayNetworkAddrs: false,
  },
}

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <SettingProvider>{children}</SettingProvider>
)

const renderSettingHook = () => renderHook(() => useSetting(), { wrapper })

describe('SettingContext network — updateNetworkSetting + saveSetting restartRequired', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockConnectDaemonWs.mockResolvedValue(undefined)
    mockGetSettings.mockResolvedValue(baseSetting)
    mockUpdateSettings.mockResolvedValue({ success: true, restartRequired: false })
    mockEmitSettingsChanged.mockResolvedValue(undefined)
    mockInvokeWithTrace.mockResolvedValue(undefined)

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockReturnValue({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    })
  })

  it('Test 1: updateNetworkSetting 调 saveSetting 后透传 restartRequired=true', async () => {
    const { result } = renderSettingHook()
    await waitFor(() => {
      expect(result.current.setting).toEqual(baseSetting)
    })

    mockUpdateSettings.mockResolvedValueOnce({ success: true, restartRequired: true })

    let outcome: { restartRequired: boolean } | undefined
    await act(async () => {
      outcome = await result.current.updateNetworkSetting({ allowRelayFallback: false })
    })

    expect(outcome).toEqual({ restartRequired: true })
  })

  it('Test 2: updateNetworkSetting 把 partial 镜像合并进 setting.network', async () => {
    const { result } = renderSettingHook()
    await waitFor(() => {
      expect(result.current.setting).toEqual(baseSetting)
    })

    mockUpdateSettings.mockResolvedValueOnce({ success: true, restartRequired: true })

    await act(async () => {
      await result.current.updateNetworkSetting({ allowRelayFallback: false })
    })

    expect(mockUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        network: expect.objectContaining({ allowRelayFallback: false }),
      })
    )
    // 关键 fence：updateNetworkSetting 不应越界改其它段，验证 general/sync 段保持原状
    const lastCall = mockUpdateSettings.mock.calls[mockUpdateSettings.mock.calls.length - 1]
    const passed = lastCall[0] as Settings
    expect(passed.general).toEqual(baseSetting.general)
    expect(passed.sync).toEqual(baseSetting.sync)
  })

  it('Test 3: setting === null 时 graceful return，updateSettings 未被调用', async () => {
    // 让 getSettings 一直 pending，setting 维持 null
    let resolveGet: ((s: Settings) => void) | undefined
    mockGetSettings.mockImplementationOnce(
      () =>
        new Promise<Settings>(resolve => {
          resolveGet = resolve
        })
    )

    const { result } = renderSettingHook()

    // 在 setting 仍为 null 的窗口期调用
    await waitFor(() => {
      expect(result.current.loading).toBe(true)
    })
    expect(result.current.setting).toBeNull()

    let outcome: { restartRequired: boolean } | undefined
    await act(async () => {
      outcome = await result.current.updateNetworkSetting({ allowRelayFallback: false })
    })

    expect(outcome).toEqual({ restartRequired: false })
    expect(mockUpdateSettings).not.toHaveBeenCalled()

    // 让 mount load 完成，免得测试结束时 act warning
    await act(async () => {
      resolveGet?.(baseSetting)
    })
  })

  it('Test 4: PUT /settings 失败时错误向上抛，caller 可 catch（不被消化）', async () => {
    const { result } = renderSettingHook()
    await waitFor(() => {
      expect(result.current.setting).toEqual(baseSetting)
    })

    mockUpdateSettings.mockRejectedValueOnce(new Error('PUT failed'))

    await act(async () => {
      await expect(
        result.current.updateNetworkSetting({ allowRelayFallback: false })
      ).rejects.toThrow('PUT failed')
    })
  })

  it('Test 5: saveSetting 透传 restartRequired=false（契约升级向后兼容）', async () => {
    const { result } = renderSettingHook()
    await waitFor(() => {
      expect(result.current.setting).toEqual(baseSetting)
    })

    mockUpdateSettings.mockResolvedValueOnce({ success: true, restartRequired: false })

    let outcome: { restartRequired: boolean } | undefined
    await act(async () => {
      outcome = await result.current.updateNetworkSetting({ allowRelayFallback: true })
    })

    expect(outcome).toEqual({ restartRequired: false })
  })
})

describe('SettingContext network — 反向命名 + 契约 fence (Pitfall 1 + Pitfall 10)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockConnectDaemonWs.mockResolvedValue(undefined)
    mockGetSettings.mockResolvedValue(baseSetting)
    mockUpdateSettings.mockResolvedValue({ success: true, restartRequired: false })
    mockEmitSettingsChanged.mockResolvedValue(undefined)
    mockInvokeWithTrace.mockResolvedValue(undefined)

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockReturnValue({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    })
  })

  it('updateNetworkSetting 不在 SettingContext 内取反 allowRelayFallback', async () => {
    const { result } = renderSettingHook()
    await waitFor(() => {
      expect(result.current.setting).toEqual(baseSetting)
    })

    mockUpdateSettings.mockResolvedValueOnce({ success: true, restartRequired: true })

    // 输入 false → 期望 PUT body 中真值 false（不取反）
    await act(async () => {
      await result.current.updateNetworkSetting({ allowRelayFallback: false })
    })
    const lastCall = mockUpdateSettings.mock.calls[mockUpdateSettings.mock.calls.length - 1]
    const passed = lastCall[0] as Settings
    expect(passed.network.allowRelayFallback).toBe(false)

    // 输入 true → 期望 PUT body 中真值 true（不取反）
    mockUpdateSettings.mockResolvedValueOnce({ success: true, restartRequired: false })
    await act(async () => {
      await result.current.updateNetworkSetting({ allowRelayFallback: true })
    })
    const lastCall2 = mockUpdateSettings.mock.calls[mockUpdateSettings.mock.calls.length - 1]
    const passed2 = lastCall2[0] as Settings
    expect(passed2.network.allowRelayFallback).toBe(true)
  })

  it('SettingContext 不暴露反向布尔镜像字段（禁止任何 UI 命名的 store 字段）', async () => {
    const { result } = renderSettingHook()
    await waitFor(() => {
      expect(result.current.setting).toEqual(baseSetting)
    })

    // 用 join() 拼接禁词避免源码 grep 误命中（与 Plan 01 fence 同模式）
    const forbidden = ['lan', 'Only'].join('')
    const forbiddenUpdater = ['update', 'Lan', 'Only'].join('')
    expect(result.current).not.toHaveProperty(forbidden)
    expect(result.current).not.toHaveProperty(forbiddenUpdater)
    expect(result.current.setting).not.toHaveProperty(forbidden)
    expect(result.current.setting?.network).not.toHaveProperty(forbidden)
  })
})
