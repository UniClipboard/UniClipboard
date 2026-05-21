/**
 * 跨 hook 实例的 in-flight 共享契约 —— FileContextMenu 与 EntryDeliveryBadge
 * 都各自调 `useResendAction()`,任一实例发起的请求必须立刻让另一实例的
 * 按钮 disable,否则用户能在右键菜单 + popover 上对同一 entry 各点一次,
 * 触发并发 IPC。
 *
 * 测试用一对 hook 实例 + 控制 `resendEntry` 的 promise 来观察 in-flight
 * 状态在 startSettle 之间的传播。
 */

import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { __resetResendActionStoreForTests, useResendAction } from '@/hooks/useResendAction'

const resendEntryMock = vi.fn()
const toastSuccessMock = vi.fn()
const toastErrorMock = vi.fn()

vi.mock('@/api/tauri-command/clipboard_delivery', async () => {
  const actual = await vi.importActual<typeof import('@/api/tauri-command/clipboard_delivery')>(
    '@/api/tauri-command/clipboard_delivery'
  )
  return {
    ...actual,
    resendEntry: (...args: unknown[]) => resendEntryMock(...args),
  }
})

vi.mock('sonner', () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}))

beforeEach(() => {
  resendEntryMock.mockReset()
  toastSuccessMock.mockReset()
  toastErrorMock.mockReset()
  __resetResendActionStoreForTests()
})

describe('useResendAction — shared in-flight store', () => {
  it('marks the same entry in-flight across independent hook instances', async () => {
    // 两个独立 renderHook —— 模拟 FileContextMenu 与 EntryDeliveryBadge
    // 同时挂载,各自调用 useResendAction()。
    const { result: instanceA } = renderHook(() => useResendAction())
    const { result: instanceB } = renderHook(() => useResendAction())

    let releaseResolve!: (value: unknown) => void
    resendEntryMock.mockReturnValueOnce(
      new Promise(resolve => {
        releaseResolve = resolve
      })
    )

    // 初始两个实例都报告 not in-flight。
    expect(instanceA.current.isEntryInFlight('entry-shared')).toBe(false)
    expect(instanceB.current.isEntryInFlight('entry-shared')).toBe(false)

    // A 触发 resend;promise 故意挂着,模拟"请求在飞"。
    let resendPromise: Promise<void>
    act(() => {
      resendPromise = instanceA.current.resendAll('entry-shared')
    })

    // 两个实例都要看到 in-flight = true (跨实例同步)。
    await waitFor(() => {
      expect(instanceA.current.isEntryInFlight('entry-shared')).toBe(true)
      expect(instanceB.current.isEntryInFlight('entry-shared')).toBe(true)
    })

    // 解开 promise,fan-out 结算后两个实例同步回 false。
    await act(async () => {
      releaseResolve({
        accepted: 1,
        duplicate: 0,
        offline: 0,
        errored: 0,
        pending: 0,
      })
      await resendPromise
    })

    expect(instanceA.current.isEntryInFlight('entry-shared')).toBe(false)
    expect(instanceB.current.isEntryInFlight('entry-shared')).toBe(false)
  })

  it('second resendAll call for the in-flight entry is a noop (no duplicate IPC)', async () => {
    const { result } = renderHook(() => useResendAction())

    let releaseResolve!: (value: unknown) => void
    resendEntryMock.mockReturnValueOnce(
      new Promise(resolve => {
        releaseResolve = resolve
      })
    )

    let firstPromise: Promise<void>
    act(() => {
      firstPromise = result.current.resendAll('entry-x')
    })

    // 第二次调用必须立刻短路 —— 不再触发 resendEntry。即便两个不同 hook
    // 实例 (右键菜单 + popover) 在飞期间各点一次,也只产生一条 IPC。
    await act(async () => {
      await result.current.resendAll('entry-x')
    })
    expect(resendEntryMock).toHaveBeenCalledTimes(1)

    await act(async () => {
      releaseResolve({
        accepted: 1,
        duplicate: 0,
        offline: 0,
        errored: 0,
        pending: 0,
      })
      await firstPromise
    })
  })

  it('peer-level in-flight is scoped by (entryId, deviceId), allowing different (entry, peer) pairs to run concurrently', async () => {
    const { result } = renderHook(() => useResendAction())

    let releaseA!: (value: unknown) => void
    let releaseB!: (value: unknown) => void
    resendEntryMock
      .mockReturnValueOnce(
        new Promise(resolve => {
          releaseA = resolve
        })
      )
      .mockReturnValueOnce(
        new Promise(resolve => {
          releaseB = resolve
        })
      )

    let promiseA: Promise<void>
    let promiseB: Promise<void>
    act(() => {
      promiseA = result.current.resendToPeer('entry-1', 'dev-a')
    })
    act(() => {
      promiseB = result.current.resendToPeer('entry-1', 'dev-b')
    })

    await waitFor(() => {
      expect(result.current.isPeerInFlight('entry-1', 'dev-a')).toBe(true)
      expect(result.current.isPeerInFlight('entry-1', 'dev-b')).toBe(true)
      expect(result.current.isPeerInFlight('entry-1', 'dev-c')).toBe(false)
      // entry-wide 锁未被 peer-level 锁波及。
      expect(result.current.isEntryInFlight('entry-1')).toBe(false)
    })

    expect(resendEntryMock).toHaveBeenCalledTimes(2)

    await act(async () => {
      releaseA({ accepted: 1, duplicate: 0, offline: 0, errored: 0, pending: 0 })
      releaseB({ accepted: 1, duplicate: 0, offline: 0, errored: 0, pending: 0 })
      await Promise.all([promiseA, promiseB])
    })

    expect(result.current.isPeerInFlight('entry-1', 'dev-a')).toBe(false)
    expect(result.current.isPeerInFlight('entry-1', 'dev-b')).toBe(false)
  })

  it('null entryId is a noop and does not pollute the in-flight store', async () => {
    const { result } = renderHook(() => useResendAction())

    await act(async () => {
      await result.current.resendAll(null)
      await result.current.resendToPeer(null, 'dev-a')
    })

    expect(resendEntryMock).not.toHaveBeenCalled()
    expect(result.current.isEntryInFlight(null)).toBe(false)
    expect(result.current.isPeerInFlight(null, 'dev-a')).toBe(false)
  })
})
