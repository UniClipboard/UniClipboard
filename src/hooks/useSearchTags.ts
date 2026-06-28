import { useEffect, useState } from 'react'
import { getSearchTags } from '@/api/daemon/search'
import { createLogger } from '@/lib/logger'
import {
  defaultSearchTagOptions,
  mergeSearchTagOptions,
  type SearchTagOption,
} from '@/lib/search-tags'

const log = createLogger('use-search-tags')

export function useSearchTags(): SearchTagOption[] {
  const [tags, setTags] = useState<SearchTagOption[]>(() => defaultSearchTagOptions())

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
  }, [])

  return tags
}
