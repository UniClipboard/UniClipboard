/**
 * Phase 5 (#747) —— useEntryDelivery 在 dispatch 完成后自动刷新。
 *
 * 关注点:
 * 1. 首次挂载就拉一次 view(回归 phase 1 行为)。
 * 2. 后端经 daemon WS emit `clipboard.delivery_status_changed` 且 entryId
 *    匹配 → refetch。
 * 3. 事件 entryId 与当前打开的 entry 不匹配 → 不 refetch(避免抖动 / 流量浪费)。
 * 4. 切换 entryId / 组件卸载 → 取消订阅,不再触发刷新。
 *
 * 不测的部分(同样重要,但不在本 hook 职责内):
 * - 后端 emit 时机 → 由 dispatch_entry.rs 的 Rust 单元测试覆盖。
 * - WS 事件 wire shape → 由 uc-webserver 的 emitter 与契约常量守门。
 */

import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getEntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import { useEntryDelivery } from '../useEntryDelivery'

// ── mock daemon WS ────────────────────────────────────────────────────
//
// `daemonWs.subscribe(topics, cb)` 同步返回 unsubscribe fn。测试把 cb 抓出来,
// 直接调它模拟后端 WS 推送;每次 subscribe 都返回同一个 unsubscribe spy,
// 以便断言 cleanup 真的释放。
type WsEvent = { topic: string; eventType: string; payload: unknown }
let capturedListener: ((event: WsEvent) => void) | null = null
const unlistenSpy = vi.fn()

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn((_topics: string[], cb: (event: WsEvent) => void) => {
      capturedListener = cb
      return unlistenSpy
    }),
  },
}))

// 构造一条 delivery WS 事件帧(topic + eventType + payload)。
function deliveryEvent(entryId: string): WsEvent {
  return {
    topic: 'clipboard',
    eventType: 'clipboard.delivery_status_changed',
    payload: { entryId, targetDeviceId: 'peer-1' },
  }
}

// ── mock fetch 入口 ───────────────────────────────────────────────────

vi.mock('@/api/tauri-command/clipboard_delivery', () => ({
  getEntryDeliveryView: vi.fn(),
}))

const mockGet = vi.mocked(getEntryDeliveryView)

function viewWithStatus(entryId: string, marker: string) {
  // marker 只是为了让两次响应在断言里能区分,业务字段保持最小可分形态。
  return {
    entryId,
    source: { tag: 'local' as const },
    deliveries: [
      {
        targetDeviceId: 'peer-1',
        targetDeviceName: marker,
        status: { tag: 'delivered' as const },
        reasonDetail: null,
        updatedAtMs: 1_700_000_000_000,
      },
    ],
  }
}

describe('useEntryDelivery', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    capturedListener = null
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('在挂载时拉一次 view', async () => {
    mockGet.mockResolvedValueOnce(viewWithStatus('entry-1', 'initial'))
    const { result } = renderHook(() => useEntryDelivery('entry-1'))

    await waitFor(() => {
      expect(result.current.delivery).not.toBeNull()
    })
    expect(mockGet).toHaveBeenCalledTimes(1)
    expect(mockGet).toHaveBeenCalledWith('entry-1')
    expect(result.current.delivery?.deliveries[0].targetDeviceName).toBe('initial')
    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
  })

  it('收到匹配 entryId 的事件 → refetch 并更新状态', async () => {
    mockGet
      .mockResolvedValueOnce(viewWithStatus('entry-1', 'initial'))
      .mockResolvedValueOnce(viewWithStatus('entry-1', 'after-event'))

    const { result } = renderHook(() => useEntryDelivery('entry-1'))

    await waitFor(() => {
      expect(result.current.delivery?.deliveries[0].targetDeviceName).toBe('initial')
    })

    // subscribe 同步返回,listener 在渲染时即被抓到。
    await waitFor(() => {
      expect(capturedListener).not.toBeNull()
    })

    await act(async () => {
      capturedListener!(deliveryEvent('entry-1'))
    })

    await waitFor(() => {
      expect(result.current.delivery?.deliveries[0].targetDeviceName).toBe('after-event')
    })
    expect(mockGet).toHaveBeenCalledTimes(2)
  })

  it('事件 entryId 不匹配时不会触发 refetch', async () => {
    mockGet.mockResolvedValueOnce(viewWithStatus('entry-1', 'initial'))

    renderHook(() => useEntryDelivery('entry-1'))

    await waitFor(() => {
      expect(mockGet).toHaveBeenCalledTimes(1)
    })

    await waitFor(() => {
      expect(capturedListener).not.toBeNull()
    })

    // 推一条 entry_id=别人 的事件 —— 当前 hook 不该响应。
    await act(async () => {
      capturedListener!(deliveryEvent('entry-other'))
    })

    // 给 React 一次微任务时间,确保任何潜在的 refetch 都已经被排进队列。
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(mockGet).toHaveBeenCalledTimes(1)
  })

  it('卸载时调用 unlisten,不再响应事件', async () => {
    mockGet.mockResolvedValue(viewWithStatus('entry-1', 'initial'))

    const { unmount } = renderHook(() => useEntryDelivery('entry-1'))

    await waitFor(() => {
      expect(capturedListener).not.toBeNull()
    })

    unmount()
    // unsubscribe 是同步调用,卸载即触发。
    expect(unlistenSpy).toHaveBeenCalledTimes(1)

    // 即便卸载后事件再来,hook 内部 cancelled flag 会让 fetch 跳过 setState,
    // mockGet 不会被再次调用。
    capturedListener!(deliveryEvent('entry-1'))
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(mockGet).toHaveBeenCalledTimes(1)
  })

  it('entryId 切换为 null → 清空状态且不再 fetch', async () => {
    mockGet.mockResolvedValueOnce(viewWithStatus('entry-1', 'initial'))

    const { result, rerender } = renderHook((id: string | null) => useEntryDelivery(id), {
      initialProps: 'entry-1' as string | null,
    })

    await waitFor(() => {
      expect(result.current.delivery).not.toBeNull()
    })
    expect(mockGet).toHaveBeenCalledTimes(1)

    rerender(null)

    expect(result.current.delivery).toBeNull()
    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
  })
})
