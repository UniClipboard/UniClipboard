import { listen } from '@tauri-apps/api/event'
import { useState, useEffect, useCallback } from 'react'
import { getLifecycleStatus, retryLifecycle } from '@/api/lifecycle'
import type { LifecycleStatusDto } from '@/api/types'

export function useLifecycleStatus() {
  const [status, setStatus] = useState<LifecycleStatusDto | null>(null)
  const [retrying, setRetrying] = useState(false)

  useEffect(() => {
    // Fetch initial status via daemon HTTP
    getLifecycleStatus()
      .then(setStatus)
      .catch(() => {
        // If the call fails, leave status null (unknown)
        setStatus(null)
      })

    // Listen for lifecycle events from Tauri
    const unlistenPromise = listen<{ type: string }>('lifecycle://event', () => {
      // Refresh status from daemon HTTP when lifecycle events occur
      getLifecycleStatus()
        .then(setStatus)
        .catch(() => {})
    })

    return () => {
      unlistenPromise.then(unlisten => unlisten?.())
    }
  }, [])

  const retry = useCallback(async () => {
    setRetrying(true)
    try {
      await retryLifecycle()
      const newStatus = await getLifecycleStatus()
      setStatus(newStatus)
    } catch {
      try {
        const newStatus = await getLifecycleStatus()
        setStatus(newStatus)
      } catch {
        // ignore
      }
    } finally {
      setRetrying(false)
    }
  }, [])

  return { status, retry, retrying }
}
