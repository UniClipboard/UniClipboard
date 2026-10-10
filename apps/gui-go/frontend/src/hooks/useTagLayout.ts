import { useEffect, useState } from 'react'
import { getHistoryTagLayout, type HistoryTagLayoutDto } from '@/api/daemon/history-tags'
import { useEncryptionSessionState } from '@/hooks/useEncryptionSessionState'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-tag-layout')

/** The daemon's tag layout; `null` until it loads, and whenever it cannot
 * (locked session, a profile without tags): tag surfaces then fall back to
 * neutral colors and an empty sidebar section. */
export function useTagLayout(revision = 0): HistoryTagLayoutDto | null {
  const { isLocked } = useEncryptionSessionState()
  const [layout, setLayout] = useState<HistoryTagLayoutDto | null>(null)

  useEffect(() => {
    let cancelled = false
    getHistoryTagLayout()
      .then(next => {
        if (!cancelled) setLayout(next)
      })
      .catch(err => {
        log.debug({ err }, 'Failed to load the tag layout')
        if (!cancelled) setLayout(null)
      })
    return () => {
      cancelled = true
    }
  }, [isLocked, revision])

  return layout
}
