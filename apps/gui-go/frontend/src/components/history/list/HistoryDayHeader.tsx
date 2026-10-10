import { useTranslation } from 'react-i18next'
import { useNow } from '@/hooks/useRelativeTime'
import { formatDayHeading } from './history-list-format'

/** HList.dc.html day header above the first row of each calendar day: the
 * relative day, its date, and how many loaded rows it holds. Owns its clock
 * subscription so "Today" rolls over without re-rendering the list. */
function HistoryDayHeader({ at, count }: { at: number; count: number }) {
  const { t, i18n } = useTranslation()
  const now = useNow()
  const { title, detail } = formatDayHeading(at, now, i18n.language, t)
  return (
    <h3 className="flex h-10 items-baseline gap-2 pb-1.5 pl-4.5 pr-4 pt-4">
      <span className="text-ui-body font-semibold text-foreground">{title}</span>
      <span className="text-ui-caption text-muted-foreground">{detail}</span>
      <span className="ml-auto font-mono text-ui-caption text-muted-foreground tabular-nums">
        {t('history.subtitle', { count })}
      </span>
    </h3>
  )
}

export default HistoryDayHeader
