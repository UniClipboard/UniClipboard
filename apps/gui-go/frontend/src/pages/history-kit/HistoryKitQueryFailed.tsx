import { useTranslation } from 'react-i18next'
import { SEARCH_QUERY_FAILURE_COPY, type SearchQueryFailure } from '@/lib/search-query-failure'
import { Button, ContentUnavailable } from '@/ui-kit'

/** The list query failed: say so and offer to run it again, instead of showing an empty history. */
export function HistoryKitQueryFailed({
  reason,
  onRetry,
}: {
  reason: SearchQueryFailure
  onRetry: () => void
}) {
  const { t } = useTranslation()
  return (
    <ContentUnavailable
      icon="info"
      title={t(SEARCH_QUERY_FAILURE_COPY[reason].title)}
      description={t(SEARCH_QUERY_FAILURE_COPY[reason].description)}
    >
      {reason !== 'invalid' && <Button onClick={onRetry}>{t('history.kit.retry')}</Button>}
    </ContentUnavailable>
  )
}
