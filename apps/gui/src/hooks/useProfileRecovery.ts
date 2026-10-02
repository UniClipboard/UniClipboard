import { useCallback, useEffect, useMemo, useState } from 'react'
import { daemonWs } from '@/lib/daemon-ws'
import { commands } from '@/lib/ipc'
import type { ProfileRecoveryResponse } from '@/lib/ipc-bindings.generated'
import { createLogger } from '@/lib/logger'

const log = createLogger('profile-recovery')

/** Recovery is queried before setup/settings, which are unavailable until keys are restored. */
export function useProfileRecovery(enabled: boolean) {
  const [revision, setRevision] = useState(0)
  const generation = useMemo(() => ({ enabled, revision }), [enabled, revision])
  const [result, setResult] = useState<{
    generation: object
    status: ProfileRecoveryResponse | null
    failed: boolean
  } | null>(null)
  const refresh = useCallback(() => setRevision(value => value + 1), [])
  useEffect(() => {
    if (!enabled) return
    let cancelled = false
    let request = 0
    const check = async () => {
      const current = ++request
      try {
        const next = await commands.getProfileRecovery()
        if (!cancelled && current === request) {
          setResult({ generation, status: next, failed: false })
        }
      } catch {
        if (!cancelled && current === request) {
          setResult({ generation, status: null, failed: true })
          log.warn('Profile recovery query failed; keeping content hidden')
        }
      }
    }
    const unsubscribe = daemonWs.subscribe(['encryption', 'system'], () => void check())
    const timer = window.setInterval(() => void check(), 5_000)
    void check()
    return () => {
      cancelled = true
      unsubscribe()
      window.clearInterval(timer)
    }
  }, [enabled, generation])
  const current = enabled && result?.generation === generation ? result : null
  return { status: current?.status ?? null, failed: current?.failed ?? false, refresh }
}
