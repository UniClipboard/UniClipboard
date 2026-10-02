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

  // Re-fetch when the lock state flips: `GET /search/tags` is fully gated behind
  // an unlocked session (tag counts are content-derived), so it returns 423 while
  // locked. This `.catch` swallows that and keeps the builtin defaults; the
  // refetch on unlock then merges in the custom tags a History view that mounted
  // while locked would otherwise never see.
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
