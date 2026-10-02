import { useCallback, useEffect, useRef, useState } from 'react'
import { getRelayOverview } from '@/api/daemon'
import { createLogger } from '@/lib/logger'
import type { RelayOverview } from '@/types/setting'

const log = createLogger('relay-overview')

export interface RelayOverviewState {
  overview: RelayOverview | null
  loading: boolean
  failed: boolean
  reload: () => Promise<void>
}

/**
 * Loads the Engine-owned relay overview. The overview is re-read whenever
 * the custom relay list or the LAN-only setting changes, so the displayed
 * state always comes from Engine rather than from local assumptions. A stale
 * response never overwrites a newer one.
 */
export function useRelayOverview(
  customRelays: unknown,
  allowRelayFallback: boolean
): RelayOverviewState {
  const [overview, setOverview] = useState<RelayOverview | null>(null)
  const [loading, setLoading] = useState(true)
  const [failed, setFailed] = useState(false)
  const requestRef = useRef(0)

  const reload = useCallback(async () => {
    const request = ++requestRef.current
    setLoading(true)
    try {
      const next = await getRelayOverview()
      if (request !== requestRef.current) return
      setOverview(next)
      setFailed(false)
    } catch (err) {
      if (request !== requestRef.current) return
      log.error({ err }, 'Failed to load relay overview')
      setFailed(true)
    } finally {
      if (request === requestRef.current) setLoading(false)
    }
  }, [])

  useEffect(() => {
    void reload()
    return () => {
      requestRef.current += 1
    }
  }, [reload, customRelays, allowRelayFallback])

  return { overview, loading, failed, reload }
}
