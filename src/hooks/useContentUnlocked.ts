import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { daemonWs } from '@/lib/daemon-ws'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('content-lock')

/** Every window queries the same host-owned grant; events only trigger a recheck. */
export function useContentUnlocked(enabled = true) {
  const [revision, setRevision] = useState(0)
  const generation = useMemo(() => ({ enabled, revision }), [enabled, revision])
  const [result, setResult] = useState<{ generation: object; unlocked: boolean } | null>(null)
  const refresh = useCallback(() => setRevision(value => value + 1), [])

  useEffect(() => {
    if (!enabled) return
    let cancelled = false
    let request = 0
    const check = async () => {
      const current = ++request
      try {
        const result = await commands.getContentUnlocked()
        if (!cancelled && current === request) setResult({ generation, unlocked: result })
      } catch {
        if (!cancelled && current === request) {
          setResult({ generation, unlocked: false })
          log.warn('Could not verify content access; keeping content hidden')
        }
      }
    }
    const unlisten = listen('content-lock-changed', () => void check())
    void unlisten
      .then(() => {
        if (!cancelled) void check()
      })
      .catch(() => {
        log.warn('Content lock listener unavailable; status checks remain active')
      })
    const unsubscribe = daemonWs.subscribe(['encryption'], () => void check())
    const timer = window.setInterval(() => void check(), 5_000)
    window.addEventListener('focus', check)
    void check()
    return () => {
      cancelled = true
      window.clearInterval(timer)
      window.removeEventListener('focus', check)
      unsubscribe()
      void unlisten.then(stop => stop()).catch(() => {})
    }
  }, [enabled, generation])

  return {
    unlocked: enabled && result?.generation === generation ? result.unlocked : null,
    refresh,
  }
}
