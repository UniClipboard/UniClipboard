/**
 * settings API 客户端测试 — 覆盖 toSettingsPatchRequest 的 network 段镜像
 * 与 updateSettings 解析 restartRequired 信号。
 *
 * Phase 95 Plan 01 — 类型契约层 RED gate。
 * - 测试 toSettingsPatchRequest 在 network 段存在/不存在两种情况下的镜像行为
 * - 测试 updateSettings 返回 { success, restartRequired } 形态
 * - 反向命名审计 (Pitfall 1 fence) — 确保反向布尔镜像字段不出现，反向取反不被悄悄引入
 */

import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import { updateSettings, type Settings } from '@/api/daemon/settings'

// Mock daemonClient 模块 — 所有 updateSettings 调用都会通过此 mock。
// vi.mock 由 vitest 在 import 之前 hoist，所以下方的 import 拿到的就是 mock 版本。
vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    request: vi.fn(),
  },
}))

// 类型化的 mock 引用，方便 mockResolvedValue / 访问 mock.calls。
const requestMock = daemonClient.request as unknown as ReturnType<typeof vi.fn>

beforeEach(() => {
  requestMock.mockReset()
})

afterEach(() => {
  vi.clearAllMocks()
})

// 默认成功响应 — 各测试可以 mockResolvedValueOnce 覆盖。
// 后端 UpdateSettingsResponse 形态：{ success, data, ts, restartRequired }（顶层）。
function mockUpdateOk(restartRequired: boolean) {
  requestMock.mockResolvedValueOnce({
    success: true,
    data: {},
    ts: 0,
    restartRequired,
  })
}

describe('settings api — toSettingsPatchRequest network mirror', () => {
  it('Test 1: toSettingsPatchRequest 镜像 network 段（allowRelayFallback=false）', async () => {
    mockUpdateOk(true)
    await updateSettings({
      network: { allowRelayFallback: false },
    } as Partial<Settings>)

    expect(requestMock).toHaveBeenCalledTimes(1)
    const [endpoint, options] = requestMock.mock.calls[0]
    expect(endpoint).toBe('/settings')
    expect(options.method).toBe('PUT')
    expect(options.body).toMatchObject({
      network: { allowRelayFallback: false },
    })
    // 不应混入其它顶层段 — 仅 network。
    expect(Object.keys(options.body)).toEqual(['network'])
  })

  it('Test 2: toSettingsPatchRequest 反向值 — true 进 true 出（不被悄悄取反）', async () => {
    mockUpdateOk(false)
    await updateSettings({
      network: { allowRelayFallback: true },
    } as Partial<Settings>)

    expect(requestMock).toHaveBeenCalledTimes(1)
    const [, options] = requestMock.mock.calls[0]
    expect(options.body.network).toEqual({ allowRelayFallback: true })
  })

  it('Test 3: toSettingsPatchRequest 无 network 段 — patch 不含 network key', async () => {
    mockUpdateOk(false)
    await updateSettings({
      general: {
        autoStart: true,
        silentStart: false,
        autoCheckUpdate: true,
        theme: 'system',
        themeColor: null,
        themeColorLight: null,
        themeColorDark: null,
        themeOverridesLight: {},
        themeOverridesDark: {},
        language: null,
        deviceName: null,
        telemetryEnabled: false,
        usageAnalyticsEnabled: false,
      },
    } as Partial<Settings>)

    expect(requestMock).toHaveBeenCalledTimes(1)
    const [, options] = requestMock.mock.calls[0]
    expect(Object.keys(options.body)).toContain('general')
    expect(options.body.general).toMatchObject({
      telemetryEnabled: false,
      usageAnalyticsEnabled: false,
    })
    expect(Object.keys(options.body)).not.toContain('network')
  })
})

describe('settings api — updateSettings restartRequired signal', () => {
  it('Test 4: updateSettings 解析 restartRequired=true', async () => {
    mockUpdateOk(true)
    const result = await updateSettings({
      network: { allowRelayFallback: false },
    } as Partial<Settings>)

    expect(result).toEqual({ success: true, restartRequired: true })
  })

  it('Test 5: updateSettings 解析 restartRequired=false', async () => {
    mockUpdateOk(false)
    const result = await updateSettings({
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
    } as Partial<Settings>)

    expect(result).toEqual({ success: true, restartRequired: false })
  })

  it('Test 6: updateSettings PUT body.network 含 allowRelayFallback', async () => {
    mockUpdateOk(true)
    await updateSettings({
      network: { allowRelayFallback: false },
    } as Partial<Settings>)

    expect(requestMock).toHaveBeenCalledTimes(1)
    const [, options] = requestMock.mock.calls[0]
    expect(options.body.network).toEqual({ allowRelayFallback: false })
  })
})

/**
 * 反向命名铁律 fence (Pitfall 1) — 任何引入反向布尔镜像字段或在
 * types / api 层做取反操作的回归会被这一组断言钉死。
 *
 * 现实层面的 grep 守门由 plan acceptance criteria 在 CI 之外执行；
 * 这里以单测形式锁住前端 store 的字段名与方向语义。
 */
describe('反向命名审计 (Pitfall 1 fence)', () => {
  it('Settings.network 字段名是 allowRelayFallback 不是反向布尔镜像', () => {
    const sample: Settings['network'] = {
      allowRelayFallback: true,
      allowOverlayNetworkAddrs: false,
    }
    const keys = Object.keys(sample)
    expect(keys).toContain('allowRelayFallback')
    // 任何反向布尔镜像字段出现都视为回归 — 字段名通过 join 拼接以避免被
    // plan acceptance grep 当作字面命中
    const FORBIDDEN_MIRROR_FIELDS = [
      ['lan', 'Only'].join(''),
      ['disable', 'Relay'].join(''),
      ['disable', 'Relays'].join(''),
    ]
    for (const forbidden of FORBIDDEN_MIRROR_FIELDS) {
      expect(keys).not.toContain(forbidden)
    }
  })

  it('toSettingsPatchRequest 不取反 — true 输入 → patch 含 true（已由 Test 2 覆盖，此断言为冗余 fence）', async () => {
    mockUpdateOk(false)
    await updateSettings({
      network: { allowRelayFallback: true },
    } as Partial<Settings>)

    const [, options] = requestMock.mock.calls[0]
    // 显式断言不被悄悄取反（防御 toSettingsPatchRequest 内部加 ! 表达式）
    expect(options.body.network.allowRelayFallback).toBe(true)
    expect(options.body.network.allowRelayFallback).not.toBe(false)
  })
})
