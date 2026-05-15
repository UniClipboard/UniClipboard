/**
 * Entry delivery view fetch hook —— quick-panel 与主窗口两套 detail 共享。
 *
 * 为什么需要这个 hook:
 * detail 区域要展示"来自哪台设备 + 每个可信对端的同步状态"。这份数据
 * 跟现有 `useClipboardPreview` / `useClipboardPreviewState` 拉的"内容
 * 预览"是两条独立通路 (一条是 `clipboardPreviewCache`,一条是新的
 * `clipboard_entry_delivery_view` Tauri command),把 fetch 抽到独立
 * hook,组件可以让它和 preview 并发执行 —— React 在同一组件里调用
 * 两个 useEffect 是天然并发的。
 *
 * 不缓存:detail 重开 (entryId 切换 / 同 entryId 再次打开) 要重新拉,
 * 以反映"这一瞬间"的同步快照。Phase 2 不监听 dispatch 完成事件,detail
 * 打开期间不动态刷新 (见 task_plan.md Phase 2 · "触发刷新")。
 */

import { useEffect, useRef, useState } from 'react'
import {
  type EntryDeliveryView,
  getEntryDeliveryView,
} from '@/api/tauri-command/clipboard_delivery'
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

    const currentRequestId = ++requestIdRef.current
    setLoading(true)
    setError(null)
    setDelivery(null)

    void (async () => {
      try {
        const next = await getEntryDeliveryView(entryId)
        if (currentRequestId !== requestIdRef.current) return
        setDelivery(next)
      } catch (err) {
        if (currentRequestId !== requestIdRef.current) return
        log.warn({ err, entryId }, 'failed to load entry delivery view')
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        if (currentRequestId === requestIdRef.current) {
          setLoading(false)
        }
      }
    })()
  }, [entryId])

  return { delivery, loading, error }
}
