import { useTranslation } from 'react-i18next'
import { Button, ContentUnavailable } from '@/ui-kit'

/** The list query failed: say so and offer to run it again, instead of showing an empty history. */
export function HistoryKitQueryFailed({
  reason,
  onRetry,
}: {
  reason: 'unavailable' | 'failed'
  onRetry: () => void
}) {
  const { t } = useTranslation()
  return (
    <ContentUnavailable
      icon="info"
      title={t('history.kit.queryFailedTitle')}
      description={t(
        reason === 'unavailable' ? 'history.kit.queryUnavailable' : 'history.kit.queryFailed'
      )}
    >
      <Button onClick={onRetry}>{t('history.kit.retry')}</Button>
    </ContentUnavailable>
  )
}
