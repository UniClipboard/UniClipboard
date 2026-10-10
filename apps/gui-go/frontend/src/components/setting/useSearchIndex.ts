import { useEffect, useRef, useState } from 'react'
import { getSearchStatus, triggerSearchRebuild, type SearchStatusData } from '@/api/daemon'
import { createLogger } from '@/lib/logger'

const log = createLogger('search-index-settings')

export function useSearchIndex() {
  const [status, setStatus] = useState<SearchStatusData | null>(null)
  const [starting, setStarting] = useState(false)
  const [revision, setRevision] = useState(0)
  const alive = useRef(false)
  const startingRef = useRef(false)

  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
    }
  }, [])

  useEffect(() => {
    let active = true
    let pending = false
    const refresh = async () => {
      if (pending) return
      pending = true
      try {
        const response = await getSearchStatus()
        if (!active) return
        setStatus(response.data)
        if (response.data.state !== 'rebuilding') clearInterval(timer)
      } catch (err) {
        if (!active) return
        log.error({ err }, 'Failed to load search index status')
        setStatus(null)
        clearInterval(timer)
      } finally {
        pending = false
        if (active && revision > 0) {
          startingRef.current = false
          setStarting(false)
        }
      }
    }
    // Skip ticks while a request is in flight; one effect owns the whole timer.
    const timer = setInterval(() => {
      void refresh()
    }, 2000)
    void refresh()
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [revision])

  const rebuild = async () => {
    if (startingRef.current || status?.state === 'rebuilding') return
    startingRef.current = true
    setStarting(true)
    try {
      await triggerSearchRebuild()
      if (alive.current) setRevision(value => value + 1)
    } catch (err) {
      log.error({ err }, 'Failed to trigger search index rebuild')
      startingRef.current = false
      if (alive.current) setStarting(false)
    }
  }
  return { status, rebuilding: starting || status?.state === 'rebuilding', rebuild }
}
