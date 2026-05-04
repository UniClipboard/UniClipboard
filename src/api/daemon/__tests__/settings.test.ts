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
function mockUpdateOk(restartRequired: boolean) {
  requestMock.mockResolvedValueOnce({
    data: { success: true, restartRequired },
    ts: 0,
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
        language: null,
        deviceName: null,
        telemetryEnabled: false,
      },
    } as Partial<Settings>)

    expect(requestMock).toHaveBeenCalledTimes(1)
    const [, options] = requestMock.mock.calls[0]
    expect(Object.keys(options.body)).toContain('general')
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
