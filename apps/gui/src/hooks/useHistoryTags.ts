import { useEffect, useState } from 'react'
import { listHistoryTags, type HistoryTagDto } from '@/api/daemon/history-tags'
import { useEncryptionSessionState } from '@/hooks/useEncryptionSessionState'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-history-tags')

export interface HistoryTagsState {
  /** This device's tags, most used first. */
  tags: HistoryTagDto[]
  /** False until the list loads, and whenever it cannot (session or content
   * locked, a profile without tag support, or a daemon without the endpoint):
   * tag surfaces hide instead of showing an error. */
  available: boolean
}

/** Local history tags. Bump `revision` after a tag change to refetch. */
export function useHistoryTags(revision = 0): HistoryTagsState {
  const { isLocked } = useEncryptionSessionState()
  const [state, setState] = useState<HistoryTagsState>({ tags: [], available: false })

  useEffect(() => {
    let cancelled = false
    listHistoryTags()
      .then(tags => {
        if (!cancelled) setState({ tags, available: true })
      })
      .catch(err => {
        log.debug({ err }, 'Failed to load history tags')
        if (!cancelled) setState({ tags: [], available: false })
      })
    return () => {
      cancelled = true
    }
  }, [isLocked, revision])

  return state
}
