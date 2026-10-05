import { useEffect, useState } from 'react'
import { summarizeEntryTags, type HistoryEntryTagSummaryDto } from '@/api/daemon/history-tags'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-selection-tag-summary')

/**
 * Which local tags a selection carries, from the daemon rather than the loaded
 * rows: a selection may reach past the loaded window. `null` while loading,
 * disabled, or when the summary cannot be fetched. Bump `revision` after a tag
 * change to refetch.
 */
export function useSelectionTagSummary(
  entryIds: string[],
  enabled: boolean,
  revision = 0
): HistoryEntryTagSummaryDto | null {
  const [summary, setSummary] = useState<HistoryEntryTagSummaryDto | null>(null)
  const key = enabled ? entryIds.join(',') : ''

  useEffect(() => {
    if (key === '') return
    let cancelled = false
    summarizeEntryTags(key.split(','))
      .then(result => {
        if (!cancelled) setSummary(result)
      })
      .catch(err => {
        log.debug({ err }, 'Failed to summarize selection tags')
        if (!cancelled) setSummary(null)
      })
    return () => {
      cancelled = true
    }
  }, [key, revision])

  return key === '' ? null : summary
}
