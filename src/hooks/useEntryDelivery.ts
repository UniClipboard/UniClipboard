/**
 * Entry delivery view fetch hook —— quick-panel 与主窗口两套 detail 共享。
 *
 * 为什么需要这个 hook:
 * detail 区域要展示"来自哪台设备 + 每个可信对端的同步状态"。这份数据
 * 跟现有 `useClipboardPreview` / `useClipboardPreviewState` 拉的"内容
 * 预览"是两条独立通路 (一条是 `clipboardPreviewCache`,一条是
 * `clipboard_entry_delivery_view` Tauri command),把 fetch 抽到独立
 * hook,组件可以让它和 preview 并发执行 —— React 在同一组件里调用
 * 两个 useEffect 是天然并发的。
 *
 * Phase 5 (Issue #747):订阅后端 `clipboard-delivery-status-changed`
 * 事件,匹配当前 `entryId` 时重新拉取 view,detail 视图无需手动切换
 * entry 就能反映"对端 ack 完成"。事件丢失或乱序由 refetch 幂等吸收 ——
 * 不依赖事件本身的状态,事件仅作为"该不该 refetch"的触发信号。
 */

import { useEffect, useRef, useState } from 'react'
import {
  type EntryDeliveryView,
  getEntryDeliveryView,
} from '@/api/tauri-command/clipboard_delivery'
import { events } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('entry-delivery')

export interface EntryDeliveryHookResult {
  delivery: EntryDeliveryView | null
  loading: boolean
  /** entry 不存在 / facade 不可用 等 fetch 失败的标记,组件据此降级渲染。 */
  error: string | null
}

export function useEntryDelivery(entryId: string | null): EntryDeliveryHookResult {
  const [delivery, setDelivery] = useState<EntryDeliveryView | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const requestIdRef = useRef(0)

  useEffect(() => {
    if (!entryId) {
      requestIdRef.current++
      setDelivery(null)
      setLoading(false)
      setError(null)
      return
    }

    let cancelled = false

    // fetch 入口抽出来,首次进入与事件触发的 refetch 共用同一份请求路径。
    // requestIdRef 保留"丢弃过期响应"语义 —— 事件抖动 / 用户快速切 entry
    // 时,只让最后一次请求的结果落到 state 上,中间响应即便晚到也会被
    // ID 不匹配的 guard 丢掉,避免老快照覆盖新快照。
    const fetchDelivery = async (initial: boolean) => {
      const currentRequestId = ++requestIdRef.current
      if (initial) {
        setLoading(true)
        setError(null)
        setDelivery(null)
      }
      try {
        const next = await getEntryDeliveryView(entryId)
        if (cancelled || currentRequestId !== requestIdRef.current) return
        setDelivery(next)
        // 事件驱动的 refetch 隐式清掉旧 error —— 上次错可能是 entry 还
        // 没落库的瞬态,delivery 落库后事件到达此刻已能拉到。
        setError(null)
      } catch (err) {
        if (cancelled || currentRequestId !== requestIdRef.current) return
        log.warn({ err, entryId }, 'failed to load entry delivery view')
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        if (!cancelled && currentRequestId === requestIdRef.current && initial) {
          setLoading(false)
        }
      }
    }

    void fetchDelivery(true)

    // tauri-specta 的 `listen` 返回 `Promise<UnlistenFn>`;在 Promise resolve
    // 之前组件可能就被卸载,所以保留一个 cancelled flag,并在 cleanup 时
    // 用 then 链调用 unlisten,避免悬挂监听往一个已卸载的组件 setState。
    const unlistenPromise = events.clipboardDeliveryStatusChanged.listen(event => {
      if (cancelled) return
      // 严格按 entryId 匹配 —— 多 entry 并行 dispatch 时,后端会推多个事
      // 件,只有匹配当前打开 entry 的事件值得 refetch;否则纯属带宽浪费。
      if (event.payload.entryId !== entryId) return
      void fetchDelivery(false)
    })

    return () => {
      cancelled = true
      requestIdRef.current++
      void unlistenPromise
        .then(unlisten => unlisten())
        .catch(err => {
          log.debug({ err }, 'unlisten clipboard-delivery-status-changed failed')
        })
    }
  }, [entryId])

  return { delivery, loading, error }
}
