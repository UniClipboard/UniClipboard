import { useCallback, useEffect, useRef, useState } from 'react'
import { getStorageStats, type StorageStats } from '@/api/storage'
import { createLogger } from '@/lib/logger'

const log = createLogger('storage-stats')

export function useStorageStats() {
  const [state, setState] = useState<{
    stats: StorageStats | null
    loading: boolean
    error: string | null
  }>({ stats: null, loading: true, error: null })
  const generation = useRef(0)
  const alive = useRef(false)
  const refresh = useCallback(async () => {
    if (!alive.current) return
    const id = ++generation.current
    setState(current => ({ ...current, loading: true, error: null }))
    try {
      const stats = await getStorageStats()
      if (alive.current && generation.current === id)
        setState({ stats, loading: false, error: null })
    } catch (err) {
      if (!alive.current || generation.current !== id) return
      log.error({ err }, 'Failed to load storage stats')
      setState(current => ({
        ...current,
        loading: false,
        error: err instanceof Error ? err.message : String(err),
      }))
    }
  }, [])
  useEffect(() => {
    alive.current = true
    void refresh()
    return () => {
      alive.current = false
      generation.current += 1
    }
  }, [refresh])
  return { ...state, refresh }
}
