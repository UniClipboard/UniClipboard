import { useEffect, useState } from 'react'
import { countSearch } from '@/api/daemon/search'
import { useEncryptionSessionState } from '@/hooks/useEncryptionSessionState'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-library-counts')

/** Entry counts of the Library rows: every entry, and the pinned ones. */
export interface LibraryCounts {
  all: number
  pinned: number
}

// Coalesces a burst of list changes (a paste stack, a bulk pin) into one count.
const REFRESH_DELAY_MS = 400

/**
 * Counts for the Library sidebar's All items and Pinned rows, from one batched
 * `/search/count` request. `null` until loaded and while the session is locked
 * (counts are content-derived and gated like search). Pass a `revision` that
 * changes whenever entries are added, removed or pinned (History passes its
 * item list) to recount after the change settles.
 */
export function useLibraryCounts(revision?: unknown): LibraryCounts | null {
  const { isLocked } = useEncryptionSessionState()
  const [counts, setCounts] = useState<LibraryCounts | null>(null)

  useEffect(() => {
    if (isLocked) {
      setCounts(null)
      return
    }
    const controller = new AbortController()
    const timer = setTimeout(() => {
      countSearch([{ query: '' }, { query: '', tags: 'favorited' }], controller.signal)
        .then(([all, pinned]) => setCounts({ all, pinned }))
        .catch(err => {
          if (!controller.signal.aborted) log.debug({ err }, 'Failed to count library rows')
        })
    }, REFRESH_DELAY_MS)
    return () => {
      clearTimeout(timer)
      controller.abort()
    }
  }, [isLocked, revision])

  return counts
}
