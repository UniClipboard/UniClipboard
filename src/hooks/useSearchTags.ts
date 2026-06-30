import { useEffect, useState } from 'react'
import { getSearchTags } from '@/api/daemon/search'
import { useEncryptionSessionState } from '@/hooks/useEncryptionSessionState'
import { createLogger } from '@/lib/logger'
import {
  defaultSearchTagOptions,
  mergeSearchTagOptions,
  type SearchTagOption,
} from '@/lib/search-tags'

const log = createLogger('use-search-tags')

export function useSearchTags(): SearchTagOption[] {
  const { isLocked } = useEncryptionSessionState()
  const [tags, setTags] = useState<SearchTagOption[]>(() => defaultSearchTagOptions())

  // Re-fetch when the lock state flips: `GET /search/tags` only includes custom
  // tags in an unlocked session, so a History view that mounted while locked
  // would otherwise stay pinned to builtin tags for the rest of the session.
  useEffect(() => {
    let cancelled = false
    getSearchTags()
      .then(response => {
        if (!cancelled) setTags(mergeSearchTagOptions(response.data))
      })
      .catch(err => {
        log.debug({ err }, 'Failed to load searchable tags')
      })
    return () => {
      cancelled = true
    }
  }, [isLocked])

  return tags
}
