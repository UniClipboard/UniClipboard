import { listen } from '@tauri-apps/api/event'
import { useState, useEffect, useCallback } from 'react'
import { getLifecycleStatus, retryLifecycle, LifecycleState } from '@/api/lifecycle'

export function useLifecycleStatus() {
  const [status, setStatus] = useState<LifecycleState>('Idle')
  const [retrying, setRetrying] = useState(false)

  useEffect(() => {
    // Check initial status
    getLifecycleStatus()
      .then(setStatus)
      .catch(() => {
        // If the command fails, assume idle
        setStatus('Idle')
      })

    // Listen for lifecycle events
    const unlistenPromise = listen<{ type: string }>('lifecycle://event', () => {
      // Refresh status when lifecycle events occur
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
      // Refresh status even on failure
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
