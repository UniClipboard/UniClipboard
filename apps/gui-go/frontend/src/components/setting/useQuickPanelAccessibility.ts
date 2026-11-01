import { useCallback, useEffect, useRef, useState } from 'react'
import {
  getQuickPanelAccessibilityPermission,
  refreshQuickPanelTriggers,
} from '@/api/tauri-command'
import { createLogger } from '@/lib/logger'

export type AccessibilityStatus = 'checking' | 'granted' | 'denied' | 'error'
const log = createLogger('quick-panel-accessibility')

export function useQuickPanelAccessibility(isMac: boolean) {
  const [status, setStatus] = useState<AccessibilityStatus>('checking')
  const generation = useRef(0)
  const pending = useRef<{
    generation: number
    again: boolean
    promise: Promise<void>
  } | null>(null)
  const refresh = useCallback((): Promise<void> => {
    if (!isMac) return Promise.resolve()
    if (pending.current?.generation === generation.current) {
      pending.current.again = true
      return pending.current.promise
    }
    const request = generation.current
    const check = { generation: request, again: false, promise: Promise.resolve() }
    pending.current = check
    const run = async () => {
      do {
        check.again = false
        setStatus('checking')
        try {
          const granted = await getQuickPanelAccessibilityPermission()
          if (request !== generation.current) return
          setStatus(granted ? 'granted' : 'denied')
          // The helper checks its own grant. The UI process grant is not a listener receipt.
          await refreshQuickPanelTriggers().catch(err => {
            log.error({ err }, 'Failed to request quick panel listener refresh')
          })
        } catch (err) {
          log.error({ err }, 'Failed to check Accessibility permission')
          if (request === generation.current) setStatus('error')
        }
      } while (check.again && request === generation.current)
    }
    check.promise = run().finally(() => {
      if (pending.current === check) pending.current = null
    })
    return check.promise
  }, [isMac])

  useEffect(() => {
    if (!isMac) return
    const onFocus = () => {
      void refresh()
    }
    const onVisibility = () => {
      if (document.visibilityState === 'visible') void refresh()
    }
    void refresh()
    window.addEventListener('focus', onFocus)
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      generation.current += 1
      window.removeEventListener('focus', onFocus)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [isMac, refresh])

  return { status, refresh }
}
