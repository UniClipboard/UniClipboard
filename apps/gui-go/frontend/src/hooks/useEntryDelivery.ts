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
 * ADR-008 P3-3 GAP-WS-1:订阅 daemon WS 的 `clipboard.delivery_status_changed`
 * 事件(`clipboard` topic),匹配当前 `entryId` 时重新拉取 view,detail 视图
 * 无需手动切换 entry 就能反映"对端 ack 完成"。改走 WS(取代旧的进程内
 * Tauri 事件 `clipboard-delivery-status-changed`)后,GUI 转为纯 client 时
 * (B2'-3 删除 in-process host_event_bus)这条 refetch 信号仍然可用。事件
 * 丢失或乱序由 refetch 幂等吸收 —— 不依赖事件本身的状态,事件仅作为
 * "该不该 refetch"的触发信号。
 */

import { useEffect, useRef, useState } from 'react'
import {
  type EntryDeliveryView,
  getEntryDeliveryView,
} from '@/api/tauri-command/clipboard_delivery'
import { daemonWs } from '@/lib/daemon-ws'
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
    let cancelled = false

    if (!entryId) {
      requestIdRef.current++
      setDelivery(null)
      setLoading(false)
      setError(null)
      return () => {
        cancelled = true
      }
    }

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

    // `daemonWs.subscribe` 同步返回 unsubscribe(不像 tauri-specta `listen`
    // 返回 Promise),所以无需 then/cancelled 双保险来处理"卸载早于订阅
    // 建立"。仍保留 `cancelled` flag 给异步 fetchDelivery 的丢弃过期响应。
    const unsubscribe = daemonWs.subscribe(['clipboard'], event => {
      if (cancelled) return
      if (event.eventType !== 'clipboard.delivery_status_changed') return
      const payload = event.payload as { entryId: string; targetDeviceId: string }
      // 严格按 entryId 匹配 —— 多 entry 并行 dispatch 时,后端会推多个事件,
      // 只有匹配当前打开 entry 的事件值得 refetch;否则纯属带宽浪费。
      if (payload.entryId !== entryId) return
      void fetchDelivery(false)
    })

    return () => {
      cancelled = true
      requestIdRef.current++
      unsubscribe()
    }
  }, [entryId])

  return { delivery, loading, error }
}
