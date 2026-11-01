import { useTranslation } from 'react-i18next'
import HistoryDetailOutbound from '@/components/history/detail/HistoryDetailOutbound'
import { useAppSelector } from '@/store/hooks'
import { isOutboundRunning } from '@/store/slices/outboundProgressSlice'
import { Text } from '@/ui-kit'

/** Active sends remain visible even when their history entry has not loaded. */
export function HistoryKitOutboundTransfers({
  deviceNames,
}: {
  deviceNames: Record<string, string>
}) {
  const { t } = useTranslation()
  const byEntry = useAppSelector(state => state.outboundProgress.byEntry)
  const activeEntries = Object.entries(byEntry).filter(([, targets]) =>
    Object.values(targets).some(isOutboundRunning)
  )
  if (activeEntries.length === 0) return null
  return (
    <section
      className="flex shrink-0 flex-col gap-2 px-4 pb-3"
      aria-label={t('clipboard.transfer.sending')}
    >
      <Text role="headline">{t('clipboard.transfer.sending')}</Text>
      {activeEntries.map(([entryId]) => (
        <HistoryDetailOutbound key={entryId} entryId={entryId} deviceNames={deviceNames} />
      ))}
    </section>
  )
}
