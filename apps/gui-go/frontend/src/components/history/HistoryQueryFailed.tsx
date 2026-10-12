import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { SEARCH_QUERY_FAILURE_COPY, type SearchQueryFailure } from '@/lib/search-query-failure'

export function HistoryQueryFailed({
  reason,
  onRetry,
}: {
  reason: SearchQueryFailure
  onRetry: () => void
}) {
  const { t } = useTranslation()
  return (
    <div
      role="alert"
      className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center"
    >
      <p className="text-ui-section">{t(SEARCH_QUERY_FAILURE_COPY[reason].title)}</p>
      <p className="text-ui-body text-muted-foreground">
        {t(SEARCH_QUERY_FAILURE_COPY[reason].description)}
      </p>
      {reason !== 'invalid' && <Button onClick={onRetry}>{t('history.kit.retry')}</Button>}
    </div>
  )
}
